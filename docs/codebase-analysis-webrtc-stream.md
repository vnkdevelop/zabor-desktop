# Codebase Analysis: WebRTC Dynamic Stream Negotiation & Track Lifecycle

## 1. Executive Summary

This report delivers a comprehensive architectural and dependency analysis of the WebRTC dynamic stream negotiation pipeline, screen sharing track lifecycle (video and system audio), and state synchronization across Zustand stores in Zabor Desktop.

The analysis is conducted by the **Codebase Analyzer** in accordance with `AGENTS.md`, `PROJECT.md`, and the authoritative prompt (`ORIGINAL_REQUEST.md`). The primary focus is verifying how dynamic stream tracks bind, unbind, and renegotiate without regressions in 1-on-1 calls vs. group voice channels, confirming that component topologies and blast radiuses are bounded, and identifying circular dependencies or concurrency edge cases.

---

## 2. Component Topology & Import Graph

### 2.1 Architectural Hierarchy

The screen sharing and media streaming architecture spans four distinct execution boundaries:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                             Renderer UI Layer                               │
│        App.tsx  ──►  StreamCard.tsx  ──►  StreamPicker.tsx                 │
└───────────────────────┬──────────────────────────────────┬──────────────────┘
                        │                                  │
                        ▼                                  ▼
┌─────────────────────────────────────┐  ┌────────────────────────────────────┐
│         Zustand Store Layer         │  │       Signaling & WebRTC Layer     │
│  useAppStore.ts (remoteVideoStreams,│  │  signalr.ts (SignalRService)       │
│  voiceUsers, currentCallUser,       │◄─┼─► webrtc.ts (WebRTCManager)        │
│  activeStreamId, streamVolumes)     │  │  (Mutual import, deferred runtime) │
└─────────────────────────────────────┘  └─────────────────┬──────────────────┘
                                                           │
                                                           ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                          Electron Main & Native Layer                       │
│  preload/index.ts  ──►  main/index.ts (electron-native-screenshare)         │
│  playback-buffer-processor.ts (AudioWorklet 48kHz float32 loopback buffer)  │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 2.2 Dependency & Import Matrix

| Module / Component | Inbound Dependencies (Imported By) | Outbound Dependencies (Imports) | Cyclical Dependency Status |
| :--- | :--- | :--- | :--- |
| `src/renderer/src/services/webrtc.ts` | `signalr.ts`, `App.tsx`, `AudioSettings.tsx`, `NoiseSuppressionSettings.tsx`, `ChatPanel.tsx` | `signalr.ts`, `useAppStore.ts`, `useSpeakingStore.ts`, `i18n.ts`, worklet URLs, `audio-assets.ts` | **Mutual Import with `signalr.ts`**: Static module import cycle exists (`webrtc.ts` ⇄ `signalr.ts`). Both instantiate singletons at the bottom of the file (`export const webrtc = new WebRTCManager()`, `export const signalRService = new SignalRService()`). Neither invokes methods on the other during module evaluation/field initialization. Safe in runtime, but must be treated carefully to avoid TDZ. |
| `src/renderer/src/services/signalr.ts` | `webrtc.ts`, `App.tsx`, `ChatPanel.tsx`, `TitleBar.tsx` | `webrtc.ts`, `chatPeer.ts`, `useAppStore.ts`, audio sound assets | See above. |
| `src/renderer/src/store/useAppStore.ts` | `webrtc.ts`, `signalr.ts`, `App.tsx`, `StreamCard.tsx`, multiple UI components | `useSpeakingStore.ts` | **Acyclic (Leaf store)**. No imports from services or UI. |
| `src/renderer/src/App.tsx` | Root entry point (`main.tsx`) | `webrtc.ts`, `signalr.ts`, `useAppStore.ts`, `StreamCard.tsx`, `StreamPicker.tsx`, UI components | **Acyclic (Top-level orchestrator)**. |
| `src/renderer/src/components/Stream/StreamCard.tsx` | `App.tsx` | `useAppStore.ts`, `AvatarImg.tsx`, `anime.ts`, `react-i18next` | **Acyclic (Leaf UI)**. Receives `stream: MediaStream \| null` as prop; does NOT import `webrtc` or `signalr`. |
| `src/renderer/src/components/Stream/StreamPicker.tsx` | `App.tsx` | `usePresence.ts`, `anime.ts`, `react-i18next` | **Acyclic (Leaf UI)**. Invokes `onSelect` callback; does not communicate with services directly. |
| `src/main/index.ts` | Electron main entry (`package.json`) | `electron-native-screenshare`, Node.js built-ins (`fs`, `crypto`, `path`) | **Acyclic (Main process)**. Communicates via IPC handlers (`startStreamAudioCapture`, `stopStreamAudioCapture`, `onStreamAudioData`). |

