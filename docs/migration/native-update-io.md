# Operation-owned native update IO and protected recovery

Original IO/helper implementation continued from clean source `3321ee4` and was
saved as `88f3cd41`, isolated in `native-release-packaging`. The redirect continuation
owns only `update_io.rs`, `update_io_tests.rs`, nested `update_url.rs`,
`verify-update-io.py` and this document. No helper, C++, `lib.rs`, shared Cargo file,
shell, Slint, Electron validator, DSP/SDK/CRT/ONNX pin or release approval is changed
by the redirect continuation.

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

HTTP uses an explicit signed-release URL policy, no proxy or automatic redirect
fallback, a single overall deadline, bounded chunks, signed total length and
streaming SHA-256. The only HTTP test endpoint is explicit `127.0.0.1:PORT` under
a sandbox scope. Production URLs are not contacted. Stale operations, late
cancelled responses, wrong lengths/digests and incomplete downloads are not staged
success.

### Operation-owned manual release redirects

`UpdateIo::download(operation, candidate, cancellation, url, budget)` keeps its
existing public signature and result type. Private `url_policy::RedirectChain`
is wired by `update_io.rs` with a path attribute, without another root module or
dependency. It validates the initial URL before admission and validates every
`Location` before the next request. `Policy::none()` remains mandatory.

- Native initial URL is exactly
  `https://github.com/vnkdevelop/zabor-desktop/releases/download/v{signed_version}/{signed_package_name}`.
  The package name must be `ZABOR-Native-Beta-{signed_version}-x64.zip`.
- Subsequent native hops permit only that exact GitHub route or HTTPS
  `release-assets.githubusercontent.com`, with no explicit port. There is no
  wildcard `githubusercontent.com`, alternate GitHub route, private host, HTTP
  downgrade or fallback endpoint. GitHub changes to this allowlist require a
  separately reviewed policy change, not a permissive retry.
- Only 301, 302, 303, 307 and 308 are followed, as fresh credential-free GETs.
  There are at most **4 redirects / 5 requests**. Exactly one nonempty `Location`
  is required; raw `Location` and resolved URL are each bounded to **8192 bytes**.
  Invalid header text, controls, whitespace, backslashes, userinfo and fragments
  fail closed. Scheme-relative URLs are refused. Relative references use URL
  resolution and must preserve the current origin before allowlist validation.
- The original URL and accepted hop URLs are fingerprinted for loop detection.
  Signed CDN query bytes are retained as opaque request data, not interpreted as
  approval or copied into staging names, reports or error text. Raw SAS URLs are
  not logged by this module; transport errors map to fixed error codes.
- A fresh private reqwest client has no caller headers, authentication, cookie
  provider or credentials, disables referer generation and proxies, and has
  automatic redirects disabled. Response cookies/headers are not copied into
  any subsequent request. Redirect bodies are dropped rather than staged.
- The deadline is captured before job admission and used with `timeout_at` for
  every hop and body read. Connection timeout is additionally capped at 5 seconds.
  Redirects do not reset the budget. The returned archive privately retains that
  deadline; staging may shorten it with its own budget but cannot extend it.
- Every hop checks the same root, original operation and cancellation token before
  and after request/validation. There is one registered download join, not a new
  job or operation per redirect. Admission barriers, blocking-owner joins,
  cancellation-safe drain and signed receipt/acceptance checks are unchanged.
- Sandbox mode permits only an initial explicit `http://127.0.0.1:PORT` and
  redirects on that exact origin. It cannot transition into the native policy,
  another port, localhost alias, private host or production CDN.

Final success still requires HTTP **200**, exact signed byte length and exact
signed SHA-256, including when `Content-Length` is absent. A permitted redirect
host or a completed HTTP request is never a release/installation receipt.

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

- Main-owned module/dependency wiring is untouched in this worktree. The parent
  reports that `88f3cd41` was already integrated as `8768a18`, with module wiring
  `a2dac6a` and 46 passing main tests. This continuation neither repeats nor edits
  that work. Its temporary harness supplies isolated module wiring and uses only
  existing lock-pinned dependencies, without a version or root-lock update.
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

## Original `88f3cd41` checked scoped result

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

## Checked primary library integration

The saved child source88f3cd4 was imported as8768a18. Main-owned Windows module/
dependency wiring is checked and saved as
`a2dac6a802a2451564d89a09edd68bc101606b17`: update_helper/update_io are exposed by
the native library, and direct zip9.4.1/flate2 1.1.10 dependencies reuse existing
root-lock pins. Only two local zabor-native dependency edges changed; no external
dependency version/checksum, SDK/CRT/DSP/ONNX or Electron validator changed.

Primary grouped check `sh_1bbe873d90017BKbgO7lUZskqp` passed46 distinct tests
in the main native crate, both native-media all-targets Clippy modes, fmt, normal
native build and separately compiled staticCRT/W4WX fixture helper. Main
Graphify7385nodes/18711edges/302communities;7priorzero-nodewarnings/HTML5000limit
remain. Frozen13source/pinhashes are unchanged. Evidence:
`migration-artifacts/native-data/update-io-integration-2026-11-08/checked-v1/` and
`save-verification.json`. Own temporary helper/fixtures/runner were removed.

