# Audited legacy-client cutover closure

Read-only audit completed 2026-11-08 by `ses_e43cb9554ffeu6iHaRzbyeHsQS`, against
primary `fd8783f0d744b586051ce62fdf89a686e2e4700b`. This is a conditional removal
manifest, not complete-native parity acceptance. No files were deleted or builds,
imports, production requests or release operations performed by the auditor.
Its initial graph report identified05034dde rather than the audited HEAD; targeted
tracked-source searches supplied the findings. Do not treat that report as current
whole-project acceptance. Primary Graphify resync is in the original pending check.

## Original-project preservation hazard

Primary `node_modules` is a Windows junction to:
`C:\Users\porad\Documents\zabor\Frontend\zabor-desktop\node_modules`.
Primary independently verified the reparse target with read-only fsutil query.
Never recursively clean that path or run npm installation/postinstall/rebuild
through it. Electron postinstall copies/rebuilds binaries in the junction target.
Preserve the original user folder and immutable checkpoint
`d088795457483bad1859d45616a9451342cada74` unchanged. Do not execute legacy sync or
release launchers; release-beta can fall back to the original user folder.

## Conditional removal candidates

All paths below are relative to the primary repository, not the original user
folder. Group notation is an audit inventory, not blanket recursive-delete
permission. Every removal waits for replacement, reverse-reference cutover and
final parity acceptance. Required reference/configuration material is retained.

| Group | Candidate | Cutover condition |
| --- | --- | --- |
| Electron main | `src/main/` | Native window/IPC/files/security/updater/RC and tests accepted; release scripts still import updatePolicy.ts |
| Preload bridge | `src/preload/` | All exposed operations replaced, active Electron/Vite routes retired |
| Diagnostics | `src/shared/diagnostics.ts` | Preserve exact sanitization reference and retire main/renderer imports |
| React renderer | `src/renderer/` source closure | Never wholesale-delete: subtract retained catalogs/assets/models and UI/DSP/achievement references |
| Browser ONNX | `src/renderer/public/ort*` | Remove browser production payload only; retain browser reference pins and required native ONNX DLLs |
| Browser Rust voice | `src/renderer/public/rust-voicing/` | Retain manifest and Electron DSP parity evidence before active WASM payload removal |
| Browser DFN | `src/renderer/public/deepfilternet3/pkg/df_bg.wasm` | Native neural reference still uses it; model archive is separate and retained |
| WASM adapter | `rust/crates/zabor-dsp-wasm/` | Still workspace/default member; surgical workspace/build/test cutover, never remove zabor-dsp |
| Managed helpers | `build/ZaborUpdater.exe`, `build/ZaborInputSimulator.exe` | Native updater/RC accepted; corresponding C# source retained as reference |
| Electron addon | `build/native/electron_native_screenshare.node` | Native C++/WASAPI replacements accepted and postinstall/rebuild/package references retired |
| Legacy DSP launcher | `build/start-legacy-dsp.bat` | Retire Electron extraResources and active distribution route |
| Legacy tools | `scripts/build-input-simulator.cjs`, `build-splash.cjs`, `build-rust-voicing.cjs`, `rebuild-native-audio.cjs`, `fetch-deepfilter-assets.cjs`, `check-deepfilter-wasm.mjs`, `benchmark-client-dsp.mjs` | No bulk removal; retain pinned model/DSP/reference verification, retire active runtime invocations first |
| Legacy release chain | `sync.bat`, `release.bat`, `release-beta.bat`, `promote-beta.bat`; `scripts/release.mjs`, `release-guard.mjs`, `release-plan.mjs`, `beta-manifest.mjs`, `stable-manifest.mjs`, `publish-beta.mjs`, `promote-beta.mjs`, `client-release-gate.mjs` | Native-only guarded distribution; preserve configuration/approval evidence and never run fallback launchers |
| Observed TS artifacts | `dist-electron/tsconfig.node.tsbuildinfo`, `tsconfig.web.tsbuildinfo` | Bounded cleanup only after acceptance; ignored out/dist/release patterns prove neither existence nor ownership |
| Old active configuration | Root npm manifests, Electron/Vite/builder/TS/Tailwind/PostCSS/ESLint/dev-app-update config | Retire production role, retain configuration/reference tooling; do not delete dependency junction |

## Dependencies that must be cut over first

- `native.bat:4-11` runs Node-based ONNX/libclang/RNNoise preparation even for ordinary
  developer launch. A packaged native executable/bootstrap must run without Node,
  npm, Electron or development caches. Node-based dev/reference tests are not an
  established end-user production dependency.
- `strings.rs:11-12` embeds original renderer RUEN. `neural.rs:10-13` embeds DFN3 and
  Silero models from renderer/public, and native packaging hashes those same paths.
  Native-owned shared resources must keep exact bytes/license/provenance while all
  compile/test/packaging references are updated. Existing catalogs differ from the
  old d088795 checkpoint; do not substitute old locales without exact-byte checks.
- WebCrypto, voice/manual-gate/mic/VAD/Silero/neural compatibility tests still run
  Node or original TS/WASM; visual captures use React/CSS/Tailwind/Electron; native
  achievements/presence/localization tests read original definitions/catalogs.
  Keep those tests/fixtures/capture inputs and immutable reference resolution.
- Copy-import export.cjs is snapshot-only transitional Electron tooling. It must
  remain until real Chromium recovery, independent account/key proof, private
  zeroizing pipe, registered original process/children and app UI are accepted.
- Native updater executable is still preview-only. Host/health/full drain/bootstrap
  gates remain. Package allowlist currently excludes separate native update helper;
  its certified native distribution route must be explicitly accepted.
- Root npm build/CI still select Electron/WASM. Native is not a Rust default member.
  Final cutover must select native executable/features explicitly; old build success
  does not prove native readiness. Do not rewrite old config before preserving it.

## Material outside deletion scope

Retain current RUEN/catalogs, model bytes, sound assets/source licenses, icons/fonts,
native SVG/C++/Rust camera/input/stream-audio/DSP/update-helper code, vendored patches,
legal notices/installer license texts and pinned SDK/model/runtime caches. `rust/target`
is not wholly disposable: .cargo configuration points to required cached inputs.

Retain tests, migration-artifacts, docs, graphify-out, backend benchmarks and exact
interoperability/reference fixtures/configuration. Backend .NET and SFU Go are not
legacy frontend; this tree's .NET benchmark source does not establish production
backend/SFU readiness. Original user source/profile/settings/history remain untouched.

Complete UI/sound/device/physical/two-device/media/SFU/RC/long-session/import recovery/
updater drain/crash/bootstrap/license/trust acceptance remains open. Absence of
Electron imports from selected Rust files establishes none of those gates.
