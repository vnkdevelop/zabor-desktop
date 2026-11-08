# Native completion and legacy cutover

Updated: 2026-11-08. Coordination plan, not complete-native acceptance.

The owner explicitly requested several agents to accelerate the full migration
and eventual removal of the old client code. The final client has no Electron or
WebView. Backend .NET, SFU Go and required native C/C++ libraries remain. Legacy
removal is a separate verified cutover after the native replacements and final
reference comparisons pass; do not remove the working reference to hide gaps.
The original user project/profile and immutable Git checkpoints remain unchanged.

## Concurrent ownership

All new worktrees were created from primary `fd8783f`. Each agent moves its own
session to its worktree, uses the existing primary graph before source reads,
and saves incremental verified commits. Shared integration and final acceptance
remain with the primary `rust/native-client` worktree.

| Workstream | Session | Isolated worktree | Owned scope |
| --- | --- | --- | --- |
| Actual app UI/actions | `ses_e43ccd019ffeP2Q43srQQQySTh` | `native-app-completion` | Registered Session commands; achievements/modal/focus/hidden-store/time; members/profile/channel/friend/clipboard/call feedback; shell and app UI |
| Chat/file durability | `ses_e43ccd018ffeZoyZjCvJ7rCR4m` | `native-data-finalization` | Chat/store/files/file jobs/transport; only shell/session_io and its tests; original final-save/revocation/recovery |
| Media lifecycle | `ses_e43ccd016ffenUW5YGKTE0xwjY` | `native-media-finalization` | Media/Voice/video/camera/audio/capture original owners, late callbacks and retirement; no shell/UI/data/input |
| RC desktop adapter | `ses_e44a7ab1cffe4T8WKFIaW7g0ef` | `native-rc-integration` | Saved desktop primitives, registered cursor actor/cleanup, viewport/frame adapter, passive pill/root/viewer; no shell/app/Voice/media |
| Update host/bootstrap | `ses_e43e7cdf2ffe1lSpsf2DYPu9Gu` | `native-update-host` | Registered original helper/children/process, authenticated health, selector recovery/baseline APIs; no shell/UI/import |
| Copy-import helper | `ses_e44a7ac4cffeI1oF29tnNf75s2` | `native-profile-import` | Existing original process/children/private zeroizing pipe and synthetic Chromium recovery; no duplicate task or status polling |
| Read-only legacy cutover audit | `ses_e43cb9554ffeu6iHaRzbyeHsQS` | Primary read-only | Exact legacy removal closure and retained assets/tools/evidence; no edits, builds or deletion |
| Native-only cutover preparation | `ses_e43c709e4ffeg1cd0BIQobP66H` | `native-cutover-preparation` | Lightweight exact-byte native asset/reference/launcher preparation; no old-code deletion or shared package edits |

### Provider launch outcomes

The new UI/data/media/RC/update-host/cutover starts returned pre-consume quota
errors. They have returned no verified new source or completed implementation;
assignments above are a durable backlog, not six running or completed code tasks.
Original import-helper later also returned an automatic pre-consume quota error;
its unfinished source/resources are preserved, and no helper/process completion is
inferred. It was not polled or replaced with duplicate work.
The read-only audit completed and is retained in
[native-legacy-cutover.md](native-legacy-cutover.md). Primary independently confirmed
the node_modules junction into the original user project; never recursively clean
it or run npm install/postinstall/rebuild through that shared link. No legacy sync/
release launcher may be used as a native fallback.

The one short fresh-context data retry `ses_e43c0da25ffeNPUfvdY3E1N8Qj` also returned
an automatic pre-consume quota error, without verified code. No more repeated
launch attempts are scheduled against the failed shared quota admission. Do not
claim parallel implementation progress from an admitted tool call. Original
import WIP/joins/fixtures remain necessary; primary local checks continue
independently. A later continuation must use these compact saved scopes rather
than replaying the long failed contexts and must retain honest provider outcomes.

### Preserved unfinished implementation after quota errors

Read-only Git status subsequently found uncommitted WIP in the app, data, media,
RC and update-host worktrees. Failed quota admission did not imply clean trees;
only the absence of verified returned source was established. Preserve the WIP
and original jobs/children/resources. Data has seven modified chat/store/files/
jobs/SessionIo/test paths; media has seven audio/runtime/camera/media/stream-audio/
video paths. RC has desktop and remote-input/viewer edits and new native adapter
directories. Update-host has update_helper WIP and new Rust/C++ host files.
Cutover-preparation is clean. Status is not compilation or owner-retirement proof.

