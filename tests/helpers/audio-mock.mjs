import { EventEmitter } from 'node:events';
import {
  MockMediaStreamTrack,
  MockMediaStream,
  MockAudioNode,
  MockGainNode,
  MockAudioContext as BaseMockAudioContext,
  MockAudioElement
} from './webrtc-stream-mock.mjs';

export { MockAudioElement };

export const MIC_TEST_DURATION_MS = 5000;
export const MIC_TEST_SILENCE_DBFS = -50;
export const MIC_CAPTURE_TIMEOUT_MS = 5000;
export const MIC_TEST_MONITOR_SETTLE_MS = 10;
export const MIC_TEST_PIPELINE_FLUSH_MS = 10;
export const MIC_TEST_CAPTURE_GRACE_MS = 800;
export const ULTRA_MIC_LATENCY_HINT_S = 0.005;

export class MockMediaDeviceInfo {
  constructor({ deviceId = '', kind = 'audioinput', label = '', groupId = 'default-group' } = {}) {
    this.deviceId = deviceId;
    this.kind = kind;
    this.label = label;
    this.groupId = groupId;
  }
}

export class MockAudioBuffer {
  constructor({ numberOfChannels = 1, length = 48000, sampleRate = 48000 } = {}) {
    this.numberOfChannels = numberOfChannels;
    this.length = length;
    this.sampleRate = sampleRate;
    this.duration = length / sampleRate;
    this._channels = [];
    for (let i = 0; i < numberOfChannels; i++) {
      this._channels.push(new Float32Array(length));
    }
  }

  copyToChannel(source, channelNumber) {
    if (channelNumber >= 0 && channelNumber < this._channels.length) {
      this._channels[channelNumber].set(source.subarray(0, this.length));
    }
  }

  getChannelData(channelNumber) {
    return this._channels[channelNumber] || new Float32Array(this.length);
  }
}

export class MockAudioWorkletNode extends MockAudioNode {
  constructor(context, name, options = {}) {
    super();
    this.context = context;
    this.name = name;
    this.options = options;
    this.onprocessorerror = null;
    this.port = {
      onmessage: null,
      postMessage: (msg) => {
        if (msg?.type === 'start') {
          const sampleCount = msg.samples || 48000;
          const pcm = new Float32Array(sampleCount);
          const isSilent = Boolean(context.__mockSilence);
          for (let i = 0; i < sampleCount; i++) {
            pcm[i] = isSilent ? 0 : Math.sin(2 * Math.PI * 440 * (i / 48000)) * 0.45;
          }
          setTimeout(() => {
            if (typeof this.port.onmessage === 'function') {
              this.port.onmessage({ data: { type: 'captured', pcm } });
            }
          }, 15);
        }
      }
    };
  }
}

export class MockAudioBufferSourceNode extends MockAudioNode {
  constructor() {
    super();
    this.buffer = null;
    this.playbackRate = { value: 1.0 };
    this.isPlaying = false;
    this.onended = null;
  }

  start(when = 0, offset = 0) {
    this.isPlaying = true;
    this.startOffset = offset;
  }

  stop() {
    if (!this.isPlaying) return;
    this.isPlaying = false;
    if (typeof this.onended === 'function') {
      this.onended();
    }
  }
}

export class ExtendedMockAudioContext extends BaseMockAudioContext {
  constructor() {
    super();
    this.audioWorklet = {
      addModule: async () => true
    };
    this.__mockSilence = false;
  }

  createBuffer(channels, length, sampleRate) {
    return new MockAudioBuffer({ numberOfChannels: channels, length, sampleRate });
  }

  createBufferSource() {
    return new MockAudioBufferSourceNode();
  }

  createMediaStreamDestination() {
    const stream = new MockMediaStream([new MockMediaStreamTrack({ kind: 'audio', muted: false })]);
    return {
      stream,
      connect: () => {},
      disconnect: () => {}
    };
  }

  async resume() {
    this.state = 'running';
  }
}

