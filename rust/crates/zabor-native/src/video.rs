use crate::audio_runtime::{AudioCleanup, AudioCleanupError, SharedAudio, SharedAudioDevice};
use crate::media::{MediaError, PeerLifetime};
use futures_util::StreamExt;
use libwebrtc::video_frame::{
    native::VideoFrameBufferExt, BoxVideoFrame, VideoBuffer, VideoFormatType, VideoRotation,
};
use libwebrtc::video_stream::native::NativeVideoStream;
use libwebrtc::video_track::RtcVideoTrack;
use std::collections::BTreeMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

#[derive(Default)]
pub struct StreamDropGrace {
    deadlines: BTreeMap<String, Instant>,
}

impl StreamDropGrace {
    pub fn schedule(&mut self, peer: String, now: Instant) {
        self.deadlines.insert(peer, now + Duration::from_secs(30));
    }
    pub fn cancel(&mut self, peer: &str) {
        self.deadlines.remove(peer);
    }
    pub fn clear(&mut self) {
        self.deadlines.clear();
    }
    pub fn next_deadline(&self) -> Option<Instant> {
        self.deadlines.values().copied().min()
    }
    pub fn take_expired(&mut self, now: Instant) -> Vec<String> {
        let mut expired = Vec::new();
        self.deadlines.retain(|peer, deadline| {
            if *deadline <= now {
                expired.push(peer.clone());
                false
            } else {
                true
            }
        });
        expired
    }
}

pub struct DecodedVideo {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    decoded_at: Instant,
}

impl DecodedVideo {
    pub fn decoded_at(&self) -> Instant {
        self.decoded_at
    }

    pub fn stamp(&self, lease: &VideoFrameLease) -> VideoFrameStamp {
        VideoFrameStamp {
            decoded_at: self.decoded_at,
            pixels: (self.width, self.height),
            lease: lease.clone(),
        }
    }
}

#[derive(Clone)]
pub struct VideoFrameStamp {
    decoded_at: Instant,
    pixels: (u32, u32),
    lease: VideoFrameLease,
}

impl VideoFrameStamp {
    pub fn decoded_at(&self) -> Instant {
        self.decoded_at
    }

    pub fn pixels(&self) -> (u32, u32) {
        self.pixels
    }

    pub fn is_alive(&self) -> bool {
        self.lease.is_alive()
    }

    pub fn is_current_since(&self, lifetime: &PeerLifetime, since: Instant) -> bool {
        self.is_alive() && self.lease.lifetime.is_same(lifetime) && self.decoded_at >= since
    }
}

#[derive(Clone)]
pub struct VideoFrameLease {
    lifetime: PeerLifetime,
    active: Arc<AtomicBool>,
}

impl VideoFrameLease {
    pub fn is_alive(&self) -> bool {
        self.lifetime.is_alive() && self.active.load(Ordering::Acquire)
    }
}

pub struct VideoDelivery {
    pub frame: DecodedVideo,
    pub lease: VideoFrameLease,
}

struct VideoSubscription {
    task: tokio::task::AbortHandle,
    lease: VideoFrameLease,
    stop: CancellationToken,
}

impl Drop for VideoSubscription {
    fn drop(&mut self) {
        self.lease.active.store(false, Ordering::Release);
        self.stop.cancel();
        self.task.abort();
    }
}

pub struct VideoReceiver {
    track: RtcVideoTrack,
    lifetime: PeerLifetime,
    subscription: Option<VideoSubscription>,
    sink: Arc<dyn Fn(VideoDelivery) + Send + Sync>,
    cleanup: Arc<AudioCleanup>,
}

impl VideoReceiver {
    pub fn new(
        track: RtcVideoTrack,
        lifetime: PeerLifetime,
        sink: Arc<dyn Fn(VideoDelivery) + Send + Sync>,
    ) -> Self {
        Self::with_cleanup(track, lifetime, sink, Arc::new(AudioCleanup::default()))
    }

    pub fn with_audio_runtime<D: SharedAudioDevice>(
        track: RtcVideoTrack,
        lifetime: PeerLifetime,
        sink: Arc<dyn Fn(VideoDelivery) + Send + Sync>,
        runtime: &SharedAudio<D>,
    ) -> Self {
        Self::with_cleanup(track, lifetime, sink, runtime.cleanup_tracker())
    }

