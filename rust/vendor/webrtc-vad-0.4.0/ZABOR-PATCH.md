# Native VAD symbol isolation

Source: verified crates.io `webrtc-vad` 0.4.0 archive.
SHA-256: `39a1e40fd6ca90be95459152a2537f2ba4286ee1b13073f7ebcaa74fc94e3008`.
The Rust binding is MIT; bundled libfvad/WebRTC code retains its BSD-3-Clause licences.

The only modified upstream file is `build.rs`. It requires the packaged
libfvad sources instead of attempting a Git submodule download. It prefixes
libfvad's internal `WebRtc*` C identifiers with `ZaborFvad_` during compilation
to avoid duplicate symbols from the client's libwebrtc SDK. The public `fvad_*`
API and all signal processing remain unchanged. Upstream comments and licence
files are preserved. This is a local compatibility patch, not an upstream release.

ZABOR compares mode-2 activity decisions and the existing 150 ms hangover
against its unchanged WASM detector, including resets, and links the detector
in the same executable as libwebrtc.