---

## 3. WebRTC Dynamic Screen Sharing Track Lifecycle

### 3.1 Broadcaster Pipeline (`startScreenShare` & `stopScreenShare`)

#### 1. Track Acquisition & Initialization (`startScreenShare`, `webrtc.ts:5177`)
- **Video Track**: Acquired via `navigator.mediaDevices.getUserMedia(constraints)` using desktop capturer `chromeMediaSource: 'desktop'` and sourceId. Resolution and frame rates are adapted according to quality (`high`: 1920x1080@60fps, maxBitrate 6 Mbps; `low`: 1280x720@30fps, maxBitrate 2.5 Mbps). `videoTrack.contentHint = 'motion'`.
- **System Audio Track**: Acquired via `startIsolatedStreamAudio(sourceId)` (`webrtc.ts:5096`):
  - Starts native loopback capture in Main process (`electron-native-screenshare`).
  - Spawns dedicated 48 kHz `AudioContext` with `playback-buffer-processor` worklet.
  - Receives float32 PCM buffers over IPC, posts to worklet node port.
  - Derives `MediaStreamTrack` from `destination.stream.getAudioTracks()[0]` with `contentHint = 'music'`.
  - Appends audio track to `stream`.

#### 2. Transceiver Binding & Dynamic Renegotiation
- For every peer connection in `this.peerConnections.entries()`:
  - **Video Transceiver Binding**:
    - Queries `pc.getTransceivers().find(t => t.receiver.track.kind === 'video' && !t.stopped)`.
    - If transceiver exists: sets `direction = 'sendrecv'`, executes `await videoTransceiver.sender.replaceTrack(videoTrack)`, calls `setStreams(stream)`.
    - If no transceiver exists: calls `pc.addTrack(videoTrack, stream)`.
    - Applies encoding degradation preference (`maintain-framerate`) and network priority (`high`).
  - **Screen Audio Transceiver Binding**:
    - Queries audio transceivers: second audio transceiver (`audioTransceivers[1]`) is allocated for screen audio (first is reserved for microphone/voice).
    - If exists: sets `direction = 'sendrecv'`, executes `await screenAudioTransceiver.sender.replaceTrack(audioTrack)`, calls `setStreams(stream)`, invokes `this.configureAudioSender(...)`.
    - If not exists: calls `pc.addTrack(audioTrack, stream)`.
  - Calls `await this.renegotiatePeer(pc, userId)` to generate and send SDP Offer with updated media sections.
- Invokes `signalRService.startStream(quality)` to inform the server and peer participants.

#### 3. Broadcaster Teardown (`stopScreenShare`, `webrtc.ts:5287`)
- Stops stats polling interval.
- Iterates through all peer connections:
  - Identifies screen video and screen audio transceivers.
  - Sets `t.sender.replaceTrack(null)`.
  - Reverts transceiver direction to `recvonly`.
  - Invokes `void this.renegotiatePeer(pc, userId)` to synchronize SDP.
- Disables and stops local video tracks (`track.enabled = false; track.stop()`).
- Invokes `stopIsolatedStreamAudio()`:
  - Removes native IPC listener.
  - Calls `window.windowControls.stopStreamAudioCapture()`.
  - Disconnects worklet node and closes capture `AudioContext`.
- Clears rate-limiting and loss-ladder metrics.
- Invokes `signalRService.stopStream()` to notify participants.

---

### 3.2 Receiver Pipeline (`syncRemoteVideoStream`, `attachRemoteVideo`, `attachRemoteStreamAudio`)

#### 1. Dynamic Discovery (`syncRemoteVideoStream`, `webrtc.ts:4836`)
```typescript
public syncRemoteVideoStream(userId: string): void {
  const pc = this.peerConnections.get(userId)
  if (!pc || pc.connectionState === 'closed') return

  const store = useAppStore.getState()
  const user = store.voiceUsers.find(u => u.id === userId) || (store.currentCallUser?.id === userId ? store.currentCallUser : null)
  const isUserStreaming = Boolean(user?.isStreaming)

  for (const receiver of pc.getReceivers()) {
    const track = receiver.track
    if (track && track.kind === 'video' && track.readyState !== 'ended') {
      if (!track.onunmute) {
        track.onunmute = () => {
          this.syncRemoteVideoStream(userId)
        }
      }
      if (isUserStreaming || !track.muted) {
        this.attachRemoteVideo(userId, track)
      }
    }
  }

  const knownVoiceTrackId = this.userVoiceTrackIds.get(userId)
  const audioReceivers = pc.getReceivers().filter(r => r.track && r.track.kind === 'audio')
  const screenAudioReceiver = audioReceivers.find(r => r.track && r.track.id !== knownVoiceTrackId && r.track.readyState !== 'ended')
  if (screenAudioReceiver?.track) {
    if (!screenAudioReceiver.track.onunmute) {
      screenAudioReceiver.track.onunmute = () => {
        this.syncRemoteVideoStream(userId)
      }
    }
    if (isUserStreaming || !screenAudioReceiver.track.muted) {
      if (this.userStreamAudioTrackIds.get(userId) !== screenAudioReceiver.track.id) {
        const remoteVideoStream = useAppStore.getState().remoteVideoStreams[userId]
        this.attachRemoteStreamAudio(userId, screenAudioReceiver.track, screenAudioReceiver, remoteVideoStream)
      }
    }
  }
}
```