    pub(crate) fn with_cleanup(
        track: RtcVideoTrack,
        lifetime: PeerLifetime,
        sink: Arc<dyn Fn(VideoDelivery) + Send + Sync>,
        cleanup: Arc<AudioCleanup>,
    ) -> Self {
        Self {
            track,
            lifetime,
            subscription: None,
            sink,
            cleanup,
        }
    }

    pub fn is_alive(&self) -> bool {
        self.lifetime.is_alive()
    }

    pub fn lifetime(&self) -> &PeerLifetime {
        &self.lifetime
    }

    pub fn is_playing(&self) -> bool {
        self.subscription.as_ref().is_some_and(|subscription| {
            subscription.lease.is_alive() && !subscription.task.is_finished()
        })
    }

    pub fn stop(&mut self) {
        self.subscription = None;
    }

    pub fn start(&mut self) {
        if !self.is_alive() || self.is_playing() {
            return;
        }
        let Ok(prepared) = self.cleanup.prepare_task() else {
            return;
        };
        self.stop();
        let lease = VideoFrameLease {
            lifetime: self.lifetime.clone(),
            active: Arc::new(AtomicBool::new(true)),
        };
        let live = lease.clone();
        let sink = self.sink.clone();
        let mut stream = NativeVideoStream::new(self.track.clone());
        let stop = CancellationToken::new();
        let task = prepared.spawn(stop.clone(), async move {
            while let Some(frame) = stream.next().await {
                if !live.is_alive() {
                    break;
                }
                if let Ok(frame) = decode_frame(frame) {
                    if live.is_alive() {
                        sink(VideoDelivery {
                            frame,
                            lease: live.clone(),
                        });
                    }
                }
            }
            live.active.store(false, Ordering::Release);
        });
        self.subscription = Some(VideoSubscription { task, lease, stop });
    }

    pub async fn wait_for_cleanup(&self) -> Result<(), AudioCleanupError> {
        self.cleanup.wait().await
    }
}

