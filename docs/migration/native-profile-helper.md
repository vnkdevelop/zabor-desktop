# Copy-export process boundary

This independent stage follows `dfb735a` / `07cb3bd`. It does not own authentication,
account/public-key proof, consent, shell, root manifests, updater helper, packaging,
UI or full-client acceptance.

## Owning API

`rust/crates/zabor-profile-helper` is now registered in the root Rust workspace.
Its original isolated `[workspace]` has been removed; the historical standalone
Cargo.lock is retained as checked-source provenance, not the active workspace lock.
The root lock adds only the helper record, preserving all existing dependency pins.
The native manifest/lib and application wiring remain unchanged in this step.

### Primary workspace registration — 2026-11-09

Primary source636db12 integrated the checked import pair. The following independent
workspace group passed13helper tests,fmt,locked/offline metadata,strict all-targets
Clippy,normal helper feature tree,native-media library types and Graphify.
Evidence: `migration-artifacts/native-data/profile-workspace-2026-11-09/checked-v1/`.
All778previous lock records and default members remain unchanged. Only root membership,
one root lock record and the helper's standalone-workspace removal alter manifests.
The checked normal helper tree does not activate `test-source`. The primary native
manifest and the separate app-owned dev-only Slint renderer seam are not edited.
Graphify reported no topology change,retained its outputs and10zero-node warnings;
semantic documentation refresh and whole-native readiness are not claimed.
The exact guarded source save is recorded in this group's `save-verification.json`.
Native dependency,original registered worker/account-key/consent/recovery wiring and
real Chromium provenance remain required before end-to-end import acceptance.

1. Retain `OwnerRegistry` in the original application operation/drain owner.
2. `prepare()` registers an immutable operation before native process acquisition.
3. Obtain its `Cancellation`; it carries the original entry, not a fresh lookup by
   process ID or a newly issued operation ticket.
4. Construct `ExportRequest::copied_profile(runtime,helper,snapshot.root())` from
   trusted exact pinned files. It requires a local, non-reparse copied directory
   and the snapshot marker. Never supply an original profile to this API.
5. `start()` launches the native blocking owner. `finish()` joins that original
   owner and returns a `CompletedExport` with private `Zeroizing<Vec<u8>>` only
   after process/tree/pipe retirement and validation. No output Debug formatter,
   terminal, log, stderr capture or plaintext output file is provided.
6. Feed those bytes into existing `PreparedImport::prepare` or its `copy` extraction
   callback. Explicitly close the snapshot before publishing. MAIN's original
   Session/intent check and independent account/public-key proof remain required.
7. Dropping a waiter only requests cancellation. The registry keeps the original
   JoinHandle. `seal()` prevents new work; `drain()` cancels and joins all registered
   owners, including after an earlier owner failure. Run draining off the UI thread.

Registry Drop is a blocking final fallback, not asynchronous cancellation success.
Retain the registry in MAIN's registered teardown owner and explicitly drain it.
There is a 128-entry registry bound and a 64 MiB maximum output bound. At capacity,
only actually joined/closed or never-started cancelled entries are reclaimed;
outstanding owners are never detached. Existing cancellation tokens retain their
original immutable entry and observation even after registry history reclamation.

### Exact MAIN integration recipe

The separate development CLI now follows this recipe in `zabor_native::migration`,
with the native path dependency (no `test-source`), original intent/token/registry,
fresh selected-server authentication, independent expected key, explicit consent,
runtime/helper pins and whole-bundle approval. Authorized recovery is guarded by
the journal owner and original admission. See [native-import-command.md](native-import-command.md).
The historical remaining-MAIN statements below describe the earlier stage;
actual Chromium provenance and end-to-end real-profile acceptance are still required.

Bounded resume adds `MigrationOperation::with_registry` for the original caller-owned
registry and retains `MigrationRetirement` from real original entry/drain observations.
`execute_for_reuse`/`MigrationPublication::install` hold the actual profile gate until
installation under the original operation/ticket. These typed receipts are not
account/key/consent or client-readiness flags. Helper APIs, source and pins are unchanged.

