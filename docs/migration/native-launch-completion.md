# Native developer launcher and CI cutover

## Boundary and integration base

This isolated change starts at `c8a622f727a60a269752013d1e222b9864c50d49`
in `C:/Users/porad/.local/share/opencode/worktree/b1b3f5/native-launch-completion`.
The provisioned worktree was initially clean and detached at `c5d0734`; it was
switched to the requested saved base before edits. No other worktree was changed.
The initial local Graphify query found no ignored graph. The primary's existing
`graphify-out/GRAPH_REPORT.md` was subsequently read only: it reports 7,725 nodes,
19,781 edges and base `aa908e9f`, so it is an architectural map, not proof of the
current primary's source freshness. Primary sources and caches were not used for
builds or altered.

Ownership is limited to `native.bat`, package script entrypoints, the two named
workflows, scoped developer/test files under `scripts/native`, and this
document. Cargo manifests, locks, `.cargo` configuration, Slint, DSP, UI, main,
media, import, updater, and `scripts/native/packaging` remain unchanged. The
primary's certified-helper/model packaging integration is not duplicated here.

## Developer commands, not end-user startup

Use Windows x64, the Windows .NET Framework 4 compiler tools, and the installed
Rust MSVC toolchain. `native.bat` compiles a small developer launcher into
`rust/target/native-developer-tools`. Ordinary commands use no Node or Python,
never install npm packages, never invoke Electron, and never provision caches.
This launcher is **not** a bootstrapper, installed entrypoint, updater, trusted
helper, or certified end-user runtime. End users launch the separately certified
`zabor-native.exe` package; they must not need Cargo, Node, Python, or this script.

```text
cmd.exe /d /c native.bat
cmd.exe /d /s /c "native.bat help"
cmd.exe /d /c native.bat build
cmd.exe /d /c native.bat build --release
cmd.exe /d /c native.bat check
cmd.exe /d /c native.bat cache-check
```

Raw calls accept only fixed trusted selectors and presets: default run, `run`,
`build`, `build --release`, `check`, `help`, `fmt`, `clippy`, `test`, cache/preparation
or explicit reference selectors, and the fixed `run --scale 2` preset. The batch
script has no `%*` sink. Its C# launcher inspects only its own explicit cmd caller,
applies CMD `/S` outer-quote rules, and validates CMD quote/operator syntax before
decoding the small selector vocabulary. CMD does **not** use CRT backslash-quote
escaping; raw CRT-escaped runtime argv is no longer an advertised API.

Runtime/untrusted argv must be a JSON string array supplied **before shell
interpolation**, through exactly one process environment field:
`ZABOR_NATIVE_ARGV_JSON` (the JSON itself), or `ZABOR_NATIVE_ARGV_FILE` (a trusted
JSON request file path). Launch the fixed command below with that environment
already set by the host/process API or a prior user-session configuration:

```text
cmd.exe /d /v:off /s /c "native.bat data"
```

Example request content, not shell text to interpolate or echo into `/C`:

```json
["run", "--preview", "updater", "--scale", "1.25", "!LITERAL!", ""]
```

The data route preserves spaces, empty arguments, embedded/odd quotes,
backslashes, percent signs, exclamation marks and operators as data, independently
of caller `/V:ON` or `/V:OFF` and matching environment variables. Flag-first
requests and the `--` literal delimiter remain available **inside JSON argv**.
Missing, mixed or invalid requests fail closed; `data` never silently defaults
to run. Do not append runtime argv to raw `native.bat` or npm command strings,
and do not build an inline `set ... && native.bat` caller chain. `native:argv`
is an optional npm wrapper for the same fixed data selector, not npm setup.

Response-file input is decoded through a live UTF8-default/BOM-detecting reader
into one fixed 8,192-character buffer. Exactly 8,191 decoded UTF16 character units
are permitted; observing the next character rejects the request before creating
the payload string or parsing JSON. The bound remains active across short reads,
multibyte encodings and file growth. No file-length precheck or full-file read is
used. The reader/handle is disposed on EOF, overflow and read errors. JSON escape
sequences count as their decoded file-text characters before JSON interpretation.

Interactive direct invocation, CALL/batch chains, raw runtime args, unresolved
variable selectors and unsupported caret/operator syntax fail child admission.
No caller `%VAR%` or `!VAR!` reconstruction occurs. The child cannot undo commands
already queued by its caller: raw caller interpolation/quoting is the caller's
responsibility, and a queued external command can still run after child denial.

Child commands run through explicit `cmd.exe /c`, using delayed environment
expansion to avoid a second interpretation of argument values. The fully expanded
cmd command, including executable/quoting/prefix text, must fit the 8,191-character
limit; oversized command/environment transport fails before child admission.
Original nonzero child exits propagate and stop checks without retries. Native
build options are deliberately restricted to `--release`; package, manifest and
target overrides are not admitted.

Each invocation atomically reserves its own random tool directory, compiles one
immutable executable there, waits for the original compiler/launcher exit, and
then removes only that executable and its empty reserved directory. An inherited
RANDOM override is removed before generating reservation components. Concurrent
dev/build/check operations never overwrite an executing image. Existing artifacts
are not swept or deleted. Unexpected files prevent directory removal and produce
a cleanup warning rather than a recursive delete. An externally interrupted
process chain requires explicit owner reconciliation; it must not be inferred
complete or cleaned by a later invocation.

