# Operation-owned native update IO and protected recovery

Continuation of clean source `3321ee4`, isolated in `native-release-packaging`.
The ownership boundary remains new update IO/helper modules, their tests, separate
packaging scripts and this document. No `lib.rs`, shared Cargo file, shell, Slint,
Electron validator, DSP/SDK/CRT/ONNX pin or release approval is changed.

## Implemented filesystem boundary

`update_helper.rs` owns an update-area lease, protected `journal.bin`, protected
`current.bin` selector, immutable per-operation version directories and retained
signed receipts. This is an installation area, never the live native/Electron
user-data profile. Every record carries the native-beta profile, root scope,
highest accepted sequence and original operation. Journal/selector plaintext is
current-user DPAPI protected with canonical-root/identity-bound entropy. Copying
the ciphertext to another root is not a valid recovery operation.

The package is not progressively overlaid onto a running application. All files
are extracted into an owned stage, compared against the signed package digest,
exact file lengths/hashes and inventory digest, flushed, and moved to an immutable
version directory. Prepared state is persisted before the current selector can
switch. Schema-changing rollback is refused. Both old and new version trees are
retained; no original profile or installed client is deleted.

The native C++ helper in `update-helper/native_update_helper.cpp` supplies Windows
owner/DACL checks, rejects reparse paths, isolates only empty sandbox roots,
performs same-root `MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH)` replacement and
validates embedded Authenticode against an exact SHA-256 certificate DER pin.
The developer harness builds it with static CRT, `/W4 /WX`; it contains no
Electron, WebView or managed helper. A hash-pinned executable is required before
invocation. Live `UpdateRoot::open` additionally requires successful helper
Authenticode certification; an unsigned hash-only fixture helper cannot open a
live root. Runtime processes are launched directly without a shell wrapper, so
timeout/cancellation observes the actual helper PID rather than leaving a child
behind cmd.exe. Developer terminal commands remain explicit cmd.exe. No signing
key or production certificate is invented or loaded.

## Trust and release acceptance

The update manifest keeps the existing P-256/low-S exact-byte domain. Acceptance
is separately signed under `ZABOR-NATIVE-ACCEPTANCE-V1\0`. Its digest must match
the manifest, and its version/commit/inventory/schema/profile/certificate must
match the target. Real installation additionally requires all complete-native,
Windows 10/11, two-device, data compatibility and license approvals. The default
is fail-closed; a digest by itself is not approval.

Tests use ephemeral keys and signed **local-sandbox** receipts bound to a fresh
temporary root. Complete-native, Windows and two-device acceptance stay false.
A sandbox receipt cannot be interpreted as a live release receipt. No live
installation, production request, update publication or production signing occurs.

`VerifiedRelease::verify_retained` revalidates a stored receipt's signature and
exact manifest digest against the protected journal's sequence fence without
pretending an old installed receipt is a fresh network candidate. Fresh target
validation and activation expiry remain mandatory. Rollback keeps the highest
sequence so old releases cannot be replayed; a newer signed retry remains possible.

## Operation/cancellation/observation

`update_io.rs` registers an original Tokio join **before** releasing an admission
barrier and before download-stage acquisition. It retains that join separately
from the UI/result receiver. Dropping or cancelling a result cancels work; it is
not native completion. Disk workers' original blocking joins are awaited rather
than aborted. `drain` seals admission, cancels registered work and observes every
original join and outcome. Main must retain and drain the owner across app exit.

HTTP uses an explicit signed-release URL policy, no proxy or redirect fallback,
connection/request deadlines, bounded chunks, signed total length and streaming
SHA-256. The only HTTP test endpoint is explicit `127.0.0.1:PORT` under a sandbox
scope. Production URLs are not contacted. Stale operations, late cancelled
responses, wrong lengths/digests and incomplete downloads are not staged success.

## Recovery and health

Persisted transitions are `Prepared -> Switching -> AwaitingHealth -> Committed`.
Unconfirmed states recover to the retained previous version; rollback itself is
idempotent across another interruption. Health acknowledgment must carry the
original operation, exact target manifest hash and protected random challenge
before its stored deadline. Missing or stale acknowledgments do not commit.
Directory names and receipts are validated again on recovery, not trusted from
an unsigned path string. The installed manifest is compared against the retained
authenticated archive, and the full installed file set and license text hashes
are rechecked. File hashing uses bounded read buffers.

`current_executable` is the bootstrap/recovery route. During the original
`AwaitingHealth` operation the host uses `pending_executable` instead. Recovery
resets active admission: an old pre-restart operation cannot resume with a fresh
token. Cancellation also binds to the root's original operation, so replacing a
token does not undo it.

Cancelled result waiters return without aborting original blocking owners.
Cancelled drain futures return unobserved handles to the registry, retain earlier
outcomes and refuse concurrent drain ownership. Atomic replacement errors with an
uncertain outcome require reopening the protected disk journal, not retrying
stale in-memory state. A successful native join alone is not installation approval.

## Explicitly remaining integration/acceptance gates

