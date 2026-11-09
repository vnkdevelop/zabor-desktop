# Immutable native test-reference contract

## Scope and saved baseline

This isolated worktree started clean but at detached `c5d0734`, not the requested
baseline. A non-forced switch of that clean worktree selected saved
`c8a622f727a60a269752013d1e222b9864c50d49`; no reset, merge, original-project write,
legacy deletion, publication, install or production/physical operation occurred.
No project `AGENTS.md` was present. Global rules and the primary Graphify report
were read before scoped implementation; that report advertises stale `aa908e9f`.

Ownership is test-only: `scripts/native/reference-contract.cjs`, its Windows path
attribute checker, `tests/native-reference/`, four existing JS reference adapters,
reference-path hooks in `verify.cjs`/`capture-react.cjs`, and the three Rust test
reference consumers. Cargo manifests/locks/configuration, `build.rs`, production
app/media/RC behavior, launchers/CI/packaging and existing resources remain intact.
Inline standard-Node WebCrypto tests remain untouched.

## One admission boundary

The checked-in `tests/native-reference/manifest.json` pins fifteen exact files to
saved Git commit `c8a622f`: updater TSX/CSS/Tailwind configuration, achievements,
RU/EN, voice/manual-gate/mic, VAD/fvad, Silero worker/model and DFN WASM/model.
Each entry records byte count, working SHA-256, Git blob identity, Git SHA-256 and
the sole permitted checkout transformation (`exact` or CRLF-to-LF comparison).
No source JSON is reserialized. The package version/SRI inventory is independently
matched to the exact historical `package-lock.json` Git blob.

Both JS adapters and Rust tests use this same resolver. Rust no longer embeds or
reads an active renderer checkout directly: its test-only helper invokes the
resolver through explicit `cmd.exe /c`. Neither production Rust nor `build.rs`
acquires a Node/Python/reference-root dependency.

Admission rejects missing/corrupt bytes, out-of-inventory paths, traversal, ADS,
reserved Windows names, separator/case/short-name aliases, hard links, symlinks,
junctions and all Windows `FILE_ATTRIBUTE_REPARSE_POINT` ancestors. Attribute
inspection failure is also fatal. There is no alternative checkout/node_modules
fallback. Historical Git objects must remain available in the test harness's
repository; missing/mismatched provenance fails closed. This is integrity against
the reviewed manifest and local Git history, not signed publisher trust, legal
approval, adversarial protection against replacing the harness itself or an ACL
promise that nobody can write the external directory.

## Frozen root, independent of the active renderer

`ZABOR_NATIVE_REFERENCE_ROOT` explicitly selects an absolute, non-redirected local
root. Without it, the default is explicitly the current harness checkout, with
the same strict checks. This worktree checks renderer text out with LF, whereas
the retained primary has CRLF. Therefore its default catalog admission fails
deliberately. It does not substitute `d088795` or normalized/native catalogs.

`sources.json.gz` is a hash-pinned, bounded thirteen-file byte bundle prepared
from the exact named retained primary files, read-only. The two already retained
native model copies seed the remaining files, with the same Git/hash checks.
No package, node_modules, secret, .env, unrelated source, profile or existing
capture is copied. The fifteen-file total is 19,612,102 source bytes; the bundle
is 3,796,212 bytes. Its SHA-256 is
`49c869511f25ffeaee775d04af4f99066754c522d17652bda8b11fbe1043e4aa`.

Create a new external root without reading the active renderer:

```text
cmd.exe /c node scripts/native/reference-contract.cjs --freeze C:/approved/new-native-reference-root
cmd.exe /c "set ZABOR_NATIVE_REFERENCE_ROOT=C:/approved/new-native-reference-root&& node scripts/native/reference-contract.cjs --check achievements"
cmd.exe /c node --test tests/native-reference/contract.test.cjs
```

Choose an approved existing parent and an absent destination. Existing destinations
are never overwritten. Preserve the resulting reference before any separately
authorized bounded legacy deletion. The frozen source/Rust catalog contract can
run after active renderer removal: it uses only that root, the retained bundle/
native model seeds and historical Git provenance, not active renderer files.

## Explicit remaining reference gates