- Root workspace registration and the single helper lock record are checked above.
  MAIN still adds the native path dependency
  `zabor-profile-helper = { path = "../zabor-profile-helper" }`, preserving the
  app-owned dev-only renderer seam and reconciling only the native package's new
  dependency reference. The `test-source` feature must not be enabled in the shipped
  native dependency.
- Before any snapshot/export, MAIN obtains explicit copy-import consent and an
  independently authenticated account ID/login plus expected public key (or a
  separately authorized new-key state). `ImportOwner::verified` validates shape
  and key validity only: its name does not authenticate a caller. Never populate
  it from export contents, a seed/export exit code, receipts or helper metadata.
- In the original registered blocking native operation, retain one registry and
  one immutable intent/session generation. Register `prepare()` and retain its
  `Cancellation` before acquiring copied-profile or process resources. Use the
  existing `PreparedImport::copy` extraction callback to construct the trusted
  request from that callback's `Snapshot`, call the same operation's `start` and
  `finish`, and return only the zeroizing bytes. Map helper failure to
  `ImportError::Export`; keep only secret-free observations for diagnostics.
- Existing copy checks source closure/pins and rejects nested source/beta paths;
  export opens only the copy. `PreparedImport::copy` closes the copy before
  returning a preparation. Do not publish on close/source-invariance failure.
- Pass the original still-authorized consent/account/key/intent/session check into
  `PreparedImport::publish`; it already checks admission before and after rename.
  Process success or a late-created operation is not admission. For logout,
  cancellation or teardown, cancel that original token, seal the same registry,
  and drain it off the UI thread before retiring its temporary resources.
- Startup remains fail-closed through existing profile/import/session markers.
  MAIN owns the command/UI that obtains fresh consent and independent account/key
  confirmation before invoking `recover` with `PublishVerifiedStage` or
  `ConfirmPublished`. `recover` itself verifies storage/journal state, not account
  proof. Abort/rollback use the explicit matching recovery actions; never infer
  acceptance from a leftover beta directory or automatically erase its markers.
- Runtime/helper paths and the entire deployed Electron bundle (including loaded
  DLLs/resources) require independently approved deployment provenance. An EXE
  signature/pin does not attest arbitrary writable sidecars. This crate pins the
  exact runtime/helper files, not an installer or the full runtime distribution.

The original isolated helper stage did not edit common startup/UI/authentication
or package registration. Primary has since checked only workspace registration;
application/account-proof wiring is still missing. The compiled synthetic runner
is not a production account-proof implementation; its seed public metadata is
fixture expectation only.

## Windows lifetime and isolation

- The root is created suspended using exact `lpApplicationName`, quoted arguments,
  Unicode environment and an explicit inherited HANDLE_LIST. Only the private
  stdout writer and the NUL stdin/stderr handle are inherited.
- A noninherited Job Object is configured with KILL_ON_JOB_CLOSE, no breakaway,
  and assigned before ResumeThread. Failed assignment terminates and observes the
  original suspended process; it never starts an unowned fallback.
- Normal completion requires the original process handle to be signalled with
  exit code zero, the job's ActiveProcesses count to reach zero, the original pipe
  reader to join with EOF, all native handles to close, and the entire output to
  validate. EOF without successful root exit, exit without tree retirement,
  malformed/truncated/empty/oversized JSON, reader failure and late cancellation
  cannot produce successful bytes.
- The reader reserves its exact bounded output capacity before reading any secret
  bytes; growth cannot free an unzeroized earlier allocation. It uses bounded chunks
  and a private pipe; its buffers and accumulated output are zeroized. JSON validation ignores values without building a second
  credential/JWK object. Native import still performs the semantic/owner validation.
- Timeout/cancellation/oversize stop the reader and terminate only the assigned
  job. Root exit also retires surviving descendants before returning success.
  Actual root/child retirement is observed; a cancellation flag is not a join.
- Native process and reader RAII guards retain original retirement on owner unwind.
  The suspended root already has an owning guard before job assignment. Every
  acquired Job/root/thread/stdout/NUL handle belongs to the original entry;
  `handles_closed` requires
  its actual successful final CloseHandle, including stdout/NUL/reader handles.
  A failed observation/termination can remain pending indefinitely. The configured
  timeout bounds useful export work, not kernel cleanup. There is no fabricated
  hard cleanup deadline or detached original owner.
