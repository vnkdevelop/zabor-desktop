#[cfg(windows)]
#[cxx::bridge(namespace = "zabor::camera")]
mod ffi {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct OwnerStatus {
        opening: bool,
        ready: bool,
        failed: bool,
        finished: bool,
        cleanup_failed: bool,
        owner_thread_consistent: bool,
        callbacks_released: bool,
    }

    #[derive(Debug, Default)]
    struct Frame {
        width: u32,
        height: u32,
        bgra: Vec<u8>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Device {
        id: String,
        name: String,
    }

    unsafe extern "C++" {
        include!("camera.h");
        fn begin(device: &str, enumerate: bool) -> Result<u64>;
        fn status(owner: u64) -> OwnerStatus;
        fn take_frame(owner: u64) -> Frame;
        fn take_devices(owner: u64) -> Vec<Device>;
        fn cancel(owner: u64);
        fn release(owner: u64);
        #[cfg(feature = "test-source")]
        fn begin_test(mode: u8, delay_ms: u32) -> Result<u64>;
        #[cfg(feature = "test-source")]
        fn test_copy_frame(
            data: &[u8],
            width: u32,
            height: u32,
            stride: i32,
            top: usize,
        ) -> Result<Frame>;
    }

    extern "Rust" {
        fn allocate_pixels(size: usize) -> Vec<u8>;
    }
}

#[cfg(windows)]
fn allocate_pixels(size: usize) -> Vec<u8> {
    if size > 3840 * 2160 * 4 {
        return Vec::new();
    }
    vec![0; size]
}

#[cfg(all(windows, feature = "test-source"))]
pub fn copy_synthetic_frame(
    data: &[u8],
    width: u32,
    height: u32,
    stride: i32,
    top: usize,
) -> Result<Frame, CameraError> {
    ffi::test_copy_frame(data, width, height, stride, top).map_err(|_| CameraError)
}

#[cfg(windows)]
pub use ffi::{Device, Frame, OwnerStatus};

#[derive(Debug, thiserror::Error)]
#[error("native camera is unavailable")]
pub struct CameraError;

#[cfg(windows)]
struct Owner(u64);

#[cfg(windows)]
impl Drop for Owner {
    fn drop(&mut self) {
        ffi::release(self.0);
    }
}

#[cfg(windows)]
pub struct Camera {
    owner: std::sync::Arc<Owner>,
}

#[cfg(windows)]
#[derive(Clone)]
pub struct Completion {
    owner: std::sync::Arc<Owner>,
}

#[cfg(windows)]
impl Camera {
    pub fn open(device: &str) -> Result<Self, CameraError> {
        if device.len() > 32_768 || device.contains('\0') {
            return Err(CameraError);
        }
        Self::begin(device, false)
    }

    pub fn enumerate() -> Result<Self, CameraError> {
        Self::begin("", true)
    }

    fn begin(device: &str, enumerate: bool) -> Result<Self, CameraError> {
        ffi::begin(device, enumerate)
            .map(|owner| Self {
                owner: std::sync::Arc::new(Owner(owner)),
            })
            .map_err(|_| CameraError)
    }

    pub fn status(&self) -> OwnerStatus {
        ffi::status(self.owner.0)
    }

    pub fn take_frame(&self) -> Option<Frame> {
        let frame = ffi::take_frame(self.owner.0);
        (!frame.bgra.is_empty()).then_some(frame)
    }

    pub fn take_devices(&self) -> Vec<Device> {
        ffi::take_devices(self.owner.0)
    }

    pub fn cancel(&self) {
        ffi::cancel(self.owner.0);
    }

    pub fn completion(&self) -> Completion {
        Completion {
            owner: self.owner.clone(),
        }
    }

    #[cfg(feature = "test-source")]
    pub fn synthetic(mode: u8, delay_ms: u32) -> Result<Self, CameraError> {
        ffi::begin_test(mode, delay_ms)
            .map(|owner| Self {
                owner: std::sync::Arc::new(Owner(owner)),
            })
            .map_err(|_| CameraError)
    }

