# Native update host continuation

Worktree/branch: `native-update-host`, base `f2867df` (redirect source already
present). Primary redirect integration `a6f8e1f` and its 59-test/both-Clippy/build
receipt are not repeated. Latest user-reported primary context: `6b87a04`
workspace checked, sources disjoint. Prior `636db12` import-pair save and
`32cc1e7` clean/resource
`cba6f34` passed results are not revalidated here. Scope excludes
shell/main/lib/Cargo/UI/data/media/RC/import.

## Ownership and boundaries

`update_host.rs` retains the original Tokio owner before admission releases any
blocking helper, filesystem or process acquisition. Waiter cancellation is not
exit. Cancelled drain futures put unobserved original joins back; errors/panics
remain reports. No native wait/destructor runs under an application acceptance lock.

`update_host_process.rs` retains the actual helper PID and its pipe-reader join.
The helper starts the selected immutable target suspended, assigns it to a
kill-on-close Windows Job before resuming, and observes zero active descendants
plus the original target exit before emitting `E`. Rust separately waits the
original helper and reader. `S/H/A` are not descendant-exit evidence. Abnormal
helper exit without `E` fails closed, even though OS Job closure requests cleanup.

Health uses a random first-instance, local-only, current-user-DACL named pipe and
restricted inherited bootstrap handles, not command-line secrets or public ports.
The fixed 264-byte envelope binds operation, original owner/helper/target PID and
creation times, deadline, protected challenge, canonical-root identity digest,
manifest digest, native-beta profile digest, host nonce and owner SID digest.
Server checks the OS client PID/creation/user and exact echoed envelope. The Rust
client checks its own PID/profile, trusted expected root/manifest and private
bootstrap envelope before acknowledging. It disables delegation/impersonation.
Do not acknowledge merely because a window or current PID exists.

`UpdateHost::bootstrap` opens the protected root and resolves recovery/selector
before returning a current executable. `establish_baseline` requires the real
certified helper and signed complete-native acceptance before writing an empty,
already protected installation root. Baseline writes retain the original host
operation/token/deadline. Interrupted initial creation is not an accepted baseline;
partial roots fail closed and are not automatically overwritten/deleted.

Live packages now require exactly `zabor-native-update-helper.exe`; its bytes are
in the signed archive/file inventory and it is Authenticode-checked against the
same explicit publisher certificate during extraction/recovery. Offline package
build takes `--update-helper` and `--publisher-certificate-sha256`, validates the
staged static-CRT/system-only native image, and never signs or grants installation.
Local fixture packages remain distinguishable and cannot authorize a live root.
The host-capable image is built from `update-helper/update_host_native.cpp`, which
also retains the existing primitive verbs; the primitive-only helper is not a
launcher fallback. Compilation alone supplies no certificate or signing approval.

Primary's later model-hash/path packaging integration must merge narrowly rather
than replace `package.py`, `policy.json` or `test_package.py` wholesale. Retain
the exact `updateHelper` name, mandatory `--update-helper` route,
`copy_certified_update_helper`, signed helper inventory/hash and staged
certificate checks. Model/path integration and real signing remain separate.

## Exact primary glue (not applied here)

1. In `rust/crates/zabor-native/src/lib.rs`, expose `#[cfg(windows)] pub mod
   update_host;`. Nested bootstrap/process/tests need no additional root modules
   or dependency/lock changes. Retain the existing `forbid(unsafe_code)` policy.
2. Retain `UpdateHost::new(original_operation, overall_budget)` in the original
   app/bootstrap owner. Call `bootstrap(path, pins, certified_platform)` before
   profile acquisition; keep the returned root lease/selection. Only explicit first
   installation may call `establish_baseline` on a new protected empty root.
   Certify the exact hash-pinned host-capable helper against the configured
   `pins.publisher_certificate_sha256` under its original registered owner; do
   not reuse a boolean or certificate receipt from another helper/publisher.
3. Retain the original `UpdateIo` download/stage joins and `root.begin(operation)`
   cancellation token. Never remint an operation/token after cancellation/restart.