- Only essential Windows environment entries are inherited. NODE_OPTIONS,
  NODE_PATH, ELECTRON_RUN_AS_NODE, logging variables and unrelated server secrets
  are excluded. APPDATA/LOCALAPPDATA/HOME/USERPROFILE/TEMP/TMP point to the copy.
  Chromium flags disable background networking/component updates and DNS resolution;
  the helper additionally denies renderer requests, permissions and new windows.
- Verbatim disk paths stay pinned for trust/file checks but are converted to normal
  local disk paths for child argv/cwd. Actual Node tests exposed and verified this
  libuv/Windows boundary; UNC/device/reparse paths are not accepted.

Job ownership is not a security boundary against malicious same-user code capable
of opening the controller and duplicating its job handle. Only trusted runtime and
helper code may be launched. Parent crash closes the last job handle and requests
child termination, but cannot constitute observed joins, successful cleanup or
automatic import acceptance. Temp copies can survive crashes and require explicit
cleanup after the original resources are known retired. Existing import journals
and receipts remain fail-closed.

## Source invariance and import contracts

`Snapshot` records a complete source-tree inventory of file hashes and directories
before copying, then compares it again while copy reader handles are held. The
copy allowlist is unchanged; uncopied `Preferences` and new root entries are checked
without being copied. Enumeration and total bytes stay bounded (100,000 entries /
2 GiB); unsafe or oversized trees fail closed. Hash scratch buffers are zeroizing.
`verify_source_unchanged()` checks both
inventory and bytes; prepare checks it before conversion and after stage build.
Snapshot closure still attempts copy cleanup on a changed source and reports the
failure instead of accepting the transaction. Its original source-directory pin
is retained until closure. Source files remain read-only, and the existing parent
escape rejection prevents writing staging/gates into the original.

The process boundary does not alter current-user session/identity DPAPI, settings
JSON, SQLite owner isolation, 14-day message retention, TOFU fingerprints, deletion
tombstones, P-256 or AES-GCM. Account/public-key verification is never inferred
from exported `userId`, JWK, operation metadata or a successful process exit.

## Real Chromium prerequisite and runner

`scripts/migration/native-profile-seed.cjs` creates only a fully synthetic marked
profile, with safeStorage-protected fixture session/JWK, real file-origin Local
Storage/IndexedDB, owner/foreign/expired/deleted rows, TOFU and an attachment.
Its deterministic identity is synthetic-only; the native parent derives the
expected public key before launch, rather than trusting helper key output.
The seed returns fixture/timing metadata, never a JWK expectation.
`native-profile-roundtrip.rs` observes seed completion, closes the source process,
copies it, exports from the copy through the real Job/pipe bridge, compares source
byte inventory, imports to native beta and checks DPAPI/history/TOFU/tombstones/
attachment sanitization and rollback. It never logs export contents.

`verify-profile-helper.py` generates an approved-Temp isolated harness, mirrors the
existing vendor patches, rejects changed dependency pins, compiles the real native
and helper APIs, and optionally runs this roundtrip. It removes its harness in
`finally`; build output is isolated in this worktree's `rust/target`.

The local Electron candidate is version **44.4.3**, matching the repository npm lock:

- `C:\Users\porad\Documents\zabor\Frontend\zabor-desktop\node_modules\electron\dist\electron.exe`
- SHA-256 `bf0fe749904ca9f713ccfb2427c519fa39d0bbd0337ba411ba08785802e8d548`
- Offline WinVerifyTrust did **not** verify this candidate. No root/publisher bypass,
  network certificate fetch, reinstall, signing or self-derived hash approval was used.

Therefore actual Chromium roundtrip is **blocked by trusted-runtime provenance**.
Installed version plus a newly computed digest is not sufficient approval. MAIN may
provide an independently approved exact digest through
`ZABOR_PROFILE_HELPER_TRUSTED_SHA256`; the runner then pins that exact file. The
environment value is an explicit runtime trust gate, not account/key proof. The
runner also requires independently approved script digests through
`ZABOR_PROFILE_SEED_TRUSTED_SHA256` and `ZABOR_PROFILE_EXPORT_TRUSTED_SHA256`;
it never approves candidate scripts with their freshly computed self-hashes.
Without all applicable provenance gates, no real browser or synthetic seed starts.
Mocks and signed Node process tests do not replace Chromium origin/IndexedDB proof.