All Cargo compilation/testing commands explicitly select `-p zabor-native`,
`--manifest-path rust/Cargo.toml`, `--locked`, `--offline`, and the Windows MSVC
target. `fmt` explicitly selects the same package and checks without formatting.
Compiler output is isolated in this checkout's `rust/target/native-developer`.
The launcher fixes cache environment paths and rejects inherited Rust flags that
could override the pinned static CRT configuration. It does not change any pin.

`check` runs format, all-target Clippy, native **library and binary unit tests**,
and a production-feature debug build. Test/Clippy routes explicitly select the
existing `camera-test-source,stream-audio-test-source` synthetic native fixtures;
the normal run/build/release-artifact routes never select them. Present Rust flag
override variables are rejected even when their values are empty, so empty
environment overrides cannot suppress the pinned static CRT configuration.
It intentionally does not execute the reference integration targets
(`crypto_webcrypto`, neural reference cases, voice activity/filter reference
cases), the Electron screenshot parity harness, or a physical-device acceptance
matrix. This is a bounded developer check, not complete migration acceptance.

## Explicit cache preparation

Read-only admission uses the existing pins, not newly selected dependencies:

- ONNX Runtime 1.31.1: exact preparation identity and six file SHA256 pins.
- libclang 18.1.1: exact wheel identity and two file SHA256 pins.
- RNNoise: existing `4ac81c5c...` model archive SHA256.
- WebRTC a545de4: existing packaging-policy static library and license SHA256.

ONNX/libclang preparation and packaging pins must agree. Manifests must have the
exact pinned file map; missing/changed files and additional unhashed files fail.
The checker hashes existing files and writes nothing to these caches. WebRTC's
library/license checks do not certify every SDK header or replace package trust
and provenance verification.

```text
cmd.exe /d /c native.bat prepare
cmd.exe /d /c cargo fetch --manifest-path rust/Cargo.toml --locked --target x86_64-pc-windows-msvc
```

`prepare` is an explicitly requested **developer-only Node command** using the
three existing pinned preparation scripts in order. It stops on the original
failure. It does not install/rebuild npm packages or invoke the legacy postinstall.
It then performs complete cache admission and can still fail if the separately
vetted WebRTC SDK is absent. Restore that SDK into this worktree's
`rust/target/webrtc-win-x64-a545de4`; do not modify the primary's cached inputs.
There is no invented WebRTC archive pin, unverified download, automatic Cargo
fetch, alternate toolchain, or legacy fallback. Missing caches explain these
explicit preparation steps and fail closed.

## Active and reference npm routes

`start`, `dev`, `dev:4k`, `build`, and `test:native` now route to the developer
launcher. Invoking them through npm naturally requires npm/Node as a wrapper;
direct `native.bat` invocation does not. `postinstall` only prints explicit
developer preparation instructions; its former addon/model effects survive
only under the explicitly named `reference:postinstall` command.

Those ordinary entrypoints contain fixed trusted selectors/presets. Use the
separate `native:argv`/`native.bat data` process-environment contract for runtime
arguments; untrusted arguments appended through `npm run ... -- ARGS` are not a
secure data transport. Launcher-test entrypoints and native CI include the focused
`developer-transport.test.cjs` suite as well as the inherited launcher checks.

Legacy `reference:start`, `reference:dev`, `reference:dev:4k`, and
`reference:build` preserve the old Electron/WASM preparation and build tooling.
Existing source/configuration files remain present. Native parity is explicitly
requested through `native.bat reference-check` or `test:native:reference`, using
the existing `scripts/native/verify.cjs` without modifying it. It still requires
the independently prepared Node/Electron/Python reference environment.

`dist:win`, `dist:win:beta`, `release`, `release:beta`, `publish` and
`prepublishOnly` are blocked launcher routes. They do not run legacy release
scripts, invoke packaging, sign, install, tag, upload or publish. These developer
builds do not satisfy the certified native packaging gate.

## CI channels and artifact gate

`rust-migration.yml` handles the two native branches, `v*-rust*` tags, explicitly
named native published-release checks, and native-head/native-base pull requests
including `rust/native-client -> main`. PR triggers include main as a target;
job predicates inspect `head_ref` and `base_ref`, not the synthetic merge ref.
Its default manual lane is native. Windows/cmd/MSVC native checks explicitly invoke
format, Clippy, native library tests and a release build. Preparation uses pinned
developer scripts, with no npm installation or Electron in this active lane.
Cargo dependency fetching is explicit, locked and performed before offline
checks; it fetches workspace dependencies for the native target, not a claim
that Cargo can filter `fetch` to a package.

WebRTC provisioning restores only the exact GitHub Actions cache key
`native-webrtc-win-x64-a545de4-3e5959b5320579814706848a0641224db22f03a374493e90cbd235948977f930`.
There are no broad restore keys or download fallbacks. Missing seeded cache fails
the native job. An authorized CI cache-seeding/provenance process remains required;
this isolated task did not create or publish such a cache.

The executable must exist at
`rust/target/native-developer/x86_64-pc-windows-msvc/release/zabor-native.exe`.
An explicit existence gate precedes upload, and upload uses
`if-no-files-found: error`. The artifact is named as a **developer** build and is
not an installer or certified standalone package.

The manually selected `reference` lane runs legacy build/unit checks with
explicit reference postinstall; it is not screenshot parity or signed native
acceptance. `build.yml` retains the legacy workflow but names its verification
lane truthfully, uses `reference:build`, and restores its preparation explicitly.
Both its verify and stable release jobs reject native Rust tags, release tag
names and native branches; its verify predicate also excludes native PR heads
and bases. Its manual default channel cannot enter legacy
packaging. `v3.8.0-rust` cannot trigger the legacy stable job, even when the event
is a published release whose ref points at main. The existing non-Rust stable
event predicate remains distinct, but distribution entrypoints are fail-closed.