pub fn decode_frame(frame: BoxVideoFrame) -> Result<DecodedVideo, MediaError> {
    let width = frame.buffer.width();
    let height = frame.buffer.height();
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return Err(MediaError::Backend);
    }
    let mut buffer = frame.buffer.to_i420();
    let scale = (1920.0 / f64::from(width))
        .min(1080.0 / f64::from(height))
        .min(1.0);
    if scale < 1.0 {
        buffer = buffer.scale(
            ((f64::from(width) * scale) as i32 / 2 * 2).max(2),
            ((f64::from(height) * scale) as i32 / 2 * 2).max(2),
        );
    }
    let width = buffer.width();
    let height = buffer.height();
    let mut rgba = vec![0_u8; width as usize * height as usize * 4];
    buffer.to_argb(
        VideoFormatType::ABGR,
        &mut rgba,
        width * 4,
        width as i32,
        height as i32,
    );
    if frame.rotation == VideoRotation::VideoRotation0 {
        return Ok(DecodedVideo {
            width,
            height,
            rgba,
            decoded_at: Instant::now(),
        });
    }
    let source = image::RgbaImage::from_raw(width, height, rgba).ok_or(MediaError::Backend)?;
    let image = match frame.rotation {
        VideoRotation::VideoRotation90 => image::imageops::rotate90(&source),
        VideoRotation::VideoRotation180 => image::imageops::rotate180(&source),
        VideoRotation::VideoRotation270 => image::imageops::rotate270(&source),
        _ => source,
    };
    Ok(DecodedVideo {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
        decoded_at: Instant::now(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_mix::AudioControls;
    use crate::audio_settings::AudioSettings;
    use crate::media::{MediaEngine, MediaEvent};
    use libwebrtc::data_channel::DataChannelInit;
    use libwebrtc::peer_connection_factory::native::PeerConnectionFactoryExt;
    use libwebrtc::peer_connection_factory::PeerConnectionFactory;
    use libwebrtc::video_frame::{I420Buffer, VideoFrame};
    use libwebrtc::video_source::{native::NativeVideoSource, VideoResolution};
    use std::sync::atomic::AtomicUsize;
    use std::sync::Mutex;
    use tokio::sync::mpsc;
    use tokio::time::timeout;

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

    fn shared() -> Arc<SharedAudio<Device>> {
        SharedAudio::new(
            Arc::new(AudioControls::new(AudioSettings::default())),
            CancellationToken::new(),
            |_, _, _| Box::pin(async { panic!("Video cleanup must not start physical audio") }),
        )
    }

    fn peer() -> (MediaEngine, PeerLifetime, mpsc::Receiver<MediaEvent>) {
        let (mut engine, events) = MediaEngine::new_data_only("local".into());
        engine.authorize_peers(["remote".into()]);
        engine
            .create_data_channel(
                "remote",
                "video-cleanup-fixture",
                DataChannelInit::default(),
            )
            .unwrap();
        let lifetime = engine.peer_lifetime("remote").unwrap();
        (engine, lifetime, events)
    }

    #[test]
    fn copied_frame_timestamp_is_immutable_through_rotated_display_and_cached_stamps() {
        let (mut engine, lifetime, _events) = peer();
        let lease = VideoFrameLease {
            lifetime: lifetime.clone(),
            active: Arc::new(AtomicBool::new(true)),
        };
        let before = Instant::now();
        let frame = decode_frame(VideoFrame::new(
            VideoRotation::VideoRotation90,
            Box::new(I420Buffer::new_black(160, 96)) as libwebrtc::video_frame::BoxVideoBuffer,
        ))
        .unwrap();
        let after = Instant::now();
        let stamp = frame.stamp(&lease);
        assert!((before..=after).contains(&frame.decoded_at()));
        assert_eq!(stamp.pixels(), (96, 160));
        assert_eq!(frame.rgba.len(), 96 * 160 * 4);
        assert_eq!(stamp.decoded_at(), frame.decoded_at());
        std::thread::sleep(Duration::from_millis(5));
        let cached = stamp.clone();
        assert_eq!(cached.decoded_at(), stamp.decoded_at());
        assert!(cached.is_current_since(&lifetime, before));
        assert!(!cached.is_current_since(&lifetime, frame.decoded_at() + Duration::from_nanos(1)));
        engine.close();
        assert!(!cached.is_alive());
    }

    #[test]
    fn cached_stamp_rejects_other_original_peer_and_retired_subscription_with_same_peer() {
        let (mut engine, lifetime, _events) = peer();
        let (mut other, other_lifetime, _other_events) = peer();
        let live = Arc::new(AtomicBool::new(true));
        let lease = VideoFrameLease {
            lifetime: lifetime.clone(),
            active: live.clone(),
        };
        let frame = decode_frame(VideoFrame::new(
            VideoRotation::VideoRotation0,
            Box::new(I420Buffer::new_black(160, 96)) as libwebrtc::video_frame::BoxVideoBuffer,
        ))
        .unwrap();
        let stamp = frame.stamp(&lease);
        assert!(stamp.is_current_since(&lifetime, frame.decoded_at()));
        assert!(!stamp.is_current_since(&other_lifetime, frame.decoded_at()));
        live.store(false, Ordering::Release);
        assert!(!stamp.is_current_since(&lifetime, frame.decoded_at()));
        assert!(lifetime.is_alive());
        engine.close();
        other.close();
    }

    fn native_video() -> (PeerConnectionFactory, NativeVideoSource, RtcVideoTrack) {
        let factory = PeerConnectionFactory::default();
        let source = NativeVideoSource::new(
            VideoResolution {
                width: 160,
                height: 96,
            },
            true,
        );
        let track = factory.create_video_track("video-cleanup", source.clone());
        (factory, source, track)
    }

    struct HeldDrop {
        entered: std::sync::mpsc::Sender<()>,
        release: std::sync::mpsc::Receiver<()>,
        finished: Arc<AtomicBool>,
    }

    impl Drop for HeldDrop {
        fn drop(&mut self) {
            self.entered.send(()).unwrap();
            self.release.recv_timeout(Duration::from_secs(3)).unwrap();
            self.finished.store(true, Ordering::Release);
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn registered_video_subscription_waits_for_original_task_destruction_after_stop_or_unwind(
    ) {
        for mode in ["drop", "unwind", "sealed"] {
            let shared = shared();
            let cleanup = shared.cleanup_tracker();
            let (mut engine, lifetime, _events) = peer();
            let lease = VideoFrameLease {
                lifetime: lifetime.clone(),
                active: Arc::new(AtomicBool::new(true)),
            };
            let fresh = VideoFrameLease {
                lifetime: lifetime.clone(),
                active: Arc::new(AtomicBool::new(true)),
            };
            let stop = CancellationToken::new();
            let (entered, waiting) = std::sync::mpsc::channel();
            let (release, held) = std::sync::mpsc::channel();
            let finished = Arc::new(AtomicBool::new(false));
            let probe = HeldDrop {
                entered,
                release: held,
                finished: finished.clone(),
            };
            let started = Arc::new(tokio::sync::Semaphore::new(0));
            let task_started = started.clone();
            let prepared = cleanup.prepare_task().unwrap();
            if mode == "sealed" {
                shared.seal_cleanup().unwrap();
                assert!(cleanup.prepare_task().is_err());
            }
            let task = prepared.spawn(stop.clone(), async move {
                let _probe = probe;
                task_started.add_permits(1);
                std::future::pending::<()>().await;
            });
            let subscription = VideoSubscription {
                task,
                lease: lease.clone(),
                stop: stop.clone(),
            };
            started.acquire().await.unwrap().forget();
            let monitor = shared.monitor(true);
            let demand = shared.demand.reserve().unwrap();
            assert!(shared.demand.activate(demand));
            let snapshot = shared.mic_test.snapshot(Instant::now());
            if mode == "unwind" {
                assert!(
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                        let subscription = subscription;
                        let _ = &subscription;
                        panic!("Expected video subscription owner unwind");
                    }))
                    .is_err()
                );
            } else {
                drop(subscription);
            }
            tokio::task::spawn_blocking(move || waiting.recv_timeout(Duration::from_secs(1)))
                .await
                .unwrap()
                .unwrap();
            assert!(stop.is_cancelled());
            assert!(!lease.is_alive());
            assert!(fresh.is_alive());
            assert!(lease.lifetime.is_same(&lifetime));
            assert!(
                timeout(Duration::from_millis(30), shared.wait_for_cleanup())
                    .await
                    .is_err()
            );
            assert!(!finished.load(Ordering::Acquire));
            release.send(()).unwrap();
            timeout(Duration::from_secs(2), shared.wait_for_cleanup())
                .await
                .unwrap()
                .unwrap();
            assert!(finished.load(Ordering::Acquire));
            assert!(fresh.is_alive());
            assert!(monitor.is_current());
            assert_eq!(shared.demand.active(), demand);
            assert_eq!(shared.mic_test.snapshot(Instant::now()), snapshot);
            engine.close();
            println!("video task cleanup held_drop=true mode={mode} original_join_observed=true wait_cancellation_not_completion=true old_lease_invalidated=true fresh_lease_voice_monitor_unchanged=true native_callback_bounds=false physical_audio=false");
        }
    }

    #[tokio::test]
    async fn actual_native_video_tasks_are_joined_after_stop_restart_replacement_peer_retirement_and_drop(
    ) {
        for mode in [
            "stop",
            "restart",
            "replace",
            "drop",
            "dead-peer",
            "live-poison",
        ] {
            let shared = shared();
            let (mut engine, lifetime, _events) = peer();
            let (_factory, source, track) = native_video();
            let (sender, mut frames) = mpsc::channel(8);
            let sink: Arc<dyn Fn(VideoDelivery) + Send + Sync> = Arc::new(move |frame| {
                let _ = sender.try_send(frame);
            });
            let mut receiver = VideoReceiver::with_audio_runtime(
                track.clone(),
                lifetime.clone(),
                sink.clone(),
                shared.as_ref(),
            );
            let monitor = shared.monitor(true);
            let demand = shared.demand.reserve().unwrap();
            assert!(shared.demand.activate(demand));
            let snapshot = shared.mic_test.snapshot(Instant::now());
            receiver.start();
            let lease = receiver.subscription.as_ref().unwrap().lease.clone();
            let task = receiver.subscription.as_ref().unwrap().task.clone();
            source.capture_frame(&VideoFrame::new(
                VideoRotation::VideoRotation0,
                I420Buffer::new_black(160, 96),
            ));
            let frame = timeout(Duration::from_secs(2), frames.recv())
                .await
                .unwrap()
                .unwrap();
            assert_eq!((frame.frame.width, frame.frame.height), (160, 96));
            assert!(Arc::ptr_eq(&frame.lease.active, &lease.active));
            assert!(frame.lease.lifetime.is_same(&lifetime));
            assert!(
                timeout(Duration::from_millis(30), receiver.wait_for_cleanup())
                    .await
                    .is_err()
            );
            if mode == "dead-peer" {
                engine.disconnect_peer("remote");
                assert!(!lifetime.is_alive());
            } else if mode == "live-poison" {
                assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    lifetime.commit(|| panic!("Expected video cleanup peer poison"));
                }))
                .is_err());
                assert!(lifetime.is_alive());
                assert!(lifetime.commit(|| ()).is_none());
            }
            if mode == "replace" {
                receiver = VideoReceiver::with_audio_runtime(
                    track.clone(),
                    lifetime.clone(),
                    sink,
                    shared.as_ref(),
                );
                receiver.start();
            } else if mode == "restart" {
                receiver.stop();
                receiver.start();
            }
            if matches!(mode, "restart" | "replace") {
                assert!(!lease.is_alive());
                let fresh = &receiver.subscription.as_ref().unwrap().lease;
                assert!(fresh.is_alive());
                assert!(!Arc::ptr_eq(&fresh.active, &lease.active));
                assert!(fresh.lifetime.is_same(&lifetime));
                assert!(
                    timeout(Duration::from_millis(30), shared.wait_for_cleanup())
                        .await
                        .is_err()
                );
            }
            if mode != "drop" {
                receiver.stop();
                assert!(!receiver.is_playing());
                timeout(Duration::from_secs(2), receiver.wait_for_cleanup())
                    .await
                    .unwrap()
                    .unwrap();
            }
            drop(receiver);
            timeout(Duration::from_secs(2), shared.wait_for_cleanup())
                .await
                .unwrap()
                .unwrap();
            assert!(task.is_finished());
            assert!(!lease.is_alive());
            assert!(monitor.is_current());
            assert_eq!(shared.demand.active(), demand);
            assert_eq!(shared.mic_test.snapshot(Instant::now()), snapshot);
            assert_eq!(track.id(), "video-cleanup");
            engine.close();
            println!("video task cleanup native_stream=true mode={mode} original_join_observed=true exact_original_peer=true restart_lease_identity=true external_track_source_retained=true shared_drain_waited=true native_callback_bounds=false physical_audio=false");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn actual_video_callback_keeps_shared_join_pending_after_abort_and_receiver_drop() {
        for mode in ["stop", "drop", "replace"] {
            let shared = shared();
            let (mut engine, lifetime, _events) = peer();
            let (_factory, source, track) = native_video();
            let (sender, mut frames) = mpsc::channel(1);
            let (release, held) = std::sync::mpsc::channel();
            let held = Mutex::new(held);
            let completed = Arc::new(AtomicBool::new(false));
            let finished = completed.clone();
            let calls = Arc::new(AtomicUsize::new(0));
            let observed = calls.clone();
            let sink = Arc::new(move |frame| {
                if observed.fetch_add(1, Ordering::AcqRel) == 0 {
                    sender.try_send(frame).unwrap();
                    held.lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(3))
                        .unwrap();
                    finished.store(true, Ordering::Release);
                }
            });
            let mut receiver = VideoReceiver::with_audio_runtime(
                track.clone(),
                lifetime.clone(),
                sink,
                shared.as_ref(),
            );
            receiver.start();
            let task = receiver.subscription.as_ref().unwrap().task.clone();
            source.capture_frame(&VideoFrame::new(
                VideoRotation::VideoRotation0,
                I420Buffer::new_black(160, 96),
            ));
            let frame = timeout(Duration::from_secs(2), frames.recv())
                .await
                .unwrap()
                .unwrap();
            let monitor = shared.monitor(true);
            let demand = shared.demand.reserve().unwrap();
            assert!(shared.demand.activate(demand));
            let snapshot = shared.mic_test.snapshot(Instant::now());
            if mode == "replace" {
                receiver = VideoReceiver::with_audio_runtime(
                    track.clone(),
                    lifetime.clone(),
                    Arc::new(|_| {}),
                    shared.as_ref(),
                );
                receiver.start();
                assert!(receiver.is_playing());
                assert!(receiver.lifetime().is_same(&lifetime));
                receiver.stop();
            } else if mode == "stop" {
                receiver.stop();
            }
            drop(receiver);
            assert!(!frame.lease.is_alive());
            assert!(frame.lease.lifetime.is_same(&lifetime));
            assert!(!task.is_finished());
            assert!(!completed.load(Ordering::Acquire));
            assert!(
                timeout(Duration::from_millis(30), shared.wait_for_cleanup())
                    .await
                    .is_err()
            );
            assert!(!completed.load(Ordering::Acquire));
            release.send(()).unwrap();
            timeout(Duration::from_secs(2), shared.wait_for_cleanup())
                .await
                .unwrap()
                .unwrap();
            assert!(completed.load(Ordering::Acquire));
            assert!(task.is_finished());
            let calls_after_join = calls.load(Ordering::Acquire);
            source.capture_frame(&VideoFrame::new(
                VideoRotation::VideoRotation0,
                I420Buffer::new_black(160, 96),
            ));
            tokio::time::sleep(Duration::from_millis(30)).await;
            assert_eq!(calls.load(Ordering::Acquire), calls_after_join);
            assert!(monitor.is_current());
            assert_eq!(shared.demand.active(), demand);
            assert_eq!(shared.mic_test.snapshot(Instant::now()), snapshot);
            assert_eq!(track.id(), "video-cleanup");
            engine.close();
            println!("video task cleanup held_callback=true mode={mode} abort_not_completion=true original_join_waits_callback_and_future=true stale_frame_lease_invalidated=true post_join_local_delivery_absent=true voice_monitor_unchanged=true native_callback_bounds=false physical_audio=false");
        }
    }

    #[tokio::test]
    async fn sealed_video_start_refuses_native_acquisition_without_replacing_original_subscription()
    {
        for mode in ["configured", "playing", "stopped"] {
            let shared = shared();
            let (mut engine, lifetime, _events) = peer();
            let (_factory, source, track) = native_video();
            let deliveries = Arc::new(AtomicUsize::new(0));
            let delivered = deliveries.clone();
            let mut receiver = VideoReceiver::with_audio_runtime(
                track.clone(),
                lifetime.clone(),
                Arc::new(move |_| {
                    delivered.fetch_add(1, Ordering::AcqRel);
                }),
                shared.as_ref(),
            );
            if mode != "configured" {
                receiver.start();
            }
            if mode == "stopped" {
                receiver.stop();
                timeout(Duration::from_secs(2), shared.wait_for_cleanup())
                    .await
                    .unwrap()
                    .unwrap();
            }
            let original = receiver
                .subscription
                .as_ref()
                .map(|s| (s.lease.clone(), s.task.id(), s.stop.clone()));
            let monitor = shared.monitor(true);
            let demand = shared.demand.reserve().unwrap();
            assert!(shared.demand.activate(demand));
            let snapshot = shared.mic_test.snapshot(Instant::now());
            shared.seal_cleanup().unwrap();
            shared.seal_cleanup().unwrap();
            receiver.start();
            assert_eq!(receiver.subscription.is_some(), mode == "playing");
            if let Some((lease, id, stop)) = original {
                let current = receiver.subscription.as_ref().unwrap();
                assert!(Arc::ptr_eq(&current.lease.active, &lease.active));
                assert_eq!(current.task.id(), id);
                assert!(!stop.is_cancelled());
                assert!(lease.is_alive());
            }
            receiver.stop();
            timeout(Duration::from_secs(2), shared.wait_for_cleanup())
                .await
                .unwrap()
                .unwrap();
            receiver.start();
            assert!(receiver.subscription.is_none());
            source.capture_frame(&VideoFrame::new(
                VideoRotation::VideoRotation0,
                I420Buffer::new_black(160, 96),
            ));
            tokio::task::yield_now().await;
            assert_eq!(deliveries.load(Ordering::Acquire), 0);
            assert!(receiver.lifetime().is_same(&lifetime));
            assert!(lifetime.is_alive());
            assert!(monitor.is_current());
            assert_eq!(shared.demand.active(), demand);
            assert_eq!(shared.mic_test.snapshot(Instant::now()), snapshot);
            engine.close();
            println!("video task cleanup sealed_start=true mode={mode} registration_before_native_acquisition=true original_subscription_untouched=true admitted_stop_joined=true late_start_refused=true voice_monitor_unchanged=true native_callback_bounds=false physical_audio=false");
        }
    }

    #[tokio::test]
    async fn video_subscription_join_reports_panic_and_unrequested_abort_as_sticky_failure() {
        for mode in ["normal", "cancel", "panic", "unexpected-abort"] {
            let cleanup = Arc::new(AudioCleanup::default());
            let (mut engine, lifetime, _events) = peer();
            let lease = VideoFrameLease {
                lifetime,
                active: Arc::new(AtomicBool::new(true)),
            };
            let stop = CancellationToken::new();
            let (ready, started) = tokio::sync::oneshot::channel();
            let task = cleanup
                .prepare_task()
                .unwrap()
                .spawn(stop.clone(), async move {
                    ready.send(()).unwrap();
                    match mode {
                        "normal" => {}
                        "panic" => panic!("Expected video subscription task panic"),
                        _ => std::future::pending::<()>().await,
                    }
                });
            let subscription = VideoSubscription {
                task,
                lease: lease.clone(),
                stop,
            };
            started.await.unwrap();
            if mode == "cancel" {
                drop(subscription);
            } else {
                if mode == "unexpected-abort" {
                    subscription.task.abort();
                }
                let result = timeout(Duration::from_secs(2), cleanup.wait())
                    .await
                    .unwrap();
                assert_eq!(result.is_err(), mode != "normal");
                drop(subscription);
            }
            let result = timeout(Duration::from_secs(2), cleanup.wait())
                .await
                .unwrap();
            assert_eq!(
                result.is_err(),
                matches!(mode, "panic" | "unexpected-abort")
            );
            assert_eq!(cleanup.wait().await.is_err(), result.is_err());
            assert!(!lease.is_alive());
            engine.close();
            println!("video task cleanup join_result=true mode={mode} requested_cancel_only_accepted=true panic_unrequested_abort_sticky=true original_join_observed=true native_callback_bounds=false physical_audio=false");
        }
    }

    #[test]
    fn restarted_stream_cancels_old_drop_deadline_and_repeated_drop_resets_grace() {
        let mut grace = StreamDropGrace::default();
        let start = Instant::now();
        grace.schedule("alice".into(), start);
        grace.schedule("bob".into(), start);
        grace.cancel("alice");
        grace.schedule("bob".into(), start + Duration::from_secs(20));
        assert!(grace
            .take_expired(start + Duration::from_secs(30))
            .is_empty());
        assert_eq!(grace.next_deadline(), Some(start + Duration::from_secs(50)));
        assert_eq!(
            grace.take_expired(start + Duration::from_secs(50)),
            vec!["bob"]
        );
        assert_eq!(grace.next_deadline(), None);
        grace.schedule("alice".into(), start);
        grace.clear();
        assert!(grace
            .take_expired(start + Duration::from_secs(60))
            .is_empty());
    }

    #[test]
    fn received_frames_are_bounded_and_camera_rotation_is_preserved() {
        let buffer = I420Buffer::new_black(320, 180);
        let frame = VideoFrame::new(
            VideoRotation::VideoRotation90,
            Box::new(buffer) as Box<dyn VideoBuffer>,
        );
        let decoded = decode_frame(frame).unwrap();
        assert_eq!((decoded.width, decoded.height), (180, 320));
        assert_eq!(decoded.rgba.len(), 180 * 320 * 4);
        assert!(decoded
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 255));
    }
}