## Verification status

Completed initial native boundary checks: 9 actual Windows process tests using the
offline-verified local Node runtime, including real child/grandchild accounting,
waiter drop, timeout, pre-start and late cancellation, incomplete/oversize/nonzero
output, environment exclusion and native-owner unwind. New crate all-targets Clippy
`-D warnings` passed. Existing six mocked exporter tests passed after flag handling.

Final affected checks passed: 32 native profile tests (including whole-source
invariance and copy cleanup), 13 Windows helper tests (real root/child/grandchild
processes plus registry/thread-only cases) and 6 mocked exporter tests: **51 distinct
tests** in this stage. Helper tests ran serially with poisoned parent NODE_OPTIONS,
NODE_PATH and ELECTRON_RUN_AS_NODE; no value reached the child. The new tests cover
exact-bound allocation, failed assignment of the suspended root and registry
capacity reclamation, including finished original cancelled-thread joins.
Native no-default all-targets
Clippy `-D clippy::all` passed; the pre-existing five no-default `mic_test.rs`
dead-code warnings and Slint deprecation remain outside this scope. New helper
crate all-targets Clippy `-D warnings` and targeted Rust formatting checks passed.
The isolated harness compiled the real native/helper bridge without dependency
pin changes. Its first attempt correctly rejected an unmatched registry VAD;
mirroring the existing vendor patches fixed that prerequisite without root edits.
The later private capacity-join correction did not change public bridge APIs;
the final helper was recompiled, tested and strictly linted afterward.
No changed pin, interrupted check or untrusted Chromium candidate is accepted.

Exact affected Cargo invocations (all through `cmd.exe /c`, own isolated targets,
existing offline dependency cache and local Windows/MSVC prerequisites):

- `cargo test --manifest-path rust/crates/zabor-profile-helper/Cargo.toml -j2 --offline --locked -- --test-threads=1`
- `cargo clippy --manifest-path rust/crates/zabor-profile-helper/Cargo.toml -j2 --all-targets --offline --locked -- -D warnings`
- `cargo test --manifest-path rust/Cargo.toml -p zabor-native --no-default-features --lib profile:: -j2 --offline --locked`
- `cargo clippy --manifest-path rust/Cargo.toml -p zabor-native --no-default-features -j2 --all-targets --offline --locked -- -D clippy::all`

Helper fixtures used `ZABOR_TEST_TEMP` in approved OpenCode Temp. The original six
exporter mocks passed via `node --test scripts/migration/native-profile-export.test.cjs`.
The final seed passed `node --check`; Python AST syntax and targeted `rustfmt --check`
passed. The verifier's `--check-only` compiled, rather than ran, Chromium code.

### Observed file digests

These SHA-256 values describe checked worktree bytes, not trust approvals. MAIN
must independently approve the actual deployed bytes; checkout/package line-ending
conversion can change script digests and must not be silently self-approved.

| File | Observed SHA-256 |
| --- | --- |
| `rust/crates/zabor-profile-helper/src/lib.rs` | `d36a2eceb1601823be42445f6c1a7c58de50dc82f5b77f38019a94f6d0a8a572` |
| `rust/crates/zabor-profile-helper/src/windows.rs` | `480da1e367c2d66b1b1dbe7cbf215e2de2bea5d05eb0af9255f772b184bd63de` |
| `scripts/migration/native-profile-seed.cjs` | `5de36e67820e5b491b5e5a1049b782ae98bf53e4ce609a0794a473b0d3e371a6` |
| `scripts/migration/native-profile-export.cjs` | `ac2a5029e14fe83ca94fe9dd9dff257fe30403046b97cdc39aa55c8f4ed3d242` |
| `scripts/migration/native-profile-roundtrip.rs` | `2601f0f9561e08ce4caf43bc57a807e0cfc03b29e2f431b98e50266e32e94c1a` |

## Own-session continuation handoff

