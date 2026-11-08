# Camera application integration

Status: 134 targeted tests, fmt, normal/feature Clippy, normal native build and
inert RU/EN picker interactions passed. Not physical-camera, whole-client,
release, or shutdown acceptance.

The standalone source milestone is `99f59ca`. This stage links that boundary into
`zabor-native` behind `native-media`; synthetic acquisition is enabled only with
`camera-test-source`. Existing dependency pins and the native crate's unsafe-code
prohibition remain unchanged.

## Capture and cleanup

`CameraCapture` registers the original `AudioCleanupTask` before any camera or
WebRTC source acquisition. The registered blocking actor owns acquisition, frame
copy/conversion, source delivery, and camera retirement. Cancelling the startup
future, a picker inventory wait, or dropping the returned capture requests stop;
it does not abort the actor or convert task cancellation into successful cleanup.

The actor retains a `NativeOwner` with the original `Camera`, `Completion`, and
native source. Source disposal follows original camera completion on normal exit
and actor unwind, rather than relying on unrelated local-variable drop order.
Normal exit and stack unwind both request native stop and wait for that original
completion's `finished` status. The completion is not dropped until its native
thread has been joined. Native cleanup failures poison the shared cleanup tracker;
acquisition/refusal/read errors remain distinct from teardown failure. A separate
observer awaits the actual blocking actor's original JoinHandle before settling the
pre-registered application job. No native acquisition, conversion, or joining runs
on the UI thread or inside a CriticalIntents acceptance closure.

The actor creates a non-screencast `NativeVideoSource` from the first valid camera
frame, converts the copied BGRA pixels with the existing I420 helper, and preserves
the camera's default dimensions and frame rate within the backend safety bounds.
The capture's public Drop is stop-only; the actor retains its source until native
camera cleanup has completed. This does not prove bounded WebRTC last-Arc disposal,
universal callback quiescence, hung-driver deadlines, or whole-worker unwind.

## Share and picker wiring

The existing share owner retains Desktop/Camera variants and its original quality.
Camera start and restoration announce `StartStream("camera")` with the existing
default-camera sender behavior. Restoration checks the retained capture and track;
it does not reopen the camera. The shell takes the original Voice/events resources
under the original ticket before acquisition, then restores that whole resource
tuple with the same ticket after work outside acceptance locks. A failed camera
start restores the original session rather than ending its membership.

The existing picker has a third camera tab, exact `camera:<device id>` keys, and
the unchanged RU/EN camera labels. Its inventory uses a cancellable registered
owner and distinct loading state. Switching tabs or closing the modal cancels the
old query; late success/error UI events retain that query identity and cannot
populate another picker. An internal child token stops the inventory actor without
invalidating a successfully returned picker identity. The camera tab resets
selection, disables selection/confirmation while loading or busy, and does not
show desktop quality. Confirmation forces quality `camera`; no audio/RC parameters
are available in this camera path. Device activation requires an exact ID from
the native inventory rather than a parsed numeric alias or default-device fallback.

## Initial integration failure and guard

The first integration attempt aborted inside the pinned WebRTC adapter while
forwarding the standalone backend's tiny 4×2 byte-copy fixture. That failure is
retained in `initial-camera-integration-tests.log`; it is not a passed run. The
application now rejects frames below 16×16 before entering the native adapter,
along with its existing upper bounds. A distinct test-source-gated 160×96 video
fixture preserves the original 4×2 buffer tests without weakening their assertions.
The corrective acquisition/forwarding/drain and stream/picker results follow below.

The corrected intermediate run passed 10 capture/inventory/application-drain tests
and 9 stream tests. Its strict Clippy stage rejected a new test's constant-size
`chunks_exact` call. That diagnostic remains in `native-camera-clippy.log`; the
test now uses typed `as_chunks::<4>()` without suppression. Startup rejects a
capture already failed/finished when readiness arrives. The final run adds original
Leave/Logout camera-start
cancellation and original-ticket camera inventory dispatch, then checks both
feature/normal Clippy, affected shell/media/screen tests, the normal executable and
inert RU/EN picker interactions. Physical acquisition and complete acceptance stay
unverified regardless of those local results.

The first final run then caught a startup dependency cycle introduced by requiring
`NativeVideoSource::capture_frame` to return true before returning the source.
At that point no sender track/subscriber exists yet, so adapter non-delivery is
expected. The failed 8/11 result remains in `verified/camera-integration-tests.log`.
Readiness now requires a valid copied/converted producer frame without a subscriber
delivery prerequisite. Delivery is verified separately after a native track and
sink are attached. The corrected final run is under `verified-revision/`.

That revision passed all 134 targeted tests, fmt, both feature/normal Clippy and the
normal client build. The inert UI command itself failed to launch because a
relative forward-slash executable path was parsed incorrectly by `cmd.exe`; no UI
success was counted. The next run uses a normalized Windows path, additionally
retains the native source inside NativeOwner across unwind, and exercises the
actor-unwind test with an actual video source. Final results are under
`verified-owner-order/`; publication/readiness remain false pending full acceptance.

## Final targeted result

The owner-order source passed 10 backend, 11 camera integration, 9 stream,
93 shell-owner/Leave/Logout, 8 sender/media and 3 desktop-frame tests: 134 total.
Fmt, feature-enabled and ordinary all-targets Clippy, and the ordinary native-media
client build passed. The normal binary does not contain the synthetic-camera
markers. Its stage-only SHA-256 is recorded in `verified-owner-order/verification.json`;
this is not a new whole-client attestation or replacement for frozen predecessors.

The inert interaction checker starts from the login fixture, then exercises roster,
audio, MicTest and the full existing picker, including the added camera tab. Running
it from home was an invocation error, not auth-input acceptance; the failure remains
in the log. Correct login-fixture runs passed for RU and EN, including loading,
empty/selected/busy states, exact camera identity, forced quality and query cancel.
No physical camera/audio or production connection was used.

Graphify passed at 6332 nodes/15703 edges/276 communities; its existing seven
warnings and 5000-node HTML limit remain. No background checks remain. Camera
thumbnails, physical privacy/hotplug/long-session tests, driver/native last-Arc
deadlines, isolated stream audio and full React/native parity remain unfinished.

Targeted evidence is stored in
`migration-artifacts/native-streams/camera-integration-functional-2026-11-08/`.
All unattended integration tests remain synthetic/local and do not enumerate or
open a physical camera, microphone, output device, or production connection.
