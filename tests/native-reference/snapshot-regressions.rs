use super::*;
use std::cell::Cell;

#[test]
fn hundreds_of_catalog_lookups_admit_only_once_per_explicit_snapshot() {
    let admissions = Cell::new(0);
    let snapshot = Snapshot::admit_with("catalogs", |operation, group| {
        assert_eq!((operation, group), ("--snapshot", "catalogs"));
        admissions.set(admissions.get() + 1);
        serde_json::json!({"root":"C:/fixture/first", "files": {
            "src/renderer/src/locales/ru.json": "{\"status\":{\"offline\":\"не в сети\"}}\r\n",
            "src/renderer/src/locales/en.json": "{\"status\":{\"offline\":\"offline\"}}\r\n"
        }}).to_string()
    });
    for _ in 0..400 {
        assert_eq!(snapshot.catalog("en")["status"]["offline"], "offline");
        assert_eq!(snapshot.catalog("ru")["status"]["offline"], "не в сети");
    }
    assert_eq!(admissions.get(), 1);
    assert!(snapshot.text("src/renderer/src/locales/en.json").ends_with("\r\n"));
}

#[test]
fn new_snapshot_readmits_changed_root_and_owns_only_its_admitted_bytes() {
    let admissions = Cell::new(0);
    let root = Cell::new("C:/fixture/first");
    let admit = |_: &str, _: &str| {
        admissions.set(admissions.get() + 1);
        serde_json::json!({"root":root.get(), "files": {
            "src/renderer/src/locales/ru.json": "{}",
            "src/renderer/src/locales/en.json": serde_json::json!({"root":root.get()}).to_string()
        }}).to_string()
    };
    let first = Snapshot::admit_with("catalogs", admit);
    root.set("C:/fixture/second");
    let second = Snapshot::admit_with("catalogs", admit);
    assert_eq!(admissions.get(), 2);
    assert_eq!(first.root(), "C:/fixture/first");
    assert_eq!(second.root(), "C:/fixture/second");
    assert_eq!(first.catalog("en")["root"], "C:/fixture/first");
    assert_eq!(second.catalog("en")["root"], "C:/fixture/second");
}

#[test]
fn every_new_snapshot_propagates_admission_failure_instead_of_using_old_bytes() {
    let payload = serde_json::json!({"root":"C:/fixture/valid", "files": {
        "src/renderer/src/locales/ru.json": "{}",
        "src/renderer/src/locales/en.json": "{}"
    }}).to_string();
    let first = Snapshot::admit_with("catalogs", |_, _| payload);
    for error in ["missing reference", "corrupt reference hash", "Git provenance mismatch", "reparse root", "outside-root path"] {
        assert!(std::panic::catch_unwind(|| Snapshot::admit_with("catalogs", |_, _| panic!("{error}"))).is_err());
    }
    assert_eq!(first.root(), "C:/fixture/valid");
}
