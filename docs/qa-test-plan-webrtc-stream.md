# QA Test Plan: WebRTC Remote Stream & Dynamic Negotiation Suite

## 1. Executive Summary & Baseline Audit

### 1.1 Existing Test Harness
- **Test Runner**: Node.js built-in test runner (`node:test`, `node:assert/strict`).
- **Execution Script**:
  ```cmd
  cmd.exe /c npm test
  ```
  Defined in `package.json`:
  ```json
  "test": "node --disable-warning=MODULE_TYPELESS_PACKAGE_JSON --experimental-strip-types --import ./tests/helpers/register-hooks.mjs --test --test-concurrency=1 tests/tier1-feature-coverage.test.mjs tests/tier2-boundary-corner-cases.test.mjs tests/tier3-cross-feature-combinations.test.mjs tests/tier4-real-world-lifecycle.test.mjs"
  ```
- **Module Resolution & Hooks**:
  - `tests/helpers/register-hooks.mjs` registers `loader.mjs`.
  - `tests/helpers/loader.mjs` short-circuits `'electron'` to `./electron-mock.mjs` and `'child_process'` to `./child-process-mock.mjs`.
- **Baseline Test Suite**:
  - 4 tiers covering Updater and IPC contracts (37 passing tests, 0 failures, execution time ~15.5s).
- **Current Coverage Status for WebRTC & SignalR**:
  - **Zero automated tests** currently exist for WebRTC streaming, track binding, renegotiation, or call teardown in `tests/`.
  - All WebRTC and SignalR functionality is currently verified only through manual testing, leaving critical video/audio stream edge cases vulnerable to regressions.

---

## 2. Coverage Gap Analysis

### 2.1 Gap 1: `syncRemoteVideoStream` - Existing vs Asynchronous Receivers
- **Code Path**: `WebRTCManager.prototype.syncRemoteVideoStream(userId)` (`src/renderer/src/services/webrtc.ts:4836`).
- **Core Mechanism**:
  - Transceivers are pre-allocated during peer setup (`pc.addTransceiver('video', { direction: 'recvonly' })`).
  - When `UserStartedStreaming` arrives via SignalR, `syncRemoteVideoStream(userId)` is invoked.
  - If the receiver already exists on `pc.getReceivers()`, it checks `isUserStreaming || !track.muted`.
  - If receivers arrive asynchronously (e.g. SDP offer/answer in flight or `ontrack` firing after SignalR), `syncRemoteVideoStream(senderId)` is called in `handleOffer` (line 6018) and `handleAnswer` (line 6060).
- **Identified Gaps**:
  1. No test verifies that pre-existing transceivers immediately attach the track without waiting for a re-negotiation event.
  2. No test verifies that when `syncRemoteVideoStream` runs prior to transceiver arrival, subsequent `handleAnswer` / `handleOffer` properly completes the binding.
  3. No test asserts idempotent behavior: repeated calls to `syncRemoteVideoStream` must not instantiate duplicate `MediaStream` wrappers or duplicate audio limiter nodes.
  4. No test verifies handling when `track.readyState === 'ended'`.

### 2.2 Gap 2: Resource Cleanup & Teardown Upon Call Termination
- **Code Paths**:
  - `cleanupRemoteStream(userId)` (`webrtc.ts:6238`)
  - `cleanupRemoteStreamAudio(userId)` (`webrtc.ts:6211`)
  - `disconnectFromPeer(userId)` (`webrtc.ts:6120`)
  - `leaveAll()` (`webrtc.ts:6243`)
  - SignalR `UserStoppedStreaming` (`signalr.ts:705`), `StreamDropped` (`signalr.ts:715`), `endCall` (`signalr.ts:1885`), `leaveVoiceChannel` (`signalr.ts:1709`).