#### 2. Trigger Matrix for `syncRemoteVideoStream`
`syncRemoteVideoStream(userId)` is triggered at every critical stage in the signaling and track lifecycle:
1. **SignalR Broadcast**: `on("UserStartedStreaming", (userId, streamQuality) => { ... webrtc.syncRemoteVideoStream(userId); })` (`signalr.ts:702`).
2. **SDP Answer Handling**: After remote description is set and pending candidates are drained in `handleAnswer` (`webrtc.ts:6060`).
3. **SDP Offer Handling**: After local answer description is set and candidates drained in `handleOffer` (`webrtc.ts:6018`).
4. **Peer Handler Setup**: Transceiver video tracks registered on peer setup (`webrtc.ts:4882`).
5. **Inbound Track Unmute**: Both video track (`webrtc.ts:4849`) and screen audio track (`webrtc.ts:4864`) register `onunmute` fallback to re-sync.

#### 3. Remote Video Attachment (`attachRemoteVideo`, `webrtc.ts:4774`)
- Checks `if (track.readyState === 'ended') return`.
- Checks if existing stream already has track: `if (currentStream && currentStream.getVideoTracks().includes(track)) return`.
- Creates `new MediaStream([track])` (or adopts provided stream).
- Stores in Zustand: `useAppStore.getState().setRemoteVideoStream(userId, videoStream)`.
- Binds `track.onended = () => { this.cleanupRemoteStream(userId) }`.

#### 4. Remote Screen Audio Attachment (`attachRemoteStreamAudio`, `webrtc.ts:4786`)
- Isolates receiver latency: `this.isolateReceiverLatency(receiver)`.
- Binds `track.onended = () => { this.cleanupRemoteStreamAudio(userId) }`.
- Sets `track.contentHint = 'music'`.
- Configures muted `<audio autoplay muted>` element to keep Chromium pull clock ticking without unmanaged browser playout.
- Constructs Web Audio API chain:
  `MediaStreamSource` ➔ `DelayNode` (A/V sync) ➔ `LimiterNode` (anti-clipping) ➔ `GainNode` (volume control) ➔ `outputBusGain`.
- Initiates A/V synchronization drift tracking: `startStreamSyncMonitoring()`.
- Updates volume: `updateRemoteStreamVolume(userId)` (sets gain to 0 if stream is not focused/active in UI).

---

## 4. State Synchronization across Zustand Stores

The application state is coordinated primarily across `useAppStore.ts` with supporting speaking status in `useSpeakingStore.ts`:

### 4.1 State Keys & Mutators

| Store Key | Type | Mutator | Semantics & Consumer Behavior |
| :--- | :--- | :--- | :--- |
| `remoteVideoStreams` | `Record<string, MediaStream>` | `setRemoteVideoStream(userId, stream \| null)` | Stores active incoming MediaStream per remote user ID. When stream is `null`, key is deleted from map. Read by `App.tsx` to calculate `activeUserCount`, card dimensions, and passed to `StreamCard`. |
| `voiceUsers` | `User[]` | `setVoiceUsers`, `updateUserStatus` | Active participants in current voice channel. Contains `isStreaming: boolean` and `streamQuality`. |
| `currentCallUser` | `User \| null` | `setCurrentCallUser`, `updateUserStatus` | Active participant in 1-on-1 direct call. Contains `isStreaming: boolean` and `streamQuality`. |
| `activeStreamId` | `string \| null` | `setActiveStreamId(id)` | ID of user whose stream is currently focused/expanded. If set to `null`, `isStreamFullscreen` is automatically reset to `false`. |
| `isStreamFullscreen` | `boolean` | `setStreamFullscreen(val)` | Fullscreen status for the active stream. |
| `streamVolumes` | `Record<string, number>` | `setStreamVolume(userId, volume)` | Custom volume level (0-200%) for each remote user's screen audio. |
| `callStatus` | `'idle' \| 'calling' \| 'connected'` | `setCallStatus(status)` | 1-on-1 direct call lifecycle status. |

