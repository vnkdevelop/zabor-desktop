fn main() {
    slint_build::compile("ui/root.slint").expect("native UI compilation failed");
    if std::env::var_os("CARGO_FEATURE_NEURAL_DSP").is_some()
        && std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
    {
        let runtime = std::path::PathBuf::from(
            std::env::var_os("ORT_LIB_LOCATION").expect("run scripts/native/prepare-onnx.cjs"),
        );
        let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
        let profile = out.ancestors().nth(3).unwrap();
        for entry in std::fs::read_dir(runtime).expect("native ONNX Runtime is missing") {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|extension| extension != "dll") {
                continue;
            }
            println!("cargo:rerun-if-changed={}", path.display());
            let bytes = std::fs::read(&path).unwrap();
            for directory in [profile.to_path_buf(), profile.join("deps")] {
                std::fs::create_dir_all(&directory).unwrap();
                let target = directory.join(path.file_name().unwrap());
                if std::fs::read(&target).ok().as_deref() != Some(bytes.as_slice()) {
                    std::fs::write(target, &bytes)
                        .expect("could not install the pinned ONNX Runtime beside the executable");
                }
            }
        }
    }
}