## Verification record

All commands ran in this isolated worktree through explicit `cmd.exe /c`.
No Cargo run/build/check, npm setup, Electron process, real reference matrix,
production profile, physical input, signing, installation or release occurred.
All mock/cache fixtures and compiled test executables were created under owned
`C:/Users/porad/AppData/Local/Temp/opencode/native-launch-*` directories and removed
by the test teardown, including after failed tests. CI uses `RUNNER_TEMP` instead.

```text
cmd.exe /d /c "node --test scripts/native/developer-launcher.test.cjs"
```

The original completed check history is retained rather than hiding failures:

| Revision/check | Exit | Passed/total | Result |
| --- | ---: | ---: | --- |
| Initial scoped mocks | 1 | 43/44 | Windows separator assertion identified; paths then canonicalized |
| Revised scoped mocks | 0 | 44/44 | Passed before cache-manifest extensions |
| Extended mocks / explicit nested cmd wrappers | 1 | 53/54 | Batch metacharacter forwarding regression found in the new wrapper |
| Final extended mocks / delayed environment expansion | 0 | 54/54 | All passed; 0 skipped/cancelled; 21,858.3358 ms |

The final 54 checks cover package/target admission, runtime argument quoting,
spaces/metacharacters in the checkout path, original process exits, first-failure
stopping, missing tools/caches/pins, read-only SHA256 verification, manifest
identity/count changes, unhashed DLL rejection, explicit preparation/reference
routes, publication blocking, static entrypoints and evaluated workflow event
predicates. Mocks replace the cache admission class only at **test compilation**;
the production launcher has no environment switch that bypasses real cache
verification. Real verifier tests separately compile and exercise the actual
cache class. Small nonproduction fixtures do not claim possession of the actual
ONNX/RNNoise/WebRTC binary caches.

YAML parsing and Node syntax checks are separate from static predicate coverage:

```text
cmd.exe /d /c "node --check scripts/native/developer-launcher.test.cjs"
cmd.exe /d /c "python -c ""import yaml; from pathlib import Path; paths=['.github/workflows/rust-migration.yml','.github/workflows/build.yml']; parsed=[yaml.load(Path(p).read_text(encoding='utf-8'),Loader=yaml.BaseLoader) for p in paths]; assert all('on' in p and 'jobs' in p for p in parsed); print('YAML syntax: 2 workflows passed')"""
cmd.exe /d /c "git diff --check"
```

The test freezes all eight implementation/test/workflow files and eight immutable
pin inputs throughout each check. The original `02f31b3` delivery's local
working-byte SHA256 proofs follow and are retained as historical evidence; the
follow-up's changed-file hashes are recorded below. Git checkout line-ending
conversion can change working-byte hashes elsewhere.

| Scoped file | SHA256 |
| --- | --- |
| `native.bat` | `fdcb9124d38e90151431c5838d5f8bf2cd0004319ab799870738d3ec1ca58c1f` |
| `package.json` | `886bb56fc9092aa838aa13e5c6596a527b7f9635e17145b80b42973b8f969f5a` |
| `.github/workflows/rust-migration.yml` | `884ee2a197f4d6d2350f99017b3126f3ef7b6b05265be6b9d56e25452daad224` |
| `.github/workflows/build.yml` | `eb13965ce3ba6eafc46835320e0a0bb667c183bb8dbebea92277870961290589` |
| `scripts/native/developer-launcher.cs` | `c86bcacc66b3dce323f111fe3f8bb348f0c5f8d344445605e0077382a891fc66` |
| `scripts/native/developer-cache.cs` | `4c79138b6ec9f8265cd91f454f4739af267af8b260a74a475bd945f15097a095` |
| `scripts/native/developer-cache-regression.cs` | `a8746675002ad195d3db37d81204cdabfb7e7e6b9e52b5119c3ee2e0680ed34f` |
| `scripts/native/developer-launcher.test.cjs` | `9ceba309a53709c263bfd96b85300bd49df0b65dc857e06aaddce870325bfd9a` |

| Unchanged pin input | SHA256 |
| --- | --- |
| `.cargo/config.toml` | `8feb1d003d7cca188186af5cc2523152f2e2ac3abaa95409315f7eb015f358b9` |
| `rust/Cargo.toml` | `9da9a5dc7733144d5d424af0157c8508b7b2d9914a1ab471a4151f3e8ba22f2f` |
| `rust/Cargo.lock` | `48db95ccb660a6d1e48a4056ee8dbf0855f640e33ec78de42e6075a289c9c31c` |
| `rust/crates/zabor-native/Cargo.toml` | `c1331cebe377ff1e42428334469c4503776c2b9230d5e2df137bccf2235e3957` |
| `scripts/native/prepare-onnx.cjs` | `7172c86082fbfc548cbb0ce7e18ddc3e30cd577e7b54d14db086b6f3b30365d7` |
| `scripts/native/prepare-libclang.cjs` | `68d8dcf6476dae02c73dceed579b90945c4fa0661b3679f2d352b1b4f9299841` |
| `scripts/native/prepare-rnnoise.cjs` | `134352cee1d33eb14a4d85d9a6ab9b7a85f17fbf23310aa7f621b84049953a97` |
| `scripts/native/packaging/policy.json` | `413820a8f4dfe7941246606adb735c1cd323067bf363d33b1a7219dd49393209` |