### 4.2 Auto-Correction in `App.tsx`
`App.tsx` implements defensive reactive effects that monitor `activeStreamId` and `remoteVideoStreams`:
```typescript
// App.tsx:1055-1065
const relevantUsers = store.currentCallUser
  ? (store.currentUser ? [store.currentCallUser, store.currentUser] : [store.currentCallUser])
  : store.voiceUsers;

const streamer = relevantUsers.find(user => user.id === store.activeStreamId);
const isStreaming = streamer && (
  streamer.isStreaming || (streamer.id === store.currentUser?.id && !!webrtc.localVideoStream) || Boolean(store.remoteVideoStreams[streamer.id])
);

if (!isStreaming) {
  if (store.isStreamFullscreen) store.setStreamFullscreen(false);
  store.setActiveStreamId(null);
  setStreamRatio(16 / 9);
  return;
}
```
If a user stops streaming or disconnects without clearing `activeStreamId`, `App.tsx` detects that `isStreaming` is false and automatically resets `activeStreamId(null)` and `setStreamFullscreen(false)`.

---

## 5. Comparison: 1-on-1 Direct Calls vs Group Voice Channels

| Feature / Behavior | Group Voice Channel | 1-on-1 Direct Call | Blast Radius / Potential Risk |
| :--- | :--- | :--- | :--- |
| **Participant Source** | `store.voiceUsers` | `store.currentCallUser` (`voiceUsers` is empty) | **Critical Risk**: Any method checking only `store.voiceUsers` fails in 1-on-1 calls. `syncRemoteVideoStream` handles this correctly (`store.voiceUsers.find(...) \|\| (store.currentCallUser?.id === userId ? store.currentCallUser : null)`). Any future edits must maintain this fallback. |
| **Peer Connection Count** | Mesh ($N-1$ connections) | Exactly 1 connection | In 1-on-1 calls, renegotiation and stream setup occurs with only one peer. In group channels, renegotiation loops over all peers in `this.peerConnections`. |
| **Stream Disconnection** | `leaveChannel()` ➔ `webrtc.leaveAll()` | `endCall()` ➔ `webrtc.disconnectFromPeer(callUser.id)` | In group channels, `leaveAll()` tears down all peers and stops monitoring. In 1-on-1 calls, `CallEnded` tears down single peer, stops local stream if active, and resets call store. |
| **UI Layout & Cards** | Multi-card grid calculated by `getCardSize(activeUserCount, ...)` | 2-participant view (local user + remote user) | `activeUserCount` memo in `App.tsx:1118` explicitly differentiates `store.currentCallUser` from `store.voiceUsers`. |
| **Volume Adjustment** | `updateRemoteStreamVolume` iterated over `store.voiceUsers` | Iterated over `[store.currentCallUser]` | Handled in `App.tsx:1082`: `const relevantUsers = store.currentCallUser ? [store.currentCallUser] : store.voiceUsers`. |

---

## 6. Concurrency Risks & Edge Cases Audit

### 6.1 Muted Inbound Tracks Before First RTP Packet (Chromium W3C Behavior)
- **Problem**: When WebRTC establishes a remote transceiver, the browser initializes `receiver.track.muted = true`. Only when the first RTP packet is received does the browser trigger `unmute` and set `track.muted = false`.
- **Mitigation in Zabor**:
  - `syncRemoteVideoStream` checks `if (isUserStreaming || !track.muted)`.
  - If SignalR `UserStartedStreaming` arrived first, `isUserStreaming` is already true, so the track is immediately attached to `remoteVideoStreams`, mounting `<video>` without awaiting the first packet.
  - If SignalR is delayed, `track.onunmute` is hooked on both transceiver receiver and track, ensuring that as soon as the first RTP packet arrives, `syncRemoteVideoStream` runs and binds the track.

### 6.2 Signaling Glare & Renegotiation Collisions
- **Problem**: Broadcaster starts streaming, triggers offer; peer also triggers offer, causing `signalingState` collision (`have-local-offer`).
- **Mitigation in Zabor**:
  - `enqueueSignalingTask(userId, ...)` serializes signaling actions per peer connection.
  - `pendingRenegotiation` set tracks unfulfilled requests while state is non-stable.
  - `recoverRejectedAnswer` implements polite rollback up to `MAX_ROLLBACKS = 3`.
  - Epoch tags (`offerEpoch` and `answerEpoch`) discard stale answers from earlier renegotiation attempts.

