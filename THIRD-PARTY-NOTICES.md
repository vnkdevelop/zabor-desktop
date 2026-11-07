# Third-party notices

Updated: 2026-11-07.

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
(MIT OR Apache-2.0) is used by local tests and native atomic file saving.

Native media uses libwebrtc 0.4.0 / webrtc-sys 0.3.53 (Apache-2.0,
Copyright LiveKit, Inc.), <https://github.com/livekit/rust-sdks>, wrapping
the pinned `webrtc-a545de4` upstream SDK. The SDK's own `LICENSE.md` lists
WebRTC and bundled codec/crypto components and must accompany native packaging.
The bindings are used directly; no LiveKit cloud service is contacted.
Windows media capture/playback uses the platform WASAPI audio device module.
The native audio work-in-progress also uses wasapi 0.26.0 (MIT),
<https://github.com/HEnquist/wasapi-rs>; deep_filter 0.5.7-pre at
`d375b2d8309e0935d165700c91da9de862a99c31` (MIT OR Apache-2.0);
ndarray 0.15 and the Tract family 0.21.4 (MIT OR Apache-2.0).
`rust/vendor/tract-core-0.21.4` contains the verified upstream source and a
local constant-name collision fix, documented in `ZABOR-PATCH.md`; upstream
licences are preserved. RNNoise is bound through rnnoise2/rnnoise2-sys 0.1.2
(MIT), <https://github.com/wiiznokes/rnnoise-rs>, with upstream BSD-3-Clause
RNNoise code and the pinned model archive SHA-256
`4ac81c5c0884ec4bd5907026aaae16209b7b76cd9d7f71af582094a2f98f4b43`.
Silero reuses the existing MIT-licensed local model through ort/ort-sys
2.0.0-rc.15 (MIT OR Apache-2.0) and Microsoft ONNX Runtime 1.31.1 (MIT).
The verified runtime cache retains Microsoft's LICENSE and ThirdPartyNotices;
these must accompany the future native installer. ONNX telemetry is disabled.
Build-time preparation downloads the pinned public RNNoise archive from
media.xiph.org and Microsoft's runtime package from NuGet, not user audio.
These local inference adapters and device-thread/mixer primitives are not yet
connected to the application's microphone path and are not in published installers.
Native binding generation uses libclang 18.1.1 from the pinned Windows x64
PyPI wheel (Apache-2.0 with LLVM exceptions), with its licence retained in the
local build cache. It is a build-only tool, not an application runtime service
or a component added to the published Electron installers.
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

Разрабатываемый нативный DSP использует wasapi 0.26.0 (MIT), DeepFilterNet
на указанной ревизии, ndarray 0.15 и Tract 0.21.4 (MIT OR Apache-2.0),
rnnoise2/rnnoise2-sys 0.1.2 (MIT) с RNNoise BSD-3-Clause и закреплёнными
весами, существующую модель Silero (MIT), ort/ort-sys 2.0.0-rc.15
(MIT OR Apache-2.0) и Microsoft ONNX Runtime 1.31.1 (MIT).
Локальная копия Tract содержит только исправление коллизии имён констант;
его описание и исходные лицензии сохранены в `rust/vendor/tract-core-0.21.4`.
Телеметрия ONNX отключена. На этапе сборки с media.xiph.org и NuGet
загружаются публичные закреплённые артефакты; звук пользователя не отправляется.
Лицензии Microsoft должны войти в будущий установщик. Эти адаптеры, потоки
устройств и микшер пока не подключены к микрофонному тракту приложения
и не входят в опубликованные установщики.
Для генерации нативных bindings используется libclang 18.1.1 из закреплённого
Windows x64 PyPI wheel (Apache-2.0 с LLVM exceptions); его лицензия сохраняется
в локальном кэше сборки. Это инструмент сборки, а не служба приложения или
компонент опубликованных Electron-установщиков.

Для атомарного сохранения полученных файлов и локальных тестов используется tempfile
(MIT OR Apache-2.0). Это локальная библиотека, не внешний получатель данных.

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

The native work-in-progress uses webrtc-vad 0.4.0 (MIT),
<https://github.com/kaegi/webrtc-vad>, binding bundled libfvad/WebRTC VAD
(BSD-3-Clause). Its mode-2 decisions and 150 ms hangover are tested against
the existing WASM detector; this is a local activity detector, not content moderation.
The verified source is vendored in `rust/vendor/webrtc-vad-0.4.0`; its local
build patch namespaces internal C symbols to coexist with libwebrtc and avoids
a Git submodule download. Signal processing is unchanged; licences are preserved.
Нативный модуль использует webrtc-vad 0.4.0 (MIT) с libfvad/WebRTC VAD
(BSD-3-Clause); решения mode 2 и удержание 150 мс проверяются по WASM-эталону.
Это локальный детектор активности, а не модерация содержимого.
Проверенный upstream source сохранён в `rust/vendor/webrtc-vad-0.4.0`;
локальная правка сборки изолирует C-символы от libwebrtc и исключает загрузку
Git submodule, не меняя алгоритм и сохраняя исходные лицензии.

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
