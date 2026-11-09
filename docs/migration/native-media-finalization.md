# Media lifecycle continuation

Updated: 2026-11-09. Scoped component checks, not native-client acceptance.

## Current scoped delivery

Fresh sole owner `ses_e409ebb4bffd4z4xTWS74TOr8L` resumed the assigned worktree,
read its graph before source search, and independently reconciled the completed
`sh_1befe072e001R7QYGYXMNg7eZ8` receipts, all 124 source/pin hashes and absent
private target. The inherited 152-test and 25-test groups were not restarted.
The historical sections below remain evidence for their original source stages.

### Completed scenario corrections

- Windows VoiceSession desktop startup now uses
  `ScreenCapture::start_with_audio_runtime` with the same original SharedAudio
  tracker. Acquisition and its descendant retirement are registered before the
  factory runs. Aborting the waiter cancels its original stop flag, but retains
  the late factory and original thread join. Registered capture Drop revokes
  geometry and requests stop without synchronously joining on the caller;
  native worker join/source release run on the blocking pool. Worker panic makes
  the drain sticky-incomplete. The public synchronous standalone `start` and
  unregistered synthetic helper retain their synchronous ownership contract.
- `UserStoppedStreaming` / `StreamDropped` remove the peer's stream subscription,
  P2P/SFU backup tracks, probes and deadlines, not its independent voice lane or
  authorized peer. Audio/video admission requires the original engine peer
  lifetime; SFU audio additionally requires its selected original epoch/lifetime.
- Local share replacement restores P2P share senders previously disabled after
  SFU viewer readiness. Publisher SDP still contains only video and optional
  isolated stream audio, never the microphone; remove_track/AddTrack remain the
  removal/restart route. No SDK `set_track(None)` is introduced.
- `MicProcessor::process_shared` no longer publishes network metrics or processor
  health on success/failure. AudioFlow retains the pre-DSP capture epoch and route;
  health publication checks the original Voice generation, or original Monitor
  capture token when no Voice is active, under short resource guards. Those guards
  contain only identity checks/atomic publication, not DSP, native IO or teardown.
  The original final PCM/metrics commit and token-fenced local failure remain.
  Legacy publishing processor methods retain their existing behavior.

### Completed verification and limitations

- `sh_1bf683be4001Izj86hqni3mLFb`, `checked-v1`: runner CMD quoting failure before
  compilation; zero tests. Its failed receipt and confirmed own-target cleanup
  are retained, not represented as a code regression.
- `sh_1bf698a05001hG79ks6W8q5ZpS`, `checked-v2`: compilation/fmt/diff and five new
  regressions passed. The sixth failed because its fixture reused one track ID
  for stream and voice. Only that fixture was corrected; the five completed
  tests were not restarted. Source hashes retain the exact earlier boundary.
- `sh_1bf767cd50010OfaAq6xostNSH`, `checked-v3`: the corrected lifetime test, three
  new mic tests, the actual loopback processed-microphone path and two narrowly
  affected legacy processor tests passed, seven exact names. Combined with v2:
  twelve distinct affected names, including nine new regressions. Strict normal
  and fixture native-media/all-targets Clippy, scoped fmt/diff, normal feature-tree
  and normal build passed; test-source features are absent from the normal tree.
  All 124 frozen source/pin hashes were independently reconciled afterward.
- Evidence: `migration-artifacts/native-audio/media-scenarios-2026-11-09/`.
  Normal executable SHA256:
  `2cba093536ac310a7d8bf46ee249f5a20ad0b242ede947ca9ca77553d86a2aeb`.
  Private targets and temporary runners/draft were removed only after their
  original child commands exited. Required SDK/build caches are untouched.
- All checks use in-memory sources, synthetic thread/device owners or localhost
  peers; no real microphone, camera, screen capture, input, production endpoint
  or live profile. No new source/pin/model/static-CRT/Cargo/shell/App/RC/file edits
  outside the media scope. Published legal data-flow statements were reviewed;
  the corrections add no storage, recipient, telemetry or clip/network route.
