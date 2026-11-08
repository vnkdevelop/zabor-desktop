# Native beta packaging, signed updates and rollback boundary

Base: `3ba1ad8`, retaining the checked RC source `2108f86`. Independent branch:
`native-release-packaging`. Only `release.rs`, `update.rs`, the separate
`scripts/native/packaging/` directory and this document are owned here.

**Whole native readiness remains false.** This is a local build/package verifier
and a cryptographically checked updater contract, not release approval, a
published installer, or a completed native update service. No live profile,
production download, signing credential, publication, push, merge or shutdown is
used. Node and Python are developer-side tools; they are not package payloads.

## Distribution and composition

- Intended version: `3.8.0-rust`. Channel: `native-beta`. Profile:
  `zabor-desktop-native-beta`. Windows 10/11 x64, native Rust/Slint/WASAPI/WebRTC.
- The unchanged Electron policy classifies beta only when the first prerelease
  identifier is `beta`. It also requires the Electron beta executable and
  profile. Therefore `3.8.0-rust` is **not** a compatible Electron beta update.
  `upgrade_route` now returns `ExplicitBetaInstallation` for both legacy
  channels. `LegacyBetaBridge` is reserved, never asserted as verified.
- The local archive name is `ZABOR-Native-Beta-3.8.0-rust-x64.zip`. It is not an
  NSIS/Electron installer and is never offered to the original updater.
- Payload allowlist: `zabor-native.exe`, the two pinned ONNX DLLs for neural
  builds, `build-provenance.json`, and `licenses/`. No Electron, WebView, CLR,
  reference managed input/updater helper, JavaScript, Python, shell script, WASM
  runtime or native build `.lib` file is shipped.
- PE32+ AMD64 headers, normal and delay import tables, CLR absence, system DLL
  imports and all non-system imports are checked. Missing non-system DLLs fail,
  rather than depending on an accidental developer-machine installation.
- ZIP paths reject traversal, absolute paths, ADS, Windows reserved names,
  trailing-dot aliases, case collisions, symlinks, junctions and encryption.
  File sets, SHA-256, byte lengths and license text hashes must match exactly.
  Compressed and expanded package sizes are bounded to 1 GiB.
- Microsoft ONNX Runtime stays **1.31.1 / API 31**, with both DLLs and Microsoft
  LICENSE/ThirdPartyNotices hashes copied from the existing preparation pins.
  A 1.17.1 or arbitrary replacement DLL fails composition checks.
- WebRTC `a545de4` and the existing static MSVC CRT are unchanged. The exact
  existing static library and SDK notice are additionally hash-checked.
  libclang 18.1.1 and RNNoise model preparation pins are unchanged. No dependency
  update or SDK patch is performed.

## Local commands

Run from this worktree root. All subprocesses invoked by the builder also use
explicit `cmd.exe /d /v:off /s /c`. Cargo is locked/offline. The build receives
only named non-secret toolchain/Windows environment variables and fixed cache
paths; no `.env`, attestation secret, signing key or credential is read.

The cache argument below is the existing **read-only** pinned cache. All compiler
outputs belong to the selected worktree's separate target directory. Missing or
changed caches fail; these commands do not download dependencies or updates.

```text
cmd.exe /c python -B scripts/native/packaging/test_package.py -v
cmd.exe /c node --test scripts/native/packaging/verify-update.test.cjs
cmd.exe /c C:\Users\porad\.cargo\bin\cargo.exe fmt --manifest-path rust/Cargo.toml -p zabor-native -- --check
cmd.exe /c "set CARGO_TARGET_DIR=C:\Users\porad\.local\share\opencode\worktree\b1b3f5\native-release-packaging\rust\target-packaging&& C:\Users\porad\.cargo\bin\cargo.exe test --manifest-path rust/Cargo.toml -p zabor-native --no-default-features --lib release:: --locked --offline -j 4"
cmd.exe /c "set CARGO_TARGET_DIR=C:\Users\porad\.local\share\opencode\worktree\b1b3f5\native-release-packaging\rust\target-packaging&& C:\Users\porad\.cargo\bin\cargo.exe test --manifest-path rust/Cargo.toml -p zabor-native --no-default-features --lib update:: --locked --offline -j 4"
```

Build only the normal native executable, without installation or release claims:

