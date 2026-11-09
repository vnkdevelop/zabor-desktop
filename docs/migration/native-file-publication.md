# Native FileJobs final-publication authority

2026-11-09. Bounded component handoff; not whole-native acceptance.

## Scope and provenance

- Isolated worktree: `C:/Users/porad/.local/share/opencode/worktree/b1b3f5/native-file-publication`.
- Requested and inspected base: `046325d73acb85ba28e02d7f7bd284b609a7ef44`.
  The pre-existing empty detached worktree was moved from `c5d07348` to that base
  only after checking its clean status. No other worktree was changed.
- Architecture consulted first read-only through the original project's graph
  report, then the primary `rust-p0-backend-bench/graphify-out/GRAPH_REPORT.md`
  and its JSON FileJobs nodes. The primary graph reported `c8a622f7`, older than
  the requested base; actual base source was inspected before edits.
- Only owned source is `rust/crates/zabor-native/src/file_jobs.rs`, including its
  nested private tests. Other deliverables are this document and the exact cumulative
  `native-file-publication-source.patch` artifact added during sole-owner recovery.
  No filesystem,
  chat/store, shell/main/lib, Cargo/config, assets, packaging or legacy source edits.

## Additive API and primary caller

```rust
pub fn with_stop_and_authority(
    root: &Path,
    stop: CancellationToken,
    authority: Arc<dyn Fn() -> bool + Send + Sync + 'static>,
) -> FileJobs
```

Bind the actual original worker ticket before any job is admitted. For the primary
caller, `original_ticket`, `intents`, `global_stop` and `root` below must already
belong to that original worker; the callback must not obtain a fresh ticket.

```rust
let session_stop = intents.session_token(original_ticket);
let original_intents = intents.clone();
let publication_stop = global_stop.clone();
let files = FileJobs::with_stop_and_authority(
    root,
    session_stop,
    Arc::new(move || {
        !publication_stop.is_cancelled()
            && original_intents.is_current(original_ticket, IntentScope::Session)
    }),
);
```

- The immutable callback is installed before constructing the empty JoinSet.
  Every original blocking save captures a clone of that same Arc. There is no
  authority setter, completion-time ticket construction or private test API exposure.
- Direct `save` uses `files::save_verified_if` with that bound callback and its
  original operation's cancellation token. `finish_incoming` delegates to this
  exact same save path. Their existing public signatures and behavior are preserved.
- `new` and `with_stop` remain compatible standalone constructors with `true`
  authority. Primary shell integration must use the explicitly bound constructor.
- The callback executes synchronously in the original blocking save at admission
  and again after verified copy and `sync_all`, before final persistence. It is a
  short nonblocking authority check, not a commit closure containing filesystem IO.
  FileJobs does not acquire CriticalIntents or invoke the callback on the async owner.
- Hashing/copy/sync/persist, filesystem cleanup and native destructors remain outside
  the intent lock. Operation cancellation is rechecked after the callback returns.
  Leave keeps Session authority; Logout revokes the original ticket permanently.
- Replacement, explicit cancellation, peer disconnect and inherited session stop
  continue to revoke original operations. Event filtering, retained JoinSet entries,
  drain error retention and cooperative `finish` are unchanged. An authority panic
  is an observed original join failure, never a detached successful save.
- Admission denial in the blocking publisher does not read its already-open source
  handle, open the target or create a temporary. Existing source acquisition is
  unchanged: `save` accepts an open handle and `finish_incoming` first calls
  `ChatEngine::cached_file`. This API does not claim to prevent that upstream open;
  the primary must keep cache/SQLite acquisition outside its intent lock.

## Affected deterministic checks

Eight new tests exercise actual public original blocking saves, not injected success
events. Local verified cache/message fixtures exercise `finish_incoming` without
rerunning unchanged transport or crypto matrices.

- Direct and incoming post-sync Leave: exact original callback runs twice; Session
  remains current, Voice is revoked, destination publication succeeds.
