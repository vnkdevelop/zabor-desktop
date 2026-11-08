pub const CHANNEL_LABEL: &str = "zabor-rc-v1";
pub const CHANNEL_ID: i32 = 4243;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Signal {
    Ping = 0x10,
    HostActive = 0x11,
    HostIdle = 0x12,
    Reset = 0x13,
    HostRevoked = 0x14,
    Busy = 0x15,
    Start = 0x16,
    Stop = 0x17,
    KeyframeRequest = 0x18,
    Accept = 0x19,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Packet {
    Move { x: f32, y: f32 },
    Mouse { button: u8, down: bool },
    Wheel { vertical: i16, horizontal: i16 },
    Key { vk: u16, down: bool, modifiers: u8 },
    Signal(Signal),
}

impl Packet {
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        match bytes {
            [0x01, x @ ..] if x.len() == 8 => {
                let x_pos = f32::from_le_bytes(x[..4].try_into().ok()?);
                let y_pos = f32::from_le_bytes(x[4..].try_into().ok()?);
                (x_pos.is_finite()
                    && y_pos.is_finite()
                    && (0.0..=1.0).contains(&x_pos)
                    && (0.0..=1.0).contains(&y_pos))
                .then_some(Self::Move { x: x_pos, y: y_pos })
            }
            [opcode @ (0x02 | 0x03), button @ 0..=2] => Some(Self::Mouse {
                button: *button,
                down: *opcode == 0x02,
            }),
            [0x04, y0, y1, x0, x1] => Some(Self::Wheel {
                vertical: i16::from_le_bytes([*y0, *y1]),
                horizontal: i16::from_le_bytes([*x0, *x1]),
            }),
            [opcode @ (0x05 | 0x06), lo, hi] => Some(Self::Key {
                vk: u16::from_le_bytes([*lo, *hi]),
                down: *opcode == 0x05,
                modifiers: 0,
            }),
            [opcode @ (0x05 | 0x06), lo, hi, modifiers] if modifiers & !15 == 0 => {
                Some(Self::Key {
                    vk: u16::from_le_bytes([*lo, *hi]),
                    down: *opcode == 0x05,
                    modifiers: *modifiers,
                })
            }
            [opcode] => Some(Self::Signal(match opcode {
                0x10 => Signal::Ping,
                0x11 => Signal::HostActive,
                0x12 => Signal::HostIdle,
                0x13 => Signal::Reset,
                0x14 => Signal::HostRevoked,
                0x15 => Signal::Busy,
                0x16 => Signal::Start,
                0x17 => Signal::Stop,
                0x18 => Signal::KeyframeRequest,
                0x19 => Signal::Accept,
                _ => return None,
            })),
            _ => None,
        }
    }

    pub fn encode(self) -> Vec<u8> {
        match self {
            Self::Move { x, y } => {
                [vec![1], x.to_le_bytes().to_vec(), y.to_le_bytes().to_vec()].concat()
            }
            Self::Mouse { button, down } => vec![if down { 2 } else { 3 }, button],
            Self::Wheel {
                vertical,
                horizontal,
            } => [
                vec![4],
                vertical.to_le_bytes().to_vec(),
                horizontal.to_le_bytes().to_vec(),
            ]
            .concat(),
            Self::Key {
                vk,
                down,
                modifiers,
            } => vec![
                if down { 5 } else { 6 },
                vk as u8,
                (vk >> 8) as u8,
                modifiers,
            ],
            Self::Signal(signal) => vec![signal as u8],
        }
    }

    pub fn is_release(self) -> bool {
        matches!(
            self,
            Self::Mouse { down: false, .. } | Self::Key { down: false, .. }
        )
    }
}

pub fn key_safe(vk: u16, modifiers: u8) -> bool {
    if modifiers & !7 != 0
        || (modifiers & 6 == 6 && vk == 0x2e)
        || (modifiers & 3 == 3 && vk == 0x1b)
        || (modifiers & 4 != 0 && matches!(vk, 0x73 | 0x09))
    {
        return false;
    }
    matches!(vk, 0x08 | 0x09 | 0x0d | 0x1b | 0x20..=0x28 | 0x2d | 0x2e |
        0x30..=0x39 | 0x41..=0x5a | 0x10..=0x12 | 0x14 | 0x90 | 0x91 |
        0x60..=0x7a | 0xba..=0xc0 | 0xdb..=0xde)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_reference_bytes_and_legacy_key_packets_round_trip() {
        for packet in [
            Packet::Move { x: 0.25, y: 1.0 },
            Packet::Mouse {
                button: 2,
                down: false,
            },
            Packet::Wheel {
                vertical: -120,
                horizontal: 240,
            },
            Packet::Key {
                vk: 0x41,
                down: true,
                modifiers: 3,
            },
            Packet::Signal(Signal::Accept),
        ] {
            assert_eq!(Packet::parse(&packet.encode()), Some(packet));
        }
        assert_eq!(
            Packet::Move { x: 0.25, y: 1.0 }.encode(),
            [1, 0, 0, 128, 62, 0, 0, 128, 63]
        );
        assert_eq!(
            Packet::parse(&[5, 65, 0]),
            Some(Packet::Key {
                vk: 65,
                down: true,
                modifiers: 0
            })
        );
    }

    #[test]
    fn malformed_packets_and_nonfinite_coordinates_never_enter_input() {
        for bytes in [
            vec![],
            vec![1],
            vec![2, 3],
            vec![3, 255],
            vec![4, 0, 0],
            vec![5, 65, 0, 16],
            vec![0x16, 0],
        ] {
            assert!(Packet::parse(&bytes).is_none());
        }
        for x in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
            assert!(Packet::parse(&Packet::Move { x, y: 0.5 }.encode()).is_none());
        }
        for length in 0..32 {
            for opcode in 0..=255 {
                let mut bytes = vec![255; length];
                if !bytes.is_empty() {
                    bytes[0] = opcode;
                }
                let _ = Packet::parse(&bytes);
            }
        }
    }

    #[test]
    fn whitelist_and_system_shortcuts_match_the_reference() {
        for vk in [0x5b, 0x5c, 0x5f, 0x7b, 0, 0xffff] {
            assert!(!key_safe(vk, 0));
        }
        for (vk, modifiers) in [(0x2e, 6), (0x1b, 3), (0x73, 4), (0x09, 4), (0x41, 8)] {
            assert!(!key_safe(vk, modifiers));
        }
        for vk in [0x08, 0x41, 0x7a, 0x90, 0xde] {
            assert!(key_safe(vk, 0));
        }
    }
}
