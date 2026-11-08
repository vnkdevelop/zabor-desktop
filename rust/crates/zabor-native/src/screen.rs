use crate::media::MediaError;
use libwebrtc::desktop_capturer::{
    CaptureError, DesktopCaptureSourceType, DesktopCapturer, DesktopCapturerOptions,
};
use libwebrtc::native::yuv_helper;
use libwebrtc::video_frame::{I420Buffer, VideoFrame, VideoRotation};
use libwebrtc::video_source::{native::NativeVideoSource, VideoResolution};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
pub enum SourceKind {
    Screen,
    Window,
}

#[derive(Clone)]
pub struct ScreenSource {
    pub id: u64,
    pub title: String,
    pub kind: SourceKind,
}

impl ScreenSource {
    pub fn key(&self) -> String {
        let kind = match self.kind {
            SourceKind::Screen => "screen",
            SourceKind::Window => "window",
        };
        format!("{kind}:{}", self.id)
    }

    pub fn display_title(&self, index: usize, screen_label: &str, window_label: &str) -> String {
        match self.kind {
            SourceKind::Screen => format!("{screen_label} {}", index + 1),
            SourceKind::Window => {
                let title = self.title.rsplit(" - ").next().unwrap_or_default().trim();
                if title.is_empty() {
                    window_label.to_owned()
                } else {
                    title.to_owned()
                }
            }
        }
    }
}

pub fn capture_format(quality: &str) -> Result<(u32, u32, u32), MediaError> {
    match quality {
        "low" => Ok((1280, 720, 30)),
        "medium" => Ok((1280, 720, 60)),
        "high" => Ok((1920, 1080, 60)),
        _ => Err(MediaError::Backend),
    }
}

pub struct ScreenCapture {
    pub source: NativeVideoSource,
    stop: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ScreenCapture {
    pub fn sources() -> Vec<ScreenSource> {
        let mut sources = Vec::new();
        for (kind, source_type) in [
            (SourceKind::Screen, DesktopCaptureSourceType::Screen),
            (SourceKind::Window, DesktopCaptureSourceType::Window),
        ] {
            if let Some(capture) = DesktopCapturer::new(DesktopCapturerOptions::new(source_type)) {
                sources.extend(
                    capture
                        .get_source_list()
                        .into_iter()
                        .map(|source| ScreenSource {
                            id: source.id(),
                            title: source.title(),
                            kind,
                        }),
                );
            }
        }
        sources
    }

    pub fn start(
        selected: ScreenSource,
        width: u32,
        height: u32,
        fps: u32,
        cursor: bool,
    ) -> Result<Self, MediaError> {
        if width == 0 || height == 0 || width > 3840 || height > 2160 || !(1..=60).contains(&fps) {
            return Err(MediaError::Backend);
        }
        let source = NativeVideoSource::new(VideoResolution { width, height }, true);
        let stop = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker_failed = failed.clone();
        let frames = source.clone();
        let (ready, status) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("zabor-screen".into())
            .spawn(move || {
                let source_type = match selected.kind {
                    SourceKind::Screen => DesktopCaptureSourceType::Screen,
                    SourceKind::Window => DesktopCaptureSourceType::Window,
                };
                let mut options = DesktopCapturerOptions::new(source_type);
                options.set_include_cursor(cursor);
                let Some(mut capturer) = DesktopCapturer::new(options) else {
                    let _ = ready.send(false);
                    return;
                };
                let Some(selected) = capturer
                    .get_source_list()
                    .into_iter()
                    .find(|source| source.id() == selected.id)
                else {
                    let _ = ready.send(false);
                    return;
                };
                let mut ready = Some(ready);
                let callback_stop = worker_stop.clone();
                capturer.start_capture(Some(selected), move |result| match result {
                    Ok(frame) => {
                        match bgra_frame(
                            frame.data(),
                            frame.stride(),
                            frame.width(),
                            frame.height(),
                            width,
                            height,
                        ) {
                            Ok(buffer) => {
                                frames.capture_frame(&VideoFrame::new(
                                    VideoRotation::VideoRotation0,
                                    buffer,
                                ));
                                if let Some(ready) = ready.take() {
                                    let _ = ready.send(true);
                                }
                            }
                            Err(_) => {
                                worker_failed.store(true, Ordering::Release);
                                callback_stop.store(true, Ordering::Release);
                            }
                        }
                    }
                    Err(CaptureError::Permanent) => {
                        worker_failed.store(true, Ordering::Release);
                        callback_stop.store(true, Ordering::Release);
                        if let Some(ready) = ready.take() {
                            let _ = ready.send(false);
                        }
                    }
                    Err(CaptureError::Temporary) => {}
                });
                let period = Duration::from_secs_f64(1.0 / f64::from(fps));
                while !worker_stop.load(Ordering::Acquire) {
                    let start = Instant::now();
                    capturer.capture_frame();
                    std::thread::park_timeout(period.saturating_sub(start.elapsed()));
                }
            })
            .map_err(|_| MediaError::Backend)?;
        let capture = Self {
            source,
            stop,
            failed,
            thread: Some(thread),
        };
        match status.recv_timeout(Duration::from_secs(5)) {
            Ok(true) => Ok(capture),
            _ => Err(MediaError::Backend),
        }
    }

    pub fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }
}

