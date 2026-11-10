# Native RC application integration — 2026-11-09

## Scope and provenance

This stage owns only `native-rc-completion`, based on
`046325d73acb85ba28e02d7f7bd284b609a7ef44`. Initial discovery found the new
worktree clean and detached at `c5d07348880555a3a22b781d6b810a01e4baff5d`;
a non-forced detached switch selected the explicitly authorized base without
changing another worktree or discarding WIP.

The checked prerequisites were imported with `git cherry-pick -x`:

| Original | Imported |
| --- | --- |
| `4e7dd6451849000c4c21780f081af9f7280fd65c` | `9abb3fe95149624261d61dbde296d2e22b8bd682` |
| `6d7b4cf11c3d6c00024282d98ccd876f6b1dd278` | `a1d8fea2bccf03f9c354d375da2e9ef238ebed48` |
| `3e9cb2fe02d849be0c71f9b123dd20f32bb56ac9` | `67ceef7019333ae8a8cffff48851ceeee1c4d68a` |
| `080821fb027cb7ddbb7788030602b7bc3c1536e9` | `c00338b17c0d5f1ff9015fba946cc674b6f06970` |

The import guard matched all 15 candidate Git blobs exactly and found no root
manifest, lock, config or pin changes. The only prerequisite manifest delta is
the two already checked Windows features on pinned `windows =0.62.2`.
The original RC worktree, graph and receipts remain unchanged. Its completed
structural graph was queried read-only before source exploration. No moving
primary source, pending primary job, other owner's target or primary graph was
inspected or changed.

## Integration contract

- `shell.rs` creates canonical `Arc<ViewerRequest>` before original ToggleControl
  enqueue. The original target carries that Arc, original Voice intent ticket,
  Voice token and shared once-only STOP flag. UI/worker results retain original
  target snapshots; old same-channel request input/release/activity cannot be
  retagged for a replacement.
- The worker's original host exposes its live capture lease, consent, exact
  lifetime/channel, original token/ticket and one shared atomic fence. Host
  retirement and Drop revoke that fence before input-owner retirement. Queued
  host revoke additionally matches the original fence, not just the peer name.
- Actual decoder subscriptions construct P2P provenance once from the first
  immutable original delivery stamp and source epoch. `DecodedVideo` carries
  its shared source through VideoDelivery/FrameUpdate; DisplayedFrame clones
  that original source. SFU delivery has no admitted P2P source. Restart retires
  the old lease and cannot retag its cached frames.
- Only actually admitted UI frame rows call `ViewerDesktop::decoded_frame`.
  Layout and cached input validation never reconstruct a source or renew decoder
  time. UI readiness additionally requires the original desktop adapter.
- Existing RemoteViewer forwards absolute client-relative viewport bounds and
  confinement state. Winit focus/resize/scale, focus selection, existing modal
  release, Leave/Logout, removal and window teardown retire/resample original
  records without native acquisition or waits under acceptance gates.
- ViewerDesktop/HostPill stay UI-thread-affine and use the existing runtime's
  entered handle and SharedAudio registry. Successful original packet/PING
  admission alone reports activity. Original STOP is immediate and once-only,
  before cursor restoration and qualified queued worker release.
- The dedicated pill starts hidden. A missing or unproved hidden HWND defers
  attachment; no normal native window is shown to obtain a handle. Original
  component/surface ownership and sticky failure are retained through finish
  and existing registered completion, before app drain/seal/runtime end.

The five delegated source/UI paths are `src/shell.rs`, `ui/app.slint`,
`ui/root.slint`, `src/voice/remote_control.rs` and `src/video.rs` under
`rust/crates/zabor-native/`. New RC-only helpers are
`src/shell/rc_desktop.rs` and `src/shell/rc_ui_interactions.rs`.
Other app/common-data/Voice/media areas and all prerequisite blobs are excluded.

## Verification state

Import receipt: `migration-artifacts/native-rc-completion/2026-11-09/import.json`.
Original check job `sh_1bf350415001tJpwcpf7LpeTsH` completed: diff/fmt passed,
native all-target compile failed on the new helper's missing `slint::Model`
trait import, and no tests or Clippy ran. Source/candidate/pin/SDK guards passed;
its original target/fixture and runner were removed after child process exit.
Its unchanged receipts remain in `checked-v1/` under
`migration-artifacts/native-rc-completion/2026-11-09/`.

The import is corrected. The bounded follow-up also covers idle context rebinding:
only an RC-idle Voice session may bind its current original ticket, a live owner
cannot be reticketed, and stale context is rejected before host acquisition.
An additional synthetic regression exercises this boundary. Original corrected
job `sh_1bf4bcd8d001z8odGKh3bUASQO` completed: native all-target compile, 12
selected native tests, strict native all-target Clippy and fixture dependency
resolution passed. Source/candidate/pin/SDK guards passed. The actual-app fixture
stopped at a test-only callback return-type error before running its test;
fixture Clippy remained unrun. Its marker-owned target/fixture was retained only
after all child commands exited. Receipts remain unchanged in `checked-v2/`.

Only `rc_ui_interactions.rs` changes for the fixture correction; all native
sources still match those passing V2 receipts. Fixture-only test/fmt/Clippy will
reuse that original owned target and completed pin closure, without another
native build or regression matrix. No final graph or hook-source commit exists. Only
Fixture-only checks are frozen in original job `sh_1bf6a0237001GTXWYYwC1ElLb3`;
receipts will be retained in `checked-v3/` under the stage evidence directory.
Only
synthetic/loopback/headless/inert Slint tests are authorized, with a marker-owned
target and fixture, locked/offline two-job Cargo and immutable cached SDK inputs.

The original 54 baseline / 41 affected / 60 combined unique RC proofs are retained
historical evidence, not fresh integration tests or 95 distinct tests. This stage
does not repeat unchanged primitive matrices or claim their historical receipts
as newly executed checks of changed hook sources.

Physical HWND/render/no-activate, ClipCursor/SendInput/capture, IME/UIPI,
two-device, long-session, trust/legal and whole-native acceptance remain false.
There is no push, merge, tag, install, legacy deletion, profile/production network
operation or shutdown.
