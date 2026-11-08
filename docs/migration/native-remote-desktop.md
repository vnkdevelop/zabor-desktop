# Native remote-control desktop primitives

Isolated continuation of `2108f866ce719a210bab679c7b81192094c20445`, based on
`3ba1ad80f3e5274e01d8f9514061ea3c720ca0d3`, in branch/worktree `native-rc-ui`.
This block is not wired into the main shell and is not physical acceptance.

## Saved scope

- `rust/crates/zabor-input/src/desktop.rs`: safe viewer lease, geometry helpers,
  synthetic-testable cursor/surface lifecycle and cleanup observation.
- `rust/crates/zabor-input/src/desktop/native.rs`: Win32 cursor restriction and
  dedicated host surface adapter. No SendInput, hooks, keyboard injection or
  capture acquisition is added here.
- `rust/crates/zabor-input/src/desktop/tests.rs`: synthetic lifecycle/geometry tests
  and compile-time thread-affinity assertions.
- `rust/crates/zabor-input/src/lib.rs`: only `pub mod desktop` is added.
- `rust/crates/zabor-input/Cargo.toml`: only two features of the already pinned
  `windows = 0.62.2` dependency are added: `Win32_Graphics_Gdi` for ClientToScreen
  and `Win32_System_Threading` for the host HWND's creating-thread check.
- `rust/crates/zabor-native/ui/remote-control-pill.slint`: isolated, passive
  transparent native window; existing reference dimensions, dark rounded surface,
  white dot, clipped controller name. It contains no controller or input ownership.

The application lib/shell/UI entry points, workspace manifests/lockfile, original
input owner, media, Voice, microphone, SFU, SDK, reference Electron/React and common
handoff/status documents are unchanged. Existing host cursor hiding/theme reload,
held-input release, foreground gates, explicit consent, single-controller admission,
panic shortcuts, host priority and input inactivity watchdog remain with the
original registered input owner. No `set_track(None)` is introduced.

## API and ownership

Use `zabor_input::desktop` from safe application code:

| API | Contract |
| --- | --- |
| `ViewerCursorLease::new(hwnd)` | One immutable original viewer HWND per request. Creates no native resource and grants no control permission. |
| `acknowledge()` | Call only for that original request after accepted ACK. Repeated ACK does not refresh frame/activity state or recapture a released cursor. |
| `decoded_p2p_frame(viewport, pixels, decoded_at)` | Actual decoded P2P frame only; original decoder timestamp must be after ACK, not in the future, not older than 750 ms and not older than the previously accepted frame. |
| `relayout(viewport)` | Update the physical client video viewport after layout/DPI changes without extending decoded-frame freshness. |
| `is_ready()` | ACK plus a fresh accepted frame and confinement requested. Not a substitute for original Voice/peer/channel admission. |
| `activity()` | Original-request admitted control packet/keepalive activity, not a periodic redraw or video frame. Fifteen seconds without that activity retires confinement, preserving the existing no-packet watchdog semantics. |
| `set_confined(false/true)` | Escape release/recapture of this same live request; does not change input ownership or warp the cursor. |
| `clear_frame()` / `revoke()` | Terminal retirement. Fallback, modal/focus loss and late callbacks cannot revive the old lease. |
| `run_viewer_cursor(lease, cancelled, observation)` | Blocking actor. Refuses native acquisition without ready ACK/frame. Original cancellation fences are caller-owned; register its original blocking join before calling it. |
| `CleanupObservation::state()` | Pending, Restored, Failed or Superseded. Failed remains sticky even after a successful cleanup retry. Superseded means another restriction replaced ours and was deliberately not overwritten. |
| `physical_viewport([x,y,width,height], scale)` | Logical client bounds to physical client bounds, rounding inward. Native code verifies containment in the actual client rectangle. |
| `fitted_video_rect(viewport, pixels)` | Integer contained-video rectangle, no letterboxing. |
| `host_pill_bounds(capture, physical_size)` | Top-center placement from the original actual capture rectangle, including negative origins; no primary-display fallback. |
| `HostSurface::attach_hidden(hwnd, capture, observation)` | Windows-only; dedicated hidden current-process HWND on its creating UI thread, original live CaptureLease. |
| `HostSurface::present(bounds, authorized)` | Recheck original capture and caller's original consent/Voice/peer/channel/controller predicate; topmost, tool-window, no-activate and passive mouse-through surface. |
| `HostSurface::finish()` | Hide and restore original extended styles/topmost state, then remove the owned HWND property. No input-owner cancellation is performed by this UI primitive. |