- Synthetic registered joins do not prove physical callback quiescence, bounded
  driver/SDK destruction, runtime-abandonment success or arbitrary external
  last-Arc timing. The pinned NativeVideoSource raw keepalive is an SDK-owned,
  untracked task retaining its native source; the owned worker drain does not
  attest that task's final release. Same-peer network traffic without protocol
  Voice/share epochs, two-device/long-session/AEC/latency and final neural/default/
  visual matrices remain acceptance gates. No whole-native acceptance or release.
- Checked source checkpoint:
  `05e1659b609d0398287883eaf79d52072c8c453a`, parent `b94d36e`; eight Rust paths
  and this scoped handoff. Final Graphify completed afterward: 14,483 nodes,
  31,911 edges, 542 communities; exported dangling/self-loop/duplicate-undirected
  edge checks are zero. All 124 source/pin hashes stayed exact. Nine prior
  zero-node warnings and the 5000-node HTML skip remain; semantic documentation
  refresh and raw-producer collapse are not measured. Receipt:
  `media-scenarios-2026-11-09/checked-graph/verification.json`.
- Final contextual delivery is under
  `migration-artifacts/native-audio/media-scenarios-2026-11-09/delivery/`:
  `native-media-cumulative.patch`, `prerequisites.json`, `verification.json`.
  Prerequisites record the base and delivered Git blobs for all thirteen Rust
  paths, the new scoped document, exact commit chain and read-only primary
  `git apply --check`. This metadata-only follow-up does not alter checked Rust
  bytes or repeat completed tests/graph work. No check job remains pending.

### Safe primary integration recipe

1. Take a clean primary checkpoint and recheck the delivered prerequisite blobs.
   The cumulative delivery is based on `fd8783f0d744b586051ce62fdf89a686e2e4700b`:
   thirteen media Rust paths plus this scoped handoff, including `bfd425c`,
   `e2673df`, `b94d36e` and the current scenario correction. No root/native
   manifest, lock, helper, model, locale, shell or RC file is in that patch.
2. Use the delivered contextual patch with `git apply --check` before applying.
   Never replace whole files or apply a whole-worktree diff. If a primary-owned
   media hunk has since changed, stop and reconcile that hunk against the expected
   base/delivered blobs, preserving the primary's original owners and hooks.
3. Existing primary StartShare desktop/camera, FocusStreamAudio and final shared
   audio seal/wait handlers already exist; do not add duplicates. Their missing
   part at this boundary is integration of these checked media internals. Keep
   all actual playback/video/camera/stream-audio/desktop owners on the same tracker,
   and keep the owning Tokio runtime alive through original worker/supervisor,
   factory and descendant retirement joins. Stop admission/revoke, release
   acceptance/resource guards, request actual teardown, observe supervisors, then
   seal and wait the same tracker. Timeout or dropped waiter is not success or
   permission to discard/replace its still-owned original resource.
4. Parent must run affected integrated local tests/types/Clippy/build and update
   its graph, preserving the existing pins and avoiding another unchanged full
   matrix. This child's binary/receipt is not the integrated primary binary.
5. Actual production update-host AppDrainEvidence is still missing original app
   resource bindings: primary `attest_app_drain` deliberately rejects non-sandbox
   roots with `primary-original-app-drain-binding-required`. Bind actual Session/
   Media/Input/Files/Profile joins in their owning scopes; do not remove that guard
   or substitute sandbox/test receipts. Physical and final-client gates above
   require separately authorized real acceptance, not synthetic attestations.

## Ownership and preserved state

- Worktree: `native-media-finalization`; base `fd8783f0d744b586051ce62fdf89a686e2e4700b`.
- Resumed the seven existing modified audio-playback/runtime/camera/media/
  stream-audio/test/video files without resetting or replacing them.
- Architecture first: primary `graphify-out/GRAPH_REPORT.md` and graph vocabulary;
  coordination from primary `native-parallel-plan.md` / `native-client-handoff.md`.