## Remaining gates

Delivery is an isolated reviewable source cutover, not a completed native release.
The primary still owns integration with its staged certified-helper and mandatory
native model SHA256 packaging work. Real native CI compilation/tests require the
vetted SDK and explicit dependency caches. Existing native/reference integration,
full parity, clean-machine startup without developer tools, package trust,
provenance, signed acceptance and physical camera/audio/remote-control gates
remain separate. No full-client/native-ready claim is made here.

The single post-edit graph synchronization command was
`cmd.exe /d /c "graphify update ."`, scoped only to this worktree. It completed
AST extraction for 611 uncached files, warned that 12 sources produced zero nodes,
and exceeded its 60,000 ms foreground timeout before graph/report completion.
Only ignored `graphify-out/cache` output was observed, not a completed graph or
report. The command was not retried. Its original process completion is unknown:
a single worktree-filtered process diagnostic could not import optional `psutil`,
and no dependency was installed or alternate job launched. This is not a
refreshed architecture claim. The primary graph was not updated.

All owned launcher test, syntax, YAML and whitespace-check commands completed in
the foreground; there are no pending test/build jobs. Primary jobs were not
inspected, polled, retried or interrupted. The timed-out graph command remains
an explicit graph freshness/process-completion caveat for the integrating owner.

## Six-defect review follow-up

The read-only review `ses_e40febbe6ffeeaXyfkJeSpulfB` identified six defects in
`02f31b391242f021139c44214f3994cf6ef45916`. No primary integration occurred before
this correction. The original completion boundary was reconciled only after the
user authorized one bounded recovery audit.

At `2026-11-09T06:43:32.979308Z`, the audit used a single Toolhelp process inventory
and read-only OS identity access under existing ACLs. It filtered cmd/Graphify/
Python runtime images to the exact worktree, checked working-directory identity,
and excluded the audit's own ancestor chain. It found **zero active worktree
runtime identities and zero unresolved candidate identity reads**. A read-only
query of retained OpenCode command metadata yielded no additional original
receipt/PID. The original foreground tool receipt still records only the
60,000 ms timeout, not a child exit. This establishes resource inactivity at the
audit boundary under the user's recovery rule, **not graph success or freshness**.
The original update remains **FAILED/INCOMPLETE**, with no invented exit code.
There was no repeated snapshot, running-log read, retry, process kill, elevation,
or other-owner inspection. The owned audit script was removed after execution.

| Finding | Surgical resolution |
| --- | --- |
| P1 raw `%*` CMD injection | Removed all positional expansion from the batch wrapper; own-caller metadata is parsed by C# as data, with explicit unsupported-boundary rejection |
| P2 concurrent compiler image lock | Per-invocation reserved immutable tool images; original exit observed before exact-owned cleanup, with no existing-artifact sweep |
| P2 empty Rust flag environment overrides | Presence checked case-insensitively, including empty values; configured CRT flags cannot be silently suppressed |
| P2 missing synthetic native fixture coverage | Test/Clippy explicitly select both fixture features; unit test selection includes `--lib --bins`, not reference integration targets; production builds exclude fixtures |
| P2 PR merge-ref routing | Native target triggers include main; native and legacy predicates use actual PR head/base identities and explicit tag/published-release channels |
| P3 CMD command-length over-admission | Full expanded executable/arguments/quoting/cmd prefix checked against 8,191; oversized environment values rejected before child admission |

Only the five affected implementation/test/workflow files and this owned document
change in the follow-up. `package.json`, cache verification code, preparation
scripts, packaging policy, Cargo manifests/lock/config, and all other owners'
sources remain unchanged. The Windows Management dependency is built into the
developer compiler environment; it is not added to the native end-user runtime.

Focused command, without rerunning the original 54-case suite or a Cargo matrix:

```text
cmd.exe /d /c "node --test --test-name-pattern followup scripts/native/developer-launcher.test.cjs"
```

| Follow-up check | Actual exit | Passed/total | Observed outcome |
| --- | ---: | ---: | --- |
| Predecessor implementation plus focused regressions | 1 | 8/25 | All six findings exposed across 17 failures, including actual injected echo, truncation, empty-env admission, missing features, image-lock failure and PR/channel misrouting |
| Initial surgical correction | 0 | 25/25 | Same focused cases green |
| Extended lifetime/boundary checks | 1 | 29/30 | Fixture-only chained-batch path failed before reaching the validator; original exit 1 retained |
| Corrected fixture and concurrent build/check coverage | 0 | 31/31 | All passed; zero skipped/cancelled; 24,441.0829 ms |

The final check records 28 original compiler/mock-process completions, all with
null signals, in this ordered exit-code vector:

```json
[0,0,0,0,2,0,0,17,73,1,2,2,2,2,2,0,0,0,0,0,0,2,2,0,0,0,0,0]
```

The first four are successful test compilers; deliberate compiler failure is 1;
the two batch exit-forwarding cases are 17 and 73; validator denials are 2.
Both held original dev processes and their concurrent build/check siblings exited
0 before teardown. Their tool paths were distinct, existed while the corresponding
original process ran, and were gone only after original exits. The preserved
fixture artifact remained intact. Tests use a stdout readiness receipt plus an
explicit mock stdin release, not sleep, polling or process termination. Every
owned mock directory and executable was removed by teardown after completion.

