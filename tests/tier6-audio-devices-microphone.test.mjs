import { test, describe, beforeEach } from 'node:test';
import assert from 'node:assert/strict';
import {
  MockMediaDeviceInfo,
  MockMediaDevices,
  MockAudioElement,
  AudioDeviceEngine,
  classifyMicrophoneError,
  MIC_TEST_SILENCE_DBFS
} from './helpers/audio-mock.mjs';
import { createMockAppStore } from './helpers/webrtc-stream-mock.mjs';

describe('Tier 6: Audio Devices, Microphone Diagnostics & Audio Subsystem', () => {
  let store;
  let mediaDevices;
  let engine;

  const defaultDevices = [
    new MockMediaDeviceInfo({ deviceId: 'default', kind: 'audioinput', label: 'Default Microphone' }),
    new MockMediaDeviceInfo({ deviceId: 'communications', kind: 'audioinput', label: 'Communications Microphone' }),
    new MockMediaDeviceInfo({ deviceId: 'mic_usb_01', kind: 'audioinput', label: 'HyperX QuadCast Mic' }),
    new MockMediaDeviceInfo({ deviceId: 'mic_analog_02', kind: 'audioinput', label: 'Realtek High Definition Audio' }),
    new MockMediaDeviceInfo({ deviceId: 'default', kind: 'audiooutput', label: 'Default Speakers' }),
    new MockMediaDeviceInfo({ deviceId: 'communications', kind: 'audiooutput', label: 'Communications Speakers' }),
    new MockMediaDeviceInfo({ deviceId: 'spk_headphones_01', kind: 'audiooutput', label: 'SteelSeries Arctis Nova' }),
    new MockMediaDeviceInfo({ deviceId: 'cam_01', kind: 'videoinput', label: 'Logitech Brio 4K' })
  ];

  beforeEach(() => {
    store = createMockAppStore();
    mediaDevices = new MockMediaDevices(defaultDevices);
    engine = new AudioDeviceEngine(store, mediaDevices);
  });

  describe('Suite 1: Audio Device Enumeration, Categorization & Filtering', () => {
    test('S1-01: separates microphones (audioinput) and output devices (audiooutput)', async () => {
      const devices = await mediaDevices.enumerateDevices();
      const inputs = devices.filter(d => d.kind === 'audioinput');
      const outputs = devices.filter(d => d.kind === 'audiooutput');

      assert.equal(inputs.length, 4);
      assert.equal(outputs.length, 3);
      assert.ok(inputs.every(d => d.kind === 'audioinput'));
      assert.ok(outputs.every(d => d.kind === 'audiooutput'));
    });

    test('S1-02: identifies default and communications virtual devices', async () => {
      const devices = await mediaDevices.enumerateDevices();
      const defaultMic = devices.find(d => d.kind === 'audioinput' && d.deviceId === 'default');
      const commsMic = devices.find(d => d.kind === 'audioinput' && d.deviceId === 'communications');

      assert.ok(defaultMic);
      assert.ok(commsMic);
      assert.equal(defaultMic.label, 'Default Microphone');
      assert.equal(commsMic.label, 'Communications Microphone');
    });

    test('S1-03: handles empty device topology gracefully in headless / CI environments', async () => {
      mediaDevices.setDevices([]);
      const devices = await mediaDevices.enumerateDevices();
      assert.equal(devices.length, 0);

      const res = await engine.handleAudioDeviceChange();
      assert.equal(res.inputCount, 0);
      assert.equal(res.outputCount, 0);
    });

    test('S1-04: persists device IDs and friendly device labels across multiple enumerations', async () => {
      const first = await mediaDevices.enumerateDevices();
      const second = await mediaDevices.enumerateDevices();

      assert.equal(first.length, second.length);
      assert.equal(first[2].deviceId, second[2].deviceId);
      assert.equal(first[2].label, second[2].label);
    });
  });

  describe('Suite 2: Microphone Capture Constraints & Fallback Hierarchy', () => {
    test('S2-01: formats exact deviceId constraint when user selects non-default microphone', () => {
      const constraints = engine.buildMicConstraints('mic_usb_01', true);
      assert.deepEqual(constraints.deviceId, { exact: 'mic_usb_01' });
      assert.equal(constraints.channelCount, 1);
      assert.deepEqual(constraints.sampleRate, { ideal: 48000 });
    });

    test('S2-02: applies 48kHz mono sample rate and disables browser AGC to preserve dynamic range', () => {
      const constraints = engine.buildMicConstraints('default', true);
      assert.equal(constraints.autoGainControl, false);
      assert.equal(constraints.googAutoGainControl, false);
      assert.equal(constraints.channelCount, 1);
    });

    test('S2-03: toggles browser echo cancellation according to audio configuration', () => {
      const withAEC = engine.buildMicConstraints('default', true);
      const withoutAEC = engine.buildMicConstraints('default', false);

      assert.equal(withAEC.echoCancellation, true);
      assert.equal(withoutAEC.echoCancellation, false);
    });

    test('S2-04: falls back to system default mic when preferred deviceId is unavailable', async () => {
      engine.currentDeviceId = 'missing_mic_99';
      const stream = await engine.captureRawMicStream();

      assert.ok(stream);
      assert.equal(engine.currentDeviceId, 'default');
      const track = stream.getAudioTracks()[0];
      assert.equal(track.readyState, 'live');
    });

    test('S2-05: falls back to first available input device when default is absent', async () => {
      mediaDevices.setDevices([
        new MockMediaDeviceInfo({ deviceId: 'custom_mic_only', kind: 'audioinput', label: 'USB Mic Only' })
      ]);

      engine.currentDeviceId = 'some_absent_id';
      const stream = await engine.captureRawMicStream();

      assert.ok(stream);
      const track = stream.getAudioTracks()[0];
      assert.equal(track.getSettings().deviceId, 'custom_mic_only');
    });

    test('S2-06: creates silent fallback audio stream when all physical capture attempts fail', async () => {
      mediaDevices.setDevices([]);
      const started = await engine.startBackgroundMic();

      assert.equal(started, false);
      assert.ok(engine.rawStream);
      assert.equal(engine.rawStreamIsFallback, true);
      assert.equal(engine.rawStream.getAudioTracks().length, 1);
    });
  });

  describe('Suite 3: Audio Output Routing (setSinkId) & Speaker Selection', () => {
    test('S3-01: routes registered HTML audio elements to selected output device via setSinkId', async () => {
      const audioElem = new MockAudioElement();
      let sinkIdAssigned = null;
      audioElem.setSinkId = async (id) => { sinkIdAssigned = id; };

      engine.registerAudioElement(audioElem);
      await engine.setOutputDevice('spk_headphones_01');

      assert.equal(engine.currentOutputDeviceId, 'spk_headphones_01');
      assert.equal(sinkIdAssigned, 'spk_headphones_01');
    });

    test('S3-02: gracefully handles audio output elements when setSinkId throws', async () => {
      const audioElem = new MockAudioElement();
      audioElem.setSinkId = async () => { throw new Error('Sink not found'); };

      engine.registerAudioElement(audioElem);
      await assert.doesNotReject(async () => {
        await engine.setOutputDevice('invalid_speaker');
      });
    });

    test('S3-03: resets to default output device when switching to default', async () => {
      const audioElem = new MockAudioElement();
      let sinkIdAssigned = null;
      audioElem.setSinkId = async (id) => { sinkIdAssigned = id; };

      engine.registerAudioElement(audioElem);
      await engine.setOutputDevice('spk_headphones_01');
      assert.equal(sinkIdAssigned, 'spk_headphones_01');

      await engine.setOutputDevice('default');
      assert.equal(engine.currentOutputDeviceId, 'default');
      assert.equal(sinkIdAssigned, 'default');
    });
  });

  describe('Suite 4: Audio Device Hotplug & Dynamic Reconciliation (handleAudioDeviceChange)', () => {
    test('S4-01: detects removal of active microphone and automatically falls back to default', async () => {
      await engine.startBackgroundMic('mic_usb_01');
      assert.equal(engine.currentDeviceId, 'mic_usb_01');

      mediaDevices.removeDevice('mic_usb_01');
      const changeResult = await engine.handleAudioDeviceChange();

      assert.equal(changeResult.inputSwitched, true);
      assert.equal(engine.currentDeviceId, 'default');
    });

    test('S4-02: detects removal of active headphones/speakers and resets output to default', async () => {
      await engine.setOutputDevice('spk_headphones_01');
      assert.equal(engine.currentOutputDeviceId, 'spk_headphones_01');

      mediaDevices.removeDevice('spk_headphones_01');
      const changeResult = await engine.handleAudioDeviceChange();

      assert.equal(changeResult.outputSwitched, true);
      assert.equal(engine.currentOutputDeviceId, 'default');
    });

    test('S4-03: preserves current device selection when unrelated devices are added or removed', async () => {
      await engine.startBackgroundMic('mic_analog_02');
      await engine.setOutputDevice('spk_headphones_01');

      mediaDevices.addDevice(new MockMediaDeviceInfo({
        deviceId: 'new_mic_10',
        kind: 'audioinput',
        label: 'Blue Yeti'
      }));

      const changeResult = await engine.handleAudioDeviceChange();
      assert.equal(changeResult.inputSwitched, false);
      assert.equal(changeResult.outputSwitched, false);
      assert.equal(engine.currentDeviceId, 'mic_analog_02');
      assert.equal(engine.currentOutputDeviceId, 'spk_headphones_01');
    });
  });

  describe('Suite 5: Microphone Self-Test Pipeline (prepareMicTest, recordMicTest & Playback)', () => {
    test('S5-01: prepares mic test by prewarming background stream and audio worklet', async () => {
      await engine.prepareMicTest();
      assert.ok(engine.rawStream);
      assert.equal(engine.processedContext.state, 'running');
    });

    test('S5-02: records PCM samples and calculates duration and peak dBFS', async () => {
      let armedFired = false;
      const clip = await engine.recordMicTest(() => { armedFired = true; }, 1000);

      assert.equal(armedFired, true);
      assert.ok(clip.durationSeconds > 0);
      assert.ok(clip.peakDb > -100);
      assert.ok(clip.peakDb <= 0);
      assert.ok(clip.peakDb > MIC_TEST_SILENCE_DBFS);
      assert.equal(engine.getMicTestDuration(), clip.durationSeconds);
    });

    test('S5-03: accurately flags microphone silence below MIC_TEST_SILENCE_DBFS (-50 dBFS)', async () => {
      engine.processedContext.__mockSilence = true;
      const clip = await engine.recordMicTest(undefined, 1000);

      assert.ok(clip.peakDb <= MIC_TEST_SILENCE_DBFS, `Peak dB ${clip.peakDb} should be below silence threshold ${MIC_TEST_SILENCE_DBFS}`);
    });

    test('S5-04: aborts recording cleanly when cancelled mid-run without hanging', async () => {
      const recordPromise = engine.recordMicTest(undefined, 2000);
      engine.disposeMicTest();

      await assert.rejects(async () => {
        await recordPromise;
      }, /MIC_TEST_CANCELLED/);
    });

    test('S5-05: provides playback controls (play, pause, seek, onEnded) for recorded clip', async () => {
      engine.processedContext.__mockSilence = false;
      await engine.recordMicTest(undefined, 500);

      engine.playMicTest();
      assert.ok(engine.micTestSource);
      assert.equal(engine.micTestSource.isPlaying, true);

      engine.pauseMicTest();
      assert.equal(engine.micTestSource, null);

      engine.seekMicTest(0.25);
      assert.equal(engine.micTestOffset, 0.25);

      let endedFired = false;
      engine.onMicTestEnded(() => { endedFired = true; });
      engine.playMicTest();
      engine.micTestSource.stop();
      assert.equal(endedFired, true);
    });

    test('S5-06: disposes mic test buffer and resets playback offset cleanly', async () => {
      await engine.recordMicTest(undefined, 500);
      assert.ok(engine.getMicTestDuration() > 0);

      engine.disposeMicTest();
      assert.equal(engine.getMicTestDuration(), 0);
      assert.equal(engine.micTestBuffer, null);
      assert.equal(engine.micTestOffset, 0);
    });
  });

  describe('Suite 6: Noise Suppression & VAD Pipeline Configuration', () => {
    test('S6-01: switches between DeepFilterNet3, RNNoise, Gate and Bypass modes', () => {
      engine.setNoiseSuppressionMode('deepfilter');
      assert.equal(engine.noiseSuppressionMode, 'deepfilter');

      engine.setNoiseSuppressionMode('rnnoise');
      assert.equal(engine.noiseSuppressionMode, 'rnnoise');

      engine.setNoiseSuppressionMode('none');
      assert.equal(engine.noiseSuppressionMode, 'none');
    });

    test('S6-02: validates VAD sensitivity threshold clamping [0, 1]', () => {
      engine.setVadThreshold(0.75);
      assert.equal(engine.vadThreshold, 0.75);

      engine.setVadThreshold(1.5);
      assert.equal(engine.vadThreshold, 1.0);

      engine.setVadThreshold(-0.5);
      assert.equal(engine.vadThreshold, 0.0);
    });

    test('S6-03: bypasses browser WebRTC noise suppression when DSP engines are active', () => {
      engine.noiseSuppression = false;
      const constraints = engine.buildMicConstraints('default', true);
      assert.equal(constraints.noiseSuppression, false);
      assert.equal(constraints.googNoiseSuppression, false);
    });
  });

  describe('Suite 7: Microphone Error Classification & User Feedback', () => {
    test('S7-01: classifies permission denial into micNoAccess', () => {
      assert.equal(classifyMicrophoneError('NotAllowedError: Permission denied'), 'micNoAccess');
      assert.equal(classifyMicrophoneError('PermissionDeniedError'), 'micNoAccess');
      assert.equal(classifyMicrophoneError('SecurityError: blocked'), 'micNoAccess');
      assert.equal(classifyMicrophoneError('MIC_ACCESS_FAILED: rejected'), 'micNoAccess');
    });

    test('S7-02: classifies device lock / track start errors into micBusy', () => {
      assert.equal(classifyMicrophoneError('NotReadableError: Could not start audio source'), 'micBusy');
      assert.equal(classifyMicrophoneError('TrackStartError: Failed to allocate device'), 'micBusy');
      assert.equal(classifyMicrophoneError('AbortError: audio unit busy'), 'micBusy');
    });

    test('S7-03: classifies missing device / overconstrained errors into micNotFound', () => {
      assert.equal(classifyMicrophoneError('NotFoundError: Requested device not found'), 'micNotFound');
      assert.equal(classifyMicrophoneError('DevicesNotFoundError'), 'micNotFound');
      assert.equal(classifyMicrophoneError('OverconstrainedError: constraint deviceId'), 'micNotFound');
    });

    test('S7-04: logs and dispatches system toast notification without throwing unhandled exceptions', async () => {
      mediaDevices.permissionState = 'denied';
      const started = await engine.startBackgroundMic();

      assert.equal(started, false);
      assert.equal(engine.systemToasts.length, 1);
      assert.equal(engine.systemToasts[0].kind, 'micNoAccess');
    });
  });

  describe('Suite 8: Microphone Mute & Monitor-While-Muted Isolation', () => {
    test('S8-01: muting microphone disables audio track for peer transmission', async () => {
      await engine.startBackgroundMic();
      assert.equal(engine.rawStream.getAudioTracks()[0].enabled, true);

      engine.setMicrophoneMuted(true);
      assert.equal(engine.rawStream.getAudioTracks()[0].enabled, false);

      engine.setMicrophoneMuted(false);
      assert.equal(engine.rawStream.getAudioTracks()[0].enabled, true);
    });

    test('S8-02: monitor-while-muted maintains local audio feed for level meter and test panel', async () => {
      await engine.startBackgroundMic();
      engine.setMicrophoneMuted(true);
      assert.equal(engine.rawStream.getAudioTracks()[0].enabled, false);

      engine.setMonitorWhileMuted(true);
      assert.equal(engine.rawStream.getAudioTracks()[0].enabled, true);

      engine.setMonitorWhileMuted(false);
      assert.equal(engine.rawStream.getAudioTracks()[0].enabled, false);
    });

    test('S8-03: unmuting restores audio track state immediately', async () => {
      await engine.startBackgroundMic();
      engine.setMicrophoneMuted(true);
      engine.setMonitorWhileMuted(false);
      assert.equal(engine.rawStream.getAudioTracks()[0].enabled, false);

      engine.setMicrophoneMuted(false);
      assert.equal(engine.rawStream.getAudioTracks()[0].enabled, true);
    });
  });
});