- **Identified Gaps**:
  1. **Audio Node Disconnect**: No test verifies that `streamSourceNodes`, `streamGainNodes`, `streamLimiterNodes`, and `streamDelayNodes` are actually disconnected and removed from the active Web Audio graph.
  2. **HTML5 `<audio>` Element**: No test verifies that `streamAudioElements` are paused, have `srcObject = null`, and are removed from the internal map.
  3. **Zustand State Cleanup**: No test verifies that `useAppStore.getState().remoteVideoStreams[userId]` is cleanly deleted (`delete next[userId]`) upon `cleanupRemoteStream` and `disconnectFromPeer`.
  4. **Active Stream ID**: When a track ends abruptly (`track.onended`), `cleanupRemoteStream(userId)` is triggered, but does not explicitly clear `store.activeStreamId`. Tests must verify whether `activeStreamId` becomes stale or if UI properly falls back.
  5. **Listener Leaks**: No test checks that `track.onunmute` and `track.onended` handlers do not hold lingering references to old closures or trigger ghost state updates after peer disconnection.

### 2.3 Gap 3: Muted Track Fallback & Inbound RTP Transitions
- **Code Path**: `syncRemoteVideoStream` lines 4844–4873:
  ```typescript
  if (!track.onunmute) {
    track.onunmute = () => { this.syncRemoteVideoStream(userId) }
  }
  if (isUserStreaming || !track.muted) {
    this.attachRemoteVideo(userId, track)
  }
  ```
- **Underlying W3C Behavior**:
  - In Chromium/WebRTC, when a remote video or screen audio transceiver is negotiated, `receiver.track.muted` defaults to `true` until the first inbound RTP packet arrives.
- **Identified Gaps**:
  1. **Signaling-First Arrival**: SignalR indicates `isUserStreaming = true` while `track.muted = true`. Tests must verify immediate attachment to store so the `<video>` container mounts without waiting for the first keyframe.
  2. **RTP-First / Delayed-Signaling Arrival**: SignalR message is delayed (`isUserStreaming = false`), track arrives with `muted = true`. Tests must verify that `track.onunmute` is registered and subsequently executes `syncRemoteVideoStream` once RTP starts and `track.muted = false`.
  3. **Screen Audio Synchronization**: Screen audio track arrives muted. If attached before video, tests must verify that `this.attachRemoteStreamAudio` properly binds to `remoteVideoStream` once both are present.
  4. **Ended Track Guard**: If a muted track transitions to `'ended'` before unmuting, it must not be attached.

### 2.4 Gap 4: Rapid Start/Stop/Restart Renegotiation Cycles & Collision Handling
- **Code Paths**:
  - `renegotiatePeer(pc, userId, iceRestart)` (`webrtc.ts:5340`)
  - `enqueueSignalingTask(userId, task)` (`webrtc.ts:621`)
  - `handleOffer(senderId, offerStr)` (`webrtc.ts:5927`)
  - `handleAnswer(senderId, answerStr)` (`webrtc.ts:6043`)
  - `recoverRejectedAnswer(userId, pc)` (`webrtc.ts:5392`)
  - `flushPendingRenegotiation(userId)` (`webrtc.ts:5384`)
- **Identified Gaps**:
  1. **Rapid Burst Cycles**: When a broadcaster starts streaming, stops streaming, and immediately restarts streaming within milliseconds:
     - No test asserts that `enqueueSignalingTask` serializes requests without race conditions.
     - No test verifies that `pendingRenegotiation` prevents `signalingState` deadlock while in `have-local-offer`.
     - No test verifies that `isMakingOffer` is safely released in `finally` blocks even if SDP creation rejects.
  2. **Offer Collision (Perfect Negotiation)**:
     - Peer A and Peer B simultaneously send offers during stream start.
     - Polite peer (`me > senderId`): Must roll back local offer (`pc.setLocalDescription({ type: 'rollback' })`), queue pending renegotiation, accept remote offer, send answer, and flush pending renegotiation once stable.
     - Impolite peer (`me <= senderId`): Must ignore colliding offer, retain local offer, and await remote answer.
     - No test simulates this simultaneous glare scenario to guarantee both peers converge to `stable`.
  3. **Epoch Validation**:
     - No test verifies that stale answers from previous renegotiation epochs (`answerEpoch !== currentEpoch`) are discarded without corrupting signaling state.
  4. **Rollback Counter & Recovery**:
     - No test verifies `recoverRejectedAnswer` handling up to `MAX_ROLLBACKS` (3) before forcing full renegotiation.

