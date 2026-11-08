# Isolated desktop/window stream audio

Status: 180 targeted tests, fmt, normal/fixture Clippy, normal native build and
inert RU/EN picker checks passed. Not physical audio, complete-client, release
or shutdown acceptance. Baseline source is `df43be2`;
metadata baseline is `9985c45`. Existing React/Electron files remain unchanged.

## Selection and native boundary

`zabor-stream-audio` is a standalone Windows boundary using the existing pinned
`wasapi =0.26.0` and `windows =0.62.2`. It does not change the application's
unsafe-code prohibition or any WebRTC/DSP/CRT pins. All HWND resolution, COM,
application-loopback activation, initialization, capture, Stop, interface disposal
and COM uninitialization belong to the same registered blocking owner. The COM
owner is neither Send nor Sync and retains its owner ThreadId for runtime checks.

Screen capture uses the native client's actual process ID with
`include_tree=false`, which the pinned API maps to
`PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE`. Window capture resolves the
selected exact HWND through GetWindowThreadProcessId and includes that process
tree. Zero/invalid handles, missing processes and unsupported activation fail;
there is no fallback to default endpoint/system loopback that could retransmit
ZABOR's own playback. Native capture is not limited to a chosen render endpoint.

The pinned activation function synchronously waits for its asynchronous completion.
The owner checks cancellation before activation and after late return, but a stuck
activation cannot be interrupted safely here. The original application job stays
pending; an await timeout is not cleanup success. Process-loopback availability is
OS-dependent (Microsoft documents build 20348+); unsupported systems continue video
without audio, as the unchanged reference does. Physical availability is untested.

## Owned float buffers and teardown

Capture requests 48 kHz float32 stereo, shared event mode, automatic conversion and
no period/buffer-size querying: those queries are unsupported or unreliable for
this pinned process-loopback API. Data stays outside microphone/DSP/MicTest.

The published wasapi read helpers form a raw slice before inspecting SILENT, while
WASAPI permits a null pointer for a silent packet. This boundary instead obtains
IAudioCaptureClient on the owner and checks flags/count/pointer before byte copy.
SILENT packets allocate owned zeros without dereferencing a pointer. Non-silent
packets copy only within a bounded frame count; malformed bytes, invalid pointers
and excessive packet sizes fail closed. ReleaseBuffer is attempted for every
acquired buffer, including copy rejection/unwind. Release/Stop errors remain cleanup
errors, not ordinary capture failure. Float decoding preserves channel order and
sanitizes NaN/infinity/out-of-range values before the WebRTC i16 boundary.

Normal exit and unwind retire the native capture interfaces/event/client and COM
before the outer application's retained native audio source is disposed. Public
StreamAudioCapture Drop requests cancellation only. AudioCleanup.prepare_task is
registered before source or HWND/native acquisition; the actual original blocking
JoinHandle is observed through spawn_blocking_result. Startup cancellation/timeout
does not abort the acquisition/retirement owner. General driver/release deadlines,
universal callback quiescence and last-Arc shutdown remain unproved.

## Checked application path

The stream source has 48 kHz/two channels, no source AEC/NS/AGC and zero WebRTC
buffering. A bounded, channel-aligned 100 ms queue emits 10 ms stereo frames; a
capture discontinuity clears partial audio instead of splicing old and new clocks.
No audio demand lease, monitor ownership, mute/DSP state or MicTest clip is used.

The retained share owns optional stream audio alongside its original desktop source
and quality. Camera prepares no stream audio. Refusal/unsupported/start/track errors
do not end the video share or membership. StopShare/Leave/Logout drop that original
audio owner; normal application drain observes actual teardown. Recovery retains
the original healthy audio source and track instead of reopening native loopback.

A separate second audio transceiver/sender is reserved with the screen stream ID,
64 kbps/high priority matching the reference sender contract. Track replacement
does not replace, mute or remove the microphone. Remote SDP audio mids classify
the first audio section as voice and later audio sections as stream, avoiding a
dependence on on_track callback order. Playback requests two channels for stream
audio and one for voice; focused-stream routing/deafen behavior remains separate.
The SDK does not expose the browser's audio-track contentHint setter; disabled
source processing and a separate stream sender are used, not a claimed proof of
identical browser encoder tuning or end-to-end Opus stereo. Exact SDP/encoder and
physical audio parity remain final acceptance work.

The pinned SDK's Rust set_track(None) path passes null to a C++ method that
unconditionally dereferences it. The first sender test hit STATUS_ACCESS_VIOLATION;
the isolated reproducer and source diagnosis are retained. No SDK pin/source was
changed. Empty sender initialization now makes no set_track call. Detachment uses
PeerConnection.remove_track only on a present track, and reattachment uses AddTrack
to restore its send direction/reuse the second slot. This requires the existing
Voice microphone slot to be occupied; an audio-only engine without that invariant
rejects a new stream source instead of putting it in the voice slot. A null setter
is never used, and repeated start/stop is tested for transceiver growth.

