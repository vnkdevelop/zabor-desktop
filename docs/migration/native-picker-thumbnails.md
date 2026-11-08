# Native picker thumbnails — functional migration

Baseline: saved source `5b8d656`, metadata `e3cc8cd`, branch `rust/native-client`.
Status: 130 affected tests, normal/fixture Clippy, native build and expanded RU/EN
inert interactions passed. Saved source `0dcfef75fa2dfcd35cac38b0724c27b412682445`;
not full-client acceptance.

## Source identity and privacy

The unchanged Electron reference filters the main native HWND from window sources.
The pinned webrtc-sys 0.3.53 wrapper already disables enumeration of current-process
windows on Windows x64. The native client additionally obtains its real HWND through
the safe winit raw-window-handle API on the UI thread, rejects missing identity, and
filters the exact numeric window ID. A screen with the same numeric ID and another
application named ZABOR are not incorrectly excluded. Source IDs are not derived
from titles or thumbnails.

## Inventory and previews

Desktop and camera inventories publish source rows before preview acquisition.
The command worker only waits for inventory, not the complete preview pass. A
registered blocking actor subsequently acquires each selected-tab preview in turn.
Desktop thumbnails use the existing SDK capturer without a video track, audio,
microphone, DSP or active-share mutation. Camera previews use the existing dedicated
Media Foundation owner and retain its original Completion through native teardown.

Byte-only RGBA previews preserve aspect ratio and are bounded to 320 by 180 pixels.
Dimensions, stride and byte coverage are validated before indexed copying. Padding
is ignored, BGRA is reordered and desktop/camera pixels are made opaque. The UI
uses two-column source tiles, the existing palette, RU/EN labels and vector icons.
Exact raster/motion/accessibility parity remains deferred.

Five-second refresh preserves a still-present selected source and explicit audio/
quality choices. Removing the selected source clears selection. Inventory and image
events retain the original Voice ticket and picker query identity. Changing tab,
closing the picker, Leave, Logout and application exit cancel the original query;
late images cannot enter a new picker. Cancelled waits request stop only and do not
abort the registered native actor or equate readiness with cleanup completion.

The automatic refresh skips a query while its original actor is still running.
Registration is before acquisition, and cleanup observes the original blocking
JoinHandle on normal return, failure and unwind. Thumbnail time budgets are checked
between native operations, not represented as hard driver deadlines. A blocked
native capture/activation/release remains pending in the application drain.

## Verification and remaining scope

Initial all-targets check failed because independent picker work needed an original
Voice cancellation token, while CriticalIntents only exposed the Session token.
Added a read-only original-ticket Voice token using the existing changed epoch;
Leave/Logout revoke it, cancelling a child does not revoke Voice, and Leave still
preserves Session work. The original commit/run linearization is unchanged.
New thumbnail content properties use the non-deprecated Slint names; the unrelated
existing settings scroll warning is retained.

The initial complete targeted run (`sh_1baddb957001KczMV1CigxAyjV`) passed 130 tests,
normal/fixture Clippy, normal native build and inert RU/EN interactions. Subsequent
privacy completion clears cached pixels on failed previews and releases all source
rows on picker close, Voice retirement and application exit. Expanded inert checks
also cover HWND availability, pixel binding, unknown/wrong-tab images, two columns,
the odd last row and selected-source retention/removal.

The privacy completion's library tests passed again (7 picker, 14 camera, 4 source
and 9 intent). Its new image-size test initially used the wrong Slint size type;
the assertion now compares the width/height pair. The corrected remaining run
(`sh_1baefc137001FV4vB6eeUE029m`) passed: 96 shell guards, both Clippy modes, native
build, RU/EN expanded inert checks and 16 scoped source/binary checks. The successful
34 library results are reused rather than repeating unchanged tests (130 total).
Evidence: `migration-artifacts/native-streams/picker-thumbnails-functional-2026-11-08/corrected/verification.json`.
The normal executable excludes checked synthetic camera fixture markers. It is a
stage build, not a new full-client/audio/physical acceptance attestation.

First Graphify sync passed: 6502 nodes, 16134 edges, 280 communities. Final corrected
source sync passed: 6502 nodes, 16134 edges, 283 communities. Existing seven zero-node
warnings and the 5000-node HTML limit remain. Guarded source save `0dcfef7` passed
for 12 native paths; 16 source hashes and the checked executable remained unchanged,
and reused library sources matched. Save evidence is in the same stage directory's
`save-verification.json`. No unattended
physical enumeration/capture or production endpoints are used for these checks.

Application icons, exact loading skeleton/pair-settings layout, exact visual/motion
parity, physical privacy/hotplug/long-session and universal native callback/driver
teardown bounds remain acceptance work. Existing dependency pins, SDK sources,
microphone/monitor/Voice ownership and frozen predecessor evidence are unchanged.
No push, merge, beta publication or shutdown has occurred.