- Session `ses_e44a7ac4cffeI1oF29tnNf75s2`; worktree
  `C:\Users\porad\.local\share\opencode\worktree\b1b3f5\native-profile-import`;
  branch `native-profile-import-copy-rollback`, pre-save HEAD `07cb3bd`,
  prior checked import source `dfb735a`. The checked process-boundary source is
  `43adbcd99126d995b39a3edafc1be8bc3e08bc9c`; subsequent Python command-admission
  follow-up is recorded below and does not replace its 51-test evidence.
  Do not reset or overwrite this existing work.
- Checked stage: native `profile_import.rs` / tests (source inventory checks); export CJS
  allowed launch flags; new `rust/crates/zabor-profile-helper` manifest/own lock,
  lib/windows/tests/runtime_probe; seed CJS, roundtrip Rust, verifier Python and
  this doc. Root Cargo/Cargo.lock/native Cargo/lib/shell/UI/updater/main untouched.
- Completed: 13 Windows helper tests, 32 native profile tests, 6 mocked
  exporter tests; helper strict all-targets Clippy, native Clippy-all and targeted
  fmt; isolated native/helper harness type check with unchanged dependency pins.
- Exact original jobs: `sh_1bbcf6c8a001fpiIjzEZ0d35O6` harness PASS,
  `sh_1bbcf6cd0001a9ugi8dxWRw02c` profile PASS,
  `sh_1bbde007200189lYDdUswcNyRV` native Clippy PASS.
  `sh_1bcb07441001NGyt50068trbKj` own-worktree Graphify update completed:
  13,859 nodes / 30,182 edges / 518 communities; eight source files yielded no
  nodes and HTML visualization was skipped above its size limit. Graph data retained.
  `sh_1bcb735b4001t78VEraf5cSpox` independent review completed; child
  `ses_e4348f697ffeByopW9iEkCYFzI` performed no edits, helper launch or tests.
  Six concrete findings were addressed with scoped patches and regressions.
  Final originals completed: `sh_1bcc0add0001OuyQ7Umj34yHyQ` helper 12/12,
  `sh_1bcc0adef001vVjvR0tUIcOF9g` native profile 32/32,
  `sh_1bcc0ae0e001Zv3oAVT1QhRDGt` real-API harness compile PASS.
  `sh_1bcc0af5d001y4ISasK7gaaZWP` follow-up review completed; other fixes had no
  actionable regressions. Its remaining finished-cancelled-owner capacity finding
  was addressed by joining only finished original cancelled actors at pressure,
  scrubbing their results, then reclaiming entries. A distinct original-thread
  regression was added without pretending to test a Chromium process.
  `sh_1bcc63640001brMBx7c1gf6DdF` final helper 13/13 PASS, followed by helper
  strict all-targets Clippy PASS. `sh_1bcc867b4001TF2yQPj632tM46` final own
  `graphify update .` completed: 13,871 nodes / 30,213 edges / 541 communities.
  The eight zero-node-file warning and oversized-HTML skip remain explicit;
  code graph/report and curated backup are retained. Pending shell jobs: NONE.
  Do not poll or duplicate completed jobs. Earlier VAD mismatch and timeout are
  failures superseded by the named completed checks, not separate passing tests.
- Original ownership: registry retains original owner JoinHandle; suspended root
  assigned to non-breakaway KILL_ON_JOB_CLOSE job before resume; restricted private
  stdout/NUL inherited handles; original reader and root/tree exit observed before
  Zeroizing output. Waiter Drop cancels only; registry drain and RAII unwind retain
  joins/retirement. All test roots/children/pipes/handles have completed retirement.
- Source/root/session/key/consent gates stay as described above. Account/public-key
  proof and startup UI recovery are MAIN-owned; do not infer proof from export.
- Actual Chromium BLOCKED: local unsigned/unverified Electron 44.4.3, candidate
  SHA256 recorded above; offline WinVerifyTrust failed. No trusted hash approval,
  install, junction, legacy launcher, real profile or browser launch was attempted.
- All three completed own Cargo targets were removed, including the recreated
  final helper target. No harness/native/helper target or unfinished fixture remains.
  Graphify data and original review/check evidence are retained.