All fourteen original direct package versions and lock SRI pins are retained, but
none is admitted as an immutable transitive runtime closure. Package-dependent
voice/VAD/Silero/neural tests fail before loading checkout dependencies. Primary
node_modules is the original-project junction and was never read/traversed to
satisfy these packages. Freezing an independent reviewed transitive closure,
including native/ONNX/browser WASM/package relative imports and package versions,
is still required; simply setting `admitted: true` is rejected. No package parity
pass is claimed.

Original Tailwind configuration scans the whole renderer. Replacing its candidate
set with just updater TSX would change the capture input contract. No original
capture fixture or CSS output was safely available for freezing without invoking
the forbidden matrix. `capture-react.cjs` therefore gates before Electron startup,
and `verify.cjs` gates before preparations/builds/captures. Preserve the existing
capture evidence; independently admit the original candidate/compiled-CSS and
package closure before reopening these gates. No Electron or visual matrix ran.

## Verification and handoff

The initial resolver suite passed 26 tests. The final original suite
`sh_1befb7177001BBIkjYdIYdqaAj` passed 27/27, adding exact historical package-lock
pin verification; no skips/cancellations occurred.
Tests cover all fifteen frozen files, EOL/native catalog identity, corrupt/missing
files, aliases/junctions/hard links, immutable inventory, Git provenance and mocked
pre-package/subprocess admission for all adapters, plus the ESM source guard.

The initial narrow Rust runner stopped before compilation at a dependency-cache
fingerprint lookup (`serde_json` underscore versus package-name hyphen). Its failed
receipt is preserved as `tests/native-reference/rust-verification-v1.json`; zero
tests are credited. Its fixture/target/runner were removed after the original
process completed. V2 admitted exact primary dependency fingerprints/artifact hashes
but its first compilation also enabled unrelated protocol dependency tests requiring
avatar/base64; it stopped with zero tests credited. V3 instead compiled that support
closure without its tests, then compiled the actual changed test modules. Its first
test exposed a real new helper cmd-quoting defect; it stopped with zero tests credited.
Both original failed receipts are preserved, and both owned targets/fixtures/runners
were removed. V4 fixes the helper by using the existing explicit cmd argument pattern,
compiles the actual achievements/presence/localization test modules and their minimal
non-test dependency closure into its own approved temporary target, and rechecks all
unchanged source/pin/dependency hashes before and after the original subprocesses.
Original V4 `sh_1beff3fec001aw2zbawnlaL2xw` completed successfully: two achievement
reference tests, eight presence tests and two localization tests passed (12/12).
Both compilations succeeded. Its owned fixture/target/runner were removed, and
the exact commands/outcomes and source/SDK hashes are preserved in
`tests/native-reference/rust-verification-v4.json`. This is not a whole-native
Cargo build or media acceptance.

The read-only final retained-primary guard in `tests/native-reference/source-integrity.json`
checks all fifteen exact working hashes and historical blobs. Eight JS syntax checks
passed. Rustfmt passes for the helper/presence/localization inputs. The achievements test
file has pre-existing formatting differences under Rust 1.99.1; unrelated lines
were not reformatted. Source/pin/artifact hashes guard the original check. Final
checked commit, test counts, original pending jobs and Graphify integrity caveats
are listed in the final handoff below.

Remaining independent gates: immutable package/capture closure, full reference
matrix, visual review, physical/audio/camera devices, two-device/SFU/media/RC,
long-session/import recovery, updater drain/crash/bootstrap, release packaging,
publisher/legal trust and complete-native acceptance. Nothing here authorizes
legacy deletion or proves those gates.

## Final checked handoff

- 39 unique affected tests passed: 27 final JS admission tests plus 12 Rust tests.
  The earlier 26-test JS run is not added again. Failed Rust V1/V2/V3 receipts
  remain unchanged; their test pass count is zero.
- All fifteen retained-primary references still match their exact working hashes
  and Git provenance. The final 25-path Rust source/pin guard and nine exact direct
  dependency artifact hashes match before/after the original successful check.
- Forbidden production/resource/manifest/lock/configuration/launcher/CI/packaging
  paths have no diff. Only the documented test consumers and new scoped test
  material are changed. No unexpected WIP was discarded.
- Original Graphify job `sh_1bf01dfb40015OPhlI4s3SFDPT` ran `graphify update .`
  once, in this worktree only, starting from five bounded read-only primary graph
  artifacts. Result: 14,865 nodes, 32,687 edges, 578 communities. Sixteen source
  inputs yielded no nodes, HTML was skipped above 5,000 nodes, and the CLI explicitly
  did not refresh documentation semantics. `graph-integrity.json` reports zero
  duplicate IDs, missing/dangling endpoints, self-loops and duplicate exported
  undirected pairs. Export checks cannot reconstruct already collapsed extraction
  edges; the borrowed graph/report were not a transactional semantic snapshot.
