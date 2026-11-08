use base64::Engine;
use image::{AnimationDecoder, ImageDecoder, ImageFormat, Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read};
use std::path::Path;
use tokio_util::sync::CancellationToken;

pub const MAX_FILE_BYTES: usize = 5 * 1024 * 1024;
pub const MAX_WIRE_BYTES: usize = MAX_FILE_BYTES.div_ceil(3) * 4 + 1024;
const MAX_SOURCE_BYTES: usize = 64 * 1024 * 1024;
const MAX_AVATAR_BYTES: usize = 32 * 1024 * 1024;
const MAX_FRAMES: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Crop {
    pub scale: f64,
    pub x: f64,
    pub y: f64,
}

impl Default for Crop {
    fn default() -> Self {
        Self {
            scale: 1.0,
            x: 0.0,
            y: 0.0,
        }
    }
}

impl Crop {
    pub fn valid(self) -> bool {
        self.scale.is_finite()
            && (0.5..=5.0).contains(&self.scale)
            && self.x.is_finite()
            && self.y.is_finite()
            && self.x.abs() <= 1_000_000.0
            && self.y.abs() <= 1_000_000.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AvatarError {
    #[error("avatar exceeds resource limits")]
    TooLarge,
    #[error("invalid avatar image")]
    Invalid,
    #[error("avatar work cancelled")]
    Cancelled,
}

impl AvatarError {
    pub fn key(self) -> &'static str {
        match self {
            Self::TooLarge => "common.fileTooLarge",
            Self::Invalid | Self::Cancelled => "common.onlyImages",
        }
    }
}

pub struct Frame {
    pub pixels: RgbaImage,
    pub delay_ms: u64,
}

pub struct Source {
    pub frames: Vec<Frame>,
    gif: Option<String>,
    loops: Option<u32>,
}

pub struct Avatar {
    pub frames: Vec<Frame>,
    loops: Option<u32>,
}

pub struct Selection {
    pub wire: String,
    pub color: String,
    pub avatar: Avatar,
}

#[derive(Serialize, Deserialize)]
struct PackedGif {
    g: String,
    #[serde(default = "one")]
    s: f64,
    #[serde(default)]
    x: f64,
    #[serde(default)]
    y: f64,
}

fn one() -> f64 {
    1.0
}

fn cancelled(stop: &CancellationToken) -> Result<(), AvatarError> {
    if stop.is_cancelled() {
        Err(AvatarError::Cancelled)
    } else {
        Ok(())
    }
}

fn limits() -> image::Limits {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(96 * 1024 * 1024);
    limits
}

impl Source {
    pub fn frame_index(&self, elapsed_ms: u64, reduced_motion: bool) -> usize {
        frame_index(&self.frames, self.loops, elapsed_ms, reduced_motion)
    }
    pub fn from_file(path: &Path, stop: &CancellationToken) -> Result<Self, AvatarError> {
        cancelled(stop)?;
        let file = std::fs::File::open(path).map_err(|_| AvatarError::Invalid)?;
        let mut bytes = Vec::new();
        file.take((MAX_FILE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| AvatarError::Invalid)?;
        Self::from_bytes(&bytes, stop)
    }

    pub fn from_bytes(bytes: &[u8], stop: &CancellationToken) -> Result<Self, AvatarError> {
        cancelled(stop)?;
        if bytes.len() > MAX_FILE_BYTES {
            return Err(AvatarError::TooLarge);
        }
        let format = image::guess_format(bytes).map_err(|_| AvatarError::Invalid)?;
        if !matches!(
            format,
            ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif | ImageFormat::WebP
        ) {
            return Err(AvatarError::Invalid);
        }
        let mut frames = Vec::new();
        let mut loops = None;
        if format == ImageFormat::Gif {
            let mut decoder = image::codecs::gif::GifDecoder::new(Cursor::new(bytes))
                .map_err(|_| AvatarError::Invalid)?;
            decoder
                .set_limits(limits())
                .map_err(|_| AvatarError::TooLarge)?;
            if u64::from(decoder.dimensions().0) * u64::from(decoder.dimensions().1)
                > 4 * 1024 * 1024
            {
                return Err(AvatarError::TooLarge);
            }
            loops = gif_loops(bytes)?;
            let mut retained = 0;
            for frame in decoder.into_frames() {
                cancelled(stop)?;
                if frames.len() == MAX_FRAMES {
                    return Err(AvatarError::TooLarge);
                }
                let frame = frame.map_err(|_| AvatarError::Invalid)?;
                let (numerator, denominator) = frame.delay().numer_denom_ms();
                let delay = u64::from(numerator) / u64::from(denominator.max(1));
                let pixels = frame.into_buffer();
                retained += pixels.as_raw().len();
                if retained > MAX_SOURCE_BYTES {
                    return Err(AvatarError::TooLarge);
                }
                frames.push(Frame {
                    pixels,
                    delay_ms: if delay <= 10 { 100 } else { delay },
                });
            }
        } else {
            let mut reader = image::ImageReader::with_format(Cursor::new(bytes), format);
            reader.limits(limits());
            let mut decoder = reader.into_decoder().map_err(|_| AvatarError::Invalid)?;
            if decoder.total_bytes() > MAX_SOURCE_BYTES as u64 {
                return Err(AvatarError::TooLarge);
            }
            let orientation = decoder.orientation().map_err(|_| AvatarError::Invalid)?;
            let mut image =
                image::DynamicImage::from_decoder(decoder).map_err(|_| AvatarError::Invalid)?;
            image.apply_orientation(orientation);
            frames.push(Frame {
                pixels: image.into_rgba8(),
                delay_ms: 100,
            });
        }
        cancelled(stop)?;
        if frames.is_empty() {
            return Err(AvatarError::Invalid);
        }
        Ok(Self {
            frames,
            loops,
            gif: (format == ImageFormat::Gif).then(|| {
                format!(
                    "data:image/gif;base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(bytes)
                )
            }),
        })
    }

    pub fn select(&self, crop: Crop, stop: &CancellationToken) -> Result<Selection, AvatarError> {
        self.select_frame(crop, 0, stop)
    }

    pub fn select_frame(
        &self,
        crop: Crop,
        frame: usize,
        stop: &CancellationToken,
    ) -> Result<Selection, AvatarError> {
        cancelled(stop)?;
        if !crop.valid() {
            return Err(AvatarError::Invalid);
        }
        let selected = self.frames.get(frame).ok_or(AvatarError::Invalid)?;
        let square = render(&selected.pixels, crop, 400, true, false, stop)?;
        let color = dominant_color(&square);
        let wire = if let Some(gif) = &self.gif {
            serde_json::to_string(&PackedGif {
                g: gif.clone(),
                s: js_round(crop.scale, 1000.0),
                x: js_round(crop.x, 10.0),
                y: js_round(crop.y, 10.0),
            })
            .map_err(|_| AvatarError::Invalid)?
        } else {
            let mut bytes = Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(square)
                .write_to(&mut bytes, ImageFormat::Png)
                .map_err(|_| AvatarError::Invalid)?;
            format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())
            )
        };
        let display_crop = if self.gif.is_some() {
            Crop {
                scale: js_round(crop.scale, 1000.0),
                x: js_round(crop.x, 10.0),
                y: js_round(crop.y, 10.0),
            }
        } else {
            crop
        };
        let avatar = self.presentation(display_crop, self.gif.is_none(), false, stop)?;
        cancelled(stop)?;
        Ok(Selection {
            wire,
            color,
            avatar,
        })
    }

