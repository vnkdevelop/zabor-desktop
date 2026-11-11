# Native import integration checkpoint — 2026-11-09

Base: `1f8c4f7`. Worktree: `native-import-completion`.
Branch: `native-import-completion-integration`. Primary was only read, never edited.
Scope and human prerequisites: [native-import-command.md](native-import-command.md).

## Реализовано / Implemented

- Explicit early CLI dispatch for import/recovery/rollback; malformed/mixed commands
  cannot enter shell startup. No normal beta is opened before snapshot publication.
- Existing account API performs independent fresh login against an explicitly approved
  server. Credentials come only from environment input, never argv/exported-session proof.
- Expected source public key is separately supplied and explicitly confirmed; parser
  validates the public point without constructing a pretend authenticated owner.
- Runtime/helper pins and whole-bundle approval are explicit human inputs, never
  newly computed production self-approvals. Native helper dependency does not enable
  helper `test-source`; empty native `profile-import-test-source` gates only mock tests.
- One original intent/ticket/cancellation/helper entry/registry owns extraction and
  retirement; explicit and unwind drains precede snapshot cleanup/publication.
- Authorized recovery checks journal owner; accepting recovery checks saved login/key
  without normal beta open, with admission before/after acceptance. Abort/rollback
  preserve complete quarantined data. Fresh credentials use existing session commit
  admission after accepted profile reopen. No UI/app state is installed by this CLI.

## Completed affected checks

All terminal launches used explicit `cmd.exe /c`, offline locked Cargo and isolated
owned targets. Latest source pipeline: `sh_1bf80d2070019bps29n54cPMyw`, completed.

| Check | Observed result |
| --- | --- |
| Native migration tests, no-default + profile-import-test-source | 9 passed |
| Native profile/import/session tests, no-default | 36 passed, including 4 new authorized-recovery regressions |
| Unchanged helper Windows process/registry tests | 13 passed; original owners/readers/root/jobs/handles retired |
| Unchanged mocked Electron exporter tests | 6 passed |
| Latest native all-targets Clippy, with and without native mock-test feature | Passed `-D clippy::all` |
| Unchanged helper all-targets Clippy | Passed `-D warnings` |
| Native-media executable build, no neural/default features | Passed offline/locked |
| Latest compiled early-dispatch smoke | 3 malformed/mixed cases rejected before shell startup |
| Targeted Rust formatting and git whitespace check | Passed |
| Lock and protected-source guard | All 779 package/version/source/checksum pins unchanged |

64 distinct affected tests passed; repeated runs and command-smoke cases are not
added to that count. `shell.rs`, UI, `update_host.rs`, helper package/standalone lock,
root workspace/default members and `.cargo/config.toml` remain unchanged. Native
manifest adds only the helper path, Tokio signal support and the empty test gate;
root lock changes only native/helper and Tokio/existing signal-hook dependency references.

## Honest limits and earlier failures

The broad no-default library attempt `sh_1bf7c926a0014ytPrajHFOAZ4Y` was **not passed**:
176 passed / 44 failed / 1 ignored. The 44 updater helper/host/I/O tests require their
separate external temporary/process harness; that prerequisite was not configured
for this import-only task. Updater code and its harness were not modified or bypassed.
Subsequent latest-source scoped tests and both native Clippy commands completed.

The first Cargo attempt rejected an incomplete locked dependency reference after
enabling Tokio signal support. Offline reconciliation added only the existing
signal-hook-registry reference, without changing any pin. Earlier test-only Rust
shadowing/move errors and a new large-enum Clippy diagnostic were corrected and
rechecked. None of those failed attempts is counted as successful evidence.

Five existing no-default `mic_test.rs` dead-code warnings and the existing Slint
`viewport-y` deprecation remain; native `-D warnings` is not claimed. Neural/full
client, actual Chromium origin/safeStorage/IndexedDB, real profiles, physical devices,
production services, power-loss durability, packaging and release acceptance are
outside these completed checks. No server admission, signing or trust bypass occurred.

All owned mock/source/scratch fixtures and both temporary Cargo targets were removed
after the original jobs/process owners completed. Existing Cargo caches and the
primary's pre-existing WebRTC/libclang artifacts were retained; build only read those
artifacts. No own verification job or temporary process resource remains pending.