Final changed-file working-byte SHA256 proofs, overriding the historical entries
for these files only:

| Scoped file | SHA256 |
| --- | --- |
| `native.bat` | `1a124a0c1b8950d04215d50277a01790c50eb9c9dc12bd4a8eaeee136ac3f51d` |
| `.github/workflows/rust-migration.yml` | `e081a696c73016e6ba2a160c82468cbc766fd54898f05971d679e32ffc1a669e` |
| `.github/workflows/build.yml` | `c676cd249c7f203ea6f40563ba1695c5bdc67849b2d495f1211c5ba93d966e22` |
| `scripts/native/developer-launcher.cs` | `4fe4f549f3a2789e36242a5421ed1ad4dfc35f2beafe3c8b6756dccff7992a69` |
| `scripts/native/developer-launcher.test.cjs` | `052ef40a0bd05ac253699ac3e512c1c56a8272a98be112bfac6eba7ed59e7b4d` |

All eight immutable input hashes in the earlier table remained equal through
each focused run. Native compiler/runtime/reference/physical/trust gates are
still separate; mock feature selection does not claim those real tests ran.
One subsequent graph synchronization is required for the genuinely changed
scoped source, with explicit original-process identity and observed completion;
it is not a retry of the unchanged, incomplete original resource.

### Observed post-change graph completion

The new update ran once through an owned observer with the foreground tool timeout
disabled. Its subprocess was explicitly
`cmd.exe /d /c "graphify update ."` in this worktree. The observer retained original
identity before waiting on completion, without repeated sampling or running-log
reads:

```json
{
  "pid": 10440,
  "observerPid": 12104,
  "startedUtc": "2026-11-09T06:58:21.481412Z",
  "createdUtc": "2026-11-09T06:58:21.485043Z",
  "completedUtc": "2026-11-09T07:00:57.053768Z",
  "originalExitCode": 0,
  "timeoutDisabled": true,
  "frozenFileCount": 17,
  "frozenFilesUnchanged": true,
  "graphJsonExists": true,
  "graphReportExists": true
}
```

This new **code graph** contains 14,799 nodes, 32,569 edges and 548 communities;
the report declares 611 corpus files, zero token cost, and commit stamp `02f31b39`
(HEAD while syncing the corrected working source before its follow-up commit).
The 17-file guard ties that observation to the checked working bytes. It is not
proof that the report's commit stamp equals the later follow-up commit. The CLI
warned that 12 sources produced zero nodes and skipped HTML for exceeding 5,000
nodes. Document/paper/image semantic changes are not extracted by this code-only
command; this receipt/document addition does not claim semantic graph coverage.
The original timed-out update remains FAILED/INCOMPLETE independently of this
new, successfully observed code update.

| Scoped output | SHA256 |
| --- | --- |
| `graphify-out/graph.json` | `12cdf40b07969080a8496d2caa511674b8fb561813b99a88f6d863e2ccf23c98` |
| `graphify-out/GRAPH_REPORT.md` | `00de9d37142aa1524c4e266b84ba0073b873ec2526f66a89a1a905f54124b58b` |

These graph outputs remain ignored, intentionally retained only in this worktree;
they are not part of the source commit. Both owned observer files were deleted
after the actual original exit was observed. No graph/test/build process remains
pending. Node syntax, both YAML parses, whitespace checks, and the 16 current
source/pin ledger hashes passed separately from graph synchronization. Primary
sources, graph, jobs, caches and integration remain untouched.

## Argument-transport correction after 173232b

The same reviewer identified three remaining problems with caller reconstruction:
CRT escaped quotes were mistaken for CMD syntax protection; `%VAR%` and `!VAR!`
were reconstructed without honoring the caller's mode; and `/S` outer single
quotes were not stripped correctly. The current contract above supersedes the
first correction's raw-CRT argument advertisement. It does not claim that a
child can prevent its caller from executing already parsed command chains.

The correction is limited to CMD selector validation, explicit JSON/response-file
argv receipt, its test callers/entrypoints, and this document. The lifetime and
cleanup batch script, present-empty Rust flag rejection, synthetic-vs-production
features, PR/channel predicates, and downstream 8,191-character guard remain
inherited. A focused static inheritance check compares those five fixes directly
with `173232b394589a2aad64ec98776ddcc2cec15934`; their old 31 cases and the original
54-case suite were **not** rerun. The inherited test helper now sends batch argv
through the JSON data contract, rather than presenting its delayed-env harness
as evidence about an initial raw CMD parse.

Actual raw CMD and argv-as-data tests are distinct:

```text
cmd.exe /d /c "node --test scripts/native/developer-transport.test.cjs"
```

| Transport check | Original exit | Passed/total | Outcome |
| --- | ---: | ---: | --- |
| Predecessor 173232b plus new focused cases | 1 | 3/21 | Raw chain admitted as Cargo data, wrong/missing V:OFF reconstruction, `/S` rejection, and absent JSON contract demonstrated |
| Corrected selector/data contract | 0 | 21/21 | Same focused cases passed |
| Final action forwarding and five-fix inheritance checks | 0 | 25/25 | All passed; zero skipped/cancelled; 15,328.5324 ms |

In the genuine raw `/C` chain test, the caller's external `echo` runs in both red
and green revisions. The introduced decoder regression is **Cargo child admission**:
the predecessor admits the chain as a runtime argument, while the correction
reports unsupported CMD operators and records zero Cargo calls. The caller's
final exit remains 0 because its queued commands execute; this is not recorded as
a child exit or misrepresented as prevention of caller commands. The four JSON
data cases use a fixed initial CMD command and a process environment payload,
proving that their quote/operator content never becomes initial shell syntax.