The primary has taken over only the isolated app WIP without an agent restart.
Its scoped handoff states no accepted check and no prior pending verification.
Inherited achievements/store/clock/app bindings were preserved and formatted;
the missing root AchievementItem export was added. Primary compile-v1 passed
diff/fmt but stopped before compilation on empty MSVC discovery. checked-v2 also
stopped before compilation: the scrubbed process environment omitted ProgramData/
ALLUSERSPROFILE, which Visual Studio discovery requires. A targeted foreground
query with those nonsecret OS paths and prerelease discovery returned the installed
MSVC path. Both failed runners/owned temp fixtures were removed; zero tests passed.
No source acceptance is inferred. Subsequent checks must preserve that environment
while excluding unrelated credentials and must validate the installation path.

App job `sh_1bc500b45001ey4u6qGi61OgC9` automatically completed: all-target compile
and20unique affected tests passed;Clippy failed on test fixture type complexity
and manual leap-year divisibility. Both were corrected without lint suppression.
Temp fixtures/runner removed. The untouched achievement/store source retains its
19-test proof; no full unchanged matrix is repeated for these two small fixes.

Completed original primary-owned app job: `sh_1bca38491001fsctL46gfpYEm2`, evidence
`native-app-completion/migration-artifacts/native-ui/app-wip-continuation-2026-11-08/checked-v4/`.
The group stopped at fmt on newly discovered actual-worker main_session_tests WIP;
no tests/build/Clippy ran in checked-v4. Its temp fixtures/runner were removed.
All unfinished app changes are retained,including original worker tests,root
AchievementItem export,Fixture alias and is_multiple_of fix. No primary app job
remains pending and primary no longer edits app source while its owner resumes.
Original Session-worker held-response,actual app interactions,storage timing/crash
recovery,native DST and physical clipboard/full-client acceptance remain required.

## Renewed subagents after explicit user steering

The owner again requested continuation with subagents. Four fresh short-context
sessions and the two original app/import owners were launched to resume preserved
WIP. Original app/import owners save a short handoff and compact before proceeding;
their original process/pipe/jobs/resources must not be duplicated.
Their checks/commits are pending; tool admission is not verified progress. They
must not reset worktrees or duplicate surviving original jobs/resources.

| Current session | Worktree | Resume scope |
| --- | --- | --- |
| `ses_e435e03a8ffeSY0CH5UjHkrjlM` | `native-data-finalization` | Existing chat/files/SessionIo durability and original drain joins |
| `ses_e435e0369ffeCTMB8JMw1ttg5r` | `native-media-finalization` | Existing audio/camera/media/video retirement WIP |
| `ses_e435e0368ffe6MsJjmFNFqJZtn` | `native-rc-integration` | Existing input/viewer/desktop adapter WIP; root/app/shell excluded |
| `ses_e435e0306ffec6pYUJ4gAml0dU` | `native-update-host` | Existing host/helper/health/bootstrap WIP and guarded native helper package route |
| `ses_e43ccd019ffeP2Q43srQQQySTh` | `native-app-completion` | Sole app-source owner; resume actual worker/UI tests and preserved primary lint/export fixes |
| `ses_e44a7ac4cffeI1oF29tnNf75s2` | `native-profile-import` | Resume original helper/children/private pipe/Chromium scope without second helper or job |

The primary keeps common integration and final app acceptance,not concurrent
edits to child app source. Six task deliveries are pending,not verified commits.
Every child uses its own target,-j2,immutable pinned SDK inputs read-only,affected
checks and a short persisted handoff before compact. No new import-helper duplicate,
production/user-profile/physical operations,legacy deletion,push/publication/shutdown.

RC checked source `e3f6a38e2d035249e8440d79ebe2021962406dcc` and doc-only
`4bbbca40dada5c3c66df336d6e78da8e51f261eb` remain candidate component receipts,
not integrated physical readiness. Its own compact completed, not merely admitted.
RC next phase reuses primary immutable FrameStamp from `66e3b169`; it must not
restamp a cached frame, duplicate the input owner or change microphone/SFU tracks.

The updater agent starts from the current native library wiring and imports its
checked redirect source `0096695` in its own worktree. The primary separately
checks that source integration; this does not duplicate the new host phase.