Both native owners are `!Send`/`!Sync`. Leases and observations are `Send + Sync`.
Monotonic property cookies plus HWND/process/creating-thread identity reject a
recycled window or replacement surface. HostSurface attachment is restricted to a
hidden window; never attach it to the main application, captured target or another
process's HWND. Its Slint component handle must outlive surface cleanup.

The cursor actor has one process-wide acquisition slot. It snapshots prior clipping,
does not broaden another existing restriction, rejects a changed external clipping
owner, and restores only when current clipping still equals its own last rectangle.
All restoration remains available after foreground loss. Cleanup failure poisons
future cursor acquisition. It never calls SetCursorPos, ShowCursor, SetSystemCursor
or SPI_SETCURSORS: viewer theme and pointer position are not modified. Restoring
ClipCursor alone may still have Windows-defined pointer effects; those are untested.

Mutexes protect only small lease state; no native call, wait or destruction is made
under those mutexes. Normal actor polling is 5 ms, but this is not a guaranteed
Win32/driver deadline. Win32 has no compare-and-set ownership API for ClipCursor:
observed external changes are protected, while an external equal rectangle or a
concurrent change between GetClipCursor and ClipCursor remains a physical gate.

## Exact main-agent wiring recipe

These are required follow-up changes, not edits included in this branch.

### Host pill

1. Add to `rust/crates/zabor-native/ui/root.slint`:

   ```slint
   export { RemoteControlPillWindow } from "remote-control-pill.slint";
   ```

2. In the shell's existing `UiEvent::RemoteControl(state, targets)` handler, use only
   the already accepted host controller and its original ControlTarget. Provide a
   read-only clone of that original host's CaptureLease with the event; do not create
   a new geometry lease from a picker, monitor guess or UI image. The host control
   state currently does not expose that geometry, so the main owner must add this
   narrow read-only event/accessor wiring. Do not create a second RemoteInput owner.

3. Create `RemoteControlPillWindow` on the active Slint event loop while hidden. Obtain
   its HWND with `WinitWindowAccessor::with_winit_window`, `HasWindowHandle`,
   `RawWindowHandle::Win32`, as the existing `native_window_handle` helper does.
   A missing HWND means defer safely until native creation, not show a normal window
   to acquire one. Attach HostSurface before any Slint `show()`. Keep the Slint handle
   and HostSurface together in one UI-owned record.

4. Set `controller-name` from the existing resolved display name, lowercased by Rust.
   No untrusted HTML, new locale or hardcoded visible fallback is needed. Compute
   physical pill size from the component's logical 160 by 32 and the actual window
   scale factor. Use the original capture snapshot for placement. This intentionally
   avoids the reference helper's primary-monitor assumption; exact placement parity
   for captured windows/multiple monitors remains an acceptance decision.

5. Set the Slint window's physical position/size, call `show()` only after native
   no-activate/tool-window styles are installed so Slint renders the window, then call
   `HostSurface::present(bounds, original_authorized_predicate)`. Do not rely on raw
   SetWindowPos visibility alone to activate Slint rendering. The predicate must
   include original Voice token, original peer lifetime, original channel lease,
   explicit sharing consent, original capture and the current single-controller
   identity. A bounded UI timer reapplies this predicate, observes live capture and
   handles movement/DPI; it must not prolong the original 15-second input watchdog.