- Resumed from compacted own context; no second compact request. Final review
  found Vec-growth deallocation could retain secret bytes. WIP now reserves the
  bounded stdout allocation before reads, tracks all native handle closures,
  registers the suspended root guard before assignment, retries cleanup on unwind,
  removes JWK-bearing assertion diagnostics and sets isolated Cargo checks to -j2.
  Whole-source inventory, independent fixture expectation/script approval and
  safe registry reclamation were also added following independent review. Three
  helper regressions plus the final cancelled-thread regression and one native
  regression extend the earlier 46-test stage to 51. Final affected tests, both
  Clippy gates and fmt/harness compile passed as detailed above.
- Next integration action belongs to MAIN: register the crate/dependency and supply
  independent deployment/account/key/consent/startup gates as described above.
  Then run the existing synthetic Chromium roundtrip only with trusted runtime
  and script approval. No main edits, push/release/shutdown in this stage.
- Compact admission is not compaction completion; do not restart or delete the
  service/session and do not treat provider admission as process cleanup proof.

Earlier failing Node launch tests were corrected by explicit child-path
normalization. The actual Chromium run remains blocked, not mocked or counted
as passed. Compile-only harness files were removed by `TemporaryDirectory`;
all three owned Cargo targets were removed after completed checks. Final code graph
synchronization is complete; the scoped source commit contains this handoff. No real AppData
profile, production service, physical input, signing, push, merge, release or shutdown.

## Python command-admission follow-up

- Baseline/own branch: `43adbcd99126d995b39a3edafc1be8bc3e08bc9c` /
  `native-profile-import-copy-rollback`; same original worktree. Primary read-only
  review `ses_e433220b9ffe9l6biMuLRS3Sfy` identified one P2 in the Python verifier:
  cmd could interpret an unquoted no-space ampersand before runtime trust refusal.
- Scope is only `verify-profile-helper.py`, its new `.test.py`, and this handoff.
  The common `run_cmd` boundary validates every payload part and cwd, quotes every
  part including trailing-backslash CRT handling, and uses an explicit raw
  `cmd.exe /d /v:off /s /c` command with `shell=False`, not `list2cmdline` alone.
  Runtime and derived paths are admitted before any subprocess or temp fixture.
- Fail-closed policy rejects `& | < > ^ % " ! ( )`, empty parts, control characters
  and non-space newline/whitespace. Thus even legal `C:\R&D\electron.exe` is
  intentionally rejected; valid paths containing ordinary spaces are preserved.
  Errors do not echo rejected payloads or credentials.
- Checks: `cmd.exe /d /v:off /s /c "py -B scripts\migration\verify-profile-helper.test.py -v"`
  passed **7 tests**, including 36 runtime/mode and 54 shared-boundary metacharacter
  cases. All launches are mocked, test environments contain only synthetic values,
  bad inputs assert zero subprocess and zero fixture creation, and valid-case own
  approved-Temp harness directories are verified removed. No Cargo or Chromium ran.
- Python AST syntax passed for both files. Source guard
  `git diff 43adbcd99126d995b39a3edafc1be8bc3e08bc9c --exit-code --`
  over native/helper sources, manifests/locks, exporter/mocks, seeder, Rust runner and
  shared native/UI files passed with no changes. Existing **51 tests are retained,
  not rerun**; account/key/consent/deployment provenance gates are unchanged.
- Own original process/reader/Job handles remain retired from the saved stage;
  this fix creates no new helper/process/private-pipe operation or Cargo target.
  Own temporary test resources are cleaned. Graphify data/evidence are retained.
- Primary's separate resource job `sh_1bccce238001GGtIeujvpvjpth` is user-reported
  pending outside this scope; never inspect, poll, duplicate or clean its resources.
  Own `sh_1bcd9023a001bsyyj6t61VVH0w` Graphify update completed: 13,885 nodes /
  30,232 edges / 518 communities. Its eight zero-node-file warning and oversized
  HTML skip are retained; code graph/report/curated backup were not deleted.
  Own pending jobs: NONE. The checked three-file follow-up is the commit containing
  this section on the original branch, preserving `43adbcd` as its parent.
- Own session `ses_e44a7ac4cffeI1oF29tnNf75s2` may now compact from this durable
  handoff after the scoped save. All required source/evidence is retained; no own
  runtime, pipe, fixture or Cargo resource is pending. Compact API admission is
  not compaction completion or process cleanup. MAIN still owns integration after
  its separate original resource completion and this checked P2 follow-up.
