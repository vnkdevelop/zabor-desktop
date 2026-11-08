use super::*;
use serde_json::Value;

fn translate(language: PresenceLanguage, key: &str) -> String {
    let source = match language {
        PresenceLanguage::Ru => include_str!("../../../../../src/renderer/src/locales/ru.json"),
        PresenceLanguage::En => include_str!("../../../../../src/renderer/src/locales/en.json"),
    };
    let mut value: Value = serde_json::from_str(source).unwrap();
    for segment in key.split('.') {
        value = value[segment].clone();
    }
    value.as_str().unwrap().into()
}

fn instant(day: u8, hours: i64) -> LocalInstant {
    let midnight = i64::from(day) * 86_400_000;
    LocalInstant {
        epoch_ms: midnight + hours * 3_600_000,
        local_midnight_ms: midnight,
        local_date: LocalDate {
            year: 2026,
            month: 11,
            day,
        },
    }
}

fn render(text: PresenceText, language: PresenceLanguage) -> String {
    text.render(language, |key| translate(language, key))
}

#[test]
fn missing_invalid_and_unresolvable_local_input_preserve_offline_redaction() {
    for last in [ServerLastOnline::Missing, ServerLastOnline::Invalid] {
        let text = format_last_online(last, instant(8, 12));
        assert_eq!(text, PresenceText::Offline);
        assert_eq!(render(text, PresenceLanguage::Ru), "не в сети");
        assert_eq!(render(text, PresenceLanguage::En), "offline");
    }
    let mut invalid = instant(8, 12);
    invalid.local_date.month = 13;
    assert_eq!(
        format_last_online(ServerLastOnline::Resolved(invalid), instant(8, 12)),
        PresenceText::Offline
    );
    invalid = instant(8, 12);
    invalid.local_midnight_ms = invalid.epoch_ms + 1;
    assert_eq!(
        format_last_online(ServerLastOnline::Resolved(invalid), instant(8, 12)),
        PresenceText::Offline
    );
    invalid = instant(8, 12);
    invalid.local_date = LocalDate {
        year: 2025,
        month: 2,
        day: 29,
    };
    assert_eq!(
        format_last_online(ServerLastOnline::Resolved(instant(8, 12)), invalid),
        PresenceText::Offline
    );
}

#[test]
fn millisecond_thresholds_future_minutes_and_same_local_day_hours() {
    let now = instant(8, 23);
    for (diff, expected) in [
        (-1, PresenceText::JustNow),
        (0, PresenceText::JustNow),
        (59_999, PresenceText::JustNow),
        (60_000, PresenceText::Minutes(1)),
        (3_599_999, PresenceText::Minutes(59)),
        (3_600_000, PresenceText::Hours(1)),
        (3_600_001, PresenceText::Hours(1)),
    ] {
        let mut then = now;
        then.epoch_ms -= diff;
        assert_eq!(
            format_last_online(ServerLastOnline::Resolved(then), now),
            expected
        );
    }
    assert_eq!(
        render(PresenceText::JustNow, PresenceLanguage::Ru),
        "был(а) в сети только что"
    );
    assert_eq!(
        render(PresenceText::JustNow, PresenceLanguage::En),
        "last seen just now"
    );
}

#[test]
fn exact_ru_minute_plurals_and_en_one_other() {
    let now = instant(8, 12);
    for (count, ru_unit) in [
        (1, "минуту"),
        (2, "минуты"),
        (4, "минуты"),
        (5, "минут"),
        (11, "минут"),
        (12, "минут"),
        (14, "минут"),
        (21, "минуту"),
        (22, "минуты"),
        (25, "минут"),
        (59, "минут"),
    ] {
        let mut then = now;
        then.epoch_ms -= count * 60_000;
        let text = format_last_online(ServerLastOnline::Resolved(then), now);
        assert_eq!(text, PresenceText::Minutes(count));
        assert_eq!(
            render(text, PresenceLanguage::Ru),
            format!("был(а) в сети {count} {ru_unit} назад")
        );
        assert_eq!(
            render(text, PresenceLanguage::En),
            format!(
                "last seen {count} {} ago",
                if count == 1 { "minute" } else { "minutes" }
            )
        );
    }
}