export class MockMediaDevices extends EventEmitter {
  constructor(initialDevices = []) {
    super();
    this.devices = [...initialDevices];
    this.permissionState = 'granted';
    this.deviceBusy = false;
  }

  setDevices(devices) {
    this.devices = [...devices];
    this.emit('devicechange');
  }

  addDevice(device) {
    this.devices.push(device);
    this.emit('devicechange');
  }

  removeDevice(deviceId) {
    this.devices = this.devices.filter(d => d.deviceId !== deviceId);
    this.emit('devicechange');
  }

  async enumerateDevices() {
    return JSON.parse(JSON.stringify(this.devices));
  }

  async getUserMedia(constraints) {
    if (this.permissionState === 'denied') {
      const err = new Error('Permission denied');
      err.name = 'NotAllowedError';
      throw err;
    }

    if (this.deviceBusy) {
      const err = new Error('Device busy');
      err.name = 'NotReadableError';
      throw err;
    }

    const audioConstraints = constraints?.audio;
    if (!audioConstraints) {
      const err = new Error('Audio constraints required');
      err.name = 'TypeError';
      throw err;
    }

    const availableInputs = this.devices.filter(d => d.kind === 'audioinput');
    if (availableInputs.length === 0) {
      const err = new Error('Requested device not found');
      err.name = 'NotFoundError';
      throw err;
    }

    let targetDevice = null;
    const requestedExact = typeof audioConstraints === 'object' && audioConstraints.deviceId?.exact;
    const requestedIdeal = typeof audioConstraints === 'object' && audioConstraints.deviceId?.ideal;

    if (requestedExact) {
      targetDevice = availableInputs.find(d => d.deviceId === requestedExact);
      if (!targetDevice) {
        const err = new Error(`Overconstrained: device ${requestedExact} not found`);
        err.name = 'OverconstrainedError';
        throw err;
      }
    } else if (requestedIdeal) {
      targetDevice = availableInputs.find(d => d.deviceId === requestedIdeal) || availableInputs[0];
    } else {
      targetDevice = availableInputs.find(d => d.deviceId === 'default') || availableInputs[0];
    }

    const track = new MockMediaStreamTrack({
      kind: 'audio',
      muted: false,
      readyState: 'live'
    });
    track._deviceId = targetDevice.deviceId;
    track._label = targetDevice.label;
    track.getSettings = () => ({
      deviceId: targetDevice.deviceId,
      sampleRate: 48000,
      channelCount: 1,
      echoCancellation: typeof audioConstraints === 'object' ? Boolean(audioConstraints.echoCancellation) : true,
      autoGainControl: false,
      noiseSuppression: false
    });

    return new MockMediaStream([track]);
  }
}

export function describeMediaError(error) {
  if (error && typeof error === 'object' && 'name' in error) {
    return `${error.name || 'Error'}: ${error.message || ''}`.trim();
  }
  return error instanceof Error ? error.message : String(error);
}

export function classifyMicrophoneError(detail) {
  if (/NotReadableError|TrackStartError|AbortError/i.test(detail)) return 'micBusy';
  if (/NotFoundError|DevicesNotFoundError|OverconstrainedError/i.test(detail)) return 'micNotFound';
  if (/NotAllowedError|PermissionDeniedError|SecurityError|MIC_ACCESS_FAILED/i.test(detail)) return 'micNoAccess';
  return 'unknown';
}