    fn presentation(
        &self,
        crop: Crop,
        black: bool,
        cover: bool,
        stop: &CancellationToken,
    ) -> Result<Avatar, AvatarError> {
        if self.frames.len() * 200 * 200 * 4 > MAX_AVATAR_BYTES {
            return Err(AvatarError::TooLarge);
        }
        let frames = self
            .frames
            .iter()
            .map(|frame| {
                let mut pixels = render(&frame.pixels, crop, 200, black, cover, stop)?;
                circle(&mut pixels);
                Ok(Frame {
                    pixels,
                    delay_ms: frame.delay_ms,
                })
            })
            .collect::<Result<Vec<_>, AvatarError>>()?;
        Ok(Avatar {
            frames,
            loops: self.loops,
        })
    }
}

impl Avatar {
    pub fn from_wire(wire: &str, stop: &CancellationToken) -> Result<Self, AvatarError> {
        cancelled(stop)?;
        if wire.len() > MAX_WIRE_BYTES {
            return Err(AvatarError::TooLarge);
        }
        let packed = if wire.starts_with('{') {
            Some(serde_json::from_str::<PackedGif>(wire).map_err(|_| AvatarError::Invalid)?)
        } else {
            None
        };
        let data = packed.as_ref().map_or(wire, |packed| packed.g.as_str());
        let (prefix, encoded) = data.split_once(',').ok_or(AvatarError::Invalid)?;
        if !matches!(
            prefix,
            "data:image/png;base64"
                | "data:image/jpeg;base64"
                | "data:image/gif;base64"
                | "data:image/webp;base64"
        ) || (packed.is_some() && prefix != "data:image/gif;base64")
        {
            return Err(AvatarError::Invalid);
        }
        let crop = packed.as_ref().map_or(Crop::default(), |packed| Crop {
            scale: packed.s,
            x: packed.x,
            y: packed.y,
        });
        if !crop.valid() {
            return Err(AvatarError::Invalid);
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| AvatarError::Invalid)?;
        let source = Source::from_bytes(&bytes, stop)?;
        if packed.is_some() && source.gif.is_none() {
            return Err(AvatarError::Invalid);
        }
        source.presentation(crop, false, packed.is_none(), stop)
    }