- Direct and incoming post-sync Logout: independent uncancelled operation token
  isolates callback rejection; even a subsequently current fresh session cannot
  authorize the original save. Existing destination and unrelated partial survive.
- Same-peer/message replacement: a newer save finishes while the original is
  blocked post-sync. The original token remains cancelled even when its exact
  callback still accepts, and its old destination is preserved.
- Cancellation inside a callback that returns true is rechecked before persistence.
  Explicit message cancellation and disconnect also fail closed with current authority.
- Denied admission calls authority once, leaves the shared source cursor at zero
  and preserves an exclusively locked Windows destination without creating a temp.
- Post-sync authority panic is retained after `next_event` observes the original
  join. A cancelled wait/temporarily cancelled drain retains the original blocking
  panic for the final drain; repeated finish continues to report failure.

Callbacks assert execution on a different thread from the async owner. They probe
the real original Session ticket and briefly acquire/release a real intent commit
before test-only latching. The async test can issue Leave/Logout while the publisher
is latched, proving the latch is outside the intent lock. Latches have bounded waits
and a drop release; successful cases drain original jobs before fixture cleanup.

## Verification status

The first original check runner failed during Windows command-line preparation,
before any compilation or test. It is not counted as verification. Its replacement
uses explicit `cmd.exe /d /c` for every subprocess and owns its target and fixtures
under the approved OpenCode temp directory. Shared pinned SDK inputs are read-only.

- Original grouped job `sh_1bf07d27d001AZZiL9iEw2oopW` completed with exit 0.
  All five stages below completed successfully; no pending check or original file
  worker remains. Fourteen affected FileJobs tests passed, zero failed/ignored,
  313 unrelated library tests filtered out. This is eight new tests plus six affected
  compatibility/drain predecessors, not a rerun of the unchanged 45/33-test matrices.
- Normal and camera/stream-fixture all-targets Clippy passed with `-D warnings`.
  The existing Slint `viewport-y` deprecation in unmodified `ui/app.slint:486`
  was still emitted by its build script; it was not edited or suppressed.
- The guard hashed all 781 tracked inputs and 38,761 shared pinned SDK files:
  39,542 frozen paths, zero changed hashes through the original checks. The new
  untracked handoff document was not part of this source freeze.
- Checked source commit: `f8ab4709c1fff8cf2c89b7c43181d3631977fbe6`, direct
  child of the requested base, containing only `file_jobs.rs`. The checked working
  bytes and saved Git blob both have SHA-256
  `157edceacafeb192136ae8d36b6e0bfb87657be02374bd5e2c3afa28d3d683c5`.
- Frozen manifest SHA-256:
  `97ee08296ef453ef17351e004125654e54aca1826b976c89fe3606c2b0b2b19e`.
  Original verification JSON SHA-256:
  `b9ebef8a8401c18228c357a8110653030b97681a4b2e1a1a041af5b3176107cc`.
- Exact original guard/verification invocation:
  `cmd.exe /c python C:\Users\porad\AppData\Local\Temp\opencode\native-file-publication-checks-20261109.py`.
  Its before/after SHA-256 guard used `git ls-files -z` for tracked inputs and
  recursively hashed the four explicitly selected pinned SDK inputs; any changed
  hash or failed stage prevented a successful outcome. This temporary runner was
  removed only after its actual receipt and saved-source byte equivalence check.

Exact successful commands, run with MSVC x64 environment, Cargo on PATH,
`CARGO_NET_OFFLINE=true`, an owned `CARGO_TARGET_DIR` and `TEMP`/`TMP`, and
explicit read-only primary `LK_CUSTOM_WEBRTC`/`LIBCLANG_PATH`/`ORT_LIB_LOCATION`/
`RNNOISE_MODEL_PATH` overrides:

