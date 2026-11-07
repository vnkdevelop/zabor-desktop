[![crates.io](https://img.shields.io/crates/v/rnnoise2?style=flat-square&logo=rust)](https://crates.io/crates/rnnoise2)
[![docs.rs](https://img.shields.io/badge/docs.rs-rnnoise2-blue?style=flat-square&logo=docs.rs)](https://docs.rs/rnnoise2)
[![crates.io](https://img.shields.io/crates/v/rnnoise2_sys?style=flat-square&logo=rust)](https://crates.io/crates/rnnoise2-sys)
[![docs.rs](https://img.shields.io/badge/docs.rs-rnnoise2_sys-blue?style=flat-square&logo=docs.rs)](https://docs.rs/rnnoise2-sys)
[![license](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](#license)

## Rust binding for the [rnnoise](https://gitlab.xiph.org/xiph/rnnoise) library.

The C code is vendored. The crate may use any commit of the rnnoise repository.

Models are hosted on [this website](https://media.xiph.org/rnnoise/models/).
By default, the latest model available at the time of release will be downloaded.

You can configure which model will be used with the `RNNOISE_MODEL` environment variable.

Example: `RNNOISE_MODEL=0a8755f8e2d834eff6a54714ecc7d75f9932e845df35f8b59bc52a7cfe6e8b37`

You can also provide a local model via `RNNOISE_MODEL_PATH`, avoiding network access.
The file must be a `tar.gz`.

Example: `RNNOISE_MODEL_PATH="path/to/model.tar.gz"`

To enable optimization, only `CFLAGS="-march=native"` seems to work (see <https://github.com/rust-lang/cc-rs/issues/1734>).

You can also use the `runtime-model` feature to load models at runtime, but this feature have not been tested, and it might require some change in the C code to specify the model size.