export class AudioDeviceEngine {
  constructor(store, mediaDevices) {
    this.store = store;
    this.mediaDevices = mediaDevices;
    this.currentDeviceId = 'default';
    this.currentOutputDeviceId = 'default';
    this.activeRawDeviceId = null;
    this.echoCancellationEnabled = true;
    this.noiseSuppression = true;
    this.thresholdMode = 'manual';
    this.ultraLowLatency = false;
    this.lastMicCaptureError = null;
    this.lastReportedMicError = null;
    this.rawStream = null;
    this.rawStreamIsFallback = false;
    this.isMuted = false;
    this.monitorWhileMuted = false;

    this.processedContext = new ExtendedMockAudioContext();
    this.micOutputTap = this.processedContext.createGain();
    this.micTestBuffer = null;
    this.micTestOffset = 0;
    this.micTestRun = null;
    this.micTestSource = null;
    this.micTestGeneration = 0;
    this.onMicTestEndedCallback = null;

    this.audioElements = new Set();
    this.systemToasts = [];

    this.noiseSuppressionMode = 'deepfilter';
    this.vadThreshold = 0.5;
  }

  registerAudioElement(element) {
    this.audioElements.add(element);
    if (typeof element.setSinkId === 'function') {
      try {
        const promise = element.setSinkId(this.currentOutputDeviceId);
        if (promise && typeof promise.catch === 'function') {
          promise.catch(() => {});
        }
      } catch {}
    }
  }

  buildMicConstraints(deviceId, echoCancellation = true) {
    const useWebRtcNoiseSuppression = !this.ultraLowLatency && this.noiseSuppression && this.thresholdMode === 'manual';
    const useEchoCancellation = !this.ultraLowLatency && echoCancellation;
    const constraints = {
      channelCount: 1,
      echoCancellation: useEchoCancellation,
      noiseSuppression: useWebRtcNoiseSuppression,
      autoGainControl: false,
      sampleRate: { ideal: 48000 },
      ...(this.ultraLowLatency ? { latency: { ideal: ULTRA_MIC_LATENCY_HINT_S } } : {}),
      googAutoGainControl: false,
      googAutoGainControl2: false,
      googNoiseSuppression: useWebRtcNoiseSuppression,
      googNoiseSuppression2: useWebRtcNoiseSuppression,
      googTypingNoiseDetection: false,
      googHighpassFilter: false,
      googEchoCancellation: useEchoCancellation,
      googEchoCancellation2: useEchoCancellation,
      googAudioMirroring: false
    };
    if (deviceId && deviceId !== 'default') {
      constraints.deviceId = { exact: deviceId };
    }
    return constraints;
  }

  reportMicCaptureError(error) {
    const detail = describeMediaError(error);
    this.lastMicCaptureError = detail;
    const kind = classifyMicrophoneError(detail);
    this.systemToasts.push({ kind, detail });
    if (this.store?.getState()?.setSystemToast) {
      this.store.getState().setSystemToast(kind);
    }
  }

  async captureRawMicStream() {
    const requestedDeviceId = this.currentDeviceId;
    const attempts = [];

    if (requestedDeviceId && requestedDeviceId !== 'default') {
      attempts.push({
        label: `selected device ${requestedDeviceId}`,
        keepsSelection: true,
        run: () => this.mediaDevices.getUserMedia({
          audio: this.buildMicConstraints(requestedDeviceId, this.echoCancellationEnabled),
          video: false
        })
      });
    }

    attempts.push({
      label: 'system default',
      keepsSelection: requestedDeviceId === 'default',
      run: () => this.mediaDevices.getUserMedia({
        audio: this.buildMicConstraints(undefined, this.echoCancellationEnabled),
        video: false
      })
    });

    attempts.push({
      label: 'first available input',
      keepsSelection: false,
      run: async () => {
        const devices = await this.mediaDevices.enumerateDevices();
        const inputs = devices.filter(d => d.kind === 'audioinput' && d.deviceId);
        const input = inputs.find(d => d.deviceId !== 'default' && d.deviceId !== 'communications') || inputs[0];
        if (!input) {
          const err = new Error('No audio input devices found');
          err.name = 'NotFoundError';
          throw err;
        }
        return this.mediaDevices.getUserMedia({
          audio: this.buildMicConstraints(input.deviceId, this.echoCancellationEnabled),
          video: false
        });
      }
    });

    let firstError = null;
    for (const attempt of attempts) {
      try {
        const stream = await attempt.run();
        if (!attempt.keepsSelection) {
          this.currentDeviceId = 'default';
        }
        this.activeRawDeviceId = this.currentDeviceId;
        this.lastMicCaptureError = null;
        this.lastReportedMicError = null;
        return stream;
      } catch (error) {
        firstError = firstError || error;
        if (error.name === 'NotAllowedError' || error.name === 'PermissionDeniedError') {
          break;
        }
      }
    }

    const detail = firstError instanceof Error ? firstError.message : String(firstError);
    this.lastMicCaptureError = detail;
    throw firstError || new Error(`MIC_ACCESS_FAILED: ${detail}`);
  }