- No shell, UI, chat/data/files, RC/input, importer/updater, lib/Cargo or pin edits.
  Primary and other worktree targets, pending jobs, fixtures and caches are untouched.
- Primary exclusively owns exact-byte native RUEN/DFN3/Silero assets under
  `rust/crates/zabor-native/assets/{locales,models}`, the asset manifest/verifier
  and narrow include-path changes in `strings.rs` / `neural.rs`. None of the seven
  media WIP paths overlaps that ownership; packaging integration is separate after
  the checked media commit. Legacy/source bytes remain unchanged.

## Candidate changes

- Playback/video workers register original native stream retirement before acquisition.
  Native stream Drop runs on the blocking pool; its original observed join remains
  in the shared cleanup tracker even after sealing, task panic or task abortion.
- Original lane/frame-lease guards revoke only their own authority on every exit,
  including panic and an unrequested abort. Idle peer retirement wakes the worker.
- Camera and stream-audio health is retired on worker return/unwind only after
  native owners are destroyed. Camera cleanup errors also set failed health.
- Existing immutable copied-frame timestamps, original peer/subscription/Voice
  identities, registered acquisition and callback joins are retained. No ticket
  reminting or SDK `set_track(None)`; SFU microphone exclusion is unchanged.
- Voice and MicTest remain independent; Session survives Leave, not Logout.
  Cancellation, timeout, task abortion and receipt admission are not owner completion.

## Verification status

Original v2 completed successfully. Its receipt and all twelve frozen source/pin
hashes were independently verified by the fresh sole media owner. The 152 unique
affected tests passed with synthetic camera/stream-audio sources, in-memory native
streams, synthetic device owners and loopback peers only. Scoped fmt/diff, strict
fixture and normal native-media Clippy, normal build and Graphify update passed.
Normal executable SHA256: `88227ae82e9eae14ed45cee5d1c5dd286837003e3fdc3d586f851c71a6310844`.
Graphify: 14426 nodes, 31683 edges, 553 communities; nine zero-node source warnings
and the 5000-node HTML limit remain disclosed. Semantic documentation refresh is
not measured. The original private target and runner were removed after completion.
No physical audio/camera/screen/input, production endpoint or live profile was used.

## Remaining acceptance gates

- Actual driver/camera/screen callback quiescence and bounded native acquisition,
  destruction and shutdown, including blocked/hung native operations.
- External retained SDK source/track/device Arcs: observed owned retirement does not
  establish destruction of arbitrary outside clones or their final last-Arc timing.
- Runtime shutdown before registered monitors/retirement joins drain; final app
  ownership and no native destruction or waits under acceptance/resource locks.
- Physical devices, two-device/long-session/audio latency and full-client integration.
- Same-peer reconnect traffic without protocol Voice epochs remains unverified.
- Final full neural/default and exact visual matrices are deferred, not passed.
  No push, release, installation, legacy deletion or shutdown.

## Short handoff

Base `fd8783f`; seven inherited media paths plus this scoped document comprise the
first checked source stage. Branch `native-media-finalization`; fresh sole session
`ses_e411cbb17ffeCIpACez4vtz70n` does not resume the provider-broken old context.
Checked source commit: `bfd425c31d62b8544a2b6edd70f26850ce8a7059` (eight scoped paths).
Original v1 `sh_1bca6861b001chdvGXbJ1nFspS` completed: diff/fmt passed, runner cmd
quoting broke MSVC discovery before compilation; zero tests passed. Evidence is
`migration-artifacts/native-audio/media-finalization-2026-11-08/checked-v1/`;
own target and runner removed. Native source is unchanged by that failed runner.
Original v2 `sh_1bca81cc2001IXoso5sVetOtOe` is completed, not pending; evidence
`migration-artifacts/native-audio/media-finalization-2026-11-08/checked-v2/verification.json`.
Fresh verification confirmed all twelve hashes unchanged and 152 distinct names;
no completed test group was repeated. No inherited job or mutable target remains.
Next bounded stage audits external last-Arc, registered acquisition and native
remove-sink/Drop waits outside resource locks. Pins, DSP, source leases, original
owners/joins and SFU microphone exclusion remain unchanged. Parent owns integration;
app has a separate owner. Driver/runtime teardown and physical gates remain false.