    pub fn bytes(&self) -> usize {
        self.frames
            .iter()
            .map(|frame| frame.pixels.as_raw().len())
            .sum()
    }

    pub fn frame_index(&self, elapsed_ms: u64, reduced_motion: bool) -> usize {
        frame_index(&self.frames, self.loops, elapsed_ms, reduced_motion)
    }
}

fn frame_index(
    frames: &[Frame],
    loops: Option<u32>,
    elapsed_ms: u64,
    reduced_motion: bool,
) -> usize {
    if reduced_motion || frames.len() == 1 {
        return 0;
    }
    let duration: u64 = frames.iter().map(|frame| frame.delay_ms).sum();
    if duration == 0 {
        return 0;
    }
    if loops.is_some_and(|loops| elapsed_ms / duration >= u64::from(loops)) {
        return frames.len() - 1;
    }
    let mut position = elapsed_ms % duration;
    for (index, frame) in frames.iter().enumerate() {
        if position < frame.delay_ms {
            return index;
        }
        position -= frame.delay_ms;
    }
    0
}

fn js_round(value: f64, precision: f64) -> f64 {
    (value * precision + 0.5).floor() / precision
}

fn gif_loops(bytes: &[u8]) -> Result<Option<u32>, AvatarError> {
    if bytes.len() < 13 {
        return Err(AvatarError::Invalid);
    }
    let table_bytes = |flags: u8| {
        if flags & 0x80 != 0 {
            3usize << ((flags & 7) + 1)
        } else {
            0
        }
    };
    let mut offset = 13 + table_bytes(bytes[10]);
    let mut loops = Some(1);
    let subblocks = |mut offset: usize| -> Result<usize, AvatarError> {
        loop {
            let size = usize::from(*bytes.get(offset).ok_or(AvatarError::Invalid)?);
            offset += 1;
            if size == 0 {
                return Ok(offset);
            }
            offset = offset
                .checked_add(size)
                .filter(|offset| *offset <= bytes.len())
                .ok_or(AvatarError::Invalid)?;
        }
    };
    while offset < bytes.len() {
        match bytes[offset] {
            0x3b => return Ok(loops),
            0x21 => {
                let label = *bytes.get(offset + 1).ok_or(AvatarError::Invalid)?;
                offset += 2;
                if label == 0xff {
                    let size = usize::from(*bytes.get(offset).ok_or(AvatarError::Invalid)?);
                    let name = bytes
                        .get(offset + 1..offset + 1 + size)
                        .ok_or(AvatarError::Invalid)?;
                    let data = offset + 1 + size;
                    if matches!(name, b"NETSCAPE2.0" | b"ANIMEXTS1.0")
                        && bytes.get(data..data + 2) == Some(&[3, 1])
                    {
                        let raw = bytes.get(data + 2..data + 4).ok_or(AvatarError::Invalid)?;
                        let repeats = u16::from_le_bytes([raw[0], raw[1]]);
                        loops = if repeats == 0 {
                            None
                        } else {
                            Some(u32::from(repeats) + 1)
                        };
                    }
                }
                offset = subblocks(offset)?;
            }
            0x2c => {
                let flags = *bytes.get(offset + 9).ok_or(AvatarError::Invalid)?;
                offset += 10 + table_bytes(flags) + 1;
                offset = subblocks(offset)?;
            }
            0 => offset += 1,
            _ => return Err(AvatarError::Invalid),
        }
    }
    Err(AvatarError::Invalid)
}