4. Live `attest_app_drain` deliberately rejects with
   `primary-original-app-drain-binding-required`. The five category-shaped local
   futures are sandbox evidence only, not proof of real owners. Primary must bind
   the existing original Session/final-save/file/socket, Voice/media/capture,
   input/cursor and profile workers and observe their actual outcomes before a
   reviewed live evidence constructor is enabled. Do not replace this guard with
   no-op callbacks, status booleans or cancellation-only reports.
5. Sandbox activation uses a retained `app_drain_owner`, its successful original
   category joins and `attest_app_drain(root)`, then
   `host.activate(root, original_token, evidence)`. Retain the drain owner and
   `ProcessJob`; `ready()` observes authenticated durable health, not exit.
   Dropping/cancelling the job requests shutdown; `host.drain().await` observes
   the original helper/process/children. An unobserved process/child outcome must
   keep profile reopening closed, even if the drain future itself completed.
6. Selected target startup reads `process::HealthBootstrap::read` from the private
   inherited stdin under a registered blocking owner. Independently establish the
   trusted executing manifest/root and actual app readiness, then call
   `acknowledge(expected_root_digest, expected_manifest_digest)`. Main startup,
   real readiness and live drain wiring are not implemented by this library scope.

## Checks / compact handoff

WIP was preserved rather than reset. First original check
`sh_1bca9a68800131Vuyptr5yvna3` stopped at `/W4 /WX` on an implicit wchar-to-char
pipe-name conversion; no Rust test passed. Its owned temp directory was removed
only after the command exited. The conversion now explicitly validates ASCII.

Second original check `sh_1bcaba1cf001QdWqJZElVYGCIx` compiled the native host
and four synthetic targets with static CRT/W4WX and passed fmt, then stopped at
one authored Rust `Result<(), &str>` versus `Result<(), String>` type mismatch.
Zero Rust tests passed; owned temp target removed after original completion.

Third original check `sh_1bcae8406001hzA24VoyzcDsVx` again passed native/fmt,
but its intentionally IO-free harness left `prepare` unused in the non-test lib.
That harness boundary, not production lint policy, is corrected by exposing its
existing stage call; no warning suppression or repeated redirect tests.

Fourth original check `sh_1bcb1496a001qHhNou4kSjVr52` completed: native/fmt and
all-target Clippy passed; 8 of 9 Rust tests passed. The timeout fixture's 700ms
budget could expire before an authenticated start/exit frame, correctly leaving
`AwaitingHealth` rather than pretending children were observed. No successful
group receipt or live recovery claim follows that failed assertion. The fixture
now uses a five-second timeout budget while retaining strict rollback/exit
checks. No production deadline is relaxed. Owned v4 temp workspace was removed.

Own Graphify job `sh_1bcb619ee001oYIhXwz6zlQXrS` completed: 14,614 nodes,
32,158 edges, 541 communities. Primary outputs were not edited. Nine zero-node
sources and HTML5000 limit remain; doc semantic refresh is pending. Post-build
diagnostic has zero missing/dangling endpoints, self-loops or collapsed edges;
raw producer loss is not measured. Focused query resolves Host/HealthBootstrap/
bootstrap APIs to these own Rust sources, not main application binding.
Final original verification `sh_1bcb86707001vvLh6D9E8sz9yt` completed successfully,
`checked-v5/`: the timeout/nonce/early-exit case and four helper-distribution Python
tests passed, with the unchanged eight passing v4 Rust tests inherited. Union:
**9 Rust + 4 Python = 13 unique tests**. Both current fmt/all-target Clippy and
static-CRT/W4WX native host/four targets passed. All 19 frozen source/pin hashes
matched; the v4 inheritance guard matched the 17 unaffected hashes. The external
lock remains an exact subset of 94 existing dependency versions/checksums.
Scrubbed environment includes ProgramData/ALLUSERSPROFILE and prerelease MSVC
discovery. No production/native app, keys/certificate or user profile was used.
Original helper/target/children/reader and test command completion were observed;
owned v5 harness/targets/sandboxes were removed after completion. No own build,
test or initial Graphify job remains pending. Earlier partial receipts are retained.
Final incremental own Graphify job `sh_1bcbff9a50020t3VUt0LknSdOE` completed:
14,614 nodes / 32,158 edges / 543 communities, nine zero-node warnings and HTML
limit retained; code-only refresh, doc semantics pending. Primary graph unchanged.
No own original job is pending. Checked 13-path scope was saved in `41e0fec`;
the initial saved handoff is `86aca29`. Primary glue and remaining gates above
are still required.