## Boundary stage

- Source remains based on `bfd425c3`. Only `audio_runtime.rs` has the second-stage
  correction and five new deterministic regressions. The original green checks
  passed; the exact scoped source-save record is linked below.
- Demonstrated baseline paths: `start` retires the old device under the runtime mutex;
  `maintain` / `reset` cancel pending tokens and replace readiness under that mutex;
  the factory also publishes readiness and cancels failure under the same mutex.
  Cancellation and watch publication can invoke a retained `Waker` synchronously.
- The three new tests inspect lock availability during original device cancel/join/
  Drop and token/watch wake, retain the original acquisition through late return,
  and request a reentrant reset during successful readiness publication. No physical
  device, server, profile, replacement capture runtime or source ticket is involved.
- Preflight `sh_1beeb9e02001GcGnwg3mwGqXCg` completed before compilation: scrubbed PATH
  lacked Git. No target was created and no test ran; its failed receipt is retained
  under `media-boundary-2026-11-09/checked-red/`. Its temporary runner was removed.
- The corrected original red-stage job `sh_1beeca99d001fZrez83Ab7ZMsG` completed,
  evidence `migration-artifacts/native-audio/media-boundary-2026-11-09/checked-red-v2/`.
  It uses the explicit Git executable, `cmd.exe /c`, preserved ProgramData /
  ALLUSERSPROFILE, locked/offline `-j2`, policy-hash-verified primary SDK inputs
  read-only and a marker-owned private target in approved Temp. Compilation, fmt
  and diff passed; all three exact tests failed on the expected lock/quarantine
  assertions, not runner errors. All 124 frozen source/pin hashes were independently
  verified unchanged. Its private target is reused only for the token red and green
  incremental checks below, not a second capture runtime or client acceptance.
- SDK audit: cached libwebrtc 0.4.0 native audio/video stream Drop calls `close`,
  which synchronously removes the original sink before closing its frame queue.
  webrtc-sys 0.3.53 audio/video track removal and track destruction hold native
  mutexes around `RemoveSink`. No native/driver destructor deadline is established.
  Camera and stream-audio originals keep their acquired backend/source and registered
  blocking join; exported source/track clones may still outlive the original worker.
  `AudioCleanupOwner` still depends on the owning Tokio runtime remaining live through
  blocking retirement and join observation. Runtime abandonment and arbitrary SDK
  last-Arc destruction remain explicit parent acceptance gates, not fixed or passed.
- Legal/data-flow documents were reviewed: this lock-boundary correction adds no
  collection, file, server, Voice/PCM or disclosure route; their existing privacy and
  clip-ownership statements remain unchanged. Other ownership and pin restrictions
  above still apply. Scoped source checks do not establish whole-client readiness.

### Post-checkpoint read-only preparation

- Re-read the inherited cleanup owners, native capture/render joins, camera and
  stream-audio originals, playback/video stream retirement and reset/maintenance
  regressions. No production source was changed and no completed suite was repeated.
  At that checkpoint the original red job above still owned its compilation and
  private target; no status poll, duplicate check, temporary target or secondary
  capture was launched.
- Planned start boundary: register the original acquisition and install its pending
  quarantine under `state`, then release `state` before retiring the original device
  or launching the factory. Reset/maintenance must mark the same pending revoked
  under `state`, retain it there, and cancel/publish only after unlocking.
- Planned factory boundary: keep the original pending visible during off-lock
  readiness wake, then recheck its identity, revocation, cancellation and demand
  before installing the returned device. A reentrant reset must reject that late
  device; rejection cancellation/publication and registered descendant retirement
  must occur before releasing the original pending quarantine. Capture the MicTest
  failure token before any publication wake and fail only that captured token, never
  a replacement monitor token acquired afterward. These are unverified design
  constraints, not a checked implementation; add a focused replacement-token proof
  after the source freeze ends if the existing three regressions do not cover it.
