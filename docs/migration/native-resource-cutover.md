# Native-owned resource cutover

Prepared from primary `f7de0e0ffd8db19f7eba41e6f999f2524e5ebf8a` on 2026-11-08.
This separates compile-time native resources from the retained React/Electron
reference. No renderer resource, model pin, setting, profile or legacy code is
removed or modified. No whole-client or legal acceptance follows from this step.

## Exact byte ownership

`rust/crates/zabor-native/assets/resources.json` records four native resource paths,
the original source paths, byte counts, SHA-256 and original Git blob identities.
Native `strings.rs` and `neural.rs` embed only `../assets/catalogs/` and
`../assets/models/`; string/model behavior, parameters and ONNX API31 remain unchanged.
The DFN3 and Silero copies keep the existing notices and upstream provenance.

The current Windows RU/EN working copies use CRLF; their HEAD Git blobs use LF.
The only working-copy/Git difference is checkout EOL conversion. Copying preserves
the current working bytes, not a historical catalog or reserialized JSON.
The narrow `.gitattributes` rule disables EOL conversion for these native locale
copies so their manifest hashes survive checkout. `sourceGitSha256` separately
records the normalized original Git bytes; it is not the native file hash.
The same narrow rule marks CR as an EOL byte for Git whitespace checks; text diffs
remain enabled and catalog bytes are not normalized or reformatted during staging.
The pre-existing `assets/locales/` contains updater-only dictionaries used by
`localization.rs`; it is preserved byte-for-byte and is not the full app catalog.

## Local verification

- `cmd.exe /c python -B scripts/native/verify-resources.py` checks native bytes
  without Node, Electron, renderer resources or model download.
- `cmd.exe /c python -B scripts/native/verify-resources.py --reference` additionally
  requires exact retained working-copy bytes; a source checkout using different
  EOL conversion deliberately fails that strict comparison.
- `cmd.exe /c python -B scripts/native/verify-resources.test.py` checks the bounded
  complete inventory, corruption/EOL rejection, missing resources, reference
  opt-in, invalid paths/digests/sizes, redirected-parent rejection and native-only
  verification with no renderer.

Original primary group `sh_1bcb14ce4001SwW5xvISJjCDIN` completed partly in checked-v1:
exact native/reference bytes,11Python tests,4strings/updater-catalog tests,fmt and
normal all-targets Clippy passed. It stopped in ort-sys before neural types because
the configured ONNX1.31.1 cache was absent. The pinned RNNoise archive was also
absent in both the primary and checked original-project cache locations; no older
runtime or original-user-folder write was used. Owned fixtures/runner were removed.
The raw v1 receipt counted only4Rust tests because its parser missed Python CRLF
line endings; the complete11-test/OK log is retained unchanged. A separate correction
receipt in checked-v2 records15inherited tests,not a fabricated v1 success.

Current original primary group `sh_1bcca308a0010Aom5EoONz9INY` is pending in
`migration-artifacts/native-data/resource-cutover-2026-11-08/checked-v2/`. Existing
source hashes must exactly match v1 before inheriting its unaffected proofs.
The approved-temp `native-resource-check-v2.py` runner prepares only absent pinned
ONNX1.31.1/RNNoise caches via existing verified public-upstream scripts, then runs
only unfinished neural types,2in-memory model-load/reset tests,normalbuild and
Graphify. Both original preparation subprocesses are observed before proceeding;
no npm install,source/model-pin change,production request or user-project write.
The runner self-cleans after original completion. It uses primary's own `rust/target`,
separate from all child mutable targets; sources/pins remain frozen,no polling or
duplicate job. Expected final aggregate17tests,with15inherited rather than rerun.
No full neural/WASM/reference/visual/physical matrix is repeated for include paths.
Source-save receipt is pending. Only the primary owns these include-path changes; active app/media
owners were told not to edit these paths. Their targets/jobs remain independent.

V2 completed partly:ONNX1.31.1 and pinned RNNoise archives were prepared and
hash-verified;15inherited tests remain valid. Neural types next stopped in rnnoise
bindgen because configured build-only libclang18.1.1 was also absent. No resource,
runtime/model pin or code change is needed for this prerequisite. V2 owned temp
fixtures/runner were removed after its original subprocesses completed.

Current original continuation is `sh_1bccce238001GGtIeujvpvjpth`,checked-v3 evidence.
Its approved-temp `native-resource-check-v3.py` verifies unchanged prior sources,
prepares only absent pinned libclang18.1.1 with the existing verified script,then
unfinishedneuraltypes/2memorymodel-loadreset/normalbuild/Graphify. Existing ONNX/
RNNoise preparation and15passed tests are inherited,not repeated. Own target,-j2,
sourcefreeze/no polling/no duplicate jobs;runner self-cleans. No checked source
commit yet. Historical v1/v2 failures remain retained rather than relabelled success.

## Completed source-group verification

Original checked-v3 `sh_1bccce238001GGtIeujvpvjpth` automatically completed passed:
17unique affected tests (15unchanged inherited from v1+2memorymodel-load/reset),
fmt,normal all-targets Clippy,neural library types,normalnativebuild and Graphify.
All23source/reference/pin hashes match the initial source snapshot. The four native
resources exactly match the retained working-copy bytes; original updater-only
catalogs,renderer references,workspace/dependency/model pins remain unchanged.

The normal executable SHA-256 is
`d4b0af5f7f2349913f73a207e6fa989afaba3f8b4aaba1ca6137acbb1b59ccd0`.
This is component evidence,not default/full-neural/media/physical/client acceptance.
The previous v1/v2 failures and separate Python CRLF-count correction remain intact.
All original dependency-preparation/check subprocesses completed;own temporary
fixtures and runners were removed. Prepared dependency caches are required pinned
inputs,not disposable fixtures. No recursive target deletion is authorized.

Graphify updated7563nodes/19202edges/315communities. Ten files yielded zero nodes
(previous source warnings plus native catalog/manifest inputs);HTML was skipped
above5000nodes. Source-group success does not imply a semantic documentation refresh.
Scoped source save is guarded by exact staging and native-resource Git-byte checks;
its exact commit is recorded in checked-v3/save-verification.json after saving.
The first save guard stopped before commit because Git interpreted preserved CRLF
bytes as trailing whitespace. Only the catalog-path `cr-at-eol` attribute was added;
all checked Rust/Python/model/catalog/reference/pin bytes stayed unchanged. Scoped
staging,Git byte hashes and whitespace checks must pass before the source save.

## Remaining cutover

The update-host owner exclusively edits helper packaging while its work is active.
After delivery, change only `scripts/native/packaging/package.py` model inventory
hash inputs to the two native-owned model paths, preserving license records and
all helper changes. Until that integration, packaging still reads the original
byte-identical models. This document is the exact integration recipe, not a claim
that packaging already became renderer-independent.

Achievement/presence/localization/neural/WebCrypto/visual reference tests intentionally
continue to read the retained reference. They need immutable reference resolution
before final legacy removal. `native.bat`, Rust default members, Node dev preparation,
CI/release configuration, final asset/sound/font inventory, real trust/legal approval,
physical/multi-device/long-session and complete-native acceptance remain separate gates.
