#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresenceLanguage {
    Ru,
    En,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalDate {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

impl LocalDate {
    fn valid(self) -> bool {
        let leap = self.year % 4 == 0 && (self.year % 100 != 0 || self.year % 400 == 0);
        let days = match self.month {
            2 if leap => 29,
            2 => 28,
            4 | 6 | 9 | 11 => 30,
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            _ => return false,
        };
        (1..=9999).contains(&self.year) && self.day >= 1 && self.day <= days
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalInstant {
    pub epoch_ms: i64,
    pub local_date: LocalDate,
    pub local_midnight_ms: i64,
}

impl LocalInstant {
    fn valid(self) -> bool {
        self.local_date.valid()
            && self.epoch_ms.unsigned_abs() <= 8_640_000_000_000_000
            && (0..97_200_000)
                .contains(&(i128::from(self.epoch_ms) - i128::from(self.local_midnight_ms)))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerLastOnline {
    Missing,
    Invalid,
    Resolved(LocalInstant),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresenceText {
    Offline,
    JustNow,
    Minutes(i64),
    Hours(i64),
    Yesterday,
    Date { date: LocalDate, include_year: bool },
}

pub fn format_last_online(last: ServerLastOnline, now: LocalInstant) -> PresenceText {
    let ServerLastOnline::Resolved(then) = last else {
        return PresenceText::Offline;
    };
    if !now.valid() || !then.valid() {
        return PresenceText::Offline;
    }
    let diff = i128::from(now.epoch_ms) - i128::from(then.epoch_ms);
    if diff < 60_000 {
        return PresenceText::JustNow;
    }
    let minutes = diff / 60_000;
    if minutes < 60 {
        return PresenceText::Minutes(minutes as i64);
    }
    let midnight_diff = i128::from(now.local_midnight_ms) - i128::from(then.local_midnight_ms);
    let day_diff = (midnight_diff + 43_200_000).div_euclid(86_400_000);
    if day_diff <= 0 {
        return PresenceText::Hours((diff / 3_600_000) as i64);
    }
    if day_diff == 1 {
        return PresenceText::Yesterday;
    }
    PresenceText::Date {
        date: then.local_date,
        include_year: then.local_date.year != now.local_date.year,
    }
}

fn plural_key(language: PresenceLanguage, count: i64, hours: bool) -> &'static str {
    match (language, hours, count % 10, count % 100) {
        (PresenceLanguage::Ru, false, 1, rest) if rest != 11 => "presence.minutesAgo_one",
        (PresenceLanguage::Ru, false, 2..=4, rest) if !(12..=14).contains(&rest) => {
            "presence.minutesAgo_few"
        }
        (PresenceLanguage::Ru, false, _, _) => "presence.minutesAgo_many",
        (PresenceLanguage::Ru, true, 1, rest) if rest != 11 => "presence.hoursAgo_one",
        (PresenceLanguage::Ru, true, 2..=4, rest) if !(12..=14).contains(&rest) => {
            "presence.hoursAgo_few"
        }
        (PresenceLanguage::Ru, true, _, _) => "presence.hoursAgo_many",
        (PresenceLanguage::En, false, _, _) if count == 1 => "presence.minutesAgo_one",
        (PresenceLanguage::En, false, _, _) => "presence.minutesAgo_other",
        (PresenceLanguage::En, true, _, _) if count == 1 => "presence.hoursAgo_one",
        (PresenceLanguage::En, true, _, _) => "presence.hoursAgo_other",
    }
}

impl PresenceText {
    pub fn key(self, language: PresenceLanguage) -> &'static str {
        match self {
            Self::Offline => "presence.offline",
            Self::JustNow => "presence.justNow",
            Self::Minutes(count) => plural_key(language, count, false),
            Self::Hours(count) => plural_key(language, count, true),
            Self::Yesterday => "presence.yesterday",
            Self::Date { .. } => "presence.lastSeenDate",
        }
    }

    pub fn render(self, language: PresenceLanguage, translate: impl Fn(&str) -> String) -> String {
        let template = translate(self.key(language));
        match self {
            Self::Minutes(count) | Self::Hours(count) => {
                template.replace("{{count}}", &count.to_string())
            }
            Self::Date { date, include_year } => {
                template.replace("{{date}}", &localized_date(date, include_year, language))
            }
            _ => template,
        }
    }
}

fn localized_date(date: LocalDate, include_year: bool, language: PresenceLanguage) -> String {
    const RU: [&str; 12] = [
        "января",
        "февраля",
        "марта",
        "апреля",
        "мая",
        "июня",
        "июля",
        "августа",
        "сентября",
        "октября",
        "ноября",
        "декабря",
    ];
    const EN: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    if !date.valid() {
        return String::new();
    }
    let month = usize::from(date.month - 1);
    match (language, include_year) {
        (PresenceLanguage::Ru, false) => format!("{} {}", date.day, RU[month]),
        (PresenceLanguage::Ru, true) => format!("{} {} {} г.", date.day, RU[month], date.year),
        (PresenceLanguage::En, false) => format!("{} {}", EN[month], date.day),
        (PresenceLanguage::En, true) => format!("{} {}, {}", EN[month], date.day, date.year),
    }
}

#[cfg(test)]
#[path = "presence/tests.rs"]
mod tests;