### 6.3 Rapid Start / Stop / Restart Streaming Cycles
- **Problem**: Rapidly clicking screen share on and off can create overlapping `startScreenShare` and `stopScreenShare` invocations.
- **Mitigation in Zabor**:
  - At the beginning of `startScreenShare`, `if (this.localVideoStream) this.stopScreenShare()` guarantees teardown of prior stream before acquiring new desktop media constraints.
  - Senders call `replaceTrack(null)` rather than removing transceivers, preserving transceiver slots and avoiding transceiver index inflation in SDP.

### 6.4 Cleanup & Memory Leak Safeguards
- **Problem**: Accumulation of detached Web Audio nodes, dangling event listeners (`track.onunmute`, `track.onended`), or uncollected `<audio>` elements.
- **Verification**:
  - `cleanupRemoteStreamAudio` disconnects and nulls `streamSourceNodes`, `streamGainNodes`, `streamLimiterNodes`, and `streamDelayNodes`.
  - HTML5 `<audio>` elements are paused, `srcObject = null`, and deleted from `streamAudioElements`.
  - `track.onended` handlers invoke `cleanupRemoteStream(userId)`.
  - `disconnectFromPeer` cleans up `pc.ontrack = null`, `pc.onicecandidate = null`, and invokes `cleanupRemoteStreamAudio(userId)`.

---

## 7. Blast Radius Analysis

The blast radius of any modification to the WebRTC dynamic stream negotiation is summarized below:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                             BLAST RADIUS MAP                                │
├────────────────────────┬────────────────────────────────────────────────────┤
│ High Impact Area       │ • WebRTC Screen Video Playout (StreamCard.tsx)     │
│                        │ • Native Screen Share System Audio Playout         │
│                        │ • A/V Synchronization & Delay Compensation         │
│                        │ • Direct 1-on-1 Call Video Grid                    │
│                        │ • Group Channel Video Grid                         │
├────────────────────────┼────────────────────────────────────────────────────┤
│ Medium Impact Area     │ • Peer Renegotiation Queue (enqueueSignalingTask)  │
│                        │ • Voice Call Playout & Speaking Indicators (VAD)   │
│                        │ • Remote User Volume Sliders                       │
├────────────────────────┼────────────────────────────────────────────────────┤
│ Low / Zero Impact Area │ • Encrypted P2P Chat (chatPeer.ts / useChatStore)  │
│ (Fully Isolated)       │ • Settings, Calibration & Modals                   │
│                        │ • Desktop Updater Window & IPC                     │
│                        │ • TitleBar & Window Controls                       │
└────────────────────────┴────────────────────────────────────────────────────┘
```

---

## 8. Zero Breakage Compliance

1. **No Code Modification**: This investigation is strictly read-only. No source files have been altered.
2. **Comment Preservation**: No existing comments, compiler directives, or code structures have been deleted or modified.
3. **Type Checking Verification**: `npx tsc --noEmit` runs with 0 errors.
4. **Existing Test Suite**: `npm test` runs with 37/37 passing tests.
5. **Contract Adherence**: All contracts between SignalR events (`UserStartedStreaming`, `UserStoppedStreaming`, `StreamDropped`, `ReceiveWebRTCOffer`, `ReceiveWebRTCAnswer`) and WebRTC state machines are fully documented and preserved.

---

## 9. Recommendations for Downstream Multi-Agent Room

1. **For Architect (`architect`)**:
   - Maintain the dual-source user lookup pattern in any state helper: `store.voiceUsers.find(u => u.id === userId) || (store.currentCallUser?.id === userId ? store.currentCallUser : null)`.
   - Preserve transceiver reuse via `replaceTrack` and `direction = 'recvonly'` to avoid SDP m-line expansion.

2. **For QA Tester (`qa-tester`)**:
   - Implement automated unit/integration tests as outlined in `docs/qa-test-plan-webrtc-stream.md`:
     - Test 1: `syncRemoteVideoStream` with existing pre-allocated transceivers.
     - Test 2: `syncRemoteVideoStream` fallback when receivers arrive asynchronously.
     - Test 3: Muted track initialization and `onunmute` transition.
     - Test 4: Resource teardown on `cleanupRemoteStream` and `disconnectFromPeer`.
     - Test 5: 1-on-1 direct call stream initiation and teardown.

3. **For Performance & Security Auditors (`performance-analyzer`, `security-auditor`)**:
   - Confirm Web Audio nodes and `<audio>` elements are completely dereferenced upon stream teardown.
   - Verify no private user data or secrets are logged in WebRTC signaling diagnostics.
