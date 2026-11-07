// build.rs

use std::path::Path;
use std::collections::BTreeSet;

extern crate cc;

fn main() {
    assert!(Path::new("resources/libfvad/src/fvad.c").is_file(), "vendored libfvad sources are missing");
    let mut names = BTreeSet::new();
    collect_symbols(Path::new("resources/libfvad/src"), &mut names);
    let mut build = cc::Build::new();
    for name in names {
        build.define(name.as_str(), Some(format!("ZaborFvad_{name}").as_str()));
    }

    build
        .include("resources/libfvad/src")
        .include("resources/libfvad/src/vad")
        .include("resources/libfvad/src/signal_processing")
        .file("resources/libfvad/src/signal_processing/division_operations.c")
        .file("resources/libfvad/src/signal_processing/get_scaling_square.c")
        .file("resources/libfvad/src/signal_processing/resample_48khz.c")
        .file("resources/libfvad/src/signal_processing/resample_by_2_internal.c")
        .file("resources/libfvad/src/signal_processing/resample_fractional.c")
        .file("resources/libfvad/src/signal_processing/spl_inl.c")
        .file("resources/libfvad/src/signal_processing/energy.c")
        .file("resources/libfvad/src/vad/vad_core.c")
        .file("resources/libfvad/src/vad/vad_filterbank.c")
        .file("resources/libfvad/src/vad/vad_gmm.c")
        .file("resources/libfvad/src/vad/vad_sp.c")
        .file("resources/libfvad/src/fvad.c")
        .compile("libfvad");
}

fn collect_symbols(directory: &Path, names: &mut BTreeSet<String>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if entry.file_type().unwrap().is_dir() {
            collect_symbols(&path, names);
        } else if path.extension().is_some_and(|extension| extension == "c" || extension == "h") {
            let source = std::fs::read_to_string(path).unwrap();
            names.extend(source.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_').filter(|name| name.starts_with("WebRtc")).map(str::to_owned));
        }
    }
}
