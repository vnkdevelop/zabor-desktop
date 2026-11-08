# Native remote-control migration

Source predecessor `2d254a2243dd478bc0f33c2fb0da9648dedeb16c`, metadata predecessor
`b4b52bc0940f937c3e3e03a602413860ec8cb1dd`. Functional components passed 179 affected
tests, fmt, both Clippy modes, normal build and RU/EN inert interactions. Saved
source `2108f866ce719a210bab679c7b81192094c20445`; not a complete-client,
physical-input or release attestation.

## Boundary and wire contract

`zabor-input` is a standalone native Rust/Win32 boundary. The application retains
its forbid-unsafe policy. No managed input-helper process, Electron or WebView is
introduced. The only lockfile additions are the local crate and its application
dependency; existing WebRTC/DSP/camera/stream-audio pins are unchanged.

The parser preserves `zabor-rc-v1`, negotiated ordered SCTP channel 4243, LE float32
move coordinates, LE int16 wheel deltas, LE uint16 virtual keys, modifier bits and
one-byte signals. Three-byte legacy key packets remain accepted. Invalid lengths,
unknown operations, mouse buttons, modifier bits and nonfinite/out-of-range moves
are rejected. The safe VK whitelist and prohibited system shortcuts are preserved;
the native owner additionally checks actually held and physical modifiers rather
than trusting packet flags. Owned key-up/button-up cleanup bypasses foreground and
host-priority admission without injecting an unowned release.

## Consent and original ownership

Sharing starts with remote control disabled unless the host explicitly chooses
the existing RU/EN `stream.allowRemoteControl` setting. Cameras remain ineligible.
The corresponding offer/answer metadata is accepted only after SDP acceptance,
not from ignored/glare/stale answers. Host START is rejected without a current
authorized peer, original peer/channel identity, explicit share consent and a
live capture. Only one controller is admitted. ACK is ACCEPT for protocol v2 and
START for v1, after the registered input owner reports readiness.

Each owner is registered in the existing cleanup tracker before native acquisition.
The original Voice cancellation token, original peer lifetime, original channel
lease and original capture geometry lease remain independent. Public Drop only
requests cancellation; the original blocking join/result remains observed. Timeout
or await cancellation is not cleanup proof. Cleanup errors poison acceptance and
the native input admission. Native calls and destruction stay outside intent locks.

## Input and UI behavior

Capture geometry is copied from actual SDK frame left/top/width/height, before
video scaling, never from picker thumbnails or a primary-monitor fallback.
Geometry expires when frames stop or capture retires; late frames cannot revive
a revoked lease. Mapping handles negative desktop origins, width/height minus one
and the Windows virtual desktop. Captured-window foreground admission also checks
the exact HWND and original process. Down/wheel require the current cursor inside
the captured rectangle; key/button release remains available after focus loss.

The Win32 owner installs its own low-level hooks and pumps its own message queue.
Injected input and the ZABO signature are excluded from host activity. Real host
input releases remote-held inputs, restores the host cursor and takes priority for
two seconds. Ctrl+Alt+Backspace/Escape revokes input. A 15-second no-packet watchdog
retires the owner. Cursor hiding/restoration follows the unchanged helper's system
cursor policy; physical theme/device/privilege behavior is not yet accepted.

The viewer waits for ACK and an actually decoded fresh P2P frame before enabling
input. It retains existing SFU/P2P snapshot/fallback ownership; an active controller
uses P2P without disabling voice or the independent SFU publisher. Sender overrides
preserve full resolution/60 fps without rewriting the saved initial bitrate
contracts. Mouse normalization excludes letterboxing. Keyboard/pointer focus loss,
modal entry and explicit release stop control. Existing RU/EN strings are reused.

## Verification state

Initial standalone input tests passed 9 cases: exact wire bytes/legacy packets,
malformed values, whitelist/shortcut filtering, actual held-modifier spoofing,
negative geometry, capture retirement, host-priority/foreground release, cancelled
admission and cleanup poisoning. A must-use TranslateMessage warning was corrected.
Application check `sh_1bb2d3bf8001hrEMYcH4zlhrIU` found new-test equality against a
non-PartialEq cleanup error and six String/SharedString comparisons. Corrected
the test with exact variant matching and the UI comparisons with explicit str
borrows; no existing types or lint settings were changed. New local SCTP, consent,
versioned ACK/exclusivity and first-frame viewer tests passed in the final group.
`sh_1bb320b34001C69r6l5kjkqB1e` found identical stop branches; merged into an
equivalent short-circuit condition without lint suppression. Initial grouped job
`sh_1bb340963001Mq7xk9JaR2OVL1` checks fmt, fixture/normal all-targets Clippy, native
input, media, Voice, registered owner, viewer, screen, playback, video, local RTP,
shell guards, normal build and RU/EN inert interactions. No physical input,
production access, push, publication, merge or shutdown has occurred.

Local SCTP/consent/ACK/exclusivity/stale-owner/queue and affected media/Voice/shell
tests, Clippy, normal build and RU/EN inert checks passed in the final group. Exact
React/native visual/keyboard-layout/cursor/performance and physical/long-session
input behavior remain final gates, alongside all prior whole-client ownership,
data-integrity, audio, packaging and release acceptance gates.

The viewer command queue drops only replaceable mouse moves. If a state-changing
input cannot be queued, an original-target STOP is sent and that original channel
lease is revoked: delayed inputs cannot be applied to a replacement channel. The
host's explicit revoke also invalidates its original channel before the command
worker handles metadata. A retired RC channel is recreated within the original
peer, with new identity and fresh metadata negotiation; the microphone and file
channel are not retired. Native channel calls are outside intent locks. Bounds of
all SDK socket writes remain part of final responsiveness acceptance.

