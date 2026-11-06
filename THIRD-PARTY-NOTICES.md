# Third-party notices

Updated: 2026-11-06.

ZABOR Desktop is distributed under GPL-3.0-only. It redistributes the third-party
components listed below. Their own licence terms apply to them.

## Native Rust UI development preview

The `rust/crates/zabor-native` development preview uses Slint 1.18.1 and
slint-build 1.18.1, Copyright © Slint Developers, <https://github.com/slint-ui/slint>.
ZABOR uses their GPL-3.0-only licensing option. The preview uses the winit window
backend, femtovg renderer and AccessKit accessibility support. Exact dependency
versions and checksums are pinned in `rust/Cargo.lock`; this preview is not part
of the published Electron beta installers.

Direct utility dependencies serde, serde_json, semver and png use MIT OR
Apache-2.0. System Segoe UI fonts are resolved from Windows, not redistributed.
The native client modules additionally use Tokio/tokio-util (MIT),
tokio-tungstenite/tungstenite (MIT OR Apache-2.0), reqwest (MIT OR Apache-2.0),
futures-util (MIT OR Apache-2.0), url (MIT OR Apache-2.0), zeroize
(Apache-2.0 OR MIT), thiserror (MIT OR Apache-2.0), base64 (MIT OR Apache-2.0),
image (MIT OR Apache-2.0), and rfd (MIT).
Native persistence/cryptography uses rusqlite (MIT, bundled SQLite in the public
domain), windows-dpapi (MIT OR Apache-2.0, user scope), fs2 (MIT OR Apache-2.0),
and RustCrypto p256, aes-gcm, hmac, sha2 (MIT OR Apache-2.0). getrandom
(MIT OR Apache-2.0) provides operating-system randomness. tempfile
(MIT OR Apache-2.0) is used only by local tests.