```text
cmd.exe /d /c rustfmt --edition 2021 --check rust/crates/zabor-native/src/file_jobs.rs
cmd.exe /d /c git diff --check
cmd.exe /d /c cargo test --manifest-path rust/Cargo.toml -p zabor-native --locked --offline -j 2 --no-default-features --features native-media --lib file_jobs::tests -- --test-threads=1
cmd.exe /d /c cargo clippy --manifest-path rust/Cargo.toml -p zabor-native --locked --offline -j 2 --no-default-features --features native-media --all-targets -- -D warnings
cmd.exe /d /c cargo clippy --manifest-path rust/Cargo.toml -p zabor-native --locked --offline -j 2 --no-default-features --features native-media,camera-test-source,stream-audio-test-source --all-targets -- -D warnings
```

| Original stage | Exit | Seconds | Log SHA-256 |
| --- | ---: | ---: | --- |
| fmt | 0 | 0.286 | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| diff | 0 | 0.467 | `4e38ad79afc8246aa9c90aa2b345e18acbaf65e6673fe9ee1ac87863fc80d431` |
| tests | 0 | 756.543 | `5b54093609f437f10b94a9434b337cc16be789bccef0d02a0958ccc598e99abb` |
| normal Clippy | 0 | 642.392 | `b48e2f10c6ece417253c3a08586bc6f3f364772d589e4ff6da832b0cc564902c` |
| fixture Clippy | 0 | 82.545 | `e2ced8ad7efe695f302429134ede91a6bda15739322cda20ba26d85999b1794f` |

Original logs and proof were inspected before cleanup; their delivered job output
and the hashes/commands above preserve provenance. At that original checkpoint,
no normal executable, full
neural, visual, transport/crypto or physical-device matrix was rerun.

The failed preparation-only directory was removed separately after its delivered
failure. After the successful original job completed and its source/Git proof was
checked, the exact marker-bound `native-file-publication-checks-61yvd754` directory
and its runner were removed from the approved OpenCode temp root. This deleted only
owned targets, synthetic fixtures and verification scratch; shared SDK caches,
other worktrees and their jobs were not cleanup targets. The own graph is retained
as the requested persistent knowledge output, not a build/test temporary.

## Own graph and limits

- Original `cmd.exe /c "graphify update ."` job `sh_1bf093032001hNwoqZ0ifhH553`
  completed successfully: 615 uncached code inputs, 14,982 nodes, 33,226 edges,
  540 communities, no LLM/API token cost. Only this worktree's graph was written.
- This fresh whole-root code graph has a larger corpus than the primary's existing
  graph. It is not claimed to be an identical primary topology or semantic document
  extraction; the CLI explicitly leaves doc/paper/image semantic refresh separate.
- Twelve source inputs produced no nodes, including launch/release JSON and language
  catalogs. Graph HTML was skipped because 14,982 exceeds the 5,000-node limit.
- `cmd.exe /c "graphify diagnose multigraph --graph graphify-out/graph.json --json
  --max-examples 2"` completed read-only: zero missing/dangling/self-loop/duplicate/
  same-endpoint-collapse edges in the exported graph. Post-build diagnostics cannot
  reconstruct producer-suppressed edges; the producer scan lists ten heuristic sites.

Recovery `cmd.exe /c graphify update .` completed synchronously after the handoff
change: 179 uncached inputs, 14,986 nodes, 33,230 edges, 547 communities, zero
LLM/API token cost. The recovered source and the new handoff headings are present
in the structural graph. Twelve inputs still produced no nodes; HTML remains
skipped above 5,000 nodes. This does not claim a new full semantic extraction.
Read-only `cmd.exe /c graphify diagnose multigraph --graph graphify-out/graph.json
--json --max-examples 1` found zero missing, dangling, self-loop, exact-duplicate,
or same-endpoint-collapse edges. The same ten heuristic producer-suppression sites
remain a diagnostic limit, not evidence that the exported graph lost endpoints.
The own persistent graph/backups are retained; no other-owner graph was changed.

## Remaining primary gates

- Wire this constructor to the original shell worker's ticket, shared intents,
  session cancellation and global stop before asynchronous admission.
- Keep source/cache acquisition and all IO/native teardown outside intent locks;
  separately authorize UI/storage effects with the original context.