- All original jobs owned by this task completed. No pending original task jobs
  remain. Temporary test fixtures/targets/runners were removed; required primary
  SDK caches were read-only and retained. Other agents' jobs were not inspected,
  repeated, stopped or claimed as completed.
- The exact checked scoped commit SHA is returned in the final session handoff.
  This is reference-contract completion with package/capture gates still closed,
  not native-client acceptance or authorization to delete legacy material.

## Two-defect follow-up to `2f4c19a`

Review `ses_e40f6ce5effe3t3FaxkhAXnCS1` identified two P2 defects. This follow-up
starts from clean `2f4c19ab7221c1a70eac93aef9ccd14f22b6b371`, keeps all original
proof/failure/history files and manifest/bundle/resource bytes unchanged, and edits
only the same reference/test ownership. No other worktree or primary source is changed.

### Drive-qualified admission and owned failure cleanup

Windows freeze destinations now require a fully qualified uppercase drive prefix
before any creation/write. Drive-rooted/relative, UNC/device/volume aliases, path
traversal, trailing-dot/reserved-name and case aliases are rejected. The existing
parent's canonical volume is checked, including root realpath and `QueryDosDeviceW`
physical volume identity; redirected/substituted/network volumes fail closed.

The safe predecessor regression used an in-memory filesystem, not a drive-root
write. The original resolver failed only after 16 mocked mkdirs and 15 mocked file
writes. Its new before-write assertion correctly failed with exit 1 (31 mutations).
The corrected test passes with zero mutations. Eight focused freeze/cleanup tests
passed in original job `sh_1bf1625f3001e8Ssf4WrREfG18`, including partial write,
directory failure, existing destination and unexpected-user-file preservation.

Freeze now records an exclusive random ownership marker and the identities of
created files/directories. Failure cleanup checks that marker, canonical paths,
identities and owned byte prefixes, then uses only individual unlink and nonrecursive
rmdir calls. It never recursively deletes a reference root. An existing/racing root,
replaced content, redirected path or nonempty user tree is preserved rather than
treated as disposable. `followup-freeze-verification.json` retains the red/green
counts and exact checked inputs. All test fixtures are scoped and marker-owned.

### Explicit per-test admitted snapshots

Rust tests now create `reference::Snapshot::admit("catalogs")` once per test that
uses translations, or `admit("achievements")` for definitions plus catalogs. This
invokes the new `--snapshot GROUP` CLI exactly once, admits every required file with
the original path/hash/Git checks and current root environment, then owns the raw
UTF-8 strings and parsed RU/EN dictionaries. `snapshot.catalog(language)` performs
in-memory key lookup; `snapshot.text(path)` retains exact admitted source text.

There is no global/static/thread-local trust cache. Every new explicit snapshot
re-admits the current root and failures. A snapshot may retain its own previously
admitted bytes during a test; it cannot populate or bypass admission for another
snapshot/test. Existing assertions and language fallback behavior are preserved.
The resolver regression checks changed environment/root and corrupt bytes against
a new snapshot while proving the previous snapshot still owns its admitted bytes.
Three focused Rust snapshot regressions check 400 lookups with one admission,
changed-root ownership and new-admission error propagation.

The original actual V4 timings remain unchanged in the original receipt:
definitions 50.31s, own/peer rows 114.00s, presence 86.73s. A separate predecessor
translation counter regression uses the exact saved function with a mocked
reference subprocess boundary; it does not repeat the slow real-Node matrix.
The first follow-up runner completed the 28-test resolver suite and the expected
predecessor counter failure (400 versus a budget of one), then stopped during
compilation of the new mock regression because its reusable closure needed explicit
`&str` parameter lifetimes. Its failed original outcome and zero Rust passes are
retained in `followup-verification-v1.json`; its owned fixture/target/runner were
removed. Only that test closure annotation was corrected. V2 continues only the
unfinished narrow Rust checks, inheriting the 28 JS and eight freeze passes only
after exact unchanged-source checks. It materializes just the three required
catalog/achievement fixture files and uses the same exact read-only SDK inputs.
The follow-up receipt records original outcomes, corrected test counts, source/
read-only SDK guards and actual new runtime metrics. Package/transitive/Tailwind/
capture gates remain closed; these changes establish no full-native or physical
readiness. The exact scoped follow-up commit is returned in the session handoff.