## Primary checkpoint and current original job

Checked primary source is `a6f8e1f0efc1d3897ac3517cbb53db63facb63a4`, integrating
`0096695dcff98d5ea222512e90fd7e52f3431694` from basefd8783f. The only merge conflict
was the update IO document tail; both primary integration proof and child redirect
receipt were preserved. Source changes applied without a conflict. Cherry-pick
continuation completed after the original automatic result and exact scoped guards.

Original primary check: `sh_1bc35f16d001fGYJhcFNc7Bzcf`, automatically completed passed.
Evidence destination:
`migration-artifacts/native-data/update-redirect-integration-2026-11-08/checked-v1/`.
Group: owned temporary static-CRT `/W4 /WX` C++ helper, fmt, both native-media
Clippy modes,59unique release/update/helper/IO/policy tests,normalbuild and Graphify
all passed. Fourteen source/pin hashes unchanged; four imported source/script Git
blobs exactly match0096695. The source commit contains only its five candidate
files and excludes coordination WIP. Original helper/sandboxes/check/save runners
were removed. No primary redirect job remains pending. Native executable hash:
`662d423256f818d2c4c93a24414a1f45554409c802ab585c60bc75fcbd686871`.
Graph7563nodes/19202edges/310communities;7priorzero-nodewarnings/HTML5000limit remain.
save-verification.json records exact scoped save. Host/health/drain/trust/legal/
production/publication/shutdown and whole-native acceptance remain false.

Coordination plan,audit,handoff/status and primary IO receipt updates are saved
separately from the source cherry-pick; preserve isolated unfinished source WIP.
No primary-owned shell/check job remains pending. Subagent deliveries above await
automatic results; other worktree process/child completion is never inferred from status.

Final primary Graphify resync `sh_1bc50c8dd001hgnOOeYAAM6HUD` automatically completed
after the documentation update: no topology change,outputs retained;7prior
zero-nodewarnings remain. The source-group graph proof above still describes the
unchanged saved source. No primary graph job remains pending.

## Build, compact and integration discipline

- Keep agent file scopes disjoint. Other worktrees, targets, proofs, configs and
  unfinished fixtures are not cleanup candidates. Main/shared hook recipes must
  be explicit; unbound libraries/templates do not count as app completion.
- Use affected grouped checks, not repeated default/neural/four-DPI/hash matrices.
  The primary group uses two Cargo jobs. Prefer minimal owned harnesses for pure
  APIs; never write another agent's build target or mutable source.
- Before every own compact save a concise handoff: head, checked versus WIP files,
  exact pending jobs, original owners/process/pipe/frame/request/joins, evidence,
  remaining gates and next action. Compact context, not repository/user data or
  history. Admission, cancellation and timeout are not completion evidence.
- All terminal commands use explicit cmd.exe. Keep existing comments and working
  code, pins/reference/locales unchanged. No production/user-profile/physical input
  or capture, live installation, push, merge, publication or shutdown in these phases.
- Native destruction and waits stay outside acceptance locks. Preserve original
  Session/Voice/peer/frame/UI tickets, observed joins and owner-isolated data.

## What must pass before old client removal and publication

1. Complete the real app wiring and all missing user scenarios, including original
   RC frame/viewport/focus ownership, copy-import UI/account proof and update host.
2. Close data/final-save, socket-write cancellation and native retirement defects;
   cancellation alone never establishes driver or process cleanup.
3. Run final default/neural regression, exact React/native UI/RUEN/raster/motion/
   accessibility comparison, local .NET/Go/SFU/RC and physical/two-device/long-session
   tests. Resolve protocol/resource-limit and audio/latency/parity gaps honestly.
4. Provision real trust/certification, accepted bootstrap baseline, complete-native
   and legal approval. Prior inventory has 24 missing upstream license texts; an
   agent or unsigned local sandbox cannot manufacture these approvals.
5. Apply the audited bounded legacy-client deletion manifest in the native branch;
   update production entry points/build/release configuration to native-only and
   verify the release package contains no Electron/WebView/managed legacy helpers.
   Preserve required RUEN/assets/native DSP/models/licenses, import tooling until
   accepted, interoperability tests and permanent migration/reference evidence.
6. Only after complete acceptance push `rust/native-client` and publish beta
   `3.8.0-rust` without a main merge; then perform the previously authorized normal
   non-forced Windows shutdown. Current whole-native readiness remains false.
