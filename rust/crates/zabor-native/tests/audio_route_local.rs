use std::time::{Duration, Instant};
use zabor_native::audio_route::{DeviceInventory, DeviceRecovery};

fn inventory() -> DeviceInventory {
    DeviceInventory {
        endpoints: vec!["usb".into(), "builtin".into()],
        default: Some("builtin".into()),
    }
}

#[test]
fn explicit_native_ids_and_default_aliases_resolve_without_rewriting_the_selection() {
    let devices = inventory();
    assert_eq!(devices.resolve("usb").as_deref(), Some("usb"));
    for selected in ["", "default", "browser-hashed-id", "disconnected-usb"] {
        assert_eq!(devices.resolve(selected).as_deref(), Some("builtin"));
    }
    let selected = "usb";
    let mut devices = inventory();
    devices.endpoints.retain(|id| id != selected);
    assert_eq!(devices.resolve(selected).as_deref(), Some("builtin"));
    devices.endpoints.push(selected.into());
    assert_eq!(devices.resolve(selected).as_deref(), Some(selected));
    assert_eq!(selected, "usb");
}

#[test]
fn default_changes_are_followed_but_never_choose_an_arbitrary_endpoint() {
    let mut devices = inventory();
    devices.default = Some("usb".into());
    assert_eq!(devices.resolve("default").as_deref(), Some("usb"));
    devices.default = Some("removed".into());
    assert_eq!(devices.resolve("default"), None);
    assert_eq!(devices.resolve("usb").as_deref(), Some("usb"));
    devices.default = None;
    assert_eq!(devices.resolve("browser-hashed-id"), None);
    assert_eq!(DeviceInventory::default().resolve("default"), None);
}

#[test]
fn recovery_backoff_is_bounded_and_ends_after_thirty_seconds() {
    let start = Instant::now();
    let mut recovery = DeviceRecovery::default();
    assert!(recovery.can_retry(start));
    let mut now = start;
    for milliseconds in [250, 500, 1000, 2000, 2000, 2000] {
        recovery.failed(now);
        assert!(!recovery.can_retry(now));
        assert!(!recovery.can_retry(now + Duration::from_millis(milliseconds - 1)));
        now += Duration::from_millis(milliseconds);
        assert!(recovery.can_retry(now));
    }
    assert!(!recovery.expired(start + DeviceRecovery::DEADLINE - Duration::from_millis(1)));
    assert!(recovery.expired(start + DeviceRecovery::DEADLINE));
    assert!(!recovery.can_retry(start + DeviceRecovery::DEADLINE));
}

#[test]
fn losses_and_fast_flapping_do_not_reset_the_deadline_but_stable_recovery_does() {
    let start = Instant::now();
    let mut recovery = DeviceRecovery::default();
    recovery.lost(start);
    for second in 1..31 {
        recovery.lost(start + Duration::from_secs(second));
    }
    assert!(recovery.expired(start + Duration::from_secs(30)));
    recovery.recovered();
    assert!(recovery.can_retry(start + Duration::from_secs(30)));
    recovery.failed(start + Duration::from_secs(31));
    assert!(!recovery.expired(start + Duration::from_secs(60)));
    assert!(recovery.expired(start + Duration::from_secs(61)));
}