    #[cfg(feature = "test-source")]
    pub fn synthetic_video(mode: u8, delay_ms: u32) -> Result<Self, CameraError> {
        if mode > 5 {
            return Err(CameraError);
        }
        Self::synthetic(mode | 128, delay_ms)
    }
}

#[cfg(windows)]
impl Drop for Camera {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(windows)]
impl Completion {
    pub fn status(&self) -> OwnerStatus {
        ffi::status(self.owner.0)
    }
}

#[cfg(all(test, windows, feature = "test-source"))]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn camera(mode: u8, delay_ms: u32) -> Camera {
        Camera::synthetic(mode, delay_ms).unwrap()
    }

    fn until(mut predicate: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !predicate() {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
    }

    fn complete(completion: &Completion) -> OwnerStatus {
        until(|| completion.status().finished);
        let status = completion.status();
        assert!(status.owner_thread_consistent);
        assert!(status.callbacks_released);
        status
    }

    #[test]
    fn synthetic_frames_are_owned_bounded_and_stopped_without_driver_calls_on_caller() {
        let camera = camera(0, 0);
        until(|| camera.status().ready);
        let first = camera.take_frame().unwrap();
        assert_eq!((first.width, first.height), (4, 2));
        assert_eq!(first.bgra.len(), 32);
        until(|| camera.take_frame().is_some());
        assert_eq!(first.bgra, [19, 37, 71, 255].repeat(8));
        let completion = camera.completion();
        camera.cancel();
        assert!(camera.take_frame().is_none());
        drop(camera);
        assert!(!complete(&completion).cleanup_failed);
    }

    #[test]
    fn cancellation_during_delayed_open_retains_completion_through_resource_release() {
        let camera = camera(1, 150);
        until(|| camera.status().opening);
        let completion = camera.completion();
        let started = Instant::now();
        drop(camera);
        assert!(started.elapsed() < Duration::from_millis(100));
        let status = complete(&completion);
        assert!(!status.ready);
        assert!(!status.failed);
        assert!(!status.cleanup_failed);
    }

    #[test]
    fn completion_waits_for_delayed_callbacks_and_native_close() {
        let camera = camera(2, 150);
        until(|| camera.status().ready);
        let completion = camera.completion();
        let started = Instant::now();
        drop(camera);
        assert!(!completion.status().finished);
        assert!(!complete(&completion).cleanup_failed);
        assert!(started.elapsed() >= Duration::from_millis(140));
    }

    #[test]
    fn failed_open_and_read_never_publish_after_failure_and_still_release_owner() {
        for mode in [3, 4] {
            let camera = camera(mode, 0);
            until(|| camera.status().failed);
            assert!(camera.take_frame().is_none());
            let completion = camera.completion();
            drop(camera);
            let status = complete(&completion);
            assert!(status.failed);
            assert!(!status.cleanup_failed);
        }
    }

    #[test]
    fn failed_native_cleanup_is_not_reported_as_successful_completion() {
        let camera = camera(5, 0);
        until(|| camera.status().ready);
        let completion = camera.completion();
        drop(camera);
        assert!(complete(&completion).cleanup_failed);
    }

    #[test]
    fn distinct_owners_do_not_cancel_or_consume_each_others_frames() {
        let first = camera(0, 0);
        let second = camera(0, 0);
        until(|| first.status().ready && second.status().ready);
        let completion = first.completion();
        drop(first);
        complete(&completion);
        assert!(second.status().ready);
        assert!(!second.status().finished);
        assert!(second.take_frame().is_some());
        let completion = second.completion();
        drop(second);
        complete(&completion);
    }

    #[test]
    fn invalid_device_strings_are_rejected_without_enumeration_or_native_acquisition() {
        assert!(Camera::open("invalid\0device").is_err());
        assert!(Camera::open(&"x".repeat(32_769)).is_err());
    }

    #[test]
    fn safe_handle_and_completion_are_send_and_sync_without_com_pointer_transfer() {
        fn send_sync<T: Send + Sync>() {}
        send_sync::<Camera>();
        send_sync::<Completion>();
        let camera = camera(0, 0);
        let completion = camera.completion();
        std::thread::spawn(move || drop(camera)).join().unwrap();
        complete(&completion);
    }

    #[test]
    fn rgb32_copy_preserves_padded_top_down_and_bottom_up_rows_with_owned_alpha() {
        let data = [
            1, 2, 3, 0, 4, 5, 6, 0, 91, 92, 93, 94, 7, 8, 9, 0, 10, 11, 12, 0,
        ];
        let first = copy_synthetic_frame(&data, 2, 2, 12, 0).unwrap();
        assert_eq!((first.width, first.height), (2, 2));
        assert_eq!(
            first.bgra,
            [1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255]
        );
        let reversed = copy_synthetic_frame(&data, 2, 2, -12, 12).unwrap();
        assert_eq!(
            reversed.bgra,
            [7, 8, 9, 255, 10, 11, 12, 255, 1, 2, 3, 255, 4, 5, 6, 255]
        );
    }

    #[test]
    fn rgb32_copy_rejects_truncation_overflow_offsets_strides_and_unbounded_dimensions() {
        for (width, height, stride, top) in [
            (0, 1, 4, 0),
            (1, 0, 4, 0),
            (3841, 1, 15364, 0),
            (1, 2161, 4, 0),
            (2, 2, 4, 0),
            (2, 2, 12, 0),
            (1, 2, -4, 0),
            (1, 1, 4, usize::MAX),
            (1, 1, i32::MIN, 0),
        ] {
            assert!(copy_synthetic_frame(&[0; 16], width, height, stride, top).is_err());
        }
    }
}
