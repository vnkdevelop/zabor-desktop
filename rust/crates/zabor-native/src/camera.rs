use crate::audio_runtime::{AudioCleanup, AudioCleanupError, SharedAudio, SharedAudioDevice};
use crate::media::MediaError;
use crate::picker::{PickerImage, PickerQuery};
use crate::screen::bgra_frame;
use libwebrtc::video_frame::{VideoFrame, VideoRotation};
use libwebrtc::video_source::{native::NativeVideoSource, VideoResolution};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use zabor_camera::{Camera, CameraError, Completion, Device};

#[derive(Debug)]
pub struct CameraCapture {
    pub source: NativeVideoSource,
    stop: CancellationToken,
    failed: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
}

struct CancelOnDrop(Option<CancellationToken>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if let Some(stop) = &self.0 {
            stop.cancel();
        }
    }
}

struct NativeOwner {
    camera: Option<Camera>,
    completion: Option<Completion>,
    source: Option<NativeVideoSource>,
}

impl NativeOwner {
    fn new(camera: Camera) -> Self {
        let completion = camera.completion();
        Self {
            camera: Some(camera),
            completion: Some(completion),
            source: None,
        }
    }

    fn finish(&mut self) -> Result<(), AudioCleanupError> {
        self.camera = None;
        let result = if let Some(completion) = &self.completion {
            loop {
                let status = completion.status();
                if status.finished {
                    break if status.cleanup_failed
                        || !status.callbacks_released
                        || !status.owner_thread_consistent
                    {
                        Err(AudioCleanupError::Incomplete)
                    } else {
                        Ok(())
                    };
                }
                std::thread::park_timeout(Duration::from_millis(10));
            }
        } else {
            Ok(())
        };
        self.completion = None;
        self.source = None;
        result
    }
}

impl Drop for NativeOwner {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

impl CameraCapture {
    pub async fn picker_with_audio_runtime<D: SharedAudioDevice>(
        runtime: &SharedAudio<D>,
        query: PickerQuery,
        sources_ready: impl FnOnce(Vec<Device>) + Send + 'static,
        preview_ready: impl Fn(String, Option<PickerImage>) + Send + 'static,
    ) -> Result<Vec<Device>, MediaError> {
        query
            .inventory(runtime.cleanup_tracker(), move |query, ready| {
                let camera = match Camera::enumerate() {
                    Ok(camera) => camera,
                    Err(_) => return Ok(()),
                };
                let mut owner = NativeOwner::new(camera);
                let devices = loop {
                    if query.is_stopped() {
                        break None;
                    }
                    let camera = owner.camera.as_ref().unwrap();
                    let status = camera.status();
                    if status.failed || status.cleanup_failed {
                        break None;
                    }
                    if status.ready {
                        break Some(camera.take_devices());
                    }
                    if status.finished {
                        break None;
                    }
                    std::thread::park_timeout(Duration::from_millis(10));
                };
                owner.finish()?;
                let Some(devices) = devices.filter(|_| !query.is_stopped()) else {
                    return Ok(());
                };
                sources_ready(devices.clone());
                if ready.send(devices.clone()).is_err() {
                    query.cancel();
                    return Ok(());
                }
                for device in devices {
                    if query.is_stopped() {
                        break;
                    }
                    let camera = match Camera::open(&device.id) {
                        Ok(camera) => camera,
                        Err(_) => {
                            if !query.is_stopped() {
                                preview_ready(format!("camera:{}", device.id), None);
                            }
                            continue;
                        }
                    };
                    let image = Self::preview(camera, &query)?;
                    if !query.is_stopped() {
                        preview_ready(format!("camera:{}", device.id), image);
                    }
                }
                Ok(())
            })
            .await
    }

