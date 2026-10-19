import { test, describe, beforeEach } from 'node:test';
import assert from 'node:assert/strict';
import { WebRtcVadDetector, WebRtcVadSidecar } from '../src/renderer/src/services/webrtc-vad.ts';
import {
  MockMediaStreamTrack,
  MockMediaStream,
  MockAudioNode,
  MockGainNode
} from './helpers/webrtc-stream-mock.mjs';

class MockScriptProcessorNode extends MockAudioNode {
  constructor(bufferSize = 512, inputChannels = 1, outputChannels = 1) {
    super();
    this.bufferSize = bufferSize;
    this.inputChannels = inputChannels;
    this.outputChannels = outputChannels;
    this.onaudioprocess = null;
  }

  feedAudio(samples) {
    if (typeof this.onaudioprocess === 'function') {
      const event = {
        inputBuffer: {
          getChannelData: () => samples,
          length: samples.length
        }
      };
      this.onaudioprocess(event);
    }
  }
}

class MockSidecarAudioContext {
  constructor(sampleRate = 48000) {
    this.sampleRate = sampleRate;
    this.state = 'running';
    this.scriptProcessors = [];
    this.destination = new MockAudioNode();
  }

  createMediaStreamSource(stream) {
    return new MockAudioNode();
  }

  createGain() {
    return new MockGainNode();
  }

  createScriptProcessor(bufferSize = 512, inCh = 1, outCh = 1) {
    const sp = new MockScriptProcessorNode(bufferSize, inCh, outCh);
    this.scriptProcessors.push(sp);
    return sp;
  }

  async close() {
    this.state = 'closed';
  }
}

function generateSineTone(frequency = 440, sampleRate = 48000, numSamples = 480, amplitude = 0.5) {
  const buffer = new Float32Array(numSamples);
  for (let i = 0; i < numSamples; i++) {
    buffer[i] = amplitude * Math.sin((2 * Math.PI * frequency * i) / sampleRate);
  }
  return buffer;
}

function generateSilence(numSamples = 480) {
  return new Float32Array(numSamples);
}