### 2.5 Gap 5: 1-on-1 Direct Calls vs Group Voice Channels
- **Code Paths**:
  - Group Channel: `voiceUsers` list, `webrtc.leaveAll()`, mesh peer connections.
  - 1-on-1 Direct Call: `currentCallUser`, `endCall()`, single peer disconnection (`webrtc.disconnectFromPeer(callUser.id)`).
- **Identified Gaps**:
  1. **Store Resolution**: `syncRemoteVideoStream` checks `store.voiceUsers.find(...) || (store.currentCallUser?.id === userId ? store.currentCallUser : null)`. No test asserts that a 1-on-1 call recipient (who is NOT in `voiceUsers`) correctly triggers stream attachment.
  2. **Teardown Scoping**: `endCall()` only calls `disconnectFromPeer(callUser.id)` and does NOT call `leaveAll()`. Tests must verify that `remoteVideoStreams[callUser.id]` and stream audio nodes are completely torn down without leaving orphaned Web Audio nodes.
  3. **Multi-Peer Isolation in Group Channels**: User A streaming in a 5-person channel must bind `remoteVideoStreams[userA.id]` without altering audio playout, volume, or speaking states of Users B, C, and D.
  4. **Context Switching**: Transitioning from a 1-on-1 call directly into a group voice channel must not leak old video streams or confuse `userScreenStreamIds`.

---

## 3. Test Harness Architecture & Mock Infrastructure

To execute in the project's existing Node.js test environment (`node:test` + `--experimental-strip-types`), we define lightweight, high-fidelity mock helpers in `tests/helpers/`.

### 3.1 `tests/helpers/webrtc-stream-mock.mjs`
Provides realistic W3C WebRTC and Web Audio mocks:

```javascript
import { EventEmitter } from 'node:events';

export class MockMediaStreamTrack extends EventEmitter {
  constructor({ id = `track_${Math.random().toString(36).slice(2, 9)}`, kind = 'video', readyState = 'live', muted = true } = {}) {
    super();
    this.id = id;
    this.kind = kind;
    this.readyState = readyState;
    this._muted = muted;
    this.enabled = true;
    this.contentHint = '';
    this.onmute = null;
    this.onunmute = null;
    this.onended = null;
  }

  get muted() {
    return this._muted;
  }

  set muted(val) {
    if (this._muted === val) return;
    this._muted = val;
    if (val) {
      if (typeof this.onmute === 'function') this.onmute();
      this.emit('mute');
    } else {
      if (typeof this.onunmute === 'function') this.onunmute();
      this.emit('unmute');
    }
  }

  stop() {
    this.readyState = 'ended';
    if (typeof this.onended === 'function') this.onended();
    this.emit('ended');
  }
}

export class MockMediaStream {
  constructor(tracks = []) {
    this.id = `stream_${Math.random().toString(36).slice(2, 9)}`;
    this._tracks = [...tracks];
  }

  getTracks() { return [...this._tracks]; }
  getVideoTracks() { return this._tracks.filter(t => t.kind === 'video'); }
  getAudioTracks() { return this._tracks.filter(t => t.kind === 'audio'); }
  addTrack(t) { if (!this._tracks.includes(t)) this._tracks.push(t); }
  removeTrack(t) { this._tracks = this._tracks.filter(x => x !== t); }
}

export class MockRTCRtpReceiver {
  constructor(track) {
    this.track = track;
    this.transport = { state: 'connected' };
  }
  getParameters() { return { encodings: [{}] }; }
}

export class MockRTCRtpSender {
  constructor(track) {
    this.track = track;
    this.transport = { state: 'connected' };
    this._params = { encodings: [{}] };
  }
  async replaceTrack(t) { this.track = t; }
  getParameters() { return this._params; }
  async setParameters(p) { this._params = p; }
}

export class MockRTCRtpTransceiver {
  constructor(kind, direction = 'sendrecv', track = null) {
    this.kind = kind;
    this.direction = direction;
    this.currentDirection = direction;
    this.stopped = false;
    this.receiver = new MockRTCRtpReceiver(track || new MockMediaStreamTrack({ kind }));
    this.sender = new MockRTCRtpSender(null);
  }
  stop() { this.stopped = true; }
}

export class MockRTCPeerConnection extends EventEmitter {
  constructor(config = {}) {
    super();
    this.config = config;
    this.signalingState = 'stable';
    this.connectionState = 'connected';
    this.iceConnectionState = 'connected';
    this.localDescription = null;
    this.remoteDescription = null;
    this.transceivers = [];
    this.ontrack = null;
    this.onicecandidate = null;
    this.onconnectionstatechange = null;
    this.oniceconnectionstatechange = null;
  }

  getReceivers() { return this.transceivers.map(t => t.receiver); }
  getSenders() { return this.transceivers.map(t => t.sender); }
  getTransceivers() { return [...this.transceivers]; }

  addTransceiver(trackOrKind, init = {}) {
    const kind = typeof trackOrKind === 'string' ? trackOrKind : trackOrKind.kind;
    const track = typeof trackOrKind === 'object' ? trackOrKind : null;
    const transceiver = new MockRTCRtpTransceiver(kind, init.direction || 'sendrecv', track);
    this.transceivers.push(transceiver);
    return transceiver;
  }

  async createOffer(opts = {}) {
    return { type: 'offer', sdp: `v=0\r\no=mock 123 123 IN IP4 127.0.0.1\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\n` };
  }

  async createAnswer(opts = {}) {
    return { type: 'answer', sdp: `v=0\r\no=mock 123 123 IN IP4 127.0.0.1\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\n` };
  }

  async setLocalDescription(desc) {
    if (desc.type === 'rollback') {
      this.signalingState = 'stable';
      this.localDescription = null;
      return;
    }
    this.localDescription = desc;
    this.signalingState = desc.type === 'offer' ? 'have-local-offer' : 'stable';
  }

  async setRemoteDescription(desc) {
    this.remoteDescription = desc;
    this.signalingState = desc.type === 'offer' ? 'have-remote-offer' : 'stable';
  }

  close() {
    this.signalingState = 'closed';
    this.connectionState = 'closed';
    this.iceConnectionState = 'closed';
    this.emit('connectionstatechange');
  }
}

export class MockAudioNode {
  constructor() {
    this.connectedTo = [];
    this.isDisconnected = false;
  }
  connect(dest) { this.connectedTo.push(dest); }
  disconnect() {
    this.connectedTo = [];
    this.isDisconnected = true;
  }
}

export class MockAudioElement {
  constructor() {
    this.autoplay = false;
    this.muted = false;
    this.srcObject = null;
    this.paused = true;
  }
  async play() { this.paused = false; }
  pause() { this.paused = true; }
}
```