    fn preview(
        camera: Camera,
        query: &PickerQuery,
    ) -> Result<Option<PickerImage>, AudioCleanupError> {
        let mut owner = NativeOwner::new(camera);
        let started = std::time::Instant::now();
        let image = loop {
            if query.is_stopped() || started.elapsed() >= Duration::from_secs(2) {
                break None;
            }
            let camera = owner.camera.as_ref().unwrap();
            let status = camera.status();
            if status.failed || status.finished || status.cleanup_failed {
                break None;
            }
            if let Some(frame) = camera.take_frame() {
                break PickerImage::from_bgra(
                    &frame.bgra,
                    frame.width * 4,
                    frame.width as i32,
                    frame.height as i32,
                )
                .ok();
            }
            std::thread::park_timeout(Duration::from_millis(10));
        };
        owner.finish()?;
        Ok(image.filter(|_| !query.is_stopped()))
    }

    pub(crate) async fn start(
        device: String,
        cleanup: Arc<AudioCleanup>,
    ) -> Result<Self, MediaError> {
        Self::start_with(cleanup, CancellationToken::new(), move || {
            Camera::open(&device)
        })
        .await
    }

    async fn start_with(
        cleanup: Arc<AudioCleanup>,
        stop: CancellationToken,
        begin: impl FnOnce() -> Result<Camera, CameraError> + Send + 'static,
    ) -> Result<Self, MediaError> {
        let prepared = cleanup.prepare_task().map_err(|_| MediaError::Backend)?;
        let mut cancellation = CancelOnDrop(Some(stop.clone()));
        let failed = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker_failed = failed.clone();
        let worker_finished = finished.clone();
        let (ready, received) = oneshot::channel();
        prepared.spawn_blocking_result(move || {
            if worker_stop.is_cancelled() {
                return Ok(());
            }
            let camera = match begin() {
                Ok(camera) => camera,
                Err(_) => {
                    worker_failed.store(true, Ordering::Release);
                    return Ok(());
                }
            };
            let mut owner = NativeOwner::new(camera);
            let mut ready = Some(ready);
            while !worker_stop.is_cancelled() {
                let camera = owner.camera.as_ref().unwrap();
                let status = camera.status();
                if status.failed || status.finished || status.cleanup_failed {
                    worker_failed.store(true, Ordering::Release);
                    break;
                }
                if let Some(frame) = camera.take_frame() {
                    if frame.width < 16
                        || frame.height < 16
                        || frame.width > 3840
                        || frame.height > 2160
                    {
                        worker_failed.store(true, Ordering::Release);
                        break;
                    }
                    let buffer = match bgra_frame(
                        &frame.bgra,
                        frame.width * 4,
                        frame.width as i32,
                        frame.height as i32,
                        frame.width,
                        frame.height,
                    ) {
                        Ok(buffer) => buffer,
                        Err(_) => {
                            worker_failed.store(true, Ordering::Release);
                            break;
                        }
                    };
                    if worker_stop.is_cancelled() {
                        break;
                    }
                    let source = owner.source.get_or_insert_with(|| {
                        NativeVideoSource::new(
                            VideoResolution {
                                width: frame.width,
                                height: frame.height,
                            },
                            false,
                        )
                    });
                    source.capture_frame(&VideoFrame::new(VideoRotation::VideoRotation0, buffer));
                    if let Some(ready) = ready.take() {
                        if worker_stop.is_cancelled() || ready.send(source.clone()).is_err() {
                            worker_stop.cancel();
                            break;
                        }
                    }
                }
                std::thread::park_timeout(Duration::from_millis(10));
            }
            drop(ready);
            let result = owner.finish();
            worker_finished.store(true, Ordering::Release);
            result
        });
        let source = tokio::select! {
            biased;
            _ = stop.cancelled() => return Err(MediaError::Backend),
            result = tokio::time::timeout(Duration::from_secs(5), received) => {
                result.map_err(|_| MediaError::Backend)?.map_err(|_| MediaError::Backend)?
            }
        };
        if stop.is_cancelled() || failed.load(Ordering::Acquire) || finished.load(Ordering::Acquire)
        {
            return Err(MediaError::Backend);
        }
        cancellation.0 = None;
        Ok(Self {
            source,
            stop,
            failed,
            finished,
        })
    }

    pub async fn devices_with_audio_runtime<D: SharedAudioDevice>(
        runtime: &SharedAudio<D>,
        stop: CancellationToken,
    ) -> Result<Vec<Device>, MediaError> {
        Self::devices_with(runtime.cleanup_tracker(), stop, Camera::enumerate).await
    }

