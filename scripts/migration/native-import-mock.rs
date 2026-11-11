use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

fn main() {
    let root = PathBuf::from(std::env::args_os().nth(2).expect("synthetic root required"));
    assert!(root.join(".native-copy-snapshot").is_file());
    let mode = std::fs::read(root.join("Local State")).unwrap_or_default();
    if mode == b"delay" {
        std::thread::sleep(Duration::from_secs(2));
    }
    if mode == b"fail" {
        std::process::exit(7);
    }
    let bytes = std::fs::read(root.join("session.json")).expect("synthetic export required");
    std::io::stdout().lock().write_all(&bytes).unwrap();
}