### 3.2 Global Environment Polyfill Hook
Before executing WebRTC stream tests, `globalThis` is populated with:
- `globalThis.MediaStream = MockMediaStream;`
- `globalThis.MediaStreamTrack = MockMediaStreamTrack;`
- `globalThis.RTCPeerConnection = MockRTCPeerConnection;`
- `globalThis.Audio = MockAudioElement;`
- `globalThis.AudioNode = MockAudioNode;`

---

## 4. Proposed Automated Test Suite Specifications

We specify 5 test suites to be placed under `tests/tier5-webrtc-stream-lifecycle.test.mjs` (or integrated into the project test command).

### Suite 1: `syncRemoteVideoStream` Receiver Binding & Store Synchronization

| Case ID | Test Case Title | Setup & Scenario | Expected Assertion |
|---|---|---|---|
| **S1-01** | Pre-existing video receiver binds immediately when user starts streaming | Peer connection has pre-allocated video transceiver (`recvonly`). Remote user `user_alpha` starts streaming (`isUserStreaming = true`). Call `syncRemoteVideoStream('user_alpha')`. | `store.remoteVideoStreams['user_alpha']` is defined; contains the receiver's video track; `track.onended` is attached. |
| **S1-02** | Asynchronous receiver arrival via `handleAnswer` / `handleOffer` | `syncRemoteVideoStream` is called when no receiver exists yet (empty receivers list). Then offer/answer completes, transceiver is added, and `syncRemoteVideoStream` is invoked again. | Initial call does not throw or create empty stream. Secondary call cleanly binds the newly arrived receiver to `store.remoteVideoStreams['user_alpha']`. |
| **S1-03** | Idempotency on repeated sync invocations | `syncRemoteVideoStream('user_alpha')` is called 5 times consecutively with the same active receiver. | `store.remoteVideoStreams['user_alpha']` remains the same object reference; no duplicate MediaStream wrappers; `setRemoteVideoStream` not re-triggered unnecessarily. |
| **S1-04** | Track replacement when streamer changes source | `user_alpha` replaces window track `track_A` with screen track `track_B`. `syncRemoteVideoStream` is called. | `store.remoteVideoStreams['user_alpha'].getVideoTracks()[0].id === 'track_B'`. Old track listener does not trigger spurious teardown. |
| **S1-05** | Closed peer connection rejection | `pc.connectionState = 'closed'`. Call `syncRemoteVideoStream('user_alpha')`. | Function returns immediately without mutating store or attaching event handlers. |