```text
cmd.exe /c python -B scripts/native/packaging/package.py build-binary --cache-root C:\Users\porad\.local\share\opencode\worktree\b1b3f5\rust-p0-backend-bench\rust\target --target-dir rust/target-packaging
```

`--neural-disabled` is an explicit diagnostic variant, not the accepted complete
beta. `--release` selects Cargo's existing release profile. `--clippy` additionally
checks the matching native-media library with `-D warnings`. Neither changes pins.
The default enables native-media and neural-dsp. The executable is never launched
by the builder, so no physical audio, camera, desktop capture or profile is used.

Inventory generation is available even when upstream license texts are missing:

```text
cmd.exe /c python -B scripts/native/packaging/package.py inventory --cache-root C:\Users\porad\.local\share\opencode\worktree\b1b3f5\rust-p0-backend-bench\rust\target --output rust/target-packaging/license-inventory
```

Create a new **unsigned local** archive only after the full license text inventory
is present; an existing output is never overwritten:

```text
cmd.exe /c python -B scripts/native/packaging/package.py build --cache-root C:\Users\porad\.local\share\opencode\worktree\b1b3f5\rust-p0-backend-bench\rust\target --target-dir rust/target-packaging --output rust/target-packaging/ZABOR-Native-Beta-3.8.0-rust-x64.zip
cmd.exe /c python -B scripts/native/packaging/package.py verify rust/target-packaging/ZABOR-Native-Beta-3.8.0-rust-x64.zip
```

This build currently fails closed for missing upstream license texts. That failure
is distinct from compiler success and from signing/acceptance blockers. Test
fixtures create, verify and remove actual local ZIPs; they are not real installers.
Local package verification always reports `installationAllowed: false`.

After the server restart the supplied read-only cache root no longer contained
`webrtc-win-x64-a545de4`. The final real `build-binary --neural-disabled --clippy`
attempt stopped at that offline prerequisite check. **No newly built native app
or real application package is claimed.** Restore only the exact checked cache
bytes/pins above before running the build; no alternate SDK or download fallback
was attempted.

## License inventory and provenance

The collector traverses the Windows-target filtered `cargo metadata --locked
--offline` graph from `zabor-native`, excluding dev-only dependency edges. Normal
and build edges form a conservative transitive superset: optional features unified
by workspace metadata and build-tool dependencies are not incorrectly described
as an exact executable link map. This can include Skia/Vulkan build-side packages,
despite the actual client selecting femtovg.

Each package records its declared SPDX/license-file expression, selected Slint
GPL-3.0-only option, source/repository/authors, registry checksum from Cargo.lock,
manifest hash, local patch-document hash when present, and the exact copied
license/notice source and payload hashes. It does not replace absent upstream
copyright text with a generic MIT assumption. SDK multi-component `LICENSE.md`,
ONNX notices, libclang build-only notice, installed Rust/LLVM license bundle and
repository Phosphor/model/SQLite notices are included. Embedded DFN3/Silero model
hashes are recorded separately; absent Silero upstream license text is explicit.
The toolchain bundle and normal/build closure are deliberately conservative.

Initial real inventory: 556 components, 1,011 copied text files, 23 missing crate
notices, before adding the separately identified libclang/model records. It used
the following exact bytes:

| Item | SHA-256 |
| --- | --- |
| Cargo.lock | `de798febe6ef3ec707f994f5eb26d834ab8f04edaedbcf40440fa0d268f7129f` |
| WebRTC static `lib/webrtc.lib` | `3e5959b5320579814706848a0641224db22f03a374493e90cbd235948977f930` |
| WebRTC SDK `LICENSE.md` | `118329b312ed24efd1ae1a7b16c1ce51f7890d30ddc7e7075a04a9cd30c096c2` |
| Initial local inventory JSON | `3ee7732bd19b06a7b06a44dcb69a4e0d80e2ef104ef5fb88fff250a53b125024` |
| Final local inventory JSON | `65f998578fbe7e46e43eb8f5deab79950afc9ec6dee92e97df763edaa6bd2fed` |
| Existing DFN3 model | `c94d91f70911001c946e0fabb4aa9adc37045f45a03b56008cb0c8244cb63616` |
| Existing Silero model | `1a153a22f4509e292a94e67d6f9b85e8deb25b4988682b7e174c65279d8788e3` |