fn render(
    source: &RgbaImage,
    crop: Crop,
    size: u32,
    black: bool,
    cover: bool,
    stop: &CancellationToken,
) -> Result<RgbaImage, AvatarError> {
    cancelled(stop)?;
    let width = f64::from(source.width());
    let height = f64::from(source.height());
    if width == 0.0 || height == 0.0 {
        return Err(AvatarError::Invalid);
    }
    let ratio = if cover {
        (f64::from(size) / width).max(f64::from(size) / height)
    } else {
        (f64::from(size) / width).min(f64::from(size) / height)
    } * crop.scale;
    let left = f64::from(size) / 2.0 + crop.x * f64::from(size) / 200.0 - width * ratio / 2.0;
    let top = f64::from(size) / 2.0 + crop.y * f64::from(size) / 200.0 - height * ratio / 2.0;
    let mut result =
        RgbaImage::from_pixel(size, size, Rgba([0, 0, 0, if black { 255 } else { 0 }]));
    for y in 0..size {
        cancelled(stop)?;
        for x in 0..size {
            let sx = (f64::from(x) + 0.5 - left) / ratio - 0.5;
            let sy = (f64::from(y) + 0.5 - top) / ratio - 0.5;
            if sx < -0.5 || sy < -0.5 || sx >= width - 0.5 || sy >= height - 0.5 {
                continue;
            }
            let mut sample = [0.0; 4];
            for (ox, oy, weight) in [
                (
                    0.0,
                    0.0,
                    (1.0 - sx.fract().rem_euclid(1.0)) * (1.0 - sy.fract().rem_euclid(1.0)),
                ),
                (
                    1.0,
                    0.0,
                    sx.fract().rem_euclid(1.0) * (1.0 - sy.fract().rem_euclid(1.0)),
                ),
                (
                    0.0,
                    1.0,
                    (1.0 - sx.fract().rem_euclid(1.0)) * sy.fract().rem_euclid(1.0),
                ),
                (
                    1.0,
                    1.0,
                    sx.fract().rem_euclid(1.0) * sy.fract().rem_euclid(1.0),
                ),
            ] {
                let pixel = source.get_pixel(
                    (sx.floor() + ox).clamp(0.0, width - 1.0) as u32,
                    (sy.floor() + oy).clamp(0.0, height - 1.0) as u32,
                );
                let alpha = f64::from(pixel[3]) / 255.0;
                for channel in 0..3 {
                    sample[channel] += f64::from(pixel[channel]) * alpha * weight;
                }
                sample[3] += f64::from(pixel[3]) * weight;
            }
            let alpha = sample[3] / 255.0;
            let divisor = if black || alpha == 0.0 { 1.0 } else { alpha };
            result.put_pixel(
                x,
                y,
                Rgba([
                    (sample[0] / divisor).round() as u8,
                    (sample[1] / divisor).round() as u8,
                    (sample[2] / divisor).round() as u8,
                    if black { 255 } else { sample[3].round() as u8 },
                ]),
            );
        }
    }
    Ok(result)
}