Original continuation `sh_1bf1bb77c001HznMMLr3qGrftx` completed passed: 17 narrow
Rust tests (three snapshot, four achievement, eight presence and two localization),
both compilations and before/after source, resource, original-history and nine SDK
artifact guards. Its 36 inherited JS/freeze passes are guarded and not rerun or
double-counted. Aggregate follow-up coverage is 53 unique affected tests; the two
expected predecessor failures are not parity passes. V2's approved temporary
three-file fixture/target/runner were removed after the original process completed.

| Exact original test group | Original V4 | Follow-up V2 |
| --- | ---: | ---: |
| Achievement definitions | 50.31s | 3.55s |
| Own/peer localized rows | 114.00s | 1.25s |
| Eight presence tests | 86.73s | 8.30s |

`followup-verification-v2.json` retains the exact original commands, outcomes,
runtime metrics and source/SDK/history guards. No task-owned check remains pending.
The one subsequent worktree-only code Graphify update and export-level integrity
caveats are recorded separately in `followup-graph-integrity.json`; there is no
semantic documentation refresh or full-native acceptance claim.

Original follow-up Graphify job `sh_1bf1fabf6001qS7J1ZQGBgmUC6` completed with
exit 0: 14,900 nodes, 32,733 edges, 575 communities. Twenty-two source inputs
yielded zero nodes; HTML was skipped above 5,000 nodes. Export integrity found
zero duplicate IDs, missing/dangling endpoints, self-loops or duplicate undirected
pairs. The update was code-only, in this worktree only, and cannot establish
semantic documentation freshness or reconstruct previously collapsed edges.
All original task jobs, including this single Graphify update, have completed.

## Final cleanup-race follow-up to `b674b9b`

Review `ses_e40f6ce5effe3t3FaxkhAXnCS1` accepted both preceding P2 fixes, but found
a new check/delete race: pathname unlink/rmdir could remove a replacement object
after identity/content admission. This section supersedes the earlier automatic
cleanup description; all preceding source/proof/failure receipts remain preserved.

The reference freezer now uses a preserve-only policy, not an alleged object-bound
delete primitive. It performs no automatic unlink, rmdir, recursive removal, rename
or marker deletion on success or failure. Unnecessary sidecar ownership-marker
creation was removed. A successful freeze still validates and retains exactly the
fifteen pinned source files, with `cleanupPolicy: "preserve-on-failure"` in its receipt.

A failure after creation returns `NATIVE_REFERENCE_FREEZE_PRESERVED` and a
`freezeFailure` record identifying the persistent destination, initial creation,
`status: "failed"`, `cleanupPerformed: false`, `referenceValidated: false`, original
cause and `cleanupPolicy: "preserve-no-object-bound-delete"`. The CLI emits that
structured diagnostic with exit 1. Partial or redirected destinations are not
claimed complete, cleaned or safe to reuse. Inspection/removal of a real persistent
reference remains a separate user action; there is no pathname check/delete fallback.

The deterministic predecessor regressions intercept the actual deletion entry
after the final predecessor ownership condition, swap in synthetic user objects,
then allow the original deletion. They cover a data file, an empty directory, the
root, a failure marker and a success-finally marker. The original root/marker
identities are checked at the relevant boundary; every replacement fixture lives
wholly inside the externally marked agent test workspace. The successor instead
proves zero deletion entries and preserved replacements, truthful failed state,
unchanged owned-file success/failure, root junctions and partial user arrival.
No real user or drive-root race is exercised.

Test fixture cleanup is distinct from persistent reference cleanup. The external
controller waits for the original Node tests/workers and CLI children to exit,
then removes only its marked quiescent workspace, whose synthetic replacement
bytes are all agent-owned. The test modules no longer remove their race workspace
while their processes are running. Failed persistent destinations in those tests
are retained until that external post-process fixture cleanup.

