use zabor_native::presence::{LocalDate, LocalInstant, ServerLastOnline};

#[derive(Clone, Copy, Default)]
#[repr(C)]
struct SystemTime {
    year: u16,
    month: u16,
    weekday: u16,
    day: u16,
    hour: u16,
    minute: u16,
    second: u16,
    millis: u16,
}

fn number(source: &str) -> Option<u16> {
    (!source.is_empty() && source.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| source.parse().ok())
        .flatten()
}

fn parse(source: &str) -> Option<(SystemTime, Option<i64>)> {
    let bytes = source.as_bytes();
    if !source.is_ascii()
        || bytes.len() < 19
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !matches!(bytes[10], b'T' | b't' | b' ')
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }
    let mut time = SystemTime {
        year: number(&source[..4])?,
        month: number(&source[5..7])?,
        day: number(&source[8..10])?,
        hour: number(&source[11..13])?,
        minute: number(&source[14..16])?,
        second: number(&source[17..19])?,
        ..Default::default()
    };
    let mut position = 19;
    if bytes.get(position) == Some(&b'.') {
        position += 1;
        let start = position;
        while bytes.get(position).is_some_and(u8::is_ascii_digit) {
            position += 1;
        }
        if position == start || position - start > 7 {
            return None;
        }
        let fraction = &source[start..position];
        time.millis = number(&format!("{fraction:0<3}")[..3])?;
    }
    let offset = match &source[position..] {
        "" => None,
        "Z" | "z" => Some(0),
        suffix
            if suffix.len() == 6
                && matches!(suffix.as_bytes()[0], b'+' | b'-')
                && suffix.as_bytes()[3] == b':' =>
        {
            let hours = number(&suffix[1..3])?;
            let minutes = number(&suffix[4..])?;
            if hours > 23 || minutes > 59 {
                return None;
            }
            Some(
                (i64::from(hours) * 60 + i64::from(minutes))
                    * 60_000
                    * if suffix.starts_with('-') { -1 } else { 1 },
            )
        }
        _ => return None,
    };
    let leap = time.year.is_multiple_of(4)
        && (!time.year.is_multiple_of(100) || time.year.is_multiple_of(400));
    let days = match time.month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => return None,
    };
    if !(1601..=9999).contains(&time.year)
        || time.day == 0
        || time.day > days
        || time.hour > 23
        || time.minute > 59
        || time.second > 59
    {
        return None;
    }
    Some((time, offset))
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod native {
    use super::*;
    use std::ffi::c_void;
    const EPOCH: i64 = 116_444_736_000_000_000;
    #[cfg(test)]
    #[repr(C)]
    struct TimeZone {
        bias: i32,
        standard_name: [u16; 32],
        standard_date: SystemTime,
        standard_bias: i32,
        daylight_name: [u16; 32],
        daylight_date: SystemTime,
        daylight_bias: i32,
        key_name: [u16; 128],
        dynamic_disabled: u8,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SystemTimeToFileTime(time: *const SystemTime, file: *mut u64) -> i32;
        fn FileTimeToSystemTime(file: *const u64, time: *mut SystemTime) -> i32;
        fn SystemTimeToTzSpecificLocalTimeEx(
            zone: *const c_void,
            utc: *const SystemTime,
            local: *mut SystemTime,
        ) -> i32;
        fn TzSpecificLocalTimeToSystemTimeEx(
            zone: *const c_void,
            local: *const SystemTime,
            utc: *mut SystemTime,
        ) -> i32;
    }
    fn epoch(time: &SystemTime) -> Option<i64> {
        let mut file = 0_u64;
        if unsafe { SystemTimeToFileTime(time, &mut file) } == 0 {
            return None;
        }
        i64::try_from(file)
            .ok()?
            .checked_sub(EPOCH)
            .map(|value| value / 10_000)
    }
    pub(super) fn local_epoch(time: &SystemTime) -> Option<i64> {
        local_epoch_in(time, std::ptr::null())
    }
    fn local_epoch_in(time: &SystemTime, zone: *const c_void) -> Option<i64> {
        let mut utc = SystemTime::default();
        if unsafe { TzSpecificLocalTimeToSystemTimeEx(zone, time, &mut utc) } == 0 {
            return None;
        }
        epoch(&utc)
    }
    pub(super) fn utc_epoch(time: &SystemTime) -> Option<i64> {
        epoch(time)
    }
    pub(super) fn resolve(epoch_ms: i64) -> Option<LocalInstant> {
        resolve_in(epoch_ms, std::ptr::null())
    }
    fn resolve_in(epoch_ms: i64, zone: *const c_void) -> Option<LocalInstant> {
        if !(-11_644_473_600_000..=253_402_300_799_999).contains(&epoch_ms) {
            return None;
        }
        let file = u64::try_from(epoch_ms.checked_mul(10_000)?.checked_add(EPOCH)?).ok()?;
        let mut utc = SystemTime::default();
        let mut local = SystemTime::default();
        if unsafe { FileTimeToSystemTime(&file, &mut utc) } == 0
            || unsafe { SystemTimeToTzSpecificLocalTimeEx(zone, &utc, &mut local) } == 0
            || !(1601..=9999).contains(&local.year)
        {
            return None;
        }
        let midnight = SystemTime {
            hour: 0,
            minute: 0,
            second: 0,
            millis: 0,
            ..local
        };
        Some(LocalInstant {
            epoch_ms,
            local_date: LocalDate {
                year: i32::from(local.year),
                month: local.month as u8,
                day: local.day as u8,
            },
            local_midnight_ms: local_epoch_in(&midnight, zone)?,
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use zabor_native::presence::{format_last_online, PresenceText};

        #[test]
        fn native_timezone_api_resolves_each_midnight_across_twenty_three_and_twenty_five_hour_days(
        ) {
            let mut zone = TimeZone {
                bias: 480,
                standard_name: [0; 32],
                standard_date: SystemTime {
                    month: 11,
                    day: 1,
                    hour: 2,
                    ..Default::default()
                },
                standard_bias: 0,
                daylight_name: [0; 32],
                daylight_date: SystemTime {
                    month: 3,
                    day: 2,
                    hour: 2,
                    ..Default::default()
                },
                daylight_bias: -60,
                key_name: [0; 128],
                dynamic_disabled: 0,
            };
            for (target, value) in zone
                .key_name
                .iter_mut()
                .zip("Pacific Standard Time".encode_utf16())
            {
                *target = value;
            }
            let pointer = std::ptr::from_ref(&zone).cast();
            let instant = |source| {
                let (time, offset) = parse(source).unwrap();
                assert_eq!(offset, Some(0));
                resolve_in(utc_epoch(&time).unwrap(), pointer).unwrap()
            };
            for (previous, now, day_hours) in [
                ("2024-03-10T20:00:00Z", "2024-03-11T19:00:00Z", 23),
                ("2024-11-03T20:00:00Z", "2024-11-04T20:00:00Z", 25),
            ] {
                let previous = instant(previous);
                let now = instant(now);
                assert_eq!(
                    now.local_midnight_ms - previous.local_midnight_ms,
                    day_hours * 3_600_000
                );
                assert_eq!(
                    format_last_online(ServerLastOnline::Resolved(previous), now),
                    PresenceText::Yesterday
                );
            }
            let before = instant("2024-03-10T09:59:59Z");
            let after = instant("2024-03-10T10:00:00Z");
            assert_eq!(before.local_date, after.local_date);
            assert_eq!(before.local_midnight_ms, after.local_midnight_ms);
            let first = instant("2024-11-03T08:30:00Z");
            let repeated = instant("2024-11-03T09:30:00Z");
            assert_eq!(first.local_midnight_ms, repeated.local_midnight_ms);
            assert_eq!(repeated.epoch_ms - first.epoch_ms, 3_600_000);
        }
    }
}

pub(super) fn resolve(epoch_ms: i64) -> Option<LocalInstant> {
    #[cfg(windows)]
    {
        native::resolve(epoch_ms)
    }
    #[cfg(not(windows))]
    {
        let _ = epoch_ms;
        None
    }
}

pub(super) fn last_online(source: Option<&str>) -> ServerLastOnline {
    let Some(source) = source else {
        return ServerLastOnline::Missing;
    };
    let resolved = (|| {
        let (time, offset) = parse(source)?;
        #[cfg(windows)]
        let epoch = match offset {
            Some(offset) => native::utc_epoch(&time)?.checked_sub(offset)?,
            None => native::local_epoch(&time)?,
        };
        #[cfg(not(windows))]
        let epoch = {
            let _ = (time, offset);
            return None;
        };
        resolve(epoch)
    })();
    resolved
        .map(ServerLastOnline::Resolved)
        .unwrap_or(ServerLastOnline::Invalid)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn iso_validation_preserves_redaction_and_offsets() {
        assert_eq!(last_online(None), ServerLastOnline::Missing);
        for value in [
            "",
            "2025-02-29T00:00:00Z",
            "2024-01-01T24:00:00Z",
            "2024-01-01T00:00:00+24:00",
            "2024-01-01T00:00:00.12345678Z",
        ] {
            assert!(parse(value).is_none());
        }
        assert!(parse("2024-02-29T12:30:00.1234567+03:00").is_some());
        #[cfg(windows)]
        {
            let ServerLastOnline::Resolved(a) = last_online(Some("2024-03-31T01:30:00.123Z"))
            else {
                panic!("native timezone conversion failed")
            };
            let ServerLastOnline::Resolved(b) = last_online(Some("2024-03-31T04:30:00.123+03:00"))
            else {
                panic!("native timezone conversion failed")
            };
            assert_eq!(a, b);
            assert!((0..97_200_000).contains(&(a.epoch_ms - a.local_midnight_ms)));
        }
    }

    #[test]
    fn fractions_local_input_and_calendar_limits_are_not_guessed_or_rounded() {
        for (fraction, millis) in [("1", 100), ("01", 10), ("001", 1), ("9999999", 999)] {
            let (time, offset) = parse(&format!("2024-02-29T12:30:00.{fraction}Z")).unwrap();
            assert_eq!(time.millis, millis);
            assert_eq!(offset, Some(0));
        }
        let (time, offset) = parse("2024-02-29 12:30:00").unwrap();
        assert_eq!(offset, None);
        for source in [
            "1600-12-31T23:59:59Z",
            "10000-01-01T00:00:00Z",
            "1900-02-29T00:00:00Z",
            "2024-01-01T00:00:00.Z",
            "2024-01-01T00:00:00+00:60",
            "2024-01-01T00:00:00Z ",
            "2024-01-01T00:00:00\0",
            "２０２４-01-01T00:00:00Z",
        ] {
            assert!(parse(source).is_none(), "{source:?}");
        }
        assert!(parse("2000-02-29T00:00:00Z").is_some());
        #[cfg(windows)]
        {
            assert_eq!(
                last_online(Some("2024-02-29 12:30:00")),
                native::local_epoch(&time)
                    .and_then(resolve)
                    .map(ServerLastOnline::Resolved)
                    .unwrap_or(ServerLastOnline::Invalid)
            );
            for epoch in [i64::MIN, -11_644_473_600_001, 253_402_300_800_000, i64::MAX] {
                assert!(resolve(epoch).is_none());
            }
            for source in ["1601-01-01T00:00:00+23:59", "9999-12-31T23:59:59.999-23:59"] {
                assert_eq!(last_online(Some(source)), ServerLastOnline::Invalid);
            }
        }
    }
}