  async startBackgroundMic(deviceId) {
    if (deviceId !== undefined) this.currentDeviceId = deviceId;
    const targetDeviceId = deviceId || this.currentDeviceId || 'default';

    if (this.rawStream?.getAudioTracks().some(track => track.readyState === 'live')) {
      if (this.activeRawDeviceId !== null && (this.activeRawDeviceId || 'default') !== targetDeviceId) {
        this.rawStream.getTracks().forEach(t => t.stop());
        this.rawStream = null;
        this.activeRawDeviceId = null;
      } else {
        return true;
      }
    }

    try {
      this.rawStream = await this.captureRawMicStream();
      this.rawStreamIsFallback = false;
      return true;
    } catch (error) {
      this.reportMicCaptureError(error);
      if (!this.rawStream) {
        const silentTrack = new MockMediaStreamTrack({ kind: 'audio', muted: false });
        this.rawStream = new MockMediaStream([silentTrack]);
        this.rawStreamIsFallback = true;
      }
      return false;
    }
  }

  async setOutputDevice(deviceId) {
    this.currentOutputDeviceId = deviceId || 'default';
    await this.applyOutputDevice();
  }

  async applyOutputDevice() {
    for (const element of this.audioElements) {
      if (typeof element.setSinkId === 'function') {
        try {
          await element.setSinkId(this.currentOutputDeviceId);
        } catch {
          try {
            await element.setSinkId('');
          } catch {}
        }
      }
    }
  }

  async handleAudioDeviceChange() {
    let devices = [];
    try {
      devices = await this.mediaDevices.enumerateDevices();
    } catch {
      devices = [];
    }

    const availableInputs = new Set(
      devices.filter(d => d.kind === 'audioinput').map(d => d.deviceId)
    );
    const availableOutputs = new Set(
      devices.filter(d => d.kind === 'audiooutput').map(d => d.deviceId)
    );

    let inputSwitched = false;
    let outputSwitched = false;

    if (
      this.currentDeviceId !== 'default' &&
      this.currentDeviceId !== 'communications' &&
      !availableInputs.has(this.currentDeviceId)
    ) {
      this.currentDeviceId = 'default';
      inputSwitched = true;
      await this.startBackgroundMic('default');
    }

    if (
      this.currentOutputDeviceId !== 'default' &&
      this.currentOutputDeviceId !== 'communications' &&
      !availableOutputs.has(this.currentOutputDeviceId)
    ) {
      this.currentOutputDeviceId = 'default';
      outputSwitched = true;
      await this.applyOutputDevice();
    }

    return { inputSwitched, outputSwitched, inputCount: availableInputs.size, outputCount: availableOutputs.size };
  }

  setMicrophoneMuted(muted) {
    this.isMuted = Boolean(muted);
    if (this.rawStream) {
      this.rawStream.getAudioTracks().forEach(track => {
        track.enabled = !this.isMuted || this.monitorWhileMuted;
      });
    }
  }

  setMonitorWhileMuted(enabled) {
    this.monitorWhileMuted = Boolean(enabled);
    if (this.rawStream && this.isMuted) {
      this.rawStream.getAudioTracks().forEach(track => {
        track.enabled = this.monitorWhileMuted;
      });
    }
  }

  async prepareMicTest() {
    const started = await this.startBackgroundMic();
    if (!started) return;
    await this.processedContext.resume();
  }