    async fn devices_with(
        cleanup: Arc<AudioCleanup>,
        stop: CancellationToken,
        begin: impl FnOnce() -> Result<Camera, CameraError> + Send + 'static,
    ) -> Result<Vec<Device>, MediaError> {
        let prepared = cleanup.prepare_task().map_err(|_| MediaError::Backend)?;
        let worker_stop = stop.child_token();
        let cancellation = CancelOnDrop(Some(worker_stop.clone()));
        let (ready, received) = oneshot::channel();
        prepared.spawn_blocking_result(move || {
            if worker_stop.is_cancelled() {
                return Ok(());
            }
            let camera = match begin() {
                Ok(camera) => camera,
                Err(_) => return Ok(()),
            };
            let mut owner = NativeOwner::new(camera);
            while !worker_stop.is_cancelled() {
                let camera = owner.camera.as_ref().unwrap();
                let status = camera.status();
                if status.failed || status.cleanup_failed {
                    break;
                }
                if status.ready {
                    let devices = camera.take_devices();
                    let _ = ready.send(devices);
                    break;
                }
                if status.finished {
                    break;
                }
                std::thread::park_timeout(Duration::from_millis(10));
            }
            owner.finish()
        });
        let result = tokio::select! {
            biased;
            _ = stop.cancelled() => Err(MediaError::Backend),
            result = tokio::time::timeout(Duration::from_secs(5), received) => {
                result.map_err(|_| MediaError::Backend)?.map_err(|_| MediaError::Backend)
            }
        };
        drop(cancellation);
        result
    }

    pub fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }

    pub(crate) fn is_live(&self) -> bool {
        !self.stop.is_cancelled() && !self.failed() && !self.finished.load(Ordering::Acquire)
    }

    #[cfg(all(test, feature = "camera-test-source"))]
    pub(crate) async fn synthetic(
        cleanup: Arc<AudioCleanup>,
        mode: u8,
        delay_ms: u32,
    ) -> Result<Self, MediaError> {
        Self::start_with(cleanup, CancellationToken::new(), move || {
            Camera::synthetic_video(mode, delay_ms)
        })
        .await
    }
}

impl Drop for CameraCapture {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

#[cfg(all(test, feature = "camera-test-source"))]
mod tests {
    use super::*;
    use crate::audio_mix::AudioControls;
    use crate::audio_settings::AudioSettings;
    use crate::critical_intents::{CriticalAction, CriticalIntents, IntentScope};
    use crate::video::decode_frame;
    use futures_util::StreamExt;
    use libwebrtc::peer_connection_factory::native::PeerConnectionFactoryExt;
    use libwebrtc::peer_connection_factory::PeerConnectionFactory;
    use libwebrtc::video_stream::native::NativeVideoStream;
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc;
    use std::time::Instant;
    use tokio::time::timeout;