Final actual inventory: **559 components, 1,012 unique copied text files, 24 missing
upstream texts** (the same 23 crate notices plus Silero's model license). Legal
approval and native readiness remain false. The temporary bundles are removed;
the reproducible collector and precise provenance summary are retained here.

The 23 missing crate texts in that audited cache are:

```text
accesskit 0.24.1; accesskit_consumer 0.38.0; accesskit_windows 0.34.0;
accesskit_winit 0.33.2; clipboard-win 5.4.3; const-field-offset-macro 0.2.1;
const-field-offset 0.2.1; gl_generator 0.14.0; khronos_api 3.1.0;
libwebrtc 0.4.0; lyon_algorithms 1.0.22; lyon_extra 1.1.0;
lyon_geom 1.0.20; lyon_path 1.0.19; profiling 1.0.18; rspolib 0.1.2;
skia-bindings 0.153.3; skia-safe 0.153.3; spirv 0.4.0+sdk-1.4.341.0;
taffy 0.10.1; vtable-macro 0.5.0; vtable 0.5.0; webrtc-sys-build 0.3.21.
```

Both `inventory` and `build` accept `--license-supplements LOCAL_JSON`. Its schema
is `supplementVersion: 1`, with `texts` entries containing `package`, `version`,
`file` relative to that manifest, exact nonzero `sha256`, and `source` pointing to
an immutable 40-hex GitHub blob/raw revision. No URL is fetched. The collector
checks local bytes and records the provenance; human legal review remains a
separate gate. Silero uses package `silero-vad-model`, version
`existing-local-model`. No supplement or legal approval is fabricated here.

## Signature contract

`VerifiedRelease::verify` accepts exact raw flat JSON bytes, a 64-byte IEEE P1363
P-256 signature, and an independently pinned SEC1 public key. The signed message
is `ZABOR-NATIVE-UPDATE-V1\0` followed by those exact bytes. High-S signatures,
missing keys/signatures, duplicate/unknown JSON fields, wrong channel/profile,
architecture/platform, path overrides, zero digests and oversized inputs fail.
The signer is not the client attestation HMAC and no symmetric client secret is
reused for updates.

`NativeUpdateManifest` carries version/commit, monotonic sequence, issuance and
expiry (maximum seven days), data schema interval, package name/length/digest,
inventory digest, publisher certificate DER SHA-256 and acceptance receipt digest.
It cannot trigger equal-version/build-metadata updates or downgrades. The package
digest is checked independently. Digest fields alone are not approval or trust.

Developer-side interoperability verifier:

```text
cmd.exe /c node scripts/native/packaging/verify-update.cjs MANIFEST.json MANIFEST.sig PUBLIC-SPKI.der PACKAGE.zip NOW-UNIX-SECONDS MINIMUM-SEQUENCE
```

It uses the same domain, P1363 signature and P-256/low-S checks, but accepts public
SPKI DER rather than SEC1. Inputs must already be local. It never downloads,
signs, installs or grants readiness. There is intentionally no private-key CLI.

`authenticode.py` calls Windows WinVerifyTrust and crypt32 directly, not
PowerShell, to validate an embedded signature and **exact DER certificate pin**.
It does not match a publisher subject substring. Chain revocation excluding the
root is cache-only and fails closed if trusted cached evidence is unavailable.

```text
cmd.exe /c python -B scripts/native/packaging/authenticode.py zabor-native.exe --certificate-sha256 TRUSTED-LEAF-CERTIFICATE-DER-SHA256
```

No authorized publisher certificate/key was supplied or used. Positive Windows
signing/trust acceptance is unproven. Unit tests use ephemeral in-memory signing
keys and clearly local fixture bytes, never a stored private key or fake approval.

## Native integration required outside this ownership scope

1. Fetch a bounded signed native manifest under the original update operation and
   app/session lifetime. Pin public SEC1 key and publisher certificate in trusted
   native configuration, not values learned from an unsigned response. Resolve and
   verify the **current complete-native** acceptance receipt against its digest,
   source commit, package/inventory and full owner acceptance. Existing Electron
   `client-release-approval.json` is not this native approval and is unchanged.
2. Bounded download to an owner-isolated stage; reject unexpected redirects,
   lengths, duplicates and stale callbacks. Verify raw manifest signature before
   parsing/trusting it, then package SHA-256/length, strict ZIP composition and
   inventory digest, actual licenses and the independently pinned publisher
   Authenticode for the own executable/helper. Vendor DLLs retain their exact
   pinned upstream hashes and vendor signing provenance; never re-sign them under
   the ZABOR certificate and silently invalidate the original runtime pins.
   `UpdateModel` is presentation state only; its `Installing` phase is not a
   security permit.
3. Populate **all** `ReleaseGate` fields from verified evidence. Its default is
   closed. Passing package or unit tests never turns on complete-native, physical
   Windows/two-device/data acceptance or either signing pin. `activate` rejects
   incomplete gates, stale operations and manifests expired at activation time.
4. Register/observe the original native helper task before acquisition; drain app
   media/input/session/file owners before replacing executables. A helper must be
   native, separately signed and pinned; do not ship the managed reference helper,
   Node/Python, or force-kill the app. No app/helper integration is edited here.
5. `UpdateTransaction::stage` requires a strictly newer authenticated target and
   authenticated retained previous release, original nonzero operation, a persisted
   sequence at least as high as the installed receipt and **unchanged data schema**.
   A newer signed retry still works after rollback; replay of the rolled-back
   sequence does not. Schema migrations are rejected
   until the separate copy-import/rollback owner provides a safe contract. Never
   mutate original Electron profiles; profile copy/import is another owner.
6. Durable native helper must implement owner ACL/DPAPI-protected journal, original
   operation and both manifest digests, same-volume stage/version directories,
   flushed journal before atomic current-version replacement, bounded health
   handshake and recovery before opening profile files. These filesystem/Windows
   crash/disk/AV guarantees are **not** proven by this in-memory contract.
   Retain the previously verified installation receipt for rollback; fresh-candidate
   expiry checks must not be bypassed by arbitrary old network manifests.
7. `healthy` commits only the original operation and exact target manifest hash.
   Interrupted staged/awaiting-health work requests rollback. Rollback confirmation
   requires the original operation and exact retained previous manifest hash.
   Retain the higher sequence even after rollback; no replay/downgrade escape.
   The helper's durable journal/storage and same-user receipt trust must enforce
   this across restart. UI closure never implies installer/native cleanup success.

No production-capable installer or automatic update loop is enabled without these
integrations. Physical Windows 10/11, long-session, full UI/media/profile/data and
two-device acceptance remain with the main native migration owner.

## Verification boundary

- Python: 17 local ZIP/PE/path/license/supplement/unsigned-Authenticode tests passed.
- Node: 7 ephemeral signed-manifest/package tests passed.
- Rust: final 15 update + 4 release tests passed under locked/offline,
  no-default-features, after the rollback-retry and activation-expiry fixes.
- Total: 43 unique affected tests. Final Cargo fmt, Python syntax (3 files) and
  `git diff --check` passed. Repeated earlier runs are not counted again.
- Strict `--no-default-features --lib` Clippy reached unrelated existing dead-code
  failures in `mic_test.rs` (`MicTestDispose`, `MicTestMaintenance` and methods).
  These files and lint settings were not changed or suppressed. The correct
  native-media check/build is blocked by the missing pinned SDK cache, not passed.
- Server restart canceled an earlier Clippy/Graphify run; its already completed
  14 update tests were retained as partial evidence, then superseded by the final
  15-test update run. Only unfinished checks were resumed.
- Own Graphify created a local code-only graph: 13,769 nodes/29,596 edges/525
  communities, with a concurrent removed-fixture enumeration warning, 853
  zero-node source warnings and the 5,000-node HTML limit. Generated build/license
  inputs were present during this initial enumeration. After cleanup the final own
  update completed: **13,720 nodes/29,550 edges/520 communities**, 9 zero-node
  warnings and the same HTML limit. This is a local code graph, not a replacement
  for the main curated graph or full acceptance evidence. The main graph was never
  written. No verification job remains pending.
- No complete default/neural DSP, visual, physical or production regression is
  repeated or claimed. Existing `viewport-y` Slint deprecation remains outside
  this ownership scope.
- Temporary fixture packages, profiles and inventories are removed; private
  fixture keys were memory-only. The owned `rust/target-packaging` test/build cache
  was removed after completion. Only intentional ignored own Graphify outputs
  remain. No generated executable is retained or claimed as release evidence.
