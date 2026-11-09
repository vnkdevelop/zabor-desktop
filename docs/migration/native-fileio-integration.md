# Native FileJobs primary ownership integration

Updated: 2026-11-09. Bounded primary integration, not whole-native acceptance.

## Original checks and source boundaries

Primary starts from the preserved `14b5392` coordination checkpoint, including
`e392f75` checked reference integration. The first FileIo wiring ran once in
`sh_1bf7d932b001JMoa4fu56JY00C`, with observed exit 0:

- New FileIo unit cases: 5 passed.
- SessionIo unit and integration cases: 1 + 9 passed.
- Actual worker durable Logout, repeated Leave, partial cleanup failure and failed
  authentication cases: 4 passed.
- Strict binary/test Clippy, binary unit-test build and affected formatting passed.

Nineteen distinct selected tests passed. Exact original commands, source hashes,
test executable/log hashes and owned temporary cleanup evidence are retained in
`migration-artifacts/native-files/primary-fileio-2026-11-09/checks-v1/`.
These original results do not certify the subsequent ownership extension.

The independent FileJobs component is `f8ab4709`, final handoff `5152a0bb`, with
14 original affected tests, strict normal/fixture all-targets Clippy and an offline
native-media link. Their completed original results are inherited, not rerun or
represented as a new primary matrix. The cumulative source patch passed primary
forward admission before import. Its full handoff remains separate from primary
shell binding and live resource acceptance.

## Extended original-owner contract

`OwnedFiles` keeps the original FileJobs, ChatEngine and chat transport together.
Explicit Logout, failed authentication, ordinary worker return and worker abort
register those same resources with FileIo before releasing the worker's owner.
An unpolled/cancelled retirement waiter does not cancel the retained cleanup task
or remove its join. FileIo observes `FileJobs::finish().await` before handing
ChatEngine/cache/native destructor cleanup to its retained blocking worker.
Original join failures remain sticky after cleanup and repeated drains.

ChatIo reconciliation moves the same resource owner through its blocking job.
A cancelled receiver or stale completion retires the original owner rather than
dropping ChatEngine while its FileJobs still run. A current completion restores
the same FileJobs, engine and transport to the original worker. ChatIo drain
observes both its own jobs and their registered FileIo cleanups before cache
reopening; SessionIo shares that exact FileIo, not another category-shaped owner.

The primary bound constructor captures a strong original `Arc<CriticalIntents>`,
the already-issued Session ticket and original global stop before admitting any
save. Leave preserves Session authority. Logout or global stop rejects the old
publisher; obtaining a fresh ticket cannot renew that original callback. Source
acquisition, file IO, join waits and native teardown remain outside intent locks.

## Completed combined verification

The ownership extension and publication-API import have a completed combined
source freeze, affected FileIo/ChatIo/SessionIo/worker cases and strict Clippy.
Real original-publication tests latch the public blocking save after sync, retain
its actual SQLite lease across a cancelled drain, and require that lease to be
released only after the original save join. Panic cases must finish cleanup while
retaining failure.

The original combined `sh_1bfc72321001bBB2K6Jp6xVA4v` passed affected formatting
then failed compilation before running any tests: a module-relative `now_ms`
reference used one excess `super`. Only that reference was corrected. Its failed
`checks-v2/verification.json` and original logs remain unchanged; owned scratch
and the runner were removed after the original exit. The corrected source is
checked by the new original `sh_1bfcad1c8001TKAdVjtKgQDCPR` in `checks-v3/`.
That original job completed with exit 0:

| Fresh primary group | Passed |
| --- | ---: |
| FileIo, including public blocking-save/SQLite-lease cases | 8 |
| ChatIo, including original admitted-file-job restoration | 12 |
| SessionIo unit/integration | 1 + 9 |
| Actual worker Logout/Leave/failure/failed-authentication | 4 |

Thirty-four distinct fresh primary tests passed, zero failed/ignored. Together
with the fourteen unchanged checked component tests, the scoped union is 48;
the superseded first-wiring 19-test run is not added again. Both normal and
camera/stream-fixture binary/test Clippy passed with `-D warnings`. The actual
fixture binary unit-test build and affected formatting passed. The unmodified
Slint build-script `viewport-y` deprecation remains visible, not suppressed.
All twelve frozen source/pin hashes remained unchanged through the checks.
The original marked temporary workspace and runner were removed after exit.

Exact commands, source/test executable/log hashes and cleanup outcome are in
`migration-artifacts/native-files/primary-fileio-2026-11-09/checks-v3/verification.json`.
The original failed `checks-v2` receipt is preserved without reclassification.
New source acceptance does not certify physical, installed or whole-client
readiness. A scoped source save and post-change Graphify completion are recorded
separately by the integrating owner.

The post-change original `sh_1bfcf31ee001OWAIOP3zoRHHuH` Graphify update completed
with exit 0: 8,022 nodes, 20,730 edges and 318 communities. It observes the checked
working source before its save; source/hash receipts, not only a HEAD stamp, bind
the graph to this integration. Ten zero-node inputs, the HTML 5,000-node limit and
the separate document-semantic refresh remain explicit extraction caveats.

Launcher and other independent owners' edits, Cargo/model/SDK pins, source user
profile and legacy project remain outside this integration's scope. No physical
input/devices, production request/profile, installation, signing, publication,
push, merge or legacy deletion is authorized by these component checks.