The SDK preserves an opaque sender MSID across track replacement; the transmitted
ID is not required to equal a new local track label. Two initial new assertions
incorrectly required that equality and failed without a native crash after the
null fix. Corrected assertions verify distinct voice/stream routing, screen stream
membership and bounded original sender/transceiver identity, not invented IDs.

Picker includes the existing localized shareAudio option, default true, disabled
while busy/loading and hidden/forced false for camera. It reuses the established
native checkbox styling and the reference 600 px picker height; there is no locale
rewrite. Exact visual parity remains deferred until functional migration completes.

## Evidence and remaining gates

Local evidence is under
`migration-artifacts/native-streams/isolated-audio-functional-2026-11-08/`.
Initial standalone compilation caught the pinned Windows IsWindow Option<HWND>
signature; that diagnostic is retained and the call was corrected. No physical
camera/microphone/output/process-audio acquisition or production access is allowed
in these unattended tests. Native peer factories and synthetic PCM fixtures only.

Corrected standalone 6 tests and initial 6 application tests passed. They cover
exact include/exclude selection, silent-buffer decoding, bounded/sanitized float
copies, owner unwind/delayed join, source stereo delivery, clean vs poisoned drain,
pre-cancelled/sealed acquisition and cancelled startup observation. Existing
dependency versions are unchanged; the lock adds only the new crate and native
dependency reference. Expanded application/original-intent/sender/protocol/mixer
checks are running; they are not yet an accepted scenario.

The corrected sender tests passed all 11 media checks, and all 9 expanded capture
checks passed. Three new SignalR share fixtures then failed because they discarded
the Hub event receiver with `_`, closing their local transport before StartStream.
Those fixtures now retain `_events`, matching the established fixtures and actual
shell tuple; no client signaling behavior was changed to accommodate the tests.
The final run additionally includes affected existing media/audio path, routing,
runtime and mixer tests, not only the new synthetic source tests. Full default/
neural regression and physical/exact parity remain deferred, not accepted.

The first broader local run passed the new share tests and the existing mixer,
processed-microphone, endpoint-route and runtime tests. The old local SCTP/audio
roundtrip then exposed a phantom empty stream audio track: the reserved sender
was SendRecv even when there was no stream source. The empty slot is now RecvOnly;
AddTrack switches it to sending only for actual stream audio. The old fixture was
not changed. Four stream-sender tests, including late activation/64 kbps/priority/
slot reuse, and all three old media-local tests passed after the correction.

The first new late-sender fixture also caught an Option<MediaStreamTrack> matcher
signature mismatch; that compile diagnostic is retained and the matcher was fixed,
without changing any client logic to accommodate it. The final grouped run covers
180 targeted tests, normal/fixture Clippy/build and RU/EN inert audio-picker checks.
Source hashes and fixture exclusion are checked before saving. Results remain
pending; a temporary verification runner deletes itself on completion/failure.

Targeted checks, original-ticket cancellation, stream preservation/fallback,
receiver classification/routing, RU/EN inert interaction checks, normal executable
fixture exclusion and Graphify passed as recorded below; scoped save follows.
Full regression,
two-device audio, privacy/clock/hotplug/long-session, driver bounds, final parity,
packaging/signatures/publication and normal shutdown remain blocked by acceptance.

## Final targeted result

`final-checked/verification.json` records 180 passing tests: 6 native boundary,
12 media/sender, 9 capture/ownership, 12 share/restoration, 3 playback, 30 existing
local mixer/media/microphone/route/runtime, 94 original shell/Leave/Logout guards,
11 camera integration regressions and 3 desktop-frame regressions. Fmt and both
normal/fixture all-targets Clippy passed, including the standalone boundary.
The normal native-media application build and login-fixture RU/EN inert picker
checks passed; audio default/toggle/busy/tab behavior and camera forced-no-audio
are covered without physical acquisition. Full/default/neural/exact comparison
was not rerun or counted as accepted.

The verified 21 consumed source hashes remained unchanged across that complete
run, and the ordinary executable contained none of the test-camera/audio markers.
Its stage-only SHA-256 is `0fd77bc79320671cc24949bbf82c7b12b0fc98fd02f241aa3b8d42e63476a8de`;
this does not replace a frozen predecessor or establish whole-client attestation.
The temporary runner self-deleted; its removal and unchanged source/binary hashes
were checked after completion. No background jobs remain.

Graphify passed with 6448 nodes/15985 edges/285 communities; the existing seven
warnings and HTML 5000-node limit remain. No physical/production/push/publication/
shutdown occurred. Next work is source thumbnails/own-window filtering and adaptive
bitrate/SFU/remote control, then the remaining UI/profile/import/packaging and
ownership/data-integrity issues. Native driver/callback/last-Arc timing, physical
privacy/clocks/hotplug/long-session and exact Opus/browser tuning remain open.