### Suite 2: Complete Resource Cleanup & Call Teardown

| Case ID | Test Case Title | Setup & Scenario | Expected Assertion |
|---|---|---|---|
| **S2-01** | `cleanupRemoteStream` deletes remote video stream from store | Remote stream active in store (`store.remoteVideoStreams['user_alpha'] = stream`). Call `webrtc.cleanupRemoteStream('user_alpha')`. | `store.remoteVideoStreams['user_alpha']` is `undefined` (key removed from dictionary). |
| **S2-02** | Screen audio Web Audio nodes cleanly disconnected | `attachRemoteStreamAudio` has connected source, delay, gain, and limiter nodes. Call `webrtc.cleanupRemoteStreamAudio('user_alpha')`. | `sourceNode.isDisconnected === true`, `gainNode.isDisconnected === true`, `limiterNode.isDisconnected === true`, internal maps cleared. |
| **S2-03** | HTML5 audio element paused and disassociated | `streamAudioElements.get('user_alpha')` is playing. Call `cleanupRemoteStreamAudio('user_alpha')`. | Audio element `pause()` called, `srcObject === null`, removed from `streamAudioElements`. |
| **S2-04** | `track.onended` triggers automatic remote stream cleanup | Broadcaster stops media stream at OS level -> `track.stop()` fires `onended`. | `cleanupRemoteStream` executes; store stream set to `null`; no uncaught errors. |
| **S2-05** | Channel exit (`leaveAll`) cleans all active streaming peers | 3 peers in channel, 2 are streaming screens. User leaves channel via `webrtc.leaveAll()`. | All peer connections closed; all remoteVideoStreams deleted; all audio elements paused; all stream sync timers cleared. |

### Suite 3: Track Mute State, Fallback Logic & Inbound RTP Transitions

| Case ID | Test Case Title | Setup & Scenario | Expected Assertion |
|---|---|---|---|
| **S3-01** | Signaling-first arrival with muted track attaches immediately | `isUserStreaming = true` (SignalR event arrived). Video track has `muted = true`. Call `syncRemoteVideoStream`. | Track attaches to store immediately (`isUserStreaming || !track.muted` is true) so video element can mount. |
| **S3-02** | RTP-first arrival: unmuted track attaches without signaling | `isUserStreaming = false` (SignalR delayed). Track arrives with `muted = false`. Call `syncRemoteVideoStream`. | Track attaches to store immediately (`!track.muted` is true). |
| **S3-03** | Delayed signaling: muted track un-mutes and triggers fallback | `isUserStreaming = false`, track is `muted = true`. Call `syncRemoteVideoStream`. Then simulate inbound RTP packet: `track.muted = false` (triggers `onunmute`). | Initial call registers `track.onunmute` but does not attach. Once `track.muted = false`, `onunmute` fires and attaches track to store. |
| **S3-04** | Screen audio muted fallback | Screen audio track arrives muted. Attached to Web Audio graph with volume/gain ready, muted until RTP arrives. | `userStreamAudioTrackIds` tracked, audio source initialized, un-mute fires cleanly without audio node duplication. |
| **S3-05** | Premature track end while muted | Muted track ends (`readyState = 'ended'`) before ever un-muting. | Track is discarded; does not attach to store; `attachRemoteVideo` exits early. |

### Suite 4: Renegotiation Concurrency, Queueing & Perfect Negotiation Collision