Remaining gates: provisioned publisher/manifest/acceptance trust, positive real
certificate verification, accepted initial baseline, registered main startup and
health readiness, actual original app drain, hostile namespace/ACL/same-user/admin,
power-loss/hung-volume/Windows 10/11/physical/two-device/full-client acceptance and
legal approval (prior 24 missing upstream texts). No live installation, production
profile/request, push/release/legacy deletion or shutdown. Primary integration
remains explicitly separate from these component receipts; these gates must be
closed before enabling live installation.

## Saved short handoff before static review

Checked source: `41e0fec7ac32874aec0f27eb0ec3db2970c547ad`, 13 owned paths.
Post-save clean-tree/exact-scope/19-hash guard passed; receipt is
`migration-artifacts/native-data/update-host-2026-11-08/save-verification.json`.
WIP source at that save: none. Original own helper/children/reader/build/test/graph jobs: none
pending; no waiter cancellation was counted as native completion. Source and
permanent receipts/own graph are retained; owned temporary checks are removed.
The earlier own compact was admitted with `delivery: steer`; admission is not
completion evidence and no new compact is requested by this continuation.
Primary must apply the exact glue above and close real
trust/baseline/startup/drain/legal/full-client gates.

## Delivered-result continuation proof at `6015bb4`

The original `sh_1bcb86707001vvLh6D9E8sz9yt` result is delivered, not pending.
Its actual `checked-v5/host-tests.log` records **1 passed / 0 failed** for the
affected timeout/nonce/early-exit case; `package-tests.log` records **4 tests / OK**.
The eight passing v4 Rust tests were inherited, not rerun. Bytes at that save match
all 19 frozen v5 hashes and all 17 unaffected v4 hashes; native static-CRT/W4WX,
fmt, all-target Clippy and the 94-dependency lock-subset receipts remain applicable.
The affected fix/check/source save is already `41e0fec`, not a pending action.

`migration-artifacts/native-data/update-host-2026-11-08/continuation-verification.json`
records the original receipt hashes, the exact inherited/affected Rust test names,
the 13-test union and the source guard. Original v1-v5 receipts are unchanged;
the owned v5 temporary workspace is absent. This incremental continuation changes
only this handoff, corrects stale next-action text and preserves every checked
helper/packaging source byte. No status polling, duplicate build/test job,
primary/package model-path integration or live drain constructor was performed.

Own `graphify update .` completed in this continuation: **14,616 nodes / 32,160
edges / 561 communities**. Read-only post-build diagnostics report zero missing
or dangling endpoints, self-loops, exact duplicates and collapsed edges. The
focused query still resolves `copy_certified_update_helper` to the owned
`scripts/native/packaging/package.py`. Nine zero-node warnings, the HTML5000
limit, pending doc semantics and unmeasured raw producer loss remain explicit.
Graph/log/diagnostic proof is retained beside `continuation-verification.json`;
no primary graph was changed.

## Static-review follow-up (2026-11-09)

Review `ses_e4312bdb5ffe6Yi7TPRbCILxKy` examined `41e0fec` against `f2867df`.
This follow-up preserves `41e0fec`, `86aca29` and `6015bb4` and addresses only
the five reported host/helper-packaging findings:

1. `HeldExecutable` acquires non-reparse namespace leases and a deny-write/delete
   image handle before final verification. Target archive/metadata proofs are also
   held; the actual held image is hashed against verified package pins, and the
   helper is hashed against its original pin. Leases survive process creation and
   original helper/reader completion. Replacing bytes after a prior path-only
   verification is rejected; held image/parent renames and writes are rejected.
2. Host activation and durable health use the same final gate for the original
   operation token and the registered host-owner token after tree verification.
   Cancelling either cannot discard pending state or publish a durable commit.
3. That gate uses the earlier envelope/pending health expiry and a bounded
   monotonic deadline, not the overall 600-second budget. Wall time is read again
   immediately before persistence; public logical-time APIs also account for
   elapsed verification time. Held-verification tests advance the clock past
   envelope and pending expiry independently, then require retained rollback.