Known open component comparisons: the pinned safe SDK has no per-receiver selective
zero-playout/keyframe-generation API used by the browser; current signaling and
fresh-frame delivery are not proof of identical latency/keyframe behavior. Exact
physical keyboard-layout/IME, modal/window activation, topmost host-control pill,
cursor theme restoration and viewer cursor confinement still require final work
or acceptance. Targeted fixtures do not certify those physical/UI behaviors.

The first grouped runs stopped before executing the new tests: the combined stop
condition needed its surrounding condition collapsed for Clippy, then the new
queue guard test needed its explicit helper/Duration imports. Both were corrected
without changing original behavior or weakening assertions. Final full affected
job `sh_1bb3a02730010hkv6cpfVeKGB3` passed 179 unique tests: 9 input, 30 media,
19 Voice, 3 registered input, 2 viewer, 4 screen, 4 playback, 7 video, 3 local RTP,
98 shell. Both all-targets Clippy modes, fmt, normal build and inert RU/EN checks
passed. Evidence: `migration-artifacts/native-streams/remote-control-functional-2026-11-08/checked-v2/verification.json`.
No current jobs or temporary verification runners remain. Inert picker checks cover explicit
control consent and rejection for cameras; fixture coordinates reflect the extra
consent row. Wheel forwarding also rejects viewer letterboxing.

Final code Graphify and scoped source save passed: 23 native/workspace paths and
24 source/pin hashes, no changed code during the final grouped checks, verified
normal executable `653c41fe2b755f335434d0752e92becbc86e272437dc5a9bb95ea184aa10d670`.
Checked synthetic input/camera fixture markers are absent. Unrelated saved SFU
source/pin hashes remained unchanged. Code graph: 6870 nodes, 17027 edges,
289 communities; the existing seven zero-node warnings and 5000-node HTML limit
remain. The stage's `save-verification.json` records the source commit and scope.
Only continuation documents remained modified after saving source code.

## Active original decoded-frame provenance subset

After checked updater continuation c1e860c, the primary owns a new, not-yet-saved
RC frame subset. DecodedVideo now records a private local monotonic completion
timestamp after native RGBA copy/rotation; VideoFrameStamp retains that unchanged
timestamp, displayed pixel geometry and original VideoFrameLease. It is a local
copied/decoded-at time, not the remote capture clock or a physical latency proof.

FrameUpdate retains the selected source epoch. A separate bounded displayed-frame
map is populated only after the corresponding Slint video row is admitted. RC
readiness/input requires original Voice ticket/generation, live subscription,
selected P2P source rather than SFU, exact original P2P PeerLifetime/channel and a
decoded frame no earlier than the original ACK acceptance time. Repeated ACK does
not refresh that boundary. The cached VideoItem/image alone is never proof.

Pointer/wheel/key input attaches the retained stamp and original frame ticket to
its ControlTarget. Queue admission preserves that ticket rather than reminting
after Leave, and worker dispatch rechecks peer/subscription/ACK provenance. A
failed non-release stamp revokes the viewer; original safe key/mouse release and
STOP cleanup remain independent. UI release/VoiceEnd/Logout resets authorization;
video removal discards its displayed stamp. No microphone/SFU publisher or capture
ownership is changed.

Preliminary compile exposed only authored test arm types and sharing the original
video-generation Arc after the worker takes it. Corrected with unit-returning
capture arms and an Arc clone of the same authority; no reminted generation or
lint suppression. Pending affected group `sh_1bbfa5e9e001V9DBPOzvO5F1vj` covers
fmt, both native-media all-targets Clippy modes, video/Voice/media-RC/input-owner/
viewer/shell tests, actual native frame/SCTP and extended loopback RTP proof, normal
build, RU/EN inert interactions and Graphify. Eight source/pin hashes are frozen.
Evidence: `migration-artifacts/native-streams/rc-frame-provenance-2026-11-08/checked-v1/`.
Do not edit sources or poll until automatic completion; no verified milestone is
claimed before the passing result.

This subset does not impose a new guessed continuous frame-age timeout. Native
physical confinement/viewport/helper binding must consume the original timestamp,
not refresh it from a cached image. Continuous frame stalls, physical cursor/
focus/IME/UIPI/latency, full-resolution visual parity, two-device/long-session,
universal callback quiescence and all whole-native acceptance gates remain open.

### Checked provenance result

The described copied-frame/input subset passed as
`66e3b169448771359b0aa72f3c382843680bca37`:4nativepaths/8source+pinhashes,
155 distinct tests (9video/20Voice/4mediaRC/3input-owner/2viewer/117shell), fmt,
both native-media all-targets Clippy modes, normal native build, RU/EN inert
interactions and Graphify. Original timestamps remain unchanged through cached
stamps; another peer or retired subscription and a pre-ACK frame are rejected.
The actual loopback RTP fixture also rejects original displayed frames after
Leave/Logout/generation change before native destruction completes.

Normal executable SHA-256:
`3675cf738823f68a9154efc04668dfaa55ca9115170a52fe36d4d50be8364cbb`.
Graph7406nodes/18793edges/296communities;7priorwarnings/HTML5000limit remain.
Immutable checked-v1/save evidence is under the provenance evidence path above.
The temporary runner is removed and no primary provenance verification job remains.
No physical cursor/viewport, continuous age deadline or whole-client/release
acceptance is implied by these component tests.