The final run retained 29 original process completions, all null signals, ordered
as mock compiler; 24 actual CMD/data cases; and four read-only prior-commit reads:

```json
[0,0,2,2,0,0,0,0,0,0,0,29,0,0,0,0,0,0,2,2,2,2,2,2,2,0,0,0,0]
```

Exit 29 is the response-file runtime forwarding case; 2 values are intentional
admission denials. These tests compile only owned mock tools and use owned request
files. They do not run Cargo, the app, npm setup, reference matrices, or production
profiles. All owned test fixtures are removed after original exits. Eighteen
scoped/protected working files remain frozen throughout the focused check.

Current changed-file hashes supersede the corresponding historical entries only:

| Scoped file | SHA256 |
| --- | --- |
| `package.json` | `20276e66776944b294fc9ef0d06ddf00b4d97350e627d7946c3169d7042ea842` |
| `.github/workflows/rust-migration.yml` | `f62e89a64d8ee312ea4d4d5444f54f3b89635cf491cc8e253bb4c561d807658b` |
| `scripts/native/developer-launcher.cs` | `05f7ad7d9e86b034ba3a67d1fecff0d8cc20474a10077114ce64e0f00eedb058` |
| `scripts/native/developer-launcher.test.cjs` | `ac9a149c1ba13212634570881abd4a8828b98ec34cb23815539aa091166ed338` |
| `scripts/native/developer-transport.test.cjs` | `6b6912a3c9c1a06e1b8e72bcfc3e1d6e54ce7efba2d6d026674b90481b62e848` |

A single new graph update will record its own original identity and actual exit
after these genuine source changes. No previous graph resource is polled or
restarted. No parent progress audit, process kill or hijack is used; the launcher
reads only its own cmd ancestry to validate the fixed selector boundary.

### Observed transport graph completion

The single post-transport code update used a timeout-disabled observer and waited
on the original subprocess completion, without progress sampling or repeated log
reads. Exact command: `cmd.exe /d /c "graphify update ."`, scoped to this worktree.

```json
{
  "pid": 19076,
  "observerPid": 5504,
  "startedUtc": "2026-11-09T07:34:05.282701Z",
  "createdUtc": "2026-11-09T07:34:05.284248Z",
  "completedUtc": "2026-11-09T07:36:01.784712Z",
  "originalExitCode": 0,
  "timeoutDisabled": true,
  "frozenFileCount": 18,
  "frozenFilesUnchanged": true,
  "graphJsonExists": true,
  "graphReportExists": true
}
```

The new code graph reports 612 files, 14,820 nodes, 32,593 edges, 550 communities,
zero token cost, and HEAD stamp `173232b3` while syncing the corrected working
bytes before committing them. It still warns about 12 zero-node sources; HTML
was skipped above the 5,000-node limit. This is code synchronization, not semantic
extraction of documents or proof that its HEAD stamp equals the later commit.
The receipt addition here does not claim document-graph coverage. The original
earlier timeout remains failed/incomplete independently of subsequent updates.

| Scoped output | SHA256 |
| --- | --- |
| `graphify-out/graph.json` | `6ff15649b48818cc8f1db7800fe28e7b043b66dbcbe631bf62a46e0b321877c0` |
| `graphify-out/GRAPH_REPORT.md` | `e0ef0e1b6dd0f3b2dd5ea0b6a9a864d197515a59bed342c0c05b5ce101ac4c7e` |

Both owned observer files were removed after the actual original exit. No test
or graph job remains pending. The existing primary/trust/physical/reference gates
remain separate; this is not a full native readiness, installation or release
claim. Primary and other owners' sources, jobs and caches remain untouched.

## Bounded response-file correction after 7d47a8

The reviewer identified a single introduced P2: `File.ReadAllText` materialized
the whole response before the 8,191-character guard. The fix changes only that
call to `ReadRequestFile`, which owns a BOM-aware `StreamReader` and reads at most
LIMIT+1 characters into a fixed buffer. A string is created only after EOF with
length at most LIMIT. Overflow throws before payload materialization/JSON parse,
and the existing environment-JSON length guard remains unchanged.

No selector, exclusivity, filename, quoting, caller-mode, runtime-feature,
environment-flag, image-lifetime, release, or CI contract is changed. Only
`scripts/native/developer-launcher.cs`, the new focused response-read test file,
and this owned handoff document change. Primary `1f8c4f7` and its separately owned
reference/Rust integration paths are not read, modified, built or interrupted.

Focused command:

```text
cmd.exe /d /c "node --test scripts/native/developer-response-read.test.cjs"
```

| Response-read check | Actual exit | Passed/total | Observation |
| --- | ---: | ---: | --- |
| Predecessor plus identical new focused cases | 1 | 7/19 | Twelve failures record the requested full-read API before the decoded bound |
| Corrected bounded reader | 0 | 19/19 | All passed; zero skipped/cancelled; 12,138.534 ms |

The safe predecessor probe aliases `File`/`StreamReader` only in an owned compiled
fixture copy. It records the predecessor's `ReadAllText`/`ReadToEnd` request, but
caps its own fallback read at 8,192 characters so the test never allocates a huge
string or intentionally triggers OOM. This observes an unsafe full-read API
request; it is not a measurement or claim that a real predecessor OOM occurred.
The corrected consumer never calls that full-read API. The probe delegates its
bounded reads to a real decoder/file handle and records decoded character counts,
largest requests, read calls, byte position and growth without recording payload
values. Separate unaliased fixtures exercise real BOM/argv/error behavior.

