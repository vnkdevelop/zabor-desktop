# Project: WebRTC Dynamic Stream Negotiation Audit & Test Expansion

## Architecture
- **WebRTC Core**: `src/renderer/src/services/webrtc.ts` — Mesh P2P engine managing `RTCPeerConnection` transceivers (`replaceTrack`), Web Audio API low-latency graph (`playback-buffer-processor`, `DelayNode`, `DynamicsCompressorNode`, `GainNode`), `syncRemoteVideoStream` dual-path track ingestion (`isUserStreaming || !track.muted`).
- **Signaling Layer**: `src/renderer/src/services/signalr.ts` — SignalR hub client managing streaming events (`UserStartedStreaming`, `UserStoppedStreaming`, `StreamDropped`, `Offer`, `Answer`, `IceCandidate`).
- **State Layer**: `src/renderer/src/stores/useAppStore.ts` — Zustand store managing `remoteVideoStreams`, `voiceUsers`, `currentCallUser`, `activeStreamId`, `streamQuality`.
- **UI Consumers**: `src/renderer/src/App.tsx`, `StreamCard.tsx`, `StreamFullscreenModal.tsx` — Rendering muted video element with audio playback routed strictly through Web Audio API.
- **Automated Test Infrastructure**: Node native `--test` harness in `tests/`, expanding from Tiers 1-4 to Tier 5 (`tests/tier5-webrtc-stream-lifecycle.test.mjs`) with mock helper (`tests/helpers/webrtc-stream-mock.mjs`).

## Feature Inventory
| # | Feature | Description | Milestone | Source | Status |
|---|---------|-------------|-----------|--------|--------|
| 1 | Dynamic Transceiver Re-negotiation | Dynamic screen sharing track binding/unbinding without tearing down peer connections | M1 (QA/Test) | Survey / webrtc.ts | VERIFIED |
| 2 | Immediate Stream Appearance | Receiver dual-path track ingestion (`isUserStreaming || !track.muted`) | M1 (QA/Test) | Survey / webrtc.ts | VERIFIED |
| 3 | Clean Stream Teardown | Full disposal of Web Audio nodes, audio elements, and Zustand video map | M1 (QA/Test) | Survey / webrtc.ts | VERIFIED |
| 4 | Renegotiation Collisions & Glare | W3C Perfect Negotiation with deterministic tie-breaking (`me > senderId`) and monotonic `offerEpoch` | M1 (QA/Test) | Survey / webrtc.ts | VERIFIED |
| 5 | Track Mute/Unmute Transitions | Handling Chromium's initial track muted state and asynchronous RTP packet arrival | M1 (QA/Test) | Survey / webrtc.ts | VERIFIED |
| 6 | 1-on-1 vs Group Channel Isolation | Dual participant lookup (`voiceUsers.find(...) || currentCallUser`) and teardown routing | M1 (QA/Test) | Survey / webrtc.ts | VERIFIED |
| 7 | Automated Test Harness Expansion | 24 automated unit/integration tests in `tier5-webrtc-stream-lifecycle.test.mjs` | M1 (QA/Test) | Survey / qa-test-plan | VERIFIED |
| 8 | Performance & Memory Leak Verification | Auditing listener cleanup (`onunmute`, `onended`), AudioNode disconnection, and memory | M2 (Audit) | Survey / webrtc-spec | VERIFIED |
| 9 | Security & Privacy Compliance | OWASP audit, secrets scan, no-telemetry compliance, Zero Breakage check | M2 (Audit) | Survey / AGENTS.md | VERIFIED |
| 10 | Multi-Agent Clearances | Collecting formal sign-offs (Analyzer, Architect, QA, Performance, Security, Reviewer, Supervisor) | M3 (Clearance) | Survey / AGENTS.md | VERIFIED |

## Milestones
| # | Name | Scope | Dependencies | Status | Output |
|---|------|-------|-------------|--------|--------|
| 0 | Survey & Architecture Specification | Codebase analysis, architectural spec, test plan design | none | DONE | `docs/codebase-analysis-webrtc-stream.md`, `docs/webrtc-stream-renegotiation-spec.md`, `docs/qa-test-plan-webrtc-stream.md` |
| 1 | Automated Test Suite Expansion (QA) | Implement mock harness and Tier 5 automated tests for stream lifecycles | M0 | DONE | `tests/helpers/webrtc-stream-mock.mjs`, `tests/tier5-webrtc-stream-lifecycle.test.mjs`, `package.json` |
| 2 | Performance, Security & Forensic Audit | Profiling memory/listener leaks, security scan, integrity forensic audit | M1 | DONE | `docs/performance-audit-webrtc-stream.md`, `docs/security-audit-webrtc-stream.md` |
| 3 | Reviewer & Supervisor Clearance | Zero-breakage review, typecheck, test verification, clearance collection | M2 | DONE | `docs/code-review-webrtc-stream.md`, `docs/supervisor-clearance-webrtc-stream.md` |

## Interface Contracts
### WebRTCManager ↔ Zustand Store
- `syncRemoteVideoStream(userId: string): void`
  - Lookup: `voiceUsers.find(u => u.id === userId) || (currentCallUser?.id === userId ? currentCallUser : null)`
  - Video stream update: `useAppStore.getState().setRemoteVideoStream(userId, stream)`
  - Cleanup: `useAppStore.getState().setRemoteVideoStream(userId, null)`
- `attachRemoteStreamAudio(userId: string, audioTrack: MediaStreamTrack): void`
  - Creates 4-node Web Audio graph: `streamSourceNodes`, `streamDelayNodes`, `streamLimiterNodes`, `streamGainNodes` connected to `outputBusGain`.
- `cleanupRemoteStream(userId: string): void`
  - Invokes `cleanupRemoteStreamAudio(userId)` and removes from `remoteVideoStreams`.

### SignalRService ↔ WebRTCManager
- Event `UserStartedStreaming(userId, quality)` ➔ `updateUserStatus` ➔ `webrtc.syncRemoteVideoStream(userId)`
- Event `UserStoppedStreaming(userId)` ➔ `updateUserStatus` ➔ `webrtc.cleanupRemoteStream(userId)`
- Event `StreamDropped(userId)` ➔ `webrtc.cleanupRemoteStream(userId)`

## Code Layout
- Test mocks: `tests/helpers/webrtc-stream-mock.mjs`
- Test suite: `tests/tier5-webrtc-stream-lifecycle.test.mjs`
- Test script runner: `package.json` ("test" script)
- Core services (read-only inspection): `src/renderer/src/services/webrtc.ts`, `signalr.ts`
- Documentation:
  - `docs/codebase-analysis-webrtc-stream.md`
  - `docs/webrtc-stream-renegotiation-spec.md`
  - `docs/qa-test-plan-webrtc-stream.md`
  - `docs/performance-audit-webrtc-stream.md`
  - `docs/security-audit-webrtc-stream.md`
  - `docs/code-review-webrtc-stream.md`
  - `docs/supervisor-clearance-webrtc-stream.md`
  - `docs/PROJECT.md`
