# Native adaptive video and SFU delivery

Status: 153 unique affected tests, fmt, both Clippy modes, normal build and RU/EN
inert checks passed. Saved source `2d254a2243dd478bc0f33c2fb0da9648dedeb16c`. Source baseline
`0dcfef7`, metadata baseline `5e4430b`, branch `rust/native-client`. No complete-client
acceptance, production access, publication or shutdown.

## Preserved contracts

The initial desktop sender limits remain low 2.5 Mbit/30 fps, medium 3.5 Mbit/60 fps
and high 6 Mbit/60 fps. The independent adaptive/reference SFU ladder starts medium
at 3 Mbit. Camera sender defaults are not replaced with desktop profiles; saved
defaults now also restore resolution scaling after an adaptive desktop share.
Capture, microphone, independent stream audio, Voice/Session tickets and retained
P2P membership remain separate. Dependency pins and unchanged React/Electron
reference sources are preserved.

## Adaptive video and viewer interest

Ported the four reference loss/RTT rungs, two breaches to drop one rung, three clean
samples to recover one rung, minimum/EMA RTT baseline and 2500 ms stats cadence.
Preview is 150 kbit/s, scale 4, 2 fps. Invalid/nonfinite measurements do not mutate
the ladder. Typed RTP stats correlate remote reports to the outgoing video source;
packet deltas are reset when the stats source identity changes. The pinned typed
SDK does not retain optional-field presence and has no outbound framesDropped
field: exact absent-fraction fallback/performance-toast parity is not yet proven.

Only active-session peers can set sender interest. Viewer reports distinguish the
focused stream from preview tiles, suppress unchanged reports, and remove absent
peers. SFU viewer readiness disables only that viewer's video and optional screen
audio sender; it does not disable microphone tracks or discard P2P connections.
Fallback reactivates share senders. Sender profiles retain RTP identity and unrelated
encodings. RC controller overrides are represented in the pure profile helper but
the native remote-control session/UI/input owner are not integrated in this stage.

## SFU signaling and ownership

Publish connections are send-only video plus optional stream audio, never voice
microphone. Subscribers have exactly receive-only video/audio slots and no source
tracks even if the client is sharing its own screen. Both use the existing ICE
configuration and native factory, without a new dependency or physical acquisition.
Native peer setup, sender mutation and Drop remain outside acceptance locks.

Ported SfuPublish, SfuSubscribe, SfuPublishReady, SfuViewerReady, SfuUnsubscribe,
SfuRequestKeyframe and SfuReportFailure with the unchanged SignalR argument order,
ten-second invoke budgets and response case compatibility. Delivery epoch and
original connection lifetime fence callbacks and answers. Unknown peers, unsafe
epochs, stale delivery modes and data-only engines cannot acquire an SFU session.
Pending publish requests retain the newest epoch until the share is ready.

The safe pinned SDK exposes only current_local_description, not the pending/local
offer with gathered candidates. A bounded candidate callback therefore records
owned candidate strings and augments the exact SDP media section after up to 1200 ms
of gathering. This does not change SDK sources. Tests cover MID placement, duplicate
suppression and rejection of injected candidate lines.

Each native SFU connection is retained in the original Voice-owned engine before
awaiting SDP or signaling. Cancellation of an await is not proof of native cleanup.
Connection Drop invalidates its original lifetime before removing callbacks and
closing the native connection. Universal callback quiescence, hung-driver/SDK
deadlines and whole-worker panic teardown remain the pre-existing final gates.

## Video handoff and bounded previews

A registered VideoReceiver probes a new route before replacing the displayed
receiver. Readiness requires an actually copied/decoded frame, not ontrack or
connection state. The ready event carries the original peer lifetime and active
frame lease, so replacing a same-peer probe invalidates its queued callback.
First-frame probes retire after 6500 ms. The previous P2P video/audio track remains
available for fallback; the new route does not acquire a microphone or capture.

SFU ViewerReady is sent only after successful decoding and only for the watched
stream. Preview snapshots close the subscriber after 2600 ms and retain only the
last displayed frame. Switching to watching cancels preview retirement. Publisher
readiness has a 6500 ms watchdog; subscribers have 10000 ms readiness watchdogs.
Returning the publisher to P2P keeps it for a separate 650 ms hold. Local share
replacement does not close independent remote SFU subscriptions.

Playback replaces only a peer's stream-audio lane across routes; voice lanes remain
separate. Explicit stream stop/member removal/Voice retirement release probes and
routes. Rejoin/call recovery reset route state without changing retained capture
or healthy local share tracks.

## Verification and remaining work

Initial all-targets check found private visibility on the new pending-publish
field; corrected without suppression. New test compilation also corrected the
synchronous Hub.close signature and capture_frame's bool return in select arms.
The first affected media run passed 21 tests including actual local SFU RTP
decoding. Four new native-source tests initially lacked a Tokio runtime; converted
to runtime-owned tests without relaxing assertions. New Voice fixtures now retain
the actual Hub event receiver instead of dropping it at connection creation.

Combined job `sh_1bb0bf9a9001DOu2WdH1GQMKeH` passed 150 affected tests: 25 media,
15 Voice, 3 playback, 7 video, 3 local RTP and 97 shell guards. Clippy then found a
nested if in the new ready callback. The callback now uses the equivalent
short-circuit condition; no lint suppression or assertion removal was used.

Final job `sh_1bb13b455001QoCdFJqrm6ucsp` passed both Clippy modes, build, RU/EN
inert checks, and the affected adaptive/first-frame/playback tests (14). Two
additional cases cover typed stats source/counter reset and stream-lane replacement
without removing the voice lane. The expected unique total is 152, not 164: the
12 repeated successful tests are not counted twice. A new
maintenance guard uses the original Voice ticket, and snapshot retirement leaves
the prior UI frame while dropping dead native receivers outside acceptance locks.

A read-only ordering review found that a late P2P ontrack could replace an in-flight
SFU probe before selection. P2P now updates its backup without replacing a live SFU
probe. The added test decodes the SFU frame first, then falls back to a fresh frame
from the retained P2P source, under their respective original lifetimes. Current
job `sh_1bb1972d3001T9Dkd9jVmwxL3N` passed the six affected Voice cases, both Clippy
modes, normal build, RU/EN inert checks and 13 scoped source/pin checks. Unique
affected total: 153; no double-counted reruns. Twelve unrelated checked hashes
matched the prior stage run. Evidence:
`migration-artifacts/native-streams/adaptive-sfu-functional-2026-11-08/route-order-checked/verification.json`.
The stage executable is `6b419849787165418b18a9148d0f769abec124f675d54417e1f1bdb17e14a706`;
it is not a new complete-client/audio/physical acceptance attestation.

Final code Graphify and scoped save passed: 9 native source paths, 13 source/pin
hashes and the verified normal executable were unchanged. Checked synthetic camera
and first-frame test markers are absent from the normal executable. Code graph:
6645 nodes, 16503 edges, 290 communities; the existing seven zero-node warnings and
5000-node HTML limit remain. `save-verification.json` records the saved source.

Native remote-control consent/channel/input/cursor/panic handling remains next in
this functional group. Full Go/.NET local integration, physical/long-session media,
exact visual/motion parity, generalized ownership/data-integrity and all other final
client acceptance gates are not claimed by this component stage.
