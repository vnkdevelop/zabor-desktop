# Native camera ownership boundary

Status: 10 synthetic contract tests, 7 existing stream regressions, fmt, normal and
test-source Clippy, standalone camera build and native-media client Clippy/build
passed. Not camera or full-client acceptance.

## Backend decision

`nokhwa` was not installed. Its researched synchronous Media Foundation read and
state-only stop do not provide the cancellation needed by the native client.
`zabor-camera` instead owns an asynchronous Windows Media Foundation source reader
on a dedicated MTA thread. No Electron, WebView, OpenCV, physical audio acquisition,
or production endpoint is part of this boundary.

The existing `zabor-native` crate retains `#![forbid(unsafe_code)]`. The separate
native-library boundary has a CXX `unsafe extern "C++"` declaration, with no Rust
raw-pointer operations, unsafe blocks, or unsafe Send/Sync implementations. Its safe
API holds private, never-reused owner identifiers. Native COM pointers never cross
the bridge. The C++ library uses the existing pinned CXX 1.0.205 and static CRT
configuration; WebRTC, DSP, and ONNX pins are unchanged.
MSVC exception unwinding (`/EHsc`) and strict warnings-as-errors (`/W4 /WX`) are
enabled for the new boundary. Initial strict-build failures and their correction
remain in the evidence directory; they are not suppressed or erased.

## Ownership and cancellation

- Each operation initializes COM and Media Foundation on its own persistent owner.
  Enumeration, activation, format selection, reader requests, buffer conversion,
  Flush, media-source shutdown, COM release and runtime uninitialization run there.
- The source reader is asynchronous from creation. Only one read request is pending.
  Its thread-safe COM callback retains a sample in a separate native mailbox. The
  owner transfers that sample out and converts/copies it; Rust receives owned bytes.
  Borrowed callback-side sample AddRef and event inspection stay on Media Foundation
  callback threads; retained sample release and device operations stay on the owner.
- Camera audio is never selected. Camera dimensions and frame rate are not forced
  to desktop-sharing limits; the reader converts its default video format to RGB32.
  Frames beyond the supported 3840×2160 safety bound are rejected.
- The public frame mailbox retains at most one owned BGRA frame. Row size, stride,
  orientation, buffer extent, and address arithmetic are checked before copying.
  Media-type changes are checked rather than decoded with a stale layout.
- `Camera::cancel` and Drop request stop only. They never call a device driver,
  synchronously Flush, wait for a callback or join a running driver operation.
- A `Completion` preserves the original owner and is independent of `Camera` Drop.
  The owner closes callback admission, clears pending samples, waits for asynchronous
  Flush, shuts down/releases the source and reader, waits for the final callback
  reference to be released, and uninitializes Media Foundation/COM.
- `finished` requires joining the original native thread after all those operations
  have returned. Cancellation, frame invalidation, and a stop request are not treated
  as completion. `cleanup_failed` remains separate from acquisition/read failure.
- Dropping the last completion while a driver remains active abandons observation;
  it does not prove completion. The application integration must register a cleanup
  observer before acquisition and retain it across cancellation and late return.

## Current limits

The owner can remain pending if activation, buffer locking, Flush, shutdown, final
COM Release or Media Foundation shutdown hangs. No forced thread termination,
unverified timeout-to-success, or device-owner transfer is used. Physical driver
deadlines, camera hotplug, privacy-denial behavior and long-session stability need
final physical verification. A blocked owner must block release acceptance rather
than be silently reported as clean.

This boundary does not yet wire the camera command, picker/toggle, original Voice
acceptance ticket, native video source, channel/call restoration, or application
cleanup observer. System/process-isolated stream audio is a separate remaining
scenario. No camera or full-client readiness is claimed.

## API references

- [Asynchronous source reader](https://learn.microsoft.com/en-us/windows/win32/medfound/using-the-source-reader-in-asynchronous-mode)
- [Reader creation, native/default types and format changes](https://learn.microsoft.com/en-us/windows/win32/medfound/processing-media-data-with-the-source-reader)
- [Asynchronous Flush and pending request cancellation](https://learn.microsoft.com/en-us/windows/win32/api/mfreadwrite/nf-mfreadwrite-imfsourcereader-flush)

Local tests use a synthetic backend and the same native owner loop and COM callback
implementation. They do not enumerate, activate, read or stop a physical device.
Synthetic backend/frame-copy entrypoints are gated by the `test-source` Cargo
feature and are absent from the ordinary native-library build.
Verification results are retained under
`migration-artifacts/native-streams/camera-owner-functional-2026-11-08/`.