6. On controller stop/revoke/owner end, CaptureLease expiry, peer/channel retirement,
   Leave/Logout or application exit: stop that original timer, finish that original
   HostSurface, inspect its observation, then hide/drop its Slint handle. A Failed
   observation is a cleanup failure, never success from dropping a public handle.
   The existing revoke button and original Ctrl+Alt+Backspace/Escape hooks remain
   the only input revocation authorities; the passive pill takes no keyboard focus.

### Viewer cursor

1. Retain a new UI request record for the original ControlTarget, original Voice
   cancellation token, original viewer HWND, new ViewerCursorLease and fresh
   CleanupObservation. Compare lifetime/channel identity, not peer strings alone.
   Replacement, modal entry, pointer/keyboard/window focus loss, stream removal,
   fallback, Leave/Logout and app exit revoke that record immediately.

2. In `shell.rs`'s existing accepted RemoteControl event handling, call `acknowledge`
   only for this same original request. `viewer_ready` already means ACK plus P2P
   readiness; if used as the ACK handoff, wait for another actual decoded P2P frame
   after this call. Do not use a cached `VideoItem.frame` as a fresh frame.

3. Add immutable `decoded_at: Instant` and original selected-source/P2P provenance
   to the existing `FrameUpdate` handoff. Stamp it at the original decoder delivery
   callback, before mailbox/UI delay (the callback constructing FrameUpdate near the
   existing VideoReceiver creation). In `on_process_video`, after the existing
   FrameUpdate intent/generation/VideoFrameLease gates, publish only frames proven
   to be from the same original accepted P2P delivery source. SFU/snapshots/replayed
   mailbox frames must not refresh this lease. Current VideoItem and FrameUpdate do
   not themselves carry sufficient route/timestamp proof; that missing wiring must
   not be inferred from peer name or a current `viewer_ready` boolean.

4. Export the focused RemoteViewer's complete client-relative logical viewport to
   the shell, including its origin, not only pointer callback width/height. Use
   `physical_viewport(bounds, window.scale_factor())` and the decoded image aspect.
   On Resized/ScaleFactorChanged/layout/fullscreen changes call `relayout`, not
   `decoded_p2p_frame`. ClientToScreen supplies actual screen coordinates; do not
   add window decorations, picker dimensions or a guessed monitor origin.

5. Once `is_ready()` is true, register the cursor blocking actor with the same
   existing cleanup tracker pattern as RemoteInput, before acquisition. Preserve
   the original observer on cancellation; do not abort its join. The actor body is:

   ```rust
   let result = zabor_input::desktop::run_viewer_cursor(
       cursor_lease,
       || stop.is_cancelled() || !original_peer.is_alive() || !original_channel.is_alive(),
       observation.clone(),
   );
   ```

   `stop` must be a child of the original Voice token and explicit UI request stop.
   On an unexpected actor end/unavailable result, release only the original control
   target; on Cleanup error or Failed observation report existing incomplete-cleanup
   state. Neither cancellation nor a timeout proves that clipping was restored.

6. Intercept Escape locally in `remote-viewer.slint`/shell before VK forwarding;
   toggle `set_confined` and use existing `stream.cursorCaptured` /
   `stream.cursorReleased` strings. Escape key-up must also remain local. Ctrl+Alt+K,
   pointer cancel, focus loss and explicit release revoke the lease and retain the
   existing original-target STOP/held-input cleanup. Forwarded viewer interactions
   call `activity`, as do successfully admitted original-request PING keepalives.
   A periodic layout/frame callback does not renew packet inactivity.

No worker acquisition/native wait/destruction belongs inside CriticalIntents,
peer/channel gates or the media/Voice owner locks. This is UI/resource wiring only:
do not change original microphone, SFU publisher/subscriber or SDK track semantics.

## Verification and remaining gates

