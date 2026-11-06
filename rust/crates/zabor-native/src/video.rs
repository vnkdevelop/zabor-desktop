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
    task: tokio::task::JoinHandle<()>,
    lease: VideoFrameLease,
}

impl Drop for VideoSubscription {
    fn drop(&mut self) {
        self.lease.active.store(false, Ordering::Release);
        self.task.abort();
    }
}

pub struct VideoReceiver {
    track: RtcVideoTrack,
    lifetime: PeerLifetime,
    subscription: Option<VideoSubscription>,
    sink: Arc<dyn Fn(VideoDelivery) + Send + Sync>,
}

impl VideoReceiver {
    pub fn new(
        track: RtcVideoTrack,
        lifetime: PeerLifetime,
        sink: Arc<dyn Fn(VideoDelivery) + Send + Sync>,
    ) -> Self {
        Self {
            track,
            lifetime,
            subscription: None,
            sink,
        }
    }

    pub fn is_alive(&self) -> bool {
        self.lifetime.is_alive()
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
        self.stop();
        let lease = VideoFrameLease {
            lifetime: self.lifetime.clone(),
            active: Arc::new(AtomicBool::new(true)),
        };
        let live = lease.clone();
        let sink = self.sink.clone();
        let mut stream = NativeVideoStream::new(self.track.clone());
        let task = tokio::spawn(async move {
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
        self.subscription = Some(VideoSubscription { task, lease });
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use libwebrtc::video_frame::{I420Buffer, VideoFrame};

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
