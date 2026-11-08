use super::*;

fn png(pixels: RgbaImage) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(pixels)
        .write_to(&mut bytes, ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

fn gif() -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
        encoder
            .set_repeat(image::codecs::gif::Repeat::Infinite)
            .unwrap();
        for (pixel, delay) in [([255, 0, 0, 255], 40), ([0, 0, 255, 255], 70)] {
            encoder
                .encode_frame(image::Frame::from_parts(
                    RgbaImage::from_pixel(8, 4, Rgba(pixel)),
                    0,
                    0,
                    image::Delay::from_numer_denom_ms(delay, 1),
                ))
                .unwrap();
        }
    }
    bytes
}

#[test]
fn reference_crop_is_min_fit_400_square_black_backed_and_offset_is_preview_pixels() {
    let stop = CancellationToken::new();
    let source = Source::from_bytes(
        &png(RgbaImage::from_pixel(8, 4, Rgba([255, 0, 0, 255]))),
        &stop,
    )
    .unwrap();
    let selection = source
        .select(
            Crop {
                scale: 1.0,
                x: 10.0,
                y: 20.0,
            },
            &stop,
        )
        .unwrap();
    let raw = base64::engine::general_purpose::STANDARD
        .decode(selection.wire.split_once(',').unwrap().1)
        .unwrap();
    let crop = image::load_from_memory(&raw).unwrap().into_rgba8();
    assert_eq!(crop.dimensions(), (400, 400));
    assert_eq!(crop.get_pixel(200, 139).0, [0, 0, 0, 255]);
    assert_eq!(crop.get_pixel(200, 140).0, [255, 0, 0, 255]);
    assert_eq!(crop.get_pixel(19, 200).0, [0, 0, 0, 255]);
    assert_eq!(crop.get_pixel(20, 200).0, [255, 0, 0, 255]);
    assert_eq!(selection.color, "#ff0000");
    assert_eq!(selection.avatar.frames[0].pixels.get_pixel(0, 0)[3], 0);
}

#[test]
fn packed_gif_retains_original_bytes_js_rounding_and_animation_delays() {
    let stop = CancellationToken::new();
    let bytes = gif();
    let selection = Source::from_bytes(&bytes, &stop)
        .unwrap()
        .select(
            Crop {
                scale: 1.23456,
                x: -1.25,
                y: 12.345,
            },
            &stop,
        )
        .unwrap();
    let packed: serde_json::Value = serde_json::from_str(&selection.wire).unwrap();
    assert_eq!(packed["s"], 1.235);
    assert_eq!(packed["x"], -1.2);
    assert_eq!(packed["y"], 12.3);
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(packed["g"].as_str().unwrap().split_once(',').unwrap().1)
            .unwrap(),
        bytes
    );
    let avatar = Avatar::from_wire(&selection.wire, &stop).unwrap();
    assert_eq!(avatar.frames.len(), 2);
    assert_eq!(
        avatar
            .frames
            .iter()
            .map(|frame| frame.delay_ms)
            .collect::<Vec<_>>(),
        [40, 70]
    );
    for (time, index) in [(0, 0), (39, 0), (40, 1), (109, 1), (110, 0)] {
        assert_eq!(avatar.frame_index(time, false), index);
    }
    assert_eq!(avatar.frame_index(50, true), 0);
    assert!(avatar.bytes() <= MAX_AVATAR_BYTES);
    for (saved, displayed) in avatar.frames.iter().zip(&selection.avatar.frames) {
        assert_eq!(saved.pixels, displayed.pixels);
    }
}

#[test]
fn gif_color_is_taken_from_the_current_preview_frame_without_flattening_the_saved_animation() {
    let stop = CancellationToken::new();
    let source = Source::from_bytes(&gif(), &stop).unwrap();
    let selection = source.select_frame(Crop::default(), 1, &stop).unwrap();
    assert_eq!(selection.color, "#0000ff");
    assert_eq!(
        Avatar::from_wire(&selection.wire, &stop)
            .unwrap()
            .frames
            .len(),
        2
    );
    assert_eq!(
        selection.avatar.frames[0].pixels.get_pixel(100, 100).0,
        [255, 0, 0, 255]
    );
    assert!(matches!(
        source.select_frame(Crop::default(), 2, &stop),
        Err(AvatarError::Invalid)
    ));
}