fn circle(image: &mut RgbaImage) {
    let radius = f64::from(image.width()) / 2.0;
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        let distance = (f64::from(x) + 0.5 - radius).hypot(f64::from(y) + 0.5 - radius);
        pixel[3] = (f64::from(pixel[3]) * (radius - distance).clamp(0.0, 1.0)).round() as u8;
    }
}

pub fn dominant_color(image: &RgbaImage) -> String {
    let mut clusters = Vec::<([u8; 3], [u64; 4])>::new();
    let eligible = |pixel: &&Rgba<u8>| {
        pixel[3] >= 128
            && !pixel.0[..3].iter().all(|v| *v < 20)
            && !pixel.0[..3].iter().all(|v| *v > 235)
    };
    let saturated = |pixel: &Rgba<u8>| {
        let max = f64::from(*pixel.0[..3].iter().max().unwrap());
        let min = f64::from(*pixel.0[..3].iter().min().unwrap());
        let brightness = (max + min) / 2.0;
        max != min
            && (max - min) / (255.0 - (max + min - 255.0).abs()) > 0.3
            && brightness > 40.0
            && brightness < 220.0
    };
    let use_saturated = image.pixels().step_by(4).filter(eligible).any(saturated);
    for pixel in image
        .pixels()
        .step_by(4)
        .filter(eligible)
        .filter(|pixel| !use_saturated || saturated(pixel))
    {
        let key = [
            pixel[0] / 32 + u8::from(pixel[0] % 32 >= 16),
            pixel[1] / 32 + u8::from(pixel[1] % 32 >= 16),
            pixel[2] / 32 + u8::from(pixel[2] % 32 >= 16),
        ];
        let index = clusters
            .iter()
            .position(|(cluster, _)| *cluster == key)
            .unwrap_or_else(|| {
                clusters.push((key, [0; 4]));
                clusters.len() - 1
            });
        let sum = &mut clusters[index].1;
        sum[0] += 1;
        for i in 0..3 {
            sum[i + 1] += u64::from(pixel[i]);
        }
    }
    let mut dominant = None;
    let mut count = 0;
    for (_, sum) in clusters {
        if sum[0] > count {
            count = sum[0];
            dominant = Some([
                js_round(sum[1] as f64 / count as f64, 1.0) / 255.0,
                js_round(sum[2] as f64 / count as f64, 1.0) / 255.0,
                js_round(sum[3] as f64 / count as f64, 1.0) / 255.0,
            ]);
        }
    }
    let Some([r, g, b]) = dominant else {
        return "#C81E70".into();
    };
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let light = (max + min) / 2.0;
    let delta = max - min;
    let hue = if delta == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / delta + if g < b { 6.0 } else { 0.0 }) / 6.0
    } else if max == g {
        ((b - r) / delta + 2.0) / 6.0
    } else {
        ((r - g) / delta + 4.0) / 6.0
    };
    let saturation = if delta == 0.0 {
        0.0
    } else if light > 0.5 {
        delta / (2.0 - max - min)
    } else {
        delta / (max + min)
    };
    let saturation = (saturation * 1.2).min(1.0);
    let light = light.clamp(0.45, 0.65);
    let q = if light < 0.5 {
        light * (1.0 + saturation)
    } else {
        light + saturation - light * saturation
    };
    let p = 2.0 * light - q;
    let channel = |offset: f64| {
        let t = (hue + offset).rem_euclid(1.0);
        let value = if saturation == 0.0 {
            light
        } else if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        };
        (value * 255.0).round() as u8
    };
    format!(
        "#{:02x}{:02x}{:02x}",
        channel(1.0 / 3.0),
        channel(0.0),
        channel(-1.0 / 3.0)
    )
}

#[cfg(test)]
mod tests;
