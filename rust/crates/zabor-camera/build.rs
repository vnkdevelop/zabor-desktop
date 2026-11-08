fn main() {
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=src/camera.cpp");
    println!("cargo:rerun-if-changed=include/camera.h");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut build = cxx_build::bridge("src/lib.rs");
    if std::env::var_os("CARGO_FEATURE_TEST_SOURCE").is_some() {
        build.define("ZABOR_CAMERA_TEST_SOURCE", None);
    }
    build
        .file("src/camera.cpp")
        .include("include")
        .std("c++17")
        .flag_if_supported("/EHsc")
        .flag_if_supported("/permissive-")
        .flag_if_supported("/W4")
        .flag_if_supported("/WX")
        .define("NOMINMAX", None)
        .compile("zabor_camera");
    for library in ["ole32", "mf", "mfplat", "mfreadwrite", "mfuuid"] {
        println!("cargo:rustc-link-lib={library}");
    }
}