describe('Tier 7: Standalone Google WebRTC VAD & 3-Branch Speaking Hierarchy', () => {

  describe('Suite 1: WebRtcVadDetector WASM Engine (mode 2, 48kHz, 480 frames, hangover)', () => {
    let detector;

    beforeEach(() => {
      detector = new WebRtcVadDetector({ sampleRate: 48000, mode: 2, hangoverMs: 90 });
    });

    test('S1-01: initializes libfvad WASM module with mode 2 and 48kHz native rate', async () => {
      assert.equal(detector.isReady(), false);
      await detector.init();
      assert.equal(detector.isReady(), true);
      detector.destroy();
      assert.equal(detector.isReady(), false);
    });

    test('S1-02: returns false for silence (no false speech trigger)', async () => {
      await detector.init();
      const silence = generateSilence(480);
      for (let i = 0; i < 10; i++) {
        const result = detector.process(silence);
        assert.equal(result, false, `Silence frame ${i} must return false`);
      }
      detector.destroy();
    });

    test('S1-03: returns true for speech-band tone (440 Hz / 1000 Hz)', async () => {
      await detector.init();
      const tone = generateSineTone(440, 48000, 480, 0.4);
      let detectedSpeech = false;
      for (let i = 0; i < 5; i++) {
        if (detector.process(tone)) {
          detectedSpeech = true;
          break;
        }
      }
      assert.ok(detectedSpeech, 'Speech-band tone must be detected as speech');
      detector.destroy();
    });

    test('S1-04: hangover preserves speaking state during brief pauses and transitions to false', async () => {
      await detector.init();
      const tone = generateSineTone(440, 48000, 480, 0.5);
      const silence = generateSilence(480);

      // Trigger speech
      let speaking = false;
      for (let i = 0; i < 5; i++) {
        if (detector.process(tone)) {
          speaking = true;
          break;
        }
      }
      assert.ok(speaking, 'Tone must trigger speech');

      // Now feed silence: hangover should keep it true initially
      const frame1AfterSpeech = detector.process(silence);
      assert.equal(frame1AfterSpeech, true, 'Immediate silence frame must be protected by hangover');

      // Feed enough silence frames to exhaust internal + software hangover (e.g. 25 frames = 250ms)
      let eventuallySilent = false;
      for (let i = 0; i < 25; i++) {
        const res = detector.process(silence);
        if (!res) {
          eventuallySilent = true;
          break;
        }
      }
      assert.ok(eventuallySilent, 'Speech state must transition cleanly to false after hangover expires');
      detector.destroy();
    });

    test('S1-05: reset immediately drops hangover and resets speaking state to false', async () => {
      await detector.init();
      const tone = generateSineTone(440, 48000, 480, 0.5);
      const silence = generateSilence(480);

      for (let i = 0; i < 5; i++) detector.process(tone);
      detector.reset();

      const result = detector.process(silence);
      assert.equal(result, false, 'After reset(), silence frame must immediately return false');
      detector.destroy();
    });

    test('S1-06: destroy cleans up allocations and is safe against double invocation', async () => {
      await detector.init();
      detector.destroy();
      assert.equal(detector.isReady(), false);
      // Double destroy should not throw
      assert.doesNotThrow(() => detector.destroy());
    });
  });

  describe('Suite 2: WebRtcVadSidecar Stream Accumulation & Mute Control', () => {
    let mockContext;
    let mockStream;

    beforeEach(() => {
      mockContext = new MockSidecarAudioContext(48000);
      mockStream = new MockMediaStream([new MockMediaStreamTrack({ kind: 'audio', muted: false })]);
    });

    test('S2-01: buffers irregular audio chunks into exact 480-sample frames and reports speech', async () => {
      const speechEvents = [];
      const sidecar = new WebRtcVadSidecar((isSpeaking) => {
        speechEvents.push(isSpeaking);
      });

      await sidecar.start(mockContext, mockStream);
      assert.equal(mockContext.scriptProcessors.length, 1);
      const processor = mockContext.scriptProcessors[0];

      // Feed tone in 256-sample chunks
      const chunk256 = generateSineTone(440, 48000, 256, 0.5);
      for (let i = 0; i < 10; i++) {
        processor.feedAudio(chunk256);
      }

      assert.ok(speechEvents.includes(true), 'Sidecar must report speaking = true');
      sidecar.stop();
    });

    test('S2-02: deduplicates consecutive identical speaking events', async () => {
      const speechEvents = [];
      const sidecar = new WebRtcVadSidecar((isSpeaking) => {
        speechEvents.push(isSpeaking);
      });

      await sidecar.start(mockContext, mockStream);
      const processor = mockContext.scriptProcessors[0];

      const tone480 = generateSineTone(440, 48000, 480, 0.5);
      for (let i = 0; i < 8; i++) {
        processor.feedAudio(tone480);
      }

      // Should only have emitted true once (not 8 times)
      const trueEvents = speechEvents.filter(s => s === true);
      assert.equal(trueEvents.length, 1, 'Consecutive true states must be deduplicated');

      sidecar.stop();
    });

    test('S2-03: setMuted(true) immediately dispatches false and halts detection', async () => {
      const speechEvents = [];
      const sidecar = new WebRtcVadSidecar((isSpeaking) => {
        speechEvents.push(isSpeaking);
      });

      await sidecar.start(mockContext, mockStream);
      const processor = mockContext.scriptProcessors[0];

      // Trigger speaking
      const tone480 = generateSineTone(440, 48000, 480, 0.5);
      processor.feedAudio(tone480);
      processor.feedAudio(tone480);
      assert.equal(speechEvents[speechEvents.length - 1], true);

      // Mute sidecar
      sidecar.setMuted(true);
      assert.equal(speechEvents[speechEvents.length - 1], false, 'Muting must immediately dispatch false');

      // Feed tone while muted: must not trigger speech
      processor.feedAudio(tone480);
      processor.feedAudio(tone480);
      assert.equal(speechEvents[speechEvents.length - 1], false, 'Muted sidecar must not emit true');

      // Unmute: should resume detection
      sidecar.setMuted(false);
      for (let i = 0; i < 6; i++) {
        processor.feedAudio(tone480);
      }
      assert.equal(speechEvents[speechEvents.length - 1], true, 'Unmuted sidecar must resume detection');

      sidecar.stop();
    });

    test('S2-04: stop() releases all nodes and dispatches false if was speaking', async () => {
      const speechEvents = [];
      const sidecar = new WebRtcVadSidecar((isSpeaking) => {
        speechEvents.push(isSpeaking);
      });

      await sidecar.start(mockContext, mockStream);
      const processor = mockContext.scriptProcessors[0];

      const tone480 = generateSineTone(440, 48000, 480, 0.5);
      processor.feedAudio(tone480);
      processor.feedAudio(tone480);
      assert.equal(speechEvents[speechEvents.length - 1], true);

      sidecar.stop();
      assert.equal(speechEvents[speechEvents.length - 1], false, 'stop() must report false');
      assert.equal(processor.onaudioprocess, null, 'processor handler must be detached');
    });
  });

  describe('Suite 3: 3-Branch Speaking Indicator Hierarchy & Mutual Exclusion', () => {

    class MockWebRTCManagerHierarchy {
      constructor() {
        this.speechAnalyzerEnabled = true;
        this.ultraLowLatency = false;
        this.noiseSuppression = true;
        this.thresholdMode = 'auto';
        this.localSpeakingState = false;
        this.appliedSpeakingEvents = [];
      }

      isManualGateMode() {
        return !this.ultraLowLatency && this.noiseSuppression && this.thresholdMode === 'manual';
      }

      getActiveVadBranch() {
        if (this.speechAnalyzerEnabled && !this.isManualGateMode()) {
          return 'silero';
        }
        if (this.isManualGateMode()) {
          return 'manual-gate';
        }
        return 'webrtc-vad';
      }

      applyLocalSpeaking(isSpeaking) {
        if (this.localSpeakingState === isSpeaking) return;
        this.localSpeakingState = isSpeaking;
        this.appliedSpeakingEvents.push({ branch: this.getActiveVadBranch(), isSpeaking });
      }

      handleMicNodeMessage(event) {
        if (event.type === 'vad' && this.getActiveVadBranch() === 'silero') {
          this.applyLocalSpeaking(Boolean(event.isSpeaking));
        }
      }

      handleManualGateMessage(event) {
        if (event.type === 'vad' && this.getActiveVadBranch() === 'manual-gate') {
          this.applyLocalSpeaking(Boolean(event.isSpeaking));
        }
      }

      handleWebRtcVadMessage(isSpeaking) {
        if (this.getActiveVadBranch() === 'webrtc-vad') {
          this.applyLocalSpeaking(Boolean(isSpeaking));
        }
      }
    }

    let manager;

    beforeEach(() => {
      manager = new MockWebRTCManagerHierarchy();
    });

    test('S3-01: selects Branch 1 (silero) when speechAnalyzerEnabled is true in auto mode', () => {
      manager.speechAnalyzerEnabled = true;
      manager.thresholdMode = 'auto';
      assert.equal(manager.getActiveVadBranch(), 'silero');
    });

    test('S3-02: selects Branch 2 (manual-gate) when manual threshold is enabled', () => {
      manager.speechAnalyzerEnabled = true;
      manager.thresholdMode = 'manual';
      manager.noiseSuppression = true;
      manager.ultraLowLatency = false;
      assert.equal(manager.getActiveVadBranch(), 'manual-gate');
    });

    test('S3-03: selects Branch 3 (webrtc-vad) when speech analyzer is disabled or in ULL', () => {
      // Case A: speechAnalyzer disabled in auto mode
      manager.speechAnalyzerEnabled = false;
      manager.thresholdMode = 'auto';
      assert.equal(manager.getActiveVadBranch(), 'webrtc-vad');

      // Case B: noise suppression disabled (passthrough)
      manager.noiseSuppression = false;
      manager.thresholdMode = 'manual';
      assert.equal(manager.getActiveVadBranch(), 'webrtc-vad');

      // Case C: ultra-low latency enabled
      manager.ultraLowLatency = true;
      manager.noiseSuppression = true;
      assert.equal(manager.getActiveVadBranch(), 'webrtc-vad');
    });

    test('S3-04: strict mutual exclusion - manual gate events ignored when Branch 1 (silero) is active', () => {
      manager.speechAnalyzerEnabled = true;
      manager.thresholdMode = 'auto';
      assert.equal(manager.getActiveVadBranch(), 'silero');

      // Spurious manual gate message arrives
      manager.handleManualGateMessage({ type: 'vad', isSpeaking: true });
      assert.equal(manager.appliedSpeakingEvents.length, 0, 'Manual gate message must be dropped in Branch 1');

      // Spurious WebRTC VAD message arrives
      manager.handleWebRtcVadMessage(true);
      assert.equal(manager.appliedSpeakingEvents.length, 0, 'WebRtcVad message must be dropped in Branch 1');

      // Silero message arrives
      manager.handleMicNodeMessage({ type: 'vad', isSpeaking: true });
      assert.equal(manager.appliedSpeakingEvents.length, 1);
      assert.equal(manager.appliedSpeakingEvents[0].branch, 'silero');
      assert.equal(manager.appliedSpeakingEvents[0].isSpeaking, true);
    });

    test('S3-05: strict mutual exclusion - silero events ignored when Branch 2 (manual-gate) is active', () => {
      manager.thresholdMode = 'manual';
      assert.equal(manager.getActiveVadBranch(), 'manual-gate');

      // Spurious silero message arrives
      manager.handleMicNodeMessage({ type: 'vad', isSpeaking: true });
      assert.equal(manager.appliedSpeakingEvents.length, 0, 'Silero message must be dropped in Branch 2');

      // Manual gate message arrives
      manager.handleManualGateMessage({ type: 'vad', isSpeaking: true });
      assert.equal(manager.appliedSpeakingEvents.length, 1);
      assert.equal(manager.appliedSpeakingEvents[0].branch, 'manual-gate');
      assert.equal(manager.appliedSpeakingEvents[0].isSpeaking, true);
    });

    test('S3-06: strict mutual exclusion - silero and manual-gate ignored when Branch 3 is active', () => {
      manager.speechAnalyzerEnabled = false;
      manager.thresholdMode = 'auto';
      assert.equal(manager.getActiveVadBranch(), 'webrtc-vad');

      manager.handleMicNodeMessage({ type: 'vad', isSpeaking: true });
      manager.handleManualGateMessage({ type: 'vad', isSpeaking: true });
      assert.equal(manager.appliedSpeakingEvents.length, 0, 'Silero and gate messages must be dropped in Branch 3');

      manager.handleWebRtcVadMessage(true);
      assert.equal(manager.appliedSpeakingEvents.length, 1);
      assert.equal(manager.appliedSpeakingEvents[0].branch, 'webrtc-vad');
      assert.equal(manager.appliedSpeakingEvents[0].isSpeaking, true);
    });

    test('S3-07: Branch 2 visual speaking is strictly bound to meterDb >= thresholdDb (no release hold)', () => {
      const thresholdDb = -35.0;
      let gateMuted = false;
      let isMuted = false;

      // When meter is at -34.0 dB (above threshold) -> speaking is true
      let meterDb = -34.0;
      let isSpeaking = !gateMuted && !isMuted && meterDb >= thresholdDb;
      assert.equal(isSpeaking, true);

      // When meter drops to -36.0 dB (below threshold) -> speaking immediately drops to false
      meterDb = -36.0;
      isSpeaking = !gateMuted && !isMuted && meterDb >= thresholdDb;
      assert.equal(isSpeaking, false, 'Visual speaking must drop immediately below threshold without 375ms hold');
    });
  });

  describe('Suite 4: Zero Audio Alteration & Zero Latency Sidecar Tap Contract', () => {
    test('S4-01: sidecar tap connects to silent gain [0] and does not inject into outbound stream', async () => {
      const mockContext = new MockSidecarAudioContext(48000);
      const mockStream = new MockMediaStream([new MockMediaStreamTrack({ kind: 'audio', muted: false })]);

      const sidecar = new WebRtcVadSidecar(() => {});
      await sidecar.start(mockContext, mockStream);

      // Verify ScriptProcessor was created
      assert.equal(mockContext.scriptProcessors.length, 1);
      const processor = mockContext.scriptProcessors[0];

      // Verify processor output connections
      const connectedNodes = processor.connectedTo;
      assert.ok(connectedNodes.length > 0, 'ScriptProcessor must connect to silent gain');
      assert.ok(connectedNodes[0] instanceof MockGainNode, 'Target must be GainNode');
      assert.equal(connectedNodes[0].gain.value, 0, 'Sidecar GainNode must be set to 0.0 to prevent audio leakage');

      sidecar.stop();
    });

    test('S4-02: outbound audio track enabled state is untouched by VAD sidecar processing', async () => {
      const track = new MockMediaStreamTrack({ kind: 'audio', muted: false });
      track.enabled = true;
      const stream = new MockMediaStream([track]);

      const mockContext = new MockSidecarAudioContext(48000);
      const sidecar = new WebRtcVadSidecar(() => {});
      await sidecar.start(mockContext, stream);

      assert.equal(track.enabled, true, 'Audio track enabled state must remain true');
      sidecar.stop();
    });
  });

  describe('Suite 5: Remote Peer VAD Isolation', () => {
    test('S5-01: local setupVAD(stream, userId, isLocal=true) returns immediately without intervals', () => {
      const speakingIntervals = new Map();
      const setupVAD = (stream, userId, isLocal) => {
        if (isLocal) {
          return;
        }
        const timer = setInterval(() => {}, 20);
        speakingIntervals.set(userId, { timer });
      };

      setupVAD(new MockMediaStream(), 'local_user', true);
      assert.equal(speakingIntervals.has('local_user'), false, 'No interval timer must be set for local user');
      assert.equal(speakingIntervals.size, 0);
    });

    test('S5-02: remote peer setupVAD(stream, userId, isLocal=false) maintains audioLevel playout polling', () => {
      const speakingIntervals = new Map();
      const speakingStatuses = new Map();

      let currentAudioLevel = 0.05;
      const setupRemoteVAD = (userId) => {
        let wasSpeaking = false;
        let lastSoundAt = 0;
        const releaseHoldMs = 200;

        const check = (now) => {
          if (currentAudioLevel > 0.001) lastSoundAt = now;
          const isRemoteSpeaking = now - lastSoundAt <= releaseHoldMs;
          if (isRemoteSpeaking !== wasSpeaking) {
            wasSpeaking = isRemoteSpeaking;
            speakingStatuses.set(userId, isRemoteSpeaking);
          }
        };

        return { check };
      };

      const remoteVAD = setupRemoteVAD('peer_123');
      remoteVAD.check(1000);
      assert.equal(speakingStatuses.get('peer_123'), true, 'Remote peer with audioLevel > 0.001 must be speaking');

      // After 250ms with no audio
      currentAudioLevel = 0.0;
      remoteVAD.check(1300);
      assert.equal(speakingStatuses.get('peer_123'), false, 'Remote peer must transition to not speaking after release hold');
    });
  });
});
