#[cfg(windows)]
pub mod desktop;

pub fn normalized_point(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    pixels: (u32, u32),
) -> Option<(f32, f32)> {
    if ![x, y, width, height].iter().all(|value| value.is_finite())
        || width <= 0.0
        || height <= 0.0
        || pixels.0 == 0
        || pixels.1 == 0
    {
        return None;
    }
    let scale = (width / pixels.0 as f32).min(height / pixels.1 as f32);
    let w = pixels.0 as f32 * scale;
    let h = pixels.1 as f32 * scale;
    let nx = (x - (width - w) * 0.5) / w;
    let ny = (y - (height - h) * 0.5) / h;
    ((0.0..=1.0).contains(&nx) && (0.0..=1.0).contains(&ny)).then_some((nx, ny))
}

pub fn key_code(text: &str) -> Option<u16> {
    let mut chars = text.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    let code = ch as u32;
    if (0xf704..=0xf70e).contains(&code) {
        return Some((code - 0xf704 + 0x70) as u16);
    }
    let vk = match ch {
        '\u{08}' => 0x08,
        '\t' | '\u{19}' => 0x09,
        '\n' | '\r' => 0x0d,
        '\u{1b}' => 0x1b,
        '\u{7f}' => 0x2e,
        '\u{10}' | '\u{15}' => 0x10,
        '\u{11}' | '\u{16}' => 0x11,
        '\u{12}' | '\u{13}' => 0x12,
        '\u{14}' => 0x14,
        ' ' => 0x20,
        '\u{f700}' => 0x26,
        '\u{f701}' => 0x28,
        '\u{f702}' => 0x25,
        '\u{f703}' => 0x27,
        '\u{f727}' => 0x2d,
        '\u{f729}' => 0x24,
        '\u{f72b}' => 0x23,
        '\u{f72c}' => 0x21,
        '\u{f72d}' => 0x22,
        '\u{f72f}' => 0x91,
        '0'..='9' => code as u16,
        'a'..='z' | 'A'..='Z' => ch.to_ascii_uppercase() as u16,
        ';' | ':' => 0xba,
        '=' | '+' => 0xbb,
        ',' | '<' => 0xbc,
        '-' | '_' => 0xbd,
        '.' | '>' => 0xbe,
        '/' | '?' => 0xbf,
        '`' | '~' => 0xc0,
        '[' | '{' => 0xdb,
        '\\' | '|' => 0xdc,
        ']' | '}' => 0xdd,
        '\'' | '"' => 0xde,
        _ => {
            let lower = ch.to_lowercase().next()?;
            let index = "йцукенгшщзхъфывапролджэячсмитьбюё"
                .chars()
                .position(|ch| ch == lower)?;
            return Some(
                [
                    0x51, 0x57, 0x45, 0x52, 0x54, 0x59, 0x55, 0x49, 0x4f, 0x50, 0xdb, 0xdd, 0x41,
                    0x53, 0x44, 0x46, 0x47, 0x48, 0x4a, 0x4b, 0x4c, 0xba, 0xde, 0x5a, 0x58, 0x43,
                    0x56, 0x42, 0x4e, 0x4d, 0xbc, 0xbe, 0xc0,
                ][index],
            );
        }
    };
    Some(vk)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letterboxing_is_not_a_capture_target_and_center_edges_preserve_normalized_coordinates() {
        assert_eq!(
            normalized_point(400.0, 300.0, 800.0, 600.0, (1920, 1080)),
            Some((0.5, 0.5))
        );
        assert!(normalized_point(400.0, 0.0, 800.0, 600.0, (1920, 1080)).is_none());
        assert_eq!(
            normalized_point(0.0, 75.0, 800.0, 600.0, (1920, 1080)),
            Some((0.0, 0.0))
        );
        for width in [0.0, f32::NAN, f32::INFINITY] {
            assert!(normalized_point(0.0, 0.0, width, 10.0, (10, 10)).is_none());
        }
    }

    #[test]
    fn ru_en_and_named_keys_preserve_virtual_keys_without_system_keys_or_text_injection() {
        for (text, expected) in [
            ("a", 0x41),
            ("ф", 0x41),
            ("Й", 0x51),
            ("\u{f702}", 0x25),
            ("\n", 0x0d),
            ("\u{f70e}", 0x7a),
        ] {
            assert_eq!(key_code(text), Some(expected));
        }
        for text in ["\u{f70f}", "\u{17}", "hello", "😀"] {
            assert!(key_code(text).is_none());
        }
    }
}