#[test]
fn packed_defaults_and_raw_gif_are_accepted_but_malformed_or_foreign_sources_are_not() {
    let stop = CancellationToken::new();
    let data = format!(
        "data:image/gif;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(gif())
    );
    assert_eq!(
        Avatar::from_wire(&serde_json::json!({"g":data}).to_string(), &stop)
            .unwrap()
            .frames
            .len(),
        2
    );
    assert_eq!(Avatar::from_wire(&data, &stop).unwrap().frames.len(), 2);
    for wire in [
        "{}".to_string(),
        "https://example.invalid/avatar.gif".into(),
        serde_json::json!({"g":data,"s":0}).to_string(),
        serde_json::json!({"g":data,"x":"1"}).to_string(),
        "data:image/gif;base64,%%%".into(),
        serde_json::json!({"g":"data:image/png;base64,AA=="}).to_string(),
    ] {
        assert!(matches!(
            Avatar::from_wire(&wire, &stop),
            Err(AvatarError::Invalid)
        ));
    }
}

#[test]
fn transparent_pixels_composite_on_black_for_static_crop_and_do_not_contaminate_color() {
    let stop = CancellationToken::new();
    let source = Source::from_bytes(
        &png(RgbaImage::from_pixel(4, 4, Rgba([255, 255, 0, 0]))),
        &stop,
    )
    .unwrap();
    let selection = source.select(Crop::default(), &stop).unwrap();
    assert_eq!(selection.color, "#C81E70");
    assert_eq!(
        selection.avatar.frames[0].pixels.get_pixel(100, 100).0,
        [0, 0, 0, 255]
    );
    assert_eq!(
        dominant_color(&RgbaImage::from_pixel(4, 4, Rgba([128, 128, 128, 255]))),
        "#808080"
    );
}

#[test]
fn dominant_color_keeps_first_cluster_on_ties_and_prefers_saturated_pixels() {
    let mut pixels = RgbaImage::from_pixel(8, 4, Rgba([80, 80, 80, 255]));
    pixels.put_pixel(0, 0, Rgba([0, 0, 255, 255]));
    pixels.put_pixel(4, 0, Rgba([255, 0, 0, 255]));
    assert_eq!(dominant_color(&pixels), "#0000ff");
    assert_eq!(
        dominant_color(&RgbaImage::from_pixel(4, 4, Rgba([255, 255, 255, 255]))),
        "#C81E70"
    );
}

#[test]
fn cancellation_and_size_limits_reject_before_publication_or_large_decoding() {
    let stop = CancellationToken::new();
    assert!(matches!(
        Source::from_bytes(&vec![0; MAX_FILE_BYTES + 1], &stop),
        Err(AvatarError::TooLarge)
    ));
    assert!(matches!(
        Avatar::from_wire(&"x".repeat(MAX_WIRE_BYTES + 1), &stop),
        Err(AvatarError::TooLarge)
    ));
    let source = Source::from_bytes(&gif(), &stop).unwrap();
    stop.cancel();
    assert!(matches!(
        source.select(Crop::default(), &stop),
        Err(AvatarError::Cancelled)
    ));
    assert!(matches!(
        Source::from_bytes(&gif(), &stop),
        Err(AvatarError::Cancelled)
    ));
}

#[test]
fn frame_limits_and_finite_loop_completion_do_not_silently_flatten_animation() {
    let stop = CancellationToken::new();
    let mut source = Source::from_bytes(&gif(), &stop).unwrap();
    source.loops = Some(1);
    let avatar = source
        .presentation(Crop::default(), false, false, &stop)
        .unwrap();
    assert_eq!(avatar.frame_index(500, false), 1);
    source.frames = (0..MAX_FRAMES)
        .map(|_| Frame {
            pixels: RgbaImage::new(1, 1),
            delay_ms: 40,
        })
        .collect();
    assert!(matches!(
        source.presentation(Crop::default(), false, false, &stop),
        Err(AvatarError::TooLarge)
    ));
}

#[test]
fn invalid_scale_and_nonfinite_geometry_never_allocate_a_crop() {
    for crop in [
        Crop {
            scale: f64::NAN,
            ..Default::default()
        },
        Crop {
            scale: 5.1,
            ..Default::default()
        },
        Crop {
            x: f64::INFINITY,
            ..Default::default()
        },
        Crop {
            y: -1_000_001.0,
            ..Default::default()
        },
    ] {
        assert!(!crop.valid());
    }
    for scale in [0.5, 1.0, 3.0, 5.0] {
        assert!(Crop {
            scale,
            ..Default::default()
        }
        .valid());
    }
}

#[test]
fn gif_loop_metadata_distinguishes_missing_repeat_from_infinite_and_counts_the_initial_play() {
    let mut bytes = gif();
    let marker = bytes
        .windows(11)
        .position(|bytes| bytes == b"NETSCAPE2.0")
        .unwrap();
    assert_eq!(gif_loops(&bytes).unwrap(), None);
    bytes[marker + 13] = 1;
    bytes[marker + 14] = 0;
    assert_eq!(gif_loops(&bytes).unwrap(), Some(2));
    bytes.drain(marker - 3..marker + 16);
    assert_eq!(gif_loops(&bytes).unwrap(), Some(1));
    assert!(gif_loops(&bytes[..14]).is_err());
}