- Observe original FileJobs drains before releasing cache/ChatEngine owners on
  Logout/application teardown. Drop/abort alone is not observed completion.
- The existing post-sync check/persist boundary does not promise arbitrary
  concurrent-volume or power-loss atomicity, nor exclusion of Logout occurring
  after the final synchronous check has already returned.
- Full common-shell/client regression, physical/long-session testing, authenticated
  installer/helper/legal/trust and release acceptance remain open. No native UI,
  network/device, Electron, production/user profile, npm/node_modules, merge, push,
  publication or shutdown operation was requested or performed here.

## Sole-owner recovery and retained original lifetime

The replacement owner is `ses_e409ebb49ffeQW8hVwnkkqk3oc`, working only in
`native-file-publication`. Recovery moved the session to this assigned worktree,
read its graph report before source searches, and read the finished original
`sh_1bf07d27d001AZZiL9iEw2oopW.out` once. It did not resume the terminated session,
poll the original checks, or repeat the completed affected-test/Clippy matrix.

The recovered working source is byte-identical to the checked Git blob in
`f8ab4709c1fff8cf2c89b7c43181d3631977fbe6`; no replacement source patch was needed.
The cumulative source change remains exactly that commit relative to
`046325d73acb85ba28e02d7f7bd284b609a7ef44`, not a partial follow-up diff.

The caller recipe requires a strong `Arc<CriticalIntents>` belonging to the actual
original worker. Capture that Arc, its already-issued `IntentTicket`, and the
original global stop in the immutable `'static` callback. Do not capture the
borrowed `SessionWork`, downgrade the only owner to `Weak`, clone a boolean
snapshot, obtain another ticket in the callback, or recreate a FileJobs owner
when a waiter is cancelled. Cloning the original Arc retains the same authority
object; copying the original ticket does not renew its owner or Voice generation.

`FileJobs.authority` retains a strong callback owner for the entire FileJobs
lifetime. Every original blocking save additionally retains its own clone before
spawn, so dropping an external callback/intent handle cannot invalidate that
publisher's memory lifetime. Cancellation revokes permission; it does not release
or replace the original callback while blocking work can still use it. The
post-sync predicate reads the original Session authority again, rather than the
constructor's admission result. The already-checked direct/incoming Leave,
Logout-with-fresh-session, replacement, and cancelled-wait/panic cases exercise
this original identity and retained join contract; they are not a new test run.

The parent must retain FileJobs and its original join set until `finish().await`
actually returns on every Logout/teardown route, before releasing ChatEngine,
cache owners, or their resource receipts. A cancelled drain future still leaves
the same joins in FileJobs; continue that owner's drain, not a replacement drain.
An observed join panic keeps `failed` sticky even after the event waiter consumed
the join, so cleanup may finish but acceptance must still report failure.
`Drop`/abort is cancellation, not observed completion. No cache acquisition,
copy/sync/persist, join wait, destructor, or resource cleanup belongs inside an
intent commit. Publication checking ends before persistence; Logout after the
final check has returned is outside this component's claimed boundary.

## Exact cumulative source import recipe for the parent

The immutable source commit contains only `file_jobs.rs`. The parent can integrate
that source commit if it has not already been applied, then wire its own original
constructor/drains using the caller recipe above. No parent files were edited.
The same cumulative bytes are retained in
`docs/migration/native-file-publication-source.patch`. Recovery compared this
saved artifact byte-for-byte with Git's cumulative output and successfully ran
`cmd.exe /c git apply --check --reverse docs/migration/native-file-publication-source.patch`
against the recovered source. That read-only check did not reverse or change code.
The parent must run its own forward `git apply --check` before deciding to apply;
if the API is already integrated, neither reapply nor replace it with this base.

To regenerate the exact cumulative artifact:

```text
cmd.exe /c git diff --binary --full-index 046325d73acb85ba28e02d7f7bd284b609a7ef44 f8ab4709c1fff8cf2c89b7c43181d3631977fbe6 -- rust/crates/zabor-native/src/file_jobs.rs
```

