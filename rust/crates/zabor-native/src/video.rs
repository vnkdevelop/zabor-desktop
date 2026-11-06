use crate::media::MediaError;
use libwebrtc::video_frame::{
    native::VideoFrameBufferExt, BoxVideoFrame, VideoBuffer, VideoFormatType, VideoRotation,
};

pub struct DecodedVideo {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
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