impl Drop for ScreenCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}

pub fn bgra_frame(
    bytes: &[u8],
    stride: u32,
    width: i32,
    height: i32,
    max_width: u32,
    max_height: u32,
) -> Result<I420Buffer, MediaError> {
    if width <= 0
        || height <= 0
        || width > 8192
        || height > 8192
        || max_width == 0
        || max_height == 0
        || stride < width as u32 * 4
        || stride as usize * height as usize > bytes.len()
    {
        return Err(MediaError::Backend);
    }
    let mut buffer = I420Buffer::new(width as u32, height as u32);
    let (stride_y, stride_u, stride_v) = buffer.strides();
    let (y, u, v) = buffer.data_mut();
    yuv_helper::argb_to_i420(
        bytes, stride, y, stride_y, u, stride_u, v, stride_v, width, height,
    );
    let scale = (f64::from(max_width) / f64::from(width))
        .min(f64::from(max_height) / f64::from(height))
        .min(1.0);
    if scale < 1.0 {
        let scaled_width = ((f64::from(width) * scale) as i32 / 2 * 2).max(2);
        let scaled_height = ((f64::from(height) * scale) as i32 / 2 * 2).max(2);
        Ok(buffer.scale(scaled_width, scaled_height))
    } else {
        Ok(buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libwebrtc::video_frame::VideoBuffer;

    #[test]
    fn source_keys_keep_screen_and_window_identity_distinct_and_titles_match_picker_rules() {
        let screen = ScreenSource {
            id: 42,
            title: "device-specific monitor name".into(),
            kind: SourceKind::Screen,
        };
        let window = ScreenSource {
            id: 42,
            title: "document - editor - application ".into(),
            kind: SourceKind::Window,
        };
        assert_eq!(screen.key(), "screen:42");
        assert_eq!(window.key(), "window:42");
        assert_eq!(screen.display_title(0, "экран", "приложения"), "экран 1");
        assert_eq!(screen.display_title(1, "screen", "apps"), "screen 2");
        assert_eq!(window.display_title(4, "screen", "apps"), "application");
        let empty = ScreenSource {
            title: "  ".into(),
            ..window
        };
        assert_eq!(empty.display_title(0, "screen", "apps"), "apps");
    }

    #[test]
    fn capture_quality_preserves_low_high_and_remote_control_medium_dimensions() {
        assert_eq!(capture_format("low").unwrap(), (1280, 720, 30));
        assert_eq!(capture_format("medium").unwrap(), (1280, 720, 60));
        assert_eq!(capture_format("high").unwrap(), (1920, 1080, 60));
        for invalid in ["", "camera", "HIGH", "unknown", "1920x1080"] {
            assert!(capture_format(invalid).is_err());
        }
    }

    #[test]
    fn padded_desktop_pixels_preserve_aspect_ratio_and_validate_stride() {
        let mut pixels = vec![0_u8; 80 * 8];
        for row in pixels.chunks_mut(80) {
            for pixel in row[..64].chunks_mut(4) {
                pixel.copy_from_slice(&[0, 0, 255, 255]);
            }
        }
        let buffer = bgra_frame(&pixels, 80, 16, 8, 8, 8).unwrap();
        assert_eq!((buffer.width(), buffer.height()), (8, 4));
        assert!(buffer
            .data()
            .0
            .iter()
            .all(|value| (75..=90).contains(value)));
        assert!(bgra_frame(&pixels[..20], 80, 16, 8, 8, 8).is_err());
        assert!(bgra_frame(&pixels, 32, 16, 8, 8, 8).is_err());
    }
}
