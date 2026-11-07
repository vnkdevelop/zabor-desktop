use std::{
    env,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::bail;

fn download_model(archive_path: &Path, model_name: &str) -> anyhow::Result<()> {
    println!("Downloading {model_name}");

    let url = format!("https://media.xiph.org/rnnoise/models/{model_name}.tar.gz");

    let response = reqwest::blocking::get(&url)?;

    if !response.status().is_success() {
        bail!("download failed: {}", response.status())
    }

    let bytes = response.bytes()?;
    fs::write(archive_path, &bytes)?;

    Ok(())
}

fn unpack(tar: &Path, dst: &Path) -> anyhow::Result<()> {
    let tar_gz = fs::File::open(tar)?;
    let decoder = flate2::read::GzDecoder::new(tar_gz);
    let mut archive = tar::Archive::new(decoder);

    archive.unpack(dst)?;

    println!("Model extracted to {}", dst.display());

    Ok(())
}

const LAST_MODEL_VERSION: &str = "0a8755f8e2d834eff6a54714ecc7d75f9932e845df35f8b59bc52a7cfe6e8b37";

fn main() {
    let dst = PathBuf::from(env::var_os("OUT_DIR").unwrap());

    println!("cargo:rerun-if-env-changed=RNNOISE_MODEL_PATH");
    println!("cargo:rerun-if-env-changed=RNNOISE_MODEL");

    let mut cfg = cc::Build::new();

    if cfg!(feature = "runtime-model") {
        cfg.define("USE_WEIGHTS_FILE", None);

        cfg.include("include_runtime_model");
    } else {
        let model_path = match env::var("RNNOISE_MODEL_PATH") {
            Ok(value) => {
                if !value.ends_with("tar.gz") {
                    panic!("RNNOISE_MODEL_PATH doesn't end with tar.gz");
                }

                let path = PathBuf::from(value);
                if !path.exists() {
                    panic!("RNNOISE_MODEL_PATH value doesn't exist");
                }

                Some(path)
            }
            Err(env::VarError::NotPresent) => None,
            Err(err) => {
                panic!("Invalid RNNOISE_MODEL_PATH: {err}");
            }
        };

        let model_name = if let Some(model_path) = &model_path {
            model_path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .strip_suffix("tar.gz")
                .unwrap()
                .to_string()
        } else {
            // let model_version = fs::read_to_string("rnnoise/model_version")
            //     .unwrap()
            //     .trim()
            //     .to_string();

            let last_model_version = match env::var("RNNOISE_MODEL") {
                Ok(value) => value,
                Err(env::VarError::NotPresent) => LAST_MODEL_VERSION.to_string(),
                Err(err) => {
                    panic!("Invalid RNNOISE_MODEL: {err}");
                }
            };

            format!("rnnoise_data-{last_model_version}")
        };

        let extracted_archive_path = dst.join(&model_name);

        let archive_path = dst.join(format!("{model_name}.tar.gz"));

        if !extracted_archive_path.exists() {
            if let Some(model_path) = &model_path {
                unpack(model_path, &extracted_archive_path).unwrap();
            } else {
                download_model(&archive_path, &model_name).unwrap();
                unpack(&archive_path, &extracted_archive_path).unwrap();
            }
        }

        cfg.include(extracted_archive_path.join("src"));
        cfg.file(extracted_archive_path.join("src/rnnoise_data.c"));
    }

    cfg.include("rnnoise/include");
    cfg.include("rnnoise/src");

    let common_c_files = [
        "denoise.c",
        "rnn.c",
        "pitch.c",
        "kiss_fft.c",
        "celt_lpc.c",
        "nnet.c",
        "nnet_default.c",
        "parse_lpcnet_weights.c",
        "rnnoise_tables.c",
    ];

    for f in common_c_files {
        let source = if matches!(f, "denoise.c" | "rnn.c") {
            "reference"
        } else {
            "rnnoise/src"
        };
        cfg.file(Path::new(source).join(f));
    }

    cfg.warnings(false);

    cfg.compile("rnnoise");

    let mut builder = bindgen::builder().header("rnnoise/include/rnnoise.h");

    // Make the `FILE` type opaque, so bindgen does not pull other types from the
    // standard library.
    // Then block `FILE`, so bindgen does not emit it.
    // Then define `FILE` explicitly so that it can only be used behind a pointer.
    builder = builder.opaque_type("^FILE$");
    builder = builder.blocklist_type("^FILE$");
    builder = builder.raw_line("pub type FILE = ::std::ffi::c_void;");

    builder = builder.allowlist_function("^rnnoise_.+$");

    let mut file = File::create(dst.join("lib.rs")).unwrap();

    let bindings = builder.generate().unwrap();

    file.write_all(bindings.to_string().as_bytes()).unwrap();
}