- Exported camera/audio source clones and the retained video receiver track remain
  separate from the original registered worker/stream owner. No change to those
  APIs, track negotiation, pins, driver/runtime shutdown or parent acceptance is
  authorized or claimed by this lock-boundary stage. Graph resync follows the actual
  production source correction, not this read-only preparation.

### Focused failure-token proof plan before token-red

- Static failure path: `audio_runtime.rs` captures the MicTest token only after
  readiness publication and pending cancellation. `monitor(true)` replaces the
  MicTest generation without acquiring `SharedAudio::state`; a synchronous retained
  wake can therefore create a replacement capture before the factory fetches its
  token. `MicTest::fail` correctly fences a supplied token, but cannot distinguish
  an old factory failure when that factory supplies the newly fetched token.
- After the original red result, add one exact regression using the existing
  failing synthetic factory and separately armed readiness/cancellation wakes.
  The one-shot wake must retain its replacement `MonitorLease`, begin a replacement
  capture, and record its token without querying or locking runtime state. Assert
  the old monitor is revoked, the replacement token and Arming capture survive,
  and a genuine accepted factory failure still leaves runtime `failed` true.
  This proof has not been added or run; the current three regressions do not
  establish it. No production change or additional check job was launched.

### Original red results and checked green candidate

- The focused plan above has now been implemented as two exact regressions, one
  each for readiness and cancellation wakes. Original token-red job
  `sh_1bef920d9001bW5ZPXg5jBKK8w` completed: production bytes matched the checked
  baseline, compilation/fmt/diff passed, and both tests failed because the old
  factory failure invalidated the replacement capture token. Its failed-red
  receipt is retained in `media-boundary-2026-11-09/checked-token-red/` and its
  temporary runner was removed. No completed inherited test suite was repeated.
- Candidate `start` installs the original registered pending acquisition before
  releasing runtime state and retiring the old device. Reset/maintenance mark
  that pending revoked under state, then cancel/publish and retire off-lock.
  Factory publication retains pending across off-lock wakes and rechecks original
  identity, revocation, cancellation and demand before installation. Rejected
  device retirement is registered before pending quarantine is released. An
  accepted failure retains `state.failed` and fails only the pre-wake MicTest token.
- Original green job `sh_1befe072e001R7QYGYXMNg7eZ8` completed in `checked-green/`:
  five new regressions plus twelve acquisition/reset/maintenance unit tests and
  eight synthetic local runtime tests, strict normal/fixture native-media Clippy,
  normal feature-tree and build all passed. All 25 exact test names are unique;
  all 124 frozen source/pin hashes remained unchanged during the group. Normal
  executable SHA256: `bb2ba19de1c74ca42ced89a0c337a66ba4ee58fda1cd919cb4200aba3814fb7a`.
  The original marker-protected private target was removed after successful
  completion, and the runner self-removed. No polling, duplicate check group,
  physical device, production endpoint or live profile was used.
- Independent original Graphify job `sh_1befe0760001v6V1tcrq2oxtXc` completed in
  `checked-graph/`: 14453 nodes, 31794 edges, 546 communities. The receipt, graph/report
  hashes and all 124 unchanged source/pin hashes were independently verified. Nine
  zero-node source warnings and the 5000-node HTML skip remain disclosed; semantic
  documentation refresh is not measured. The scrubbed runner self-removed after
  completion. No graph retry or completed check group was repeated.
- Original checks are complete; no check job or mutable target remains pending.
  The checked correction changes no manifest, lock, pin, comment, source-lease or
  sender negotiation route. Driver/runtime shutdown, outside last-Arc and full-client
  gates remain unchanged. Checked second-stage source commit:
  `e2673dfc43dfbe91f918c2629c0d331dc93b0b35`, parent
  `bfd425c31d62b8544a2b6edd70f26850ce8a7059`; only `audio_runtime.rs` and this handoff.
  This documentation-only followup records that source hash; it does not change
  checked Rust bytes or rerun completed checks. Exact source/handoff commit chain,
  scope, Git blobs and receipt/cleanup verification are recorded in
  `migration-artifacts/native-audio/media-boundary-2026-11-09/checked-save/verification.json`.