For the owned 1 MiB fixed fixture, the corrected run asserts zero full-read calls,
exactly 8,192 characters consumed, largest request at most 8,192, and byte position
at most 16,384; it rejects without parsing JSON or launching even the mock Cargo
child. The fixtures also cover 8,191/8,192 boundaries, UTF8 multibyte and BOM,
UTF16 LE/BE BOM detection, JSON escape source-text counting, very short reads,
empty argv, invalid/empty JSON, missing filenames, exclusivity and fixed-selector
denial before opening a file. The forced-growth fixture starts at 8,191 bytes and
appends 4,096 bytes after the first real read through a test-only share-write
handle; the unchanged consumer bound still stops at 8,192 characters. Production
sharing semantics remain the normal reader defaults. No file-size precheck is
substituted for the bound.

The two prior test files remain byte-identical to `7d47a8` (confirmed by read-only
Git-object comparisons and the frozen hash guards); their 25/31/54 histories are
inherited, not newly executed or advertised as current full-matrix passes.
Nineteen scoped/protected files stay frozen throughout the new focused run. All
22 original compiler/request/hash-read process exits were observed before owned
temporary fixture teardown, with null signals and this ordered exit-code vector:

```json
[0,2,0,2,0,0,2,0,0,0,2,2,2,29,0,2,2,2,2,2,0,0]
```

The 29 exit is the unaliased BOM/argv forwarding case; 2 values are intentional
read/admission denials. All application/Cargo command children are owned mocks;
the compiler, CMD and Node test tooling are real. No real
Cargo/client/native device, production profile, npm setup, junction, signed
acceptance or release operation occurred. No arbitrary user oversized file was
opened. Every test request and temporary executable was removed after completion.

Current source/test proofs override only these corresponding historical entries:

| Scoped file | SHA256 |
| --- | --- |
| `scripts/native/developer-launcher.cs` | `237037b3a3a24785a87b748239f459a634a9c3ad5b39a2bd617e6c658450562e` |
| `scripts/native/developer-response-read.test.cjs` | `f741045bb40b076f0042bb78ddcee368bfe1717fffeb535494e059fbe581dae5` |

One graph synchronization will run only in this worktree after the genuine source
change, retaining its new original process identity and actual completion receipt.
No previous process/log is polled, retried or reclassified as passing.

### Observed bounded-reader graph completion

The one post-change code update ran as
`cmd.exe /d /c "graphify update ."` in this sole-owner worktree, with the observer's
foreground timeout disabled and an actual wait for original completion:

```json
{
  "pid": 7928,
  "observerPid": 19092,
  "startedUtc": "2026-11-09T08:02:43.533861Z",
  "createdUtc": "2026-11-09T08:02:43.535152Z",
  "completedUtc": "2026-11-09T08:03:39.066911Z",
  "originalExitCode": 0,
  "timeoutDisabled": true,
  "frozenFileCount": 19,
  "frozenFilesUnchanged": true,
  "graphJsonExists": true,
  "graphReportExists": true
}
```

The code report contains 613 files, 14,842 nodes, 32,637 edges and 542 communities,
with zero token cost. Its stamp is `7d47a8ec`, HEAD while syncing the corrected
working bytes before their follow-up commit; the guard binds the observation to
those checked bytes, not to a later HEAD stamp. Twelve sources still produce zero
nodes and HTML is skipped above 5,000 nodes. Document semantics and this receipt
addition are not extracted by the code-only command. Earlier timeout history is
not rewritten or turned into success by this new observed update.

| Scoped output | SHA256 |
| --- | --- |
| `graphify-out/graph.json` | `ace38a7988460898583fa3c876389ab6a74aa47172d7f68573bbb49b9915fad1` |
| `graphify-out/GRAPH_REPORT.md` | `b2f6cfdf09da1e38f38d993b9d44a2b7c2dd0e77c245cb950960d0b97fdbb03a` |

Both observer files were removed after the actual original exit; owned test
fixtures were already removed after their original process completions. No owned
test or graph job remains pending. The sole-owner source change remains distinct
from native runtime acceptance, primary integration, physical/reference matrices,
trust, installation and release gates.

## PRIMARY integration admission, 2026-11-09

The cumulative launcher candidate `0ac8bb28390caf008790ec3dc51be90171500300`
from base `c8a622f727a60a269752013d1e222b9864c50d49` was integrated into
`C:/Users/porad/.local/share/opencode/worktree/b1b3f5/rust-p0-backend-bench`,
branch `rust/native-client`, saved PRIMARY HEAD
`14b5392816bf051620dafdd2681f8dd04343c8ff`. This addendum supersedes only the
earlier statements that PRIMARY integration had not occurred. The full 662-line
candidate handoff and its failed/successful historical receipts remain intact.

The exact 11-path admission scope comes from
`migration-artifacts/native-launch/primary-integration-2026-11-09/admission.json`.
The cumulative candidate patch matched its Git-object diff and passed fresh
`git apply --check` before source changes. Its working-byte SHA256 is
`16e0635bc4ddba8c619f5121238d56e2eaf97e53ff457f7eb202ad6792f1a8c5`.
Source integration used contextual patch hunks, not Git apply or whole-file
replacement of existing files. All 11 outputs matched the candidate Git blobs
after CRLF normalization before this addendum; the ten implementation/test/
workflow files still match exactly, and this document preserves the exact
candidate prefix. Package dependencies, pins and neighboring scripts survive.