4. Native helper checks original owner/target liveness, queued control and expiry
   immediately before `ResumeThread`. Rejection drains the same assigned Job and
   observes the original suspended target before `E`; it does not start unowned
   work. A separately compiled developer-only acquisition gate and startup marker
   prove queued cancellation/expired acquisition without production timing knobs.
5. Helper static-CRT/system-only inspection and Authenticode now read the same
   held staged bytes. The returned hash is checked again under a lease spanning
   archive composition. Source-flip, staged mutation/namespace replacement and
   post-certificate composition mutation have dedicated local regressions.

`verify-update-host.py --followup` runs four new Rust tests, the two affected
prior process tests, four new packaging tests and the two affected prior packaging
tests. The other seven prior Rust and two prior packaging results are inherited
with unchanged test-definition and unaffected-source guards, not a full rerun.
All checks use own temporary targets, locked/offline dependencies, `-j2`,
static CRT/W4WX and strict all-target Clippy. This does not grant real trust,
certificate, accepted baseline, legal approval or live app-drain evidence.

Original follow-up check `sh_1bcfc43f8001gv2eTz59RI5uYH` completed successfully.
Actual receipts in
`migration-artifacts/native-data/update-host-2026-11-09/followup-v1/` record
**6 Rust + 6 packaging passed** (eight new tests and four affected prior tests).
Seven prior Rust plus two prior packaging results are inherited; union is
**13 prior + 8 new = 21 unique tests**, not 25 or a repeated full matrix.
All 14 recorded commands exited zero: two helper variants/five targets built
with static CRT/W4WX, metadata/frozen 94-dependency subset, fmt, strict all-target
Clippy, three focused Rust selections and six packaging checks. All 19 current
hashes match the check receipt; the 12 unaffected frozen source/pin hashes and
unchanged inherited test definitions match the original save. Original command,
helper/target/descendant/reader completion was observed; `cleanup.json` confirms
the owned temporary workspace was removed. No duplicate job or status poll.

Primary must merge only these eight follow-up paths on top of the original
host save; no main/shell/lib/Cargo/assets/policy/root pins were changed. The later
primary model-hash/path packaging change must retain staged helper certification,
held-image hashing and `archive_with_certified_update_helper`. Real primary
startup/readiness/drain, publisher/certificate/baseline/legal and acceptance
gates remain open; `attest_app_drain` is still fail-closed for live roots.

Own follow-up Graphify update completed: **14,646 nodes / 32,278 edges / 551
communities**, with zero post-build missing/dangling endpoints, self-loops,
duplicates or collapsed edges. `HeldExecutable` resolves to the owned helper
source. Nine zero-node warnings and HTML5000 remain; doc semantics are pending
and raw producer loss is not measured. Primary graph is unchanged.
The exact checked follow-up commit/scope/hash receipt is
`migration-artifacts/native-data/update-host-2026-11-09/post-save-verification.json`.

## Case-insensitive held pin follow-up (2026-11-09)

The same reviewer verified the five actual fixes in `99b8b79` as resolved and
reported one new P2: the held executable pin lookup used exact filename case
while archive/composition/collision policy is ASCII case-insensitive. The only
production change is `item.path.eq_ignore_ascii_case("zabor-native.exe")`.
Existing validated path, namespace/image/proof leases and held-byte hashing are
retained; collision rejection is not weakened.

The targeted regression
`pending_host_pin_accepts_mixed_case_executable_and_rejects_case_collision`
builds a signed local-fixture archive with uppercase and mixed-case executable
names, passes prepare/activation, verifies the held pin and deny-write lease,
then recovers the original selector. A colliding archive is rejected before
activation and leaves the original selector/sequence unchanged. Existing fixture
and prior test definitions are unchanged; only this test was appended.

Original red check `sh_1bd0d34b5001yOV9YRZZWdupSJ` completed on the unchanged
`99b8b79` helper and reproduced `pending-executable-pin-missing` after activation.
That expected failed test is diagnostic evidence, not a passing test receipt.
Red/green share one owned mutable target with locked/offline dependencies and
`-j2`. One unchanged primitive helper is rebuilt only as a fixture prerequisite;
the host-capable helper/target/packaging matrix is inherited, not rerun.
The prior 21-test union distinguishes 6 Rust + 6 packaging checked at `99b8b79`
from the 7 Rust + 2 packaging already inherited there. This follow-up adds one
new Rust test; no prior result is counted as a newly executed test.