- Main-owned module/dependency wiring is untouched. These sources use only
  existing lock-pinned dependencies; the temporary harness supplies module wiring
  outside the repository. `zip 9.4.1`/existing flate2 backend will need direct
  main-owned wiring, not a version or lockfile update.
- Provision trusted manifest and acceptance public keys, publisher certificate
  pin, and independently authenticated native helper. Resolve the actual complete
  owner acceptance; local test receipts are not release approval.
- Integrate the original registered helper-host/process lifetime, app/session/
  media/input/file drain and an owner-authenticated health IPC channel. `ClosedDrain`
  refuses activation by default. A test drain works only for sandbox scope.
- Open/begin/activate/recover are synchronous helper-host operations. Main must
  register their blocking/process owner before acquisition, not run them on UI or
  inside shared intent locks. The native primitive is implemented; main-owned
  host launch, authenticated IPC, process identity and resource servicing remain
  explicit integration gates.
- The application/launcher must resolve the protected selector and apply recovery
  before opening profile files. First explicit native installation must establish
  an accepted retained baseline. Original Electron/native profile import belongs
  to the copy-import owner and is not done here.
- The protected root has real Windows owner/DACL/reparse checks, but the tests are
  not a hostile same-user/admin, handle-relative namespace-race or universal ACL
  acceptance proof. DPAPI is not protection against arbitrary code already running
  as the same Windows user.
- Flushed files and atomic selector replacement are not a demonstrated power-loss,
  NTFS/AV/hung-volume, OS crash or Windows 10/11 long-session guarantee. Process
  failure injection is narrower than power failure. No directory-flush guarantee
  or full-client crash/drain acceptance is claimed.
- Deadline/cancellation checks bound admission, HTTP and normal worker boundaries;
  a synchronous driver/filesystem/DPAPI operation can still stall. A cancelled
  waiter is not a completed native task. Main must observe actual drain results.
- No actual app binary is launched, no live installation is authorized and no
  physical device or complete neural/UI matrix is exercised. Full readiness=false.

## Scoped check command

From this worktree root:

```text
cmd.exe /c python -B scripts/native/packaging/verify-update-io.py
```

It creates and cleans an external temporary harness in the approved OpenCode temp
directory, derives a minimal lock from the unchanged root lock, refuses any resolved
dependency version/checksum outside that lock, then uses locked/offline Clippy and
affected tests. No SDK/DSP/inventory scan is repeated. Final exact test counts,
failure-injection coverage, cleanup and graph result are recorded on completion;
presence of a durable journal alone never turns on whole-client readiness.

`--workspace` can reuse an external directory directly below the approved OpenCode
temp root, with a marker binding it to this worktree. It rejects unowned directories
and never uses another agent's target. The agent removes this harness, target and
sandbox roots before the final scoped save.

## Checked scoped result

- Native primitive compiled Windows x64 with static CRT, `/W4 /WX`.
- External harness Clippy **all targets**, `-D warnings`, and fmt passed without
  suppressions or changing main-owned code/feature/lint settings.
- **46 tests passed, 0 failed, 1 ignored**. They include 19 retained release/update
  tests, 19 filesystem/helper tests and 8 operation-owned IO tests. The ignored
  crash worker is separately executed in four subprocesses, each terminating with
  exit 77 at Switching, SelectorSwitched, AwaitingHealth or Committed. Only a
  durably acknowledged Committed state survives as the target on recovery.
- Real checks include current-user DPAPI/ciphertext corruption and cross-root copy,
  owner/protected DACL, native unsigned-helper rejection, actual junction and ZIP
  symlink rejection, file/manifest tampering, actual disk replacement failure,
  unchanged schema, replay/retained sequence, health nonce/hash/deadline, late
  cancelled HTTP responses, wrong response lengths/digests, HTTP timeout, sealed
  admission, cancelled drain recovery and original panic/join outcome retention.
- Actual downloaded/extracted/switched payloads are **sandbox fixture packages**;
  no real native app image is launched or installed. No production signatures,
  keys/certificates/receipts or live profile are used. Sandbox receipt acceptance
  flags remain false, and sandbox HTTP is limited to explicit loopback origins.
- The harness derives and checks an exact subset of **194 existing root-lock
  dependency versions/checksums**; no shared Cargo file or SDK pin is updated.
  Source hashes are frozen and rechecked across compilation/tests. The root lock
  remains byte-identical. No inventory/neural/UI matrix is rerun.
- Early diagnostics were resumed after fixing only authored source/runner issues:
  missing MSVC linker environment, native PE-header variable shadowing and zip
  9.4.1's fallible decoded-name API. No warning level was lowered.
- Reusable external scratch used for these checks:
  `C:\Users\porad\AppData\Local\Temp\opencode\native-update-io-check-3321ee4`.
  It is removed after the completed run; no verification process remains pending.
- Own `graphify update .` completed after the source checks and external cleanup:
  13,924 nodes, 30,146 edges, 535 communities. Existing 9 zero-node source warnings
  and the 5,000-node HTML limit remain visible. The main graph was not written.

These results verify this isolated update IO/helper source scope, not full native
Windows crash/drain, ACL-adversary, physical, two-device or release acceptance.