Its stdout and the saved artifact are 21,873 bytes, SHA-256
`9bf21fd197791280e64308b20cb122768ff7668f5f4e8897cf76dce8ba0c67b1`.
The full-index source transition is
`95f6ca1728a1ebea4fe9a6163ab201587508d538` to
`31defd044494577fac2202f545499c5731212d18`; the latter blob and the checked
working source both have SHA-256
`157edceacafeb192136ae8d36b6e0bfb87657be02374bd5e2c3afa28d3d683c5`.
Export/apply only once, checking the parent's existing patch and WIP first;
do not overwrite newer parent source or cherry-pick the unrelated base history.
Raw patch context markers deliberately preserve Git's exact whitespace; source
and Markdown whitespace checks are separate from byte equality of this artifact.

The constructor must be wired before any public save/finish_incoming admission.
Preserve existing public operation signatures and standalone constructors.
Session authority intentionally survives Leave and permanently rejects the old
owner after Logout, even when peer/message identifiers or a current session are
reused. UI/storage effects remain independently guarded by the parent's original
context. This component checkpoint does not certify the whole native migration.

## Recovery native executable link gate: completed

- New bounded job `sh_1bf6435da001GhvvSJDYYCjRfh` completed with exit 0; it built
  and linked the normal MSVC Windows x64 native-media executable without running it.
  The preparation-only predecessor `sh_1bf63914e001rZktIPjivDfJwc` failed before
  target creation or compilation and is not counted as a build or repeated matrix.
- Exact invocation:
  `cmd.exe /c python C:\Users\porad\AppData\Local\Temp\opencode\native-file-publication-link-20261109.py`.
  Its only Cargo stage, with the MSVC x64 environment, Cargo on PATH,
  `CARGO_NET_OFFLINE=true`, isolated target/TEMP/TMP, and the same explicit
  read-only pinned SDK overrides described above:

```text
cmd.exe /d /s /c cargo build --manifest-path rust/Cargo.toml -p zabor-native --bin zabor-native --locked --offline -j 2 --no-default-features --features native-media
```

- Cargo reported 12m 07s. The stage plus post-build hash verification took
  1,125.716 seconds. Build log SHA-256:
  `6f6789d5f5ff4b020a98d9c648d0fd58d9542ea8c8fe71e6b6ea90ad3e21063e`.
- The linked executable was 84,247,552 bytes, SHA-256
  `bb24c2a8816ab0657c107e9590fb4bf1e5b040560c76bd46c05b1379aa163497`.
  It was not launched, installed, published, or retained as a release artifact.
- All 39,548 paths in this recovery stage's tracked-input/pinned-SDK snapshot
  had identical before/after hashes. Snapshot SHA-256:
  `5a53c54a42b65bf3fdaba445718f197e9db3d5d3ac140544a4f4d556755e044c`.
  This is a new stage-specific snapshot, not a claim that its path set equals
  the earlier 39,542-path original verification snapshot.
- Source SHA-256 still equals the original checked Git blob. The existing Slint
  `viewport-y` build-script warning remains unmodified; there was no Rust error.
- The marker-bound `native-file-publication-link-9fosdaa6` target, log, and scratch
  were removed only after the actual Cargo subprocess exited and the successful
  hash/binary proof was captured. The runner itself then exited with 0. No shared
  caches, other-owner resources, profiles, devices, or other worktrees were removed.
  No test/Clippy matrix, full-neural build, visual capture, or physical/network
  acceptance was repeated or claimed during this recovery.

The actual-exited recovery runner, its non-polling OS-wait helper, and the own
temporary handoff draft were subsequently removed. A final path check confirmed
that all four exact owned recovery temporary paths were absent. Persistent
deliverables are the immutable checked source commit, this handoff, the cumulative
patch artifact, and this worktree's graph. There is no pending build or file job
at this component checkpoint; constructor/drain integration remains the parent's
explicitly separate gate.