Original green check `sh_1bd0ee5780012ZSH4DOVfQBwQl` completed: the exact new
test passed (1 passed / 0 failed), affected fmt and strict all-target Clippy/type
checks passed. Union: **21 inherited + 1 new = 22 unique tests**. All 19 current
source hashes and 17 unchanged source/pin hashes match; the production diff is
exactly one lookup predicate, with prior test/fixture definitions retained.
The owned red/green target was removed after original check completion.
Receipts and the exact checked follow-up commit/scope are in
`migration-artifacts/native-data/update-host-2026-11-09/case-pin-followup/`.
Only `update_helper.rs`, its targeted Rust test file and this handoff change.
Primary sources remain disjoint; live `attest_app_drain`, trust, accepted baseline
and legal gates remain fail-closed/unapproved. No production/profile/physical,
install/push/shutdown, job polling or repeated native/packaging matrix.

Own Graphify refresh completed: **14,648 nodes / 32,286 edges / 538 communities**;
post-build integrity has zero missing/dangling endpoints, self-loops, duplicates
or collapsed edges. Nine zero-node warnings, HTML5000, pending doc semantics and
unmeasured raw producer loss remain explicit. Primary graph was not changed.
Final exact commit/scope/hash proof: `case-pin-followup/post-save-verification.json`
under the receipt directory above.

## Primary integration checked — 2026-11-09

Primary `rust/native-client` applied the exact checked chain
`41e0fec -> 86aca29 -> 6015bb4 -> 99b8b79 -> 2b3b4e8` on recovery checkpoint
`c8a622f727a60a269752013d1e222b9864c50d49` without conflicts. Nine imported
source/test/helper/verifier paths preserve exact final candidate Git blobs. This
handoff retains the complete candidate history and adds only this primary receipt.
Four scoped glue paths expose Windows `update_host` and add native-owned model
inventory inputs with the existing SHA-256 pins and three targeted regressions.
Mandatory helper certification, staged/proof/image/namespace leases, case-insensitive
pinning and original-owner observation remain intact. Cargo manifests/locks,
renderer seam, resource/catalog/model bytes and other component sources are unchanged.

Original primary job `sh_1beeaa334001Ga4K0ETIdRhw0l` completed successfully:
**18 Rust + 15 packaging = 33 executed affected tests**. These are fresh primary
integration checks, not additions to the candidate's inherited 22-test union.
The group passed affected fmt, strict fixture all-target/normal lib-and-bin Clippy,
normal native-media feature closure and normal build. The host and developer-only
acquisition helper plus five synthetic targets compiled with static CRT `/W4 /WX`.
No production/native-app launch, physical capture, user profile or live endpoint
was used. All original test/check commands completed; the owned temporary helper,
targets, sandboxes and runner were removed. Required primary build caches remain.

The final guard confirmed all **183 source/pin hashes**, exact fourteen-path staged
scope and normal executable SHA-256
`c5a192908681ec0552a840ada760137045647666a0daee25442d34c7ed4e167b`.
Read-only integration review `ses_e4116d209ffewKfYKBMmKTNEZk` found no actionable
defects across all fourteen paths against `c8a622f` and `2b3b4e8`.
Receipts, source/Git/EOL provenance and original command logs are in
`migration-artifacts/native-data/update-host-integration-2026-11-09/checked-v1/`.
The exact scoped save is recorded there in `save-verification.json` after commit.

Primary Graphify refresh: **7,907 nodes / 20,321 edges / 301 communities**.
Read-only post-build integrity has zero missing/dangling endpoints, self-loops,
exact duplicates or collapsed edges. Ten zero-node warnings and HTML5000 skipping
remain; raw producer loss and semantic documentation refresh are not measured.

This is library/packaging integration acceptance only. Real main startup/health,
live original app drain, accepted initial baseline, publisher/manifest/acceptance
trust, legal/licensing and physical/two-device/long-session/full-client gates
remain incomplete. Live `attest_app_drain` still rejects with
`primary-original-app-drain-binding-required`. No installation, legacy deletion,
push, publication or shutdown occurred.