  async recordMicTest(onArmed, durationMs = MIC_TEST_DURATION_MS) {
    if (this.micTestRun) return this.micTestRun;
    this.micTestRun = this._runMicTest(durationMs, onArmed);
    try {
      return await this.micTestRun;
    } finally {
      this.micTestRun = null;
    }
  }

  async _runMicTest(durationMs, onArmed) {
    this.pauseMicTest();
    this.micTestBuffer = null;
    this.micTestGeneration++;
    const generation = this.micTestGeneration;

    const started = await this.startBackgroundMic();
    if (!started) throw new Error('MIC_TEST_NO_MIC');

    const ctx = this.processedContext;
    await ctx.resume();

    const captureNode = new MockAudioWorkletNode(ctx, 'mic-test-capture-processor');
    const sink = ctx.createMediaStreamDestination();
    captureNode.connect(sink);

    const captured = new Promise((resolve, reject) => {
      captureNode.port.onmessage = (event) => {
        if (event.data?.type === 'captured') {
          resolve(event.data.pcm);
        }
      };
      captureNode.onprocessorerror = () => reject(new Error('MIC_TEST_CAPTURE_FAILED'));
    });

    this.micOutputTap.connect(captureNode);
    this.setMonitorWhileMuted(true);

    if (generation !== this.micTestGeneration) throw new Error('MIC_TEST_CANCELLED');
    if (typeof onArmed === 'function') onArmed();

    captureNode.port.postMessage({
      type: 'start',
      samples: Math.round((durationMs / 1000) * ctx.sampleRate)
    });

    const pcm = await captured;
    if (generation !== this.micTestGeneration) throw new Error('MIC_TEST_CANCELLED');
    if (pcm.length === 0) throw new Error('MIC_TEST_EMPTY');

    let peak = 0;
    for (let i = 0; i < pcm.length; i++) {
      const mag = pcm[i] < 0 ? -pcm[i] : pcm[i];
      if (mag > peak) peak = mag;
    }

    const buffer = ctx.createBuffer(1, pcm.length, ctx.sampleRate);
    buffer.copyToChannel(pcm, 0);
    this.micTestBuffer = buffer;
    this.micTestOffset = 0;

    return {
      durationSeconds: buffer.duration,
      peakDb: Math.round(20 * Math.log10(Math.max(peak, 1e-6)))
    };
  }

  playMicTest() {
    if (!this.micTestBuffer) return;
    this.pauseMicTest();
    const source = this.processedContext.createBufferSource();
    source.buffer = this.micTestBuffer;
    source.connect(this.processedContext.destination);
    source.onended = () => {
      this.micTestOffset = this.micTestBuffer ? this.micTestBuffer.duration : 0;
      if (typeof this.onMicTestEndedCallback === 'function') {
        this.onMicTestEndedCallback();
      }
    };
    source.start(0, this.micTestOffset);
    this.micTestSource = source;
  }

  pauseMicTest() {
    if (this.micTestSource) {
      this.micTestSource.stop();
      this.micTestSource = null;
    }
  }

  seekMicTest(seconds) {
    if (!this.micTestBuffer) return;
    const clamped = Math.max(0, Math.min(seconds, this.micTestBuffer.duration));
    this.micTestOffset = clamped;
    if (this.micTestSource) {
      this.playMicTest();
    }
  }

  disposeMicTest() {
    this.pauseMicTest();
    this.micTestBuffer = null;
    this.micTestOffset = 0;
    this.micTestGeneration++;
  }

  getMicTestDuration() {
    return this.micTestBuffer ? this.micTestBuffer.duration : 0;
  }

  onMicTestEnded(cb) {
    this.onMicTestEndedCallback = cb;
  }

  setNoiseSuppressionMode(mode) {
    this.noiseSuppressionMode = mode;
  }

  setVadThreshold(val) {
    this.vadThreshold = Math.max(0, Math.min(1, val));
  }
}