Fresh receipts are retained under
`migration-artifacts/native-launch/primary-integration-2026-11-09/checks-v1`.
Two failed pre-admission observer checks remain separate: an asymmetric binary/
UTF8 comparison and a per-file Git output buffer overflow. Both occurred before
source integration and were corrected only in the owned temporary observer.
Neither is a failed implementation test, and neither receipt was overwritten.
The successful pre-admission snapshot freezes 783 protected tracked files against
the current PRIMARY HEAD, not the older candidate base. It includes Cargo/config,
packaging, helpers, reference, UI and media. The concurrently owned shell paths
and parent-owned native-resume/native-completion coordination documents are
excluded. The original user checkout and node_modules junction were not modified.

After the snapshot, parent FileIo work also changed
`rust/crates/zabor-native/src/file_jobs.rs` to bind publication authority. Its
initial/current hashes are explicitly recorded as parent-owned changes, not
launcher changes or an unchanged source claim. The final unchanged gate covers
the other 782 protected tracked files. This dependency is neither edited nor
included in the launcher checkpoint.

Fresh PRIMARY commands ran through explicit `cmd.exe /d /s /c`, observed each
original exit and recorded null signals:

| Fresh check | Actual exit | Passed/total | Duration ms |
| --- | ---: | ---: | ---: |
| Scoped package/workflow predicates | 0 | 17/17 | 916.3987 |
| CMD selector and JSON/response transport | 0 | 25/25 | 20830.6513 |
| Bounded response-file reader | 0 | 19/19 | 15881.2251 |

All 61 selected checks passed, with zero failed, skipped or cancelled cases.
The previous full 54-case launcher and 31-case follow-up dynamic suites were not
rerun. The new static package/workflow admission re-evaluates only affected
entrypoints and CI event predicates. Transport/reader inheritance checks read
the original predecessor Git blobs, not another worktree's working source.

```text
cmd.exe /d /s /c "node --test --test-name-pattern "package entrypoints|native CI|legacy workflow gates|legacy stable gate|followup P2 actual PR|followup P2 tag/release" scripts/native/developer-launcher.test.cjs"
cmd.exe /d /s /c "node --test scripts/native/developer-transport.test.cjs"
cmd.exe /d /s /c "node --test scripts/native/developer-response-read.test.cjs"
cmd.exe /d /s /c "node --check scripts/native/developer-launcher.test.cjs"
cmd.exe /d /s /c "node --check scripts/native/developer-transport.test.cjs"
cmd.exe /d /s /c "node --check scripts/native/developer-response-read.test.cjs"
```

Separate syntax checks passed for all three Node test sources, both YAML workflows
parsed with the existing Python/PyYAML tools, package JSON parsed, and the exact
11-path whitespace check passed. The launcher fixture compiled both launcher
and real cache-verifier C# sources successfully; no Cargo/native app was invoked.
The 4 launcher compiler, 29 transport and 22 response original subprocess exits
were observed before owned fixture teardown. Intentional child denials and exit
29 forwarding are expected passing assertions, not rerun failures. Every
application/Cargo/prepare/reference child was mocked. No real native/check/Cargo/
build/prepare, npm installation, Electron, profiles, physical devices, release,
signing or packaging action occurred.

This is checked developer-launcher source integration only. Real native CI needs
the vetted SDK/cache and locked dependencies; full-client runtime acceptance,
parity, clean-machine startup, certified package trust/provenance, signing and
physical audio/camera/remote-control gates remain separate. Parent shell/FileIo
integration is independent and is not admitted or committed by this checkpoint.

### PRIMARY observed graph completion

The single background code update was observed through the original child close
event, with no polling, timeout, retry, process kill or progress-log inspection:

```json
{
  "command": "cmd.exe /d /s /c \"graphify update .\"",
  "observerPid": 11872,
  "pid": 20928,
  "startedUtc": "2026-11-09T08:56:49.964Z",
  "completedUtc": "2026-11-09T08:57:17.238Z",
  "exitCode": 0,
  "signal": null
}
```

The updated PRIMARY code graph reports 8,017 nodes, 20,709 edges, 330 communities,
369 corpus files and zero input/output token cost. Its stamp `14b53928` is HEAD
while syncing checked working code before the launcher checkpoint; it is not a
claim that the stamp equals the later commit. Concurrent parent shell/FileIo WIP
was not frozen or admitted by this launcher check. Ten zero-node sources remain
warned about, HTML was skipped above 5,000 nodes, and document semantics are not
extracted by this code-only command. This subsequent receipt addendum does not
claim semantic graph coverage. No owned graph/test/build job remains pending.

| PRIMARY output | SHA256 |
| --- | --- |
| `graphify-out/graph.json` | `5809780689c5e80866fea44fbbfd00e67dc249af40d259f7bd6ff7c67f68c2eb` |
| `graphify-out/GRAPH_REPORT.md` | `5ecc8ad6103c0bb59b7dd55ff01bfda9730d5a5a8276255822ade6587b237f9a` |

The original admission remains historical. Fresh checked PRIMARY admission,
source/pin-equivalence and original completion receipts are additive under
`checks-v1`; earlier failed receipts are retained. Graph outputs and receipts are
ignored worktree evidence, not extra checkpoint source paths. Final staging,
whitespace validation and commit are restricted to the exact 11 launcher paths.
