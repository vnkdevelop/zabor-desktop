# Native profile and avatar functional migration

Base: native RC source `2108f86`, continuation `3ba1ad8`. Checked source saved as
`f264f3cde6e5784f82a5fa16fa131af87475911d`.
It is not complete-client, physical-resource, exact-raster or release acceptance.

## Implemented functional surfaces

- Native profile viewing and own-profile editing; display name uses the reference
  20 UTF-16-code-unit limit, about-me uses 150. Existing RU/EN catalog keys remain
  unchanged. Profile avatars, source selection and editing are separate from
  registration avatar state.
- Profile update preserves `UpdateProfile(displayName, avatarBase64, avatarColor,
  aboutMe)`, trims name/about-me, retains the original owner and does not replace
  non-profile worker state. No Electron or WebView is introduced.
- The source picker accepts PNG/JPEG/WebP/GIF, reads at most 5 MiB plus one byte for
  size rejection, and validates format, dimensions and allocation limits before
  publishing a source. Static EXIF orientation is applied during decoding.
- Crop preview is 200px, min-fit rather than the old center-cut, with drag, keyboard
  movement, a 0.5–3 slider in 0.05 steps and 0.5–5 wheel range. Static output is a
  400×400 black-backed PNG. Offsets are measured in the reference 200px preview.
- GIF remains its original data URL inside packed `{g,s,x,y}`; scale precision is
  0.001 and offset precision 0.1, including JavaScript rounding of negative ties.
  Native animation uses decoded composited frames, delays, repeat metadata and
  circular presentation; reduced motion selects the first frame. The decoder's
  repeat API loses the distinction between absent/infinite repeat, so bounded
  structural GIF extension parsing preserves it explicitly.
- Dominant-color logic follows the reference sampled pixels, alpha/black/white
  filtering, saturated-pixel preference, 32-step clusters with first-cluster tie
  behavior, and HSL saturation/lightness adjustment.

## Ownership and bounds

File/crop queries retain their original Session ticket, cancellation token,
setup-login or own-profile context, and unique query identity. Replaced queries,
context changes and Logout cannot publish into a later setup/profile context.
Profile requests retain a context lease; cancellation before dispatch prevents
the update. Queue admission keeps that original query ticket, not a newly minted
queue ticket. Already dispatched remote writes cannot be undone by dropping their
reply, and cancellation is not proof that the server did not execute the write.

Session work continues across Leave; Logout invalidates it. Crop work runs off
the UI thread. A semaphore admits one decode/crop worker at a time; at most eight
owned async jobs remain admitted. Jobs keep their original joins, inner worker
panics are tracked, and application close cancels queued/crop/cache work before
observing the retained jobs.

Decoded source retention is capped at 64 MiB, each GIF canvas at 4 Mi pixels,
source frame count at 256, and square avatar frame retention at 32 MiB. Active
avatar wire/cache sets have independent 64 MiB bounds and at most 32 unique
sources. Unsupported/over-budget sources are rejected rather than silently
flattened into static PNGs. Exact large-roster visible-cache scheduling and
high-resolution GIF performance remain final responsiveness/compatibility work.

The native OS file dialog is awaited to actual completion rather than abandoned
on context cancellation. Its OS/driver responsiveness and close/quiescence bounds
still need final acceptance; retaining a task is not evidence that it has exited.

## Checked functional milestone and verification history

Final V5 `sh_1bbb205b4001uzvm02G4j3fM2b` passed fmt, both native-media all-targets
Clippy modes, normal build, strict RU/EN native pointer/keyboard interactions and
final Graphify. Across V3 unchanged algorithm tests and V5 shell tests there are
134 unique passing tests, without counting repeated shell runs twice.

Saved scope:14 native files/18 source+pin hashes. Normal executable SHA-256:
`9948b4002ee685166f1b442f394000d5fdc16d008a064208794652332b4efa5c`.
Graphify:7002 nodes/17423 edges/298 communities; prior seven zero-node warnings
and 5000-node HTML limit remain. Immutable evidence: `checked-v5/verification.json`,
`source-snapshot.json` and `save-verification.json` under
`migration-artifacts/native-ui/profile-avatar-functional-2026-11-08/`.
No temporary diagnostic code/image or active primary verification job remains.

Normal all-targets Clippy `sh_1bb5d6b140016i4RuyUReCMzj5` passed its intermediate
snapshot after correcting the unsupported Slint role without suppressions.
Grouped job `sh_1bb9d034c001uvZN5gM0u66LWI` completed partially: fmt, fixture/normal
all-targets Clippy, normal native build and 134 affected tests passed (10 avatar,
5 protocol, 3 auth, 2 hub, 4 actual local WebSocket, 6 roster and 104 shell).
RU inert interaction failed at the profile display-name input length assertion;
EN interaction and final Graphify did not run. This is not a checked milestone.

Evidence target:
`migration-artifacts/native-ui/profile-avatar-functional-2026-11-08/checked-v3/`.
No actual user profile, physical capture/input or production service is exercised.
No push, publication, main merge or shutdown has occurred.

Remaining stage/final comparisons include browser raster/smoothing,
large-roster cache fairness, fresh peer-profile lookup,
login-copy behavior, complete profile actions/loading/accessibility/motion parity,
OS dialog/resource deadlines and all pre-existing whole-client acceptance gates.

The first grouped job was cancelled by a server restart. Its saved evidence has
fmt/fixture Clippy passed, but avatar test compilation was interrupted; it proves
no passed tests or completed build. V1 and failed V2 evidence are retained
unchanged; V3 includes protocol/hub and actual local WebSocket fixtures and the
existing roster tests.

The tested post-cancellation fixes include: 8 MiB bounded records accommodate
the reference 5 MiB GIF after Base64 plus JSON; WebSocket bounds include the
separator byte. Strict over-limit rejection remains. A valid near-5-MiB two-frame
GIF is used by the actual localhost profile-update/event fixture. Production
.NET/proxy message limits remain unproven. GIF color uses the selected displayed
preview frame, immediate presentation uses the saved quantized geometry, and
dragging stops when the pointer leaves the preview.

V2 stopped at fixture Clippy because the new original-ticket queue helper returned
a large error value. It now boxes that error exactly as existing CommandQueue
does, without suppression or changed admission. No tests ran in v2. V3 passed
those tests but stopped before final code Graphify; earlier evidence is not overwritten.
The pre-Box code graph passed: 7001 nodes, 17422 edges, 301 communities, with the
existing seven zero-node warnings and 5000-node HTML limit.

Field diagnostic `sh_1bbaddd6d001H5KsuEZ9rTLb9K` completed: name/about remained
empty because the pointer missed the vertically stretched field; its temporary
snapshot also exposed the header's accidental vertical centering. Corrected only
the new modal's header origin and top-aligned natural-height labels/inner layouts.
Temporary diagnostic code/image are removed; logs retain the failed development
run. Original ASCII pointer/length assertions remain and supplementary Unicode
key input now checks 20/150 UTF-16 units without broken scalar boundaries.

Completed affected final job is `sh_1bbb205b4001uzvm02G4j3fM2b`, with immutable
`checked-v5/` evidence: fmt, both native-media Clippy modes, shell tests, normal
build, RU/EN inert interactions and final Graphify. No source edits occurred
during the group. V3 remains authoritative for unchanged algorithm tests;
V5 counts their union without inflating repeated shell tests. Full neural/visual
regression remains deferred until functional completion.

V4's first inline runner had a syntax error before starting checks; its corrected
runner stopped at fmt on the newly added Unicode fixture, before compile/tests.
Fmt was applied without weakening assertions; V4 evidence remains unchanged.