Normal executable SHA-256:
`11b19deb865d700ce9e51d9460363647fe03664e7436a3bd335323533ad7a558`.
This build proves library integration, not an installed package or whole-client
receipt. Original helper-host launch/registered blocking ownership, app/resource
drain, authenticated health IPC, bootstrap baseline/recovery, real trust pins/
certification, legal approval and all listed hostile/physical/crash gates remain
required. No live root was opened or installed; publication/shutdown remain false.

## Redirect continuation source receipt

Own branch/base: `native-release-packaging` / `88f3cd41e013511c528b1e4fb9c3137a528e780a`.
Verification freezes source bytes for the duration of the external harness run.
SHA-256 of the checked working-tree bytes:

| File | SHA-256 |
| --- | --- |
| `rust/crates/zabor-native/src/update_io.rs` | `91c8c8024853a985d73d6d7c9a66aa09d796234afb96fa67eb99372e5584f690` |
| `rust/crates/zabor-native/src/update_io_tests.rs` | `1da600640095cebe496a50d17ced3ffa1b928178c20d84c736111f75d32aab18` |
| `rust/crates/zabor-native/src/update_url.rs` | `0f21f90ec0dbe9b66d4f6a9852bf3c550ae4cac94364e178c23e842755b653fd` |
| `scripts/native/packaging/verify-update-io.py` | `5a805d6024d7575ce12af4d41c86cfd3b60622f1e99face40784cb78d005ca26` |
| Unchanged `rust/Cargo.lock` | `de798febe6ef3ec707f994f5eb26d834ab8f04edaedbcf40440fa0d268f7129f` |

## Redirect continuation checked result

- The existing external frozen-dependency harness compiled the native primitive
  with static CRT, `/W4 /WX`; Rust compilation, fmt and **all-target Clippy
  `-D warnings` passed** without suppressions or shared-file changes.
- **59 passed, 0 failed, 1 ignored**: 19 release/update, 19 helper, 15 IO and
  6 pure redirect-policy tests. The existing ignored crash worker remains covered
  by its parent subprocess test; no helper implementation or new helper test was
  added by this continuation.
- Redirect tests cover the exact native initial route and CDN allowlist without
  network calls; same-origin relative/absolute loopback redirects; missing,
  duplicate, malformed, non-ASCII and oversized `Location`; resolved-URL size;
  loops and hop exhaustion; host/port/userinfo/fragment/downgrade rejection;
  cookie/referer/auth-header non-forwarding; redirect-body disposal; exact final
  200/digest/length, including truncated or oversized bodies without a length
  header; stale/cancelled original operations between hops; total deadline and
  refusing a fresh stage budget after that deadline.
- Two new test-fixture errors were corrected after the first run: a parser-invalid
  maximum HTTP length was replaced with a valid oversized redirect-body length,
  and the relative loop reference was corrected to resolve to the current URL.
  The production redirect policy was not relaxed.
- **194 existing root-lock dependency versions/checksums** remain the exact
  external subset. The root lock and frozen source hashes were rechecked by the
  successful harness. Python AST syntax and `git diff --check` also passed.
- Owned scratch
  `C:\Users\porad\AppData\Local\Temp\opencode\native-update-io-redirect-88f3cd41`
  was removed after the completed run, including its target, helper binary and
  sandbox roots. No build/test process remains pending.
- No real GitHub/CDN request, production signing, app installation/launch, push,
  merge or OS shutdown was performed. No inventory/neural/UI matrix was run.
  App-host/process identity, authenticated health, whole-app drain, OS/power-loss
  crash acceptance and provisioned production trust remain **false/unapproved**.
- Own `graphify update .` completed: **13,968 nodes, 30,247 edges, 536 communities**.
  A follow-up graph query resolves `RedirectChain`, `start`, `advance`, `validate`
  and the policy tests to `rust/crates/zabor-native/src/update_url.rs`. This is the
  CLI's code-only AST refresh, with no LLM call; doc/paper/image semantic refresh
  was not rerun. No main graph was written.
- Exact remaining Graphify diagnostics: **9 zero-node sources absent from the
  graph**, and HTML skipped because **13,968 nodes exceeds the 5,000-node limit**.
  The zero-node JSON paths are `.vscode/launch.json`, `client-release-approval.json`,
  `rust/crates/zabor-native/assets/locales/en.json`,
  `rust/crates/zabor-native/assets/locales/ru.json`,
  `rust/vendor/rnnoise2-sys-0.1.2/.cargo_vcs_info.json`,
  `rust/vendor/webrtc-vad-0.4.0/.cargo_vcs_info.json`,
  `scripts/native/packaging/policy.json`, `src/renderer/src/locales/en.json` and
  `src/renderer/src/locales/ru.json`. The CLI also reports that doc/paper/image
  changes require the AI `/graphify --update` workflow and suggests setting
  `GEMINI_API_KEY` or `GOOGLE_API_KEY` for semantic extraction. No key was requested,
  printed or provisioned here. No Rust/Clippy warning was suppressed. Git's
  LF-to-CRLF checkout-normalization notices do not indicate a diff/fmt failure.