| Case ID | Test Case Title | Setup & Scenario | Expected Assertion |
|---|---|---|---|
| **S4-01** | Rapid start/stop/restart screen sharing burst | Broadcaster rapidly toggles screen sharing 5 times in under 50ms (`startScreenShare` -> `stopScreenShare` -> `startScreenShare`). | `enqueueSignalingTask` queues tasks; no unhandled promise rejections; final transceiver direction matches final user intent (`sendrecv`). |
| **S4-02** | Simultaneous offer collision: Polite peer rolls back | Peer A (`id: 'user_999'`) and Peer B (`id: 'user_111'`) send offers simultaneously. Peer A is polite (`'user_999' > 'user_111'`). Peer A receives Peer B's offer while `have-local-offer`. | Peer A sets local description to `{ type: 'rollback' }`, sets `pendingRenegotiation`, accepts Peer B's offer, and creates answer. |
| **S4-03** | Simultaneous offer collision: Impolite peer ignores offer | Peer B (`id: 'user_111'`) receives colliding offer from Peer A (`id: 'user_999'`). Peer B is impolite (`'user_111' < 'user_999'`). | Peer B ignores the colliding offer, stays in `have-local-offer`, and awaits Peer A's answer. Both peers converge to `stable`. |
| **S4-04** | Outdated answer epoch rejection | Peer receives answer with `epoch: 1` when local offer is currently at `epoch: 2`. | Outdated answer is rejected (`answerEpoch !== currentEpoch`); local state remains intact. |
| **S4-05** | Offer rejection recovery within rollback limit | Remote peer returns invalid answer, triggering `recoverRejectedAnswer`. | Rolls back up to `MAX_ROLLBACKS` (3) before falling back to full renegotiation attempt. |

### Suite 5: 1-on-1 Direct Calls vs Group Channel Voice Streams

| Case ID | Test Case Title | Setup & Scenario | Expected Assertion |
|---|---|---|---|
| **S5-01** | 1-on-1 direct call stream resolution via `currentCallUser` | Direct call active (`currentCallUser.id = 'partner_1'`). `voiceUsers` is empty. Partner starts streaming. Call `syncRemoteVideoStream('partner_1')`. | Resolves user via `store.currentCallUser`; binds stream to `store.remoteVideoStreams['partner_1']`. |
| **S5-02** | Direct call teardown (`endCall`) isolation | 1-on-1 call ends via `signalr.endCall()`. Calls `webrtc.disconnectFromPeer('partner_1')`. | Only `'partner_1'` connection and stream are destroyed; store `currentCallUser` set to `null`; `remoteVideoStreams['partner_1']` removed. |
| **S5-03** | Group channel multi-peer stream isolation | 4 users in channel. User 1 starts streaming, User 2 talks, User 3 adjusts User 1 volume. | User 1's video binds to `remoteVideoStreams[user1]`. User 2's voice gain and speaking status are completely unaffected. |
| **S5-04** | Direct call to group channel switch | User ends direct call and immediately joins group voice channel. | Direct call stream cleanly dismounted; no residual stream audio nodes; group channel peers establish fresh mesh. |

---

## 5. Implementation Roadmap & Execution Command

### 5.1 Test File Placement
- Mock Helper: `tests/helpers/webrtc-stream-mock.mjs`
- Test Suite: `tests/tier5-webrtc-stream-lifecycle.test.mjs`

### 5.2 NPM Test Script Update
In `package.json`, append the new test file to the `test` script:
```json
"test": "node --disable-warning=MODULE_TYPELESS_PACKAGE_JSON --experimental-strip-types --import ./tests/helpers/register-hooks.mjs --test --test-concurrency=1 tests/tier1-feature-coverage.test.mjs tests/tier2-boundary-corner-cases.test.mjs tests/tier3-cross-feature-combinations.test.mjs tests/tier4-real-world-lifecycle.test.mjs tests/tier5-webrtc-stream-lifecycle.test.mjs"
```

### 5.3 Acceptance Verification Criteria
1. Running `cmd.exe /c npm test` executes all 5 tiers (37 existing + 24 new = 61 passing tests) with 0 failures.
2. `npx tsc --noEmit` exits with status code 0 (zero type errors).
3. All mock instances clean up their event listeners; no dangling timers or process hangs.