The canonical/drive-qualified/provenance/snapshot admission body, all Rust test
hooks, source/pins/model/catalog CRLF bytes and original proof history are unchanged.
The previous 17 Rust tests and exact SDK artifacts are inherited only through
unchanged hash guards, not rerun. The first original cleanup check preserved the
five expected predecessor race failures and 16 scoped green passes, but its root
junction fixture tried to rename a directory while its write handle was still
open on Windows. That fixture-only failure is retained in `cleanup-verification-v1.json`.
Only the fixture swap was moved to immediately after handle close, before failure
handling; the preserve policy did not change. The original V2 continuation passed
17/17 scoped JS tests with no skips/cancellations, plus a successful real fifteen-file
freeze with exact pinned hashes. `cleanup-verification-v2.json` retains its original
commands, outcomes, three syntax checks, whitespace/source/history/SDK guards and
post-process fixture cleanup. Original 17-test Rust/SDK proofs were not rerun.

The test driver supplies `ZABOR_REFERENCE_TEST_WORKSPACE` and
`ZABOR_REFERENCE_TEST_OWNER`, with matching `.fixture-owner` and synthetic
`user-marker.txt` bytes. These tests require that external marked workspace and
reject a missing driver before allocating replacement fixtures. A driver must wait
for all original Node/CLI processes to exit before deleting its known-owned fixtures;
it must never apply that diagnostic cleanup to a persistent user reference root.

The one later worktree-only Graphify update is recorded in `cleanup-graph-integrity.json`;
code-only export checks do not imply semantic documentation completeness.

Transitive package, Tailwind/capture, physical, publisher/legal trust and whole-native
acceptance gates remain closed. No primary/other-worktree, production, Cargo, resource,
launcher/CI or packaging bytes are changed. The exact checked follow-up commit is
returned in the final session handoff.

Original cleanup continuation `sh_1bf3b0be6001n44IdMYkBMS9jp` completed with all
17 scoped JS tests passed, fifteen exact positive-freeze resource hashes verified,
three syntax checks and whitespace/source/history/SDK guards passed. Its marked
diagnostic workspace and runner were removed after all original processes exited.
No Rust tests were rerun; the unchanged 17-test proof is inherited explicitly.
Original Graphify job `sh_1bf3d0772001YenpJe4moC4v2o` completed once with exit 0:
14,912 nodes, 32,748 edges, 577 communities, 25 zero-node input warnings and HTML
skipped above 5,000 nodes. Export checks found no duplicate IDs, missing/dangling
endpoints, self-loops or duplicate undirected pairs, but cannot recover previously
collapsed edges or establish semantic documentation freshness. No original task
job remains pending. All previous failed and successful proof/history files remain
unchanged; neither partial reference preservation nor this component verification
constitutes full-native approval.

## Primary integration — 2026-11-09

The checked `edd14ea` cumulative reference slice was restored in primary at
`1f8c4f7`. Its first primary run passed all 45 JS tests, syntax checks, candidate
Git-byte admission and the fifteen-file positive freeze, then failed strict Clippy.
The repeated test-module declarations were consolidated into one `cfg(test)`
module in `src/lib.rs`, with test-only imports in achievements, presence and
localization. Only affected new assertions were wrapped for workspace edition 2021.
The initial no-media check also exposed unrelated feature-dependent MicTest dead
code; production code and lint policy were not weakened. The corrected check uses
the existing native-media configuration.

The final primary run passed strict Clippy, the actual library test build and
17 affected Rust tests: four achievements, eight presence, two localization and
three explicitly admitted snapshot regressions. The 45 unchanged JS passes are
inherited by source/log hashes, not rerun: 62 distinct affected tests in total.
The fifteen-file freeze was revalidated. Original temporary fixtures and runners
were removed after their processes completed; the required caches remain.

That run stopped at an overly broad feature-string assertion: `renderer-software`
occurred only in Slint's build-time compiler subtree, not the shipped renderer.
The failed receipt remains unchanged. A separate normal-runtime-only dependency
tree completed with no software renderer, testing backend or test-source feature.
This reconciliation does not change Cargo manifests, dependency pins or features.
Exact receipts and hashes are under
`migration-artifacts/native-reference/primary-integration-2026-11-09/`.

В primary проверены 17 Rust-тестов и сохранены результаты 45 неизменённых
JS-тестов: всего 62 различных affected tests. Исправлено только общее подключение
тестового модуля и форматирование новых assertions. Строка `renderer-software`
относилась к build-time Slint compiler; отдельная runtime-only проверка не нашла
тестового renderer/backend/source. Старые failed receipts не переписаны.
Эти результаты не означают завершения всего клиента или разрешения удаления legacy.