Native media uses libwebrtc 0.4.0 / webrtc-sys 0.3.53 (Apache-2.0,
Copyright LiveKit, Inc.), <https://github.com/livekit/rust-sdks>, wrapping
the pinned `webrtc-a545de4` upstream SDK. The SDK's own `LICENSE.md` lists
WebRTC and bundled codec/crypto components and must accompany native packaging.
The bindings are used directly; no LiveKit cloud service is contacted.
Windows media capture/playback uses the platform WASAPI audio device module.
The copied Phosphor SVG paths in
`rust/crates/zabor-native/assets/icons/` originate from Phosphor Icons (MIT,
Copyright (c) 2020 Phosphor Icons, <https://github.com/phosphor-icons/react>);
the MIT text below applies. A complete platform-specific transitive licence
bundle remains part of native installer packaging work.

Нативное preview использует Slint/slint-build 1.18.1 по варианту GPL-3.0-only,
winit/femtovg/AccessKit и закреплённые в `rust/Cargo.lock` зависимости. Оно пока
не входит в опубликованные Electron beta-установщики. serde, serde_json, semver
и png лицензируются MIT OR Apache-2.0; Segoe UI берётся из системы Windows.
SVG-иконки происходят из Phosphor Icons (MIT, Copyright (c) 2020 Phosphor Icons).
Полный комплект лицензий транзитивных зависимостей должен войти в будущую
нативную поставку.

Нативные клиентские модули используют перечисленные выше сетевые, оконные и
графические библиотеки, а медиаядро — libwebrtc 0.4.0 / webrtc-sys 0.3.53
(Apache-2.0, LiveKit) с SDK `webrtc-a545de4`. SDK содержит собственный полный
`LICENSE.md`, который должен войти в нативную поставку. Облачный сервис LiveKit
не используется; захват и воспроизведение звука используют WASAPI Windows.

## Rust client voicing module

- File: `src/renderer/public/rust-voicing/zabor_voicing.wasm`; size and SHA-256 are recorded in its adjacent `manifest.json`
- Own ZABOR code: `zabor-dsp` and the narrow `zabor-dsp-wasm` adapter, GPL-3.0-only
- Bundles the Rust standard-library/core/allocation code, MIT OR Apache-2.0, The Rust Project Developers, <https://github.com/rust-lang/rust>
- Copyright (c) 2010 The Rust Project Developers
- Purpose: block-based voicing autocorrelation inside the existing AudioWorklet; it does not replace DeepFilterNet, RNNoise, Silero, WebRTC codecs or the desktop shell
- No external Rust crates are used by this client WASM module. Benchmark-only server dependencies are not shipped with it.

## DeepFilterNet 3 — model weights

- File: `src/renderer/public/deepfilternet3/models/DeepFilterNet3_onnx.tar.gz`
- Size: 7 983 136 bytes
- SHA-256: `c94d91f70911001c946e0fabb4aa9adc37045f45a03b56008cb0c8244cb63616`
- Upstream: <https://github.com/Rikorose/DeepFilterNet> (`models/DeepFilterNet3_onnx.tar.gz`)
- Licence: MIT **or** Apache-2.0, at the recipient's option
- Copyright (c) 2021 Hendrik Schröter

The vendored file is byte-identical to the upstream blob (git object
`1c4f4ffe1015fbb4541433cf291234ca32338871`).

If you publish measurements obtained with this model, the upstream project asks that you cite:

> H. Schröter, T. Rosenkranz, A. Escalante-B., A. Maier, *DeepFilterNet: Perceptually
> Motivated Real-Time Speech Enhancement*, INTERSPEECH 2023.

## DeepFilterNet 3 — WebAssembly build

- File: `src/renderer/public/deepfilternet3/pkg/df_bg.wasm`
- Size: 9 068 001 bytes
- SHA-256: `0027c731a1093589b78f73a997a6b10c387862f451542041b5b673eea10e4812`
- Contains: a `wasm32-unknown-unknown` build of the DeepFilterNet Rust implementation
  (MIT or Apache-2.0, copyright (c) 2021 Hendrik Schröter) behind a `wasm-bindgen` wrapper
- Built by: `.github/workflows/build-deepfilter-wasm.yml`, run 32790281941, from
  <https://github.com/Rikorose/DeepFilterNet> at commit
  `d375b2d8309e0935d165700c91da9de862a99c31` — crate `deep_filter` 0.5.7-pre with
  `--no-default-features --features wasm`, rustc 1.97.1, wasm-bindgen 0.2.92, `wasm-opt -O3`

The DeepFilterNet project does not publish a WebAssembly artifact, so this one is compiled from
its sources. It is pinned by size and SHA-256 in `scripts/fetch-deepfilter-assets.cjs`, which
refuses to build if the bytes change.

## deepfilter-standalone

- JavaScript loader for the two files above, used at build time only
- Licence: MIT
- Upstream: <https://github.com/abdullahtalal1122/deepfilternet3-standalone-raw-audio-denoising>

## Silero VAD

- File: `src/renderer/public/silero_vad.onnx`
- Licence: MIT
- Upstream: <https://github.com/snakers4/silero-vad>

## RNNoise

- Bundled through the npm package `@timephy/rnnoise-wasm` (Apache-2.0),
  <https://github.com/timephy/rnnoise-wasm>
- RNNoise itself is BSD-3-Clause, Xiph.Org Foundation / Jean-Marc Valin,
  <https://gitlab.xiph.org/xiph/rnnoise>
- The full Apache-2.0 text is at <http://www.apache.org/licenses/LICENSE-2.0>

## ONNX Runtime Web

- Files: `src/renderer/public/ort*.mjs`, `src/renderer/public/ort-wasm-*.wasm`
- Licence: MIT, copyright (c) Microsoft Corporation
- Upstream: <https://github.com/microsoft/onnxruntime>

## WebRTC VAD (fvad-wasm)

- Bundled through the npm package `@echogarden/fvad-wasm` (BSD-3-Clause),
  <https://github.com/echogarden-project/fvad-wasm>
- WebRTC VAD / libfvad itself is BSD-3-Clause, Daniel Pirch / The WebRTC project authors
- Purpose: standalone Google WebRTC Voice Activity Detection (mode 2) for local audio activity indication

## External services used at runtime

These are not bundled with the application; they are contacted over the network when
connections are established. They are listed here because they receive participants'
network addresses, which is disclosed in [PRIVACY.md](PRIVACY.md) section 7.

## STUN

- Default STUN servers configured in the client: `stun.l.google.com:19302`,
  `stun1.l.google.com:19302`, and `stun.cloudflare.com:3478`
- Operated by Google LLC and Cloudflare, Inc. respectively
- Purpose: discovering the public network address of a peer in order to establish a
  direct WebRTC connection
- The client also derives additional STUN servers from the TURN servers issued by the
  ZABOR server, when available

## TURN

- TURN credentials and server addresses are issued at runtime by the ZABOR backend
  server; no TURN provider is hard-coded in this repository
- Purpose: relaying encrypted traffic when a direct connection cannot be established
- The relaying node necessarily sees participants' network addresses

## SFU (Selective Forwarding Unit)

- SFU media node addresses and signalling are issued at runtime by the ZABOR backend
  server when a screen share has 3 or more active viewers (`>= 3`)
- Server component built with Pion WebRTC (`github.com/pion/webrtc/v4`, MIT Licence,
  copyright (c) 2018–2026 Sean DuBois and Pion contributors, <https://github.com/pion/webrtc>)
- Purpose: receiving a single screen-share upstream from the presenter and forwarding
  RTP packets in memory to 3 or more active viewers
- The SFU media node necessarily sees the network addresses of the presenter and
  subscribed viewers

## MIT licence text

Applies to every component above that is listed as MIT, with the copyright notice given in
that component's entry.

```
Permission is hereby granted, free of charge, to any person obtaining a copy of this
software and associated documentation files (the "Software"), to deal in the Software
without restriction, including without limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons
to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or
substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED,
INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR
PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE
FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
```