#[test]
fn exact_ru_hour_plurals_and_en_one_other() {
    let now = instant(8, 23);
    for (count, ru_unit) in [
        (1, "час"),
        (2, "часа"),
        (4, "часа"),
        (5, "часов"),
        (11, "часов"),
        (12, "часов"),
        (14, "часов"),
        (21, "час"),
        (22, "часа"),
        (23, "часа"),
    ] {
        let mut then = now;
        then.epoch_ms -= count * 3_600_000;
        let text = format_last_online(ServerLastOnline::Resolved(then), now);
        assert_eq!(text, PresenceText::Hours(count));
        assert_eq!(
            render(text, PresenceLanguage::Ru),
            format!("был(а) в сети {count} {ru_unit} назад")
        );
        assert_eq!(
            render(text, PresenceLanguage::En),
            format!(
                "last seen {count} {} ago",
                if count == 1 { "hour" } else { "hours" }
            )
        );
    }
}

#[test]
fn local_midnight_order_precedes_yesterday_only_after_one_hour() {
    let now = instant(8, 0);
    let mut then = instant(7, 23);
    then.epoch_ms = now.epoch_ms - 59_999;
    assert_eq!(
        format_last_online(ServerLastOnline::Resolved(then), now),
        PresenceText::JustNow
    );
    then.epoch_ms = now.epoch_ms - 60_000;
    assert_eq!(
        format_last_online(ServerLastOnline::Resolved(then), now),
        PresenceText::Minutes(1)
    );
    then.epoch_ms = now.epoch_ms - 3_600_000;
    let text = format_last_online(ServerLastOnline::Resolved(then), now);
    assert_eq!(text, PresenceText::Yesterday);
    assert_eq!(render(text, PresenceLanguage::Ru), "был(а) в сети вчера");
    assert_eq!(render(text, PresenceLanguage::En), "last seen yesterday");
}

#[test]
fn previous_local_day_23_and_25_hour_dst_midnights_round_like_reference() {
    for length in [23, 25] {
        let then = instant(7, 12);
        let mut now = instant(8, 12);
        now.local_midnight_ms = then.local_midnight_ms + length * 3_600_000;
        now.epoch_ms = now.local_midnight_ms + 12 * 3_600_000;
        assert_eq!(
            format_last_online(ServerLastOnline::Resolved(then), now),
            PresenceText::Yesterday
        );
    }
}

#[test]
fn dates_same_year_changed_year_and_exact_ru_en_months() {
    let now = instant(8, 12);
    for (month, ru, en) in [
        (1, "января", "January"),
        (2, "февраля", "February"),
        (3, "марта", "March"),
        (4, "апреля", "April"),
        (5, "мая", "May"),
        (6, "июня", "June"),
        (7, "июля", "July"),
        (8, "августа", "August"),
        (9, "сентября", "September"),
        (10, "октября", "October"),
        (11, "ноября", "November"),
        (12, "декабря", "December"),
    ] {
        let mut then = instant(5, 12);
        then.local_date.month = month;
        let text = format_last_online(ServerLastOnline::Resolved(then), now);
        assert_eq!(
            text,
            PresenceText::Date {
                date: then.local_date,
                include_year: false
            }
        );
        assert_eq!(
            render(text, PresenceLanguage::Ru),
            format!("был(а) в сети 5 {ru}")
        );
        assert_eq!(
            render(text, PresenceLanguage::En),
            format!("last seen {en} 5")
        );
        then.local_date.year = 2025;
        let text = format_last_online(ServerLastOnline::Resolved(then), now);
        assert_eq!(
            render(text, PresenceLanguage::Ru),
            format!("был(а) в сети 5 {ru} 2025 г.")
        );
        assert_eq!(
            render(text, PresenceLanguage::En),
            format!("last seen {en} 5, 2025")
        );
    }
}

#[test]
fn local_timezone_day_not_utc_controls_hours_and_year_transition() {
    let now = LocalInstant {
        epoch_ms: 86_400_000 + 1_800_000,
        local_midnight_ms: 86_400_000 - 10 * 3_600_000,
        local_date: LocalDate {
            year: 2026,
            month: 1,
            day: 1,
        },
    };
    let then = LocalInstant {
        epoch_ms: now.epoch_ms - 2 * 3_600_000,
        ..now
    };
    assert_eq!(
        format_last_online(ServerLastOnline::Resolved(then), now),
        PresenceText::Hours(2)
    );
    let then = LocalInstant {
        epoch_ms: now.epoch_ms - 3 * 86_400_000,
        local_midnight_ms: now.local_midnight_ms - 3 * 86_400_000,
        local_date: LocalDate {
            year: 2025,
            month: 12,
            day: 29,
        },
    };
    let text = format_last_online(ServerLastOnline::Resolved(then), now);
    assert_eq!(
        render(text, PresenceLanguage::En),
        "last seen December 29, 2025"
    );
}
