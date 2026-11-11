#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayAction {
    RestoreWindow,
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrayMenuStrings {
    pub restore: &'static str,
    pub quit: &'static str,
    pub tooltip: &'static str,
}

pub const STRINGS_RU: TrayMenuStrings = TrayMenuStrings {
    restore: "открыть zabor",
    quit: "выйти",
    tooltip: "zabor",
};

pub const STRINGS_EN: TrayMenuStrings = TrayMenuStrings {
    restore: "open zabor",
    quit: "quit",
    tooltip: "zabor",
};

pub fn get_tray_strings(locale: &str) -> TrayMenuStrings {
    if locale.eq_ignore_ascii_case("en") {
        STRINGS_EN
    } else {
        STRINGS_RU
    }
}

pub const TASKBAR_CREATED_MESSAGE: &str = "TaskbarCreated";

#[derive(Debug, PartialEq, Eq)]
pub enum WindowCloseAction {
    HideToTray,
    QuitApplication,
}

pub fn determine_close_action(minimize_to_tray: bool, is_explicit_quit: bool) -> WindowCloseAction {
    if is_explicit_quit {
        WindowCloseAction::QuitApplication
    } else if minimize_to_tray {
        WindowCloseAction::HideToTray
    } else {
        WindowCloseAction::QuitApplication
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tray_strings_strictly_lowercase() {
        for strings in [STRINGS_RU, STRINGS_EN] {
            assert_eq!(strings.restore, strings.restore.to_lowercase());
            assert_eq!(strings.quit, strings.quit.to_lowercase());
            assert_eq!(strings.tooltip, strings.tooltip.to_lowercase());
            assert!(!strings.restore.chars().any(|c| c.is_uppercase()));
            assert!(!strings.quit.chars().any(|c| c.is_uppercase()));
            assert!(!strings.tooltip.chars().any(|c| c.is_uppercase()));
        }
    }

    #[test]
    fn test_locale_selection() {
        assert_eq!(get_tray_strings("ru"), STRINGS_RU);
        assert_eq!(get_tray_strings("en"), STRINGS_EN);
        assert_eq!(get_tray_strings("EN"), STRINGS_EN);
        assert_eq!(get_tray_strings("unknown"), STRINGS_RU);
    }

    #[test]
    fn test_close_actions() {
        assert_eq!(
            determine_close_action(true, false),
            WindowCloseAction::HideToTray
        );
        assert_eq!(
            determine_close_action(true, true),
            WindowCloseAction::QuitApplication
        );
        assert_eq!(
            determine_close_action(false, false),
            WindowCloseAction::QuitApplication
        );
    }
}