    #[tokio::test]
    async fn registered_camera_forwards_owned_native_frames_and_drain_waits_for_original_join() {
        timeout(Duration::from_secs(10), async {
            let cleanup = Arc::new(AudioCleanup::default());
            let capture = CameraCapture::synthetic(cleanup.clone(), 2, 150)
                .await
                .unwrap();
            assert!(capture.is_live());
            assert_eq!(capture.source.video_resolution().width, 160);
            let factory = PeerConnectionFactory::default();
            let track = factory.create_video_track("local-camera-frame", capture.source.clone());
            let mut stream = NativeVideoStream::new(track);
            let frame = loop {
                let frame = decode_frame(stream.next().await.unwrap()).unwrap();
                if frame.width == 160 && frame.height == 96 && frame.rgba[0] != 0 {
                    break frame;
                }
            };
            assert!(frame.rgba.as_chunks::<4>().0.iter().all(|pixel| {
                (60..=85).contains(&pixel[0])
                    && (25..=50).contains(&pixel[1])
                    && (10..=35).contains(&pixel[2])
                    && pixel[3] == 255
            }));
            drop(stream);
            let started = Instant::now();
            drop(capture);
            assert!(started.elapsed() < Duration::from_millis(100));
            cleanup.seal().unwrap();
            assert!(timeout(Duration::from_millis(80), cleanup.wait())
                .await
                .is_err());
            cleanup.wait().await.unwrap();
            assert!(started.elapsed() >= Duration::from_millis(140));
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn sealed_and_pre_cancelled_camera_roots_never_acquire_a_native_owner() {
        let acquired = Arc::new(AtomicUsize::new(0));
        for sealed in [false, true] {
            let cleanup = Arc::new(AudioCleanup::default());
            let stop = CancellationToken::new();
            if sealed {
                cleanup.seal().unwrap();
            } else {
                stop.cancel();
            }
            let calls = acquired.clone();
            assert!(CameraCapture::start_with(cleanup.clone(), stop, move || {
                calls.fetch_add(1, Ordering::AcqRel);
                Camera::synthetic(0, 0)
            })
            .await
            .is_err());
            timeout(Duration::from_secs(5), cleanup.wait())
                .await
                .unwrap()
                .unwrap();
        }
        assert_eq!(acquired.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn cancelled_start_keeps_registration_across_late_native_return_and_actual_join() {
        timeout(Duration::from_secs(10), async {
            let cleanup = Arc::new(AudioCleanup::default());
            let stop = CancellationToken::new();
            let (entered, started) = oneshot::channel();
            let (release, released) = mpsc::channel();
            let task_cleanup = cleanup.clone();
            let task_stop = stop.clone();
            let start = tokio::spawn(async move {
                CameraCapture::start_with(task_cleanup, task_stop, move || {
                    entered.send(()).unwrap();
                    released.recv().unwrap();
                    Camera::synthetic(1, 150)
                })
                .await
            });
            started.await.unwrap();
            stop.cancel();
            assert!(start.await.unwrap().is_err());
            cleanup.seal().unwrap();
            assert!(timeout(Duration::from_millis(50), cleanup.wait())
                .await
                .is_err());
            release.send(()).unwrap();
            cleanup.wait().await.unwrap();
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn aborting_only_the_start_wait_does_not_abort_cleanup_or_accept_pending_acquisition() {
        timeout(Duration::from_secs(10), async {
            let cleanup = Arc::new(AudioCleanup::default());
            let (entered, started) = oneshot::channel();
            let (release, released) = mpsc::channel();
            let task_cleanup = cleanup.clone();
            let start = tokio::spawn(async move {
                CameraCapture::start_with(task_cleanup, CancellationToken::new(), move || {
                    entered.send(()).unwrap();
                    released.recv().unwrap();
                    Camera::synthetic(1, 150)
                })
                .await
            });
            started.await.unwrap();
            start.abort();
            assert!(start.await.unwrap_err().is_cancelled());
            cleanup.seal().unwrap();
            assert!(timeout(Duration::from_millis(50), cleanup.wait())
                .await
                .is_err());
            release.send(()).unwrap();
            cleanup.wait().await.unwrap();
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn acquisition_failure_does_not_poison_clean_drain_but_native_cleanup_failure_does() {
        timeout(Duration::from_secs(10), async {
            let cleanup = Arc::new(AudioCleanup::default());
            assert!(CameraCapture::synthetic(cleanup.clone(), 3, 0)
                .await
                .is_err());
            cleanup.wait().await.unwrap();
            let cleanup = Arc::new(AudioCleanup::default());
            let capture = CameraCapture::synthetic(cleanup.clone(), 5, 0)
                .await
                .unwrap();
            drop(capture);
            assert!(cleanup.wait().await.is_err());
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn owner_unwind_keeps_native_completion_until_join_and_reports_incomplete_drain() {
        timeout(Duration::from_secs(10), async {
            let cleanup = Arc::new(AudioCleanup::default());
            let (entered, started) = oneshot::channel();
            cleanup
                .prepare_task()
                .unwrap()
                .spawn_blocking_result(move || {
                    let mut owner = NativeOwner::new(Camera::synthetic_video(2, 150).unwrap());
                    let frame = loop {
                        if let Some(frame) = owner.camera.as_ref().unwrap().take_frame() {
                            break frame;
                        }
                        std::thread::yield_now();
                    };
                    let buffer = bgra_frame(
                        &frame.bgra,
                        frame.width * 4,
                        frame.width as i32,
                        frame.height as i32,
                        frame.width,
                        frame.height,
                    )
                    .unwrap();
                    let source = NativeVideoSource::new(
                        VideoResolution {
                            width: frame.width,
                            height: frame.height,
                        },
                        false,
                    );
                    source.capture_frame(&VideoFrame::new(VideoRotation::VideoRotation0, buffer));
                    owner.source = Some(source);
                    entered.send(()).unwrap();
                    panic!("synthetic registered camera actor unwind");
                });
            started.await.unwrap();
            let started = Instant::now();
            assert!(cleanup.wait().await.is_err());
            assert!(started.elapsed() >= Duration::from_millis(130));
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn cancelled_inventory_wait_retains_the_original_registered_owner_cleanup() {
        timeout(Duration::from_secs(10), async {
            let cleanup = Arc::new(AudioCleanup::default());
            let stop = CancellationToken::new();
            let (entered, started) = oneshot::channel();
            let task_cleanup = cleanup.clone();
            let task_stop = stop.clone();
            let inventory = tokio::spawn(async move {
                CameraCapture::devices_with(task_cleanup, task_stop, move || {
                    let camera = Camera::synthetic(1, 150)?;
                    while !camera.status().opening {
                        std::thread::yield_now();
                    }
                    entered.send(()).unwrap();
                    Ok(camera)
                })
                .await
            });
            started.await.unwrap();
            stop.cancel();
            assert!(inventory.await.unwrap().is_err());
            assert!(timeout(Duration::from_millis(80), cleanup.wait())
                .await
                .is_err());
            cleanup.wait().await.unwrap();
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn tiny_backend_frames_are_rejected_before_entering_the_native_video_adapter() {
        timeout(Duration::from_secs(5), async {
            let cleanup = Arc::new(AudioCleanup::default());
            assert!(
                CameraCapture::start_with(cleanup.clone(), CancellationToken::new(), || {
                    Camera::synthetic(0, 0)
                })
                .await
                .is_err()
            );
            cleanup.wait().await.unwrap();
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn successful_inventory_retires_only_its_child_and_keeps_picker_identity_current() {
        timeout(Duration::from_secs(5), async {
            let cleanup = Arc::new(AudioCleanup::default());
            let query = CancellationToken::new();
            let devices = CameraCapture::devices_with(cleanup.clone(), query.clone(), || {
                Camera::synthetic(0, 0)
            })
            .await
            .unwrap();
            assert!(devices.is_empty());
            assert!(!query.is_cancelled());
            cleanup.wait().await.unwrap();
            query.cancel();
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn picker_camera_preview_is_byte_only_and_waits_for_the_original_native_owner() {
        timeout(Duration::from_secs(5), async {
            let cleanup = Arc::new(AudioCleanup::default());
            let query = PickerQuery::new(CancellationToken::new());
            let started = Instant::now();
            let image = query
                .inventory(cleanup.clone(), |query, ready| {
                    let camera = Camera::synthetic_video(2, 150).unwrap();
                    let image = CameraCapture::preview(camera, &query)?.unwrap();
                    ready.send(image).unwrap();
                    Ok(())
                })
                .await
                .unwrap();
            assert!(started.elapsed() >= Duration::from_millis(130));
            assert_eq!((image.width, image.height), (160, 96));
            assert_eq!(image.rgba, [71, 37, 19, 255].repeat(160 * 96));
            assert!(!query.is_cancelled());
            cleanup.wait().await.unwrap();
            assert!(!query.is_running());
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn cancelled_picker_camera_preview_keeps_native_join_pending_and_never_delivers_an_image()
    {
        timeout(Duration::from_secs(5), async {
            let cleanup = Arc::new(AudioCleanup::default());
            let query = PickerQuery::new(CancellationToken::new());
            let (entered, started) = oneshot::channel();
            let (release, released) = mpsc::channel();
            let worker_query = query.clone();
            let worker_cleanup = cleanup.clone();
            let task = tokio::spawn(async move {
                worker_query
                    .inventory(worker_cleanup, move |query, ready| {
                        let camera = Camera::synthetic_video(2, 150).unwrap();
                        while !camera.status().ready {
                            std::thread::yield_now();
                        }
                        entered.send(()).unwrap();
                        released.recv().unwrap();
                        assert!(CameraCapture::preview(camera, &query)?.is_none());
                        assert!(ready.send(()).is_err());
                        Ok(())
                    })
                    .await
            });
            started.await.unwrap();
            query.cancel();
            assert!(task.await.unwrap().is_err());
            release.send(()).unwrap();
            assert!(timeout(Duration::from_millis(60), cleanup.wait())
                .await
                .is_err());
            cleanup.wait().await.unwrap();
            assert!(!query.is_running());
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn picker_camera_acquisition_failure_keeps_fallback_but_cleanup_failure_blocks_acceptance(
    ) {
        let cleanup = Arc::new(AudioCleanup::default());
        let query = PickerQuery::new(CancellationToken::new());
        let image = query
            .inventory(cleanup.clone(), |query, ready| {
                let image = CameraCapture::preview(Camera::synthetic(3, 0).unwrap(), &query)?;
                ready.send(image).unwrap();
                Ok(())
            })
            .await
            .unwrap();
        assert!(image.is_none());
        cleanup.wait().await.unwrap();
        let cleanup = Arc::new(AudioCleanup::default());
        let query = PickerQuery::new(CancellationToken::new());
        assert!(query
            .inventory::<()>(cleanup.clone(), |query, _| {
                CameraCapture::preview(Camera::synthetic_video(5, 0).unwrap(), &query)?;
                Ok(())
            })
            .await
            .is_err());
        assert!(cleanup.wait().await.is_err());
    }

    struct Device;

    impl SharedAudioDevice for Device {
        fn failed(&self) -> bool {
            false
        }

        fn cancel(&self) {}

        fn finish_shutdown(&self) -> Result<(), AudioCleanupError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn application_shared_runtime_drain_observes_camera_join_without_opening_audio() {
        timeout(Duration::from_secs(5), async {
            let controls = Arc::new(AudioControls::new(AudioSettings::default()));
            let runtime =
                SharedAudio::<Device>::new(controls, CancellationToken::new(), |_, _, _| {
                    Box::pin(async { panic!("Camera drain test must not acquire physical audio") })
                });
            let capture = CameraCapture::synthetic(runtime.cleanup_tracker(), 2, 150)
                .await
                .unwrap();
            drop(capture);
            runtime.seal_cleanup().unwrap();
            assert!(
                timeout(Duration::from_millis(80), runtime.wait_for_cleanup())
                    .await
                    .is_err()
            );
            runtime.wait_for_cleanup().await.unwrap();
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn leave_and_logout_revoke_original_camera_start_without_accepting_late_native_return() {
        timeout(Duration::from_secs(10), async {
            for action in [CriticalAction::Leave, CriticalAction::Logout] {
                let intents = CriticalIntents::default();
                let ticket = intents.ticket();
                let session = intents.session_token(ticket);
                let cleanup = Arc::new(AudioCleanup::default());
                let (entered, started) = oneshot::channel();
                let (result, ()) = tokio::join!(
                    intents.run(
                        ticket,
                        IntentScope::Voice,
                        CameraCapture::start_with(
                            cleanup.clone(),
                            CancellationToken::new(),
                            move || {
                                let camera = Camera::synthetic_video(1, 150)?;
                                while !camera.status().opening {
                                    std::thread::yield_now();
                                }
                                entered.send(()).unwrap();
                                Ok(camera)
                            },
                        )
                    ),
                    async {
                        started.await.unwrap();
                        intents.request(action);
                    }
                );
                assert!(result.is_none());
                assert!(!intents.is_current(ticket, IntentScope::Voice));
                assert_eq!(session.is_cancelled(), action == CriticalAction::Logout);
                cleanup.seal().unwrap();
                assert!(timeout(Duration::from_millis(80), cleanup.wait())
                    .await
                    .is_err());
                cleanup.wait().await.unwrap();
            }
        })
        .await
        .unwrap();
    }
}