Affected checks use locked/offline Cargo and private target directories. Synthetic
tests cover contained/negative/DPI geometry, ACK/frame freshness, late callbacks,
15-second inactivity, original restriction preservation, external supersession,
foreground-independent restoration, terminal host authorization, cleanup failure,
retry, Drop/unwind and thread-affinity assertions. The original nine input tests
remain unchanged. The isolated Slint compile never constructs or displays a window.
The first checker put generated Rust under a crate-wide `forbid(unsafe_code)` and
failed E0453: Slint 1.18.1's own `ItemTreeVTable_static` macro emits an incompatible
`allow(unsafe_code)`. The saved application already separates library
`forbid(unsafe_code)` from binary manifest `unsafe_code = deny`; the checker mirrors
that exact separation. No application/SDK code or lint policy is changed, and no
new suppression is added. Strict forbid on generated UI itself remains an upstream
compatibility gate, not a passed check.

Final input block checks passed: `cargo fmt -p zabor-input -- --check`,
`cargo test -p zabor-input --locked --offline` (27 tests: 18 new desktop tests and
the unchanged nine original input tests), and
`cargo clippy -p zabor-input --all-targets --locked --offline -- -D warnings`.
This compiles the Windows adapter without executing it. The first new DPI test
incorrectly expected truncation at 125 percent scale; it was corrected to exact
inward edge rounding (left 13, width 999), with no production change or assertion
weakening. Initial new-code Clippy findings and a deprecated atomic method were
corrected without suppressions; final checks are warning-free.

The corrected isolated Slint check passed `cargo check --all-targets --locked
--offline` with Slint/slint-build 1.18.1 and the saved application's library/binary
lint split. Both generated component code and the checker targets compile; no
window or desktop operation is executed. The strict-forbid E0453 result above is
retained explicitly rather than reported as success.

After all input/Slint compile and test processes actually completed, the three
agent-created temporary directories `native-rc-ui-target`, `native-rc-pill-target`
and `native-rc-pill-check` beneath the approved OpenCode temporary directory were
removed, with their absence verified. No shared Cargo target, main worktree or
other agent's files were removed.

The first own-worktree code Graphify completed with 13,740 nodes, 29,731 edges and
523 communities. Eight zero-node source warnings and the 5,000-node HTML limit
remain; HTML generation was skipped, not accepted. The final incremental sync was
cancelled by a server restart. Passed tests/fmt/Clippy/Slint results are retained;
only that unfinished sync is resumed. No main-worktree graph is updated.

Final source saving is guarded: compare the six checked source/config hashes,
require successful own-worktree `graphify update .`, recheck the same hashes and
exact seven-path staged scope, run diff checks, then commit on `native-rc-ui`.
The guard reports final graph counts and commit SHA and verifies the three check
directories plus its own temporary runner are absent. It does not repeat the
completed regression checks or declare a cancelled job successful. Code Graphify
is structural only; semantic documentation/image extraction is not claimed.

Resumed final Graphify passed: 13,748 nodes, 29,738 edges and 532 communities;
`graph.json` and `GRAPH_REPORT.md` were updated only in this worktree. The eight
zero-node warnings and skipped HTML remain. Both pre/post-sync checks confirmed
the same six verified source/config hashes and exact seven-path staged scope.
The temporary finalizer was removed and absence checked even though its Git save
step returned an error; HEAD/staging were subsequently checked before retrying
only that unfinished save. No completed compile/test/graph job was duplicated.

No real SendInput/hooks/GetClipCursor/ClipCursor/system-cursor operation, physical
capture or GUI interaction is used in these tests. No production/network endpoint,
managed helper, WebView, push, merge, release or shutdown is involved.

Physical/acceptance gates remain: full-screen/topmost composition and no-activate/
mouse-through behavior; lazy Slint creation/render lifecycle; DPI/multi-monitor/
negative-origin placement and cursor races; physical Escape/panic/modifier cleanup;
host cursor theme/custom/animated/large cursor restoration; keyboard layout/IME;
UIPI, secure-desktop, integrity-level and OS privilege restrictions; real driver
call responsiveness and long-session teardown. Synthetic tests are not evidence
that these gates passed, and the shell wiring itself remains unaccepted until the
main agent integrates and checks it.