## Exact existing API contract for primary original drains

These are existing APIs, not new shell/main hooks or whole-client drain attestation.
The boundary correction passed the scoped original green checks above.

| API | Contract |
| --- | --- |
| `SharedAudio<D>::reset(&self)` | Synchronous revocation/retirement request, not completion. Retains the original pending acquisition until its factory returns; cancellation/publication and owned device retirement run after the runtime state mutex is released. |
| `SharedAudio<D>::try_prepare_reset(&self) -> Result<Option<AudioRuntimeReset<'_, D>>, &'static str>` | `None` means a required resource lock is busy. The plan's `apply()` revokes under its retained guards; `release_resources()` releases those guards. Dropping an applied plan then cancels/publishes and retires the original device. `resources_complete()` describes resource-lock availability, not completed shutdown. |
| `SharedAudio<D>::seal_cleanup(&self) -> Result<(), AudioCleanupError>` | Idempotent, irreversible cutoff for new root registrations. Already registered factories/tasks may still register their original descendant retirement. Call only after teardown operations that need root retirement registration; a new device retirement after sealing uses the fail-closed synchronous fallback and makes cleanup incomplete. |
| `SharedAudio<D>::wait_for_cleanup(&self) -> Result<(), AudioCleanupError>` | Async observation of the shared tracker. `Ok(())` requires zero registered jobs and no recorded cleanup failure; failures return sticky `Incomplete`. It does not cancel work, seal admission, impose a native deadline, or prove destruction of untracked external SDK clones. An unsealed zero-job observation is not a final cutoff. |
| `SharedAudioDevice::finish_shutdown(&self) -> Result<(), AudioCleanupError>` | Synchronous original-owner shutdown observation. The default is `Unsupported`; `NativeAudio` invokes both original capture and render `join_shutdown()` calls and maps their failure to `Incomplete`. Registered retirement completes only after this observation and release of the owned device Arc. |
| `AudioPlayback::clear(&mut self)` / `VideoReceiver::stop(&mut self)` | Revoke the original subscription, request cancellation and abort its task; neither is destructor completion. The original task join and blocking native-stream Drop remain separately registered. Their async `wait_for_cleanup()` methods observe their own configured tracker. |

- Shared aggregation must preserve the original tracker: within the native crate,
  `SharedAudio::cleanup_tracker()` and `AudioPlayback::with_cleanup()` are
  `pub(crate)`; `VideoReceiver::with_audio_runtime()` is public. The standalone
  `AudioPlayback::new()` / `VideoReceiver::new()` constructors create independent
  trackers, so the shared runtime drain does not cover them automatically. Camera
  and stream-audio startup retain their supplied original tracker and blocking join.
- Primary must stop new admission, revoke original capture/subscription authorities,
  release its acceptance/resource guards, request teardown on the actual owners,
  and observe its original supervisor/worker joins before sealing and awaiting the
  final shared drain. Keep the owning Tokio runtime alive through all blocking Drop
  work and async join observers. A timed-out/abandoned waiter must retain the same
  original owners and tracker; it is not permission to replace them or claim success.
- `MediaEngine::close()` is synchronous peer/track/ADM teardown, not this async
  cleanup observation and not evidence of bounded SDK destructor/callback quiescence.
  This stage adds no aggregate app drain, update-host attestation, sender-track
  negotiation change or primary hook. Never substitute `RtpSender::set_track(None)`.
- The inherited checked stage proves registered stream Drop and task-join handling
  across return/panic/abort. The five new regressions failed on their predecessor
  defects and passed with the correction, alongside the twenty narrowly affected
  original acquisition/reset/maintenance tests. None of these component proofs
  establishes arbitrary outside last-Arc timing, physical device readiness,
  runtime-abandonment success or whole-client shutdown bounds.
