import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';
import ts from 'typescript';

const text = readFileSync(new URL('../src/renderer/src/services/webrtc.ts', import.meta.url), 'utf8');
const source = ts.createSourceFile('webrtc.ts', text, ts.ScriptTarget.Latest, true);
const manager = source.statements.find(node => ts.isClassDeclaration(node) && node.name?.text === 'WebRTCManager');
const names = [
  'idleAudioTimer', 'idleAudioGeneration', 'idleAudioReleasePromise', 'micTestPrepared',
  'backgroundMicCaptures', 'backgroundMicPromise', 'vadWorkerGeneration', 'cancelVadWorkerInit',
  'micTransmissionReadyNode', 'micTransmissionReadyPromise', 'cancelMicTransmissionReady', 'micTransmissionRequestId',
  'vadWorkerInitPromise', 'hasAudioDemand', 'cancelIdleAudioRelease', 'scheduleIdleAudioRelease',
  'releaseIdleAudio', 'releaseMicTestPreparation', 'startBackgroundMic', 'ensureVadWorker', 'terminateVadWorker',
  'warmUpSmartNoiseSuppression', 'cleanupProcessedStreamSourceOnly', 'cleanupProcessedStream'
];
const members = names.map(name => manager.members.find(node => node.name?.getText(source) === name).getText(source)).join('\n');
const compiled = ts.transpileModule(`class Engine { ${members} }`, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;

function fixture(overrides = {}) {
  const state = { currentChannelId: null, currentCallUser: null, callStatus: 'idle', currentUser: null };
  const timers = new Map();
  let nextTimer = 0;
  const setTimeout = (fn, delay) => { const id = ++nextTimer; timers.set(id, { fn, delay }); return id; };
  const clearTimeout = id => timers.delete(id);
  const Engine = runInNewContext(`${compiled}\nEngine`, {
    DOMException, Error, console, setTimeout, clearTimeout, MediaStream: class {},
    window: { setTimeout, clearTimeout, location: { href: 'https://fixture.invalid/' } },
    URL, useAppStore: { getState: () => state }, signalRService: { setSpeakingState() {} },
    VAD_WORKER_READY_TIMEOUT_MS: 30_000, SILERO_MODEL_LOAD_TIMEOUT_MS: 15_000,
    withTimeout: promise => promise,
    ...overrides
  });
  const engine = Object.assign(new Engine(), {
    peerConnections: new Map(), sfuSubConnections: new Map(), sfuPubConnection: null,
    localVideoStream: null, localStream: null, rawStream: null, micTestRun: null, micTestPlaying: false,
    calibrationInProgress: false, calibrationForcesAnalyzer: false, activeStartPromise: null,
    microphoneGeneration: 0, micLevelListeners: new Set(), vadWorker: null, vadWorkerReady: false,
    vadEpoch: 0, micNode: null, manualGateNode: null, processedContext: null, micNodeReady: false,
    micRumbleFilters: [], outputMixContext: null, outputBusGain: null, outputPeakGuard: null,
    currentDeviceId: 'default', stopSilenceMonitor() {}, stopAudioHealthMonitoring() {},
    clearVAD() {}, applyLocalSpeaking() {}, analyzerFeedsWorklet: () => true
  });
  return { engine, state, timers };
}

test('idle release is excluded by channel, call, peers, stream, mic test, calibration and pending capture', () => {
  const { engine, state } = fixture();
  assert.equal(engine.hasAudioDemand(), false);
  for (const key of ['currentChannelId', 'currentCallUser', 'callStatus']) {
    const original = state[key];
    state[key] = key === 'callStatus' ? 'connecting' : 'fixture';
    assert.equal(engine.hasAudioDemand(), true, key);
    state[key] = original;
  }
  for (const key of ['sfuPubConnection', 'localVideoStream', 'micTestPrepared', 'micTestRun', 'micTestPlaying',
    'calibrationInProgress', 'calibrationForcesAnalyzer', 'activeStartPromise', 'backgroundMicCaptures', 'backgroundMicPromise']) {
    const original = engine[key];
    engine[key] = true;
    assert.equal(engine.hasAudioDemand(), true, key);
    engine[key] = original;
  }
  engine.peerConnections.set('peer', {});
  assert.equal(engine.hasAudioDemand(), true);
  engine.peerConnections.clear();
  engine.sfuSubConnections.set('peer', {});
  assert.equal(engine.hasAudioDemand(), true);
});

test('a rejoin or a cancelled idle timer cannot release a newer audio session', async () => {
  const { engine, state, timers } = fixture();
  let releases = 0;
  engine.releaseIdleAudio = async () => { releases++; };
  engine.scheduleIdleAudioRelease();
  const first = [...timers.values()][0];
  assert.equal(first.delay, 10_000);
  state.currentChannelId = 'joined';
  first.fn();
  assert.equal(releases, 0);
  state.currentChannelId = null;
  engine.scheduleIdleAudioRelease();
  const last = [...timers.values()].at(-1);
  engine.cancelIdleAudioRelease();
  last.fn();
  assert.equal(releases, 0);
});

test('idle cleanup releases only local audio resources and never calls network teardown', async () => {
  const { engine } = fixture();
  const events = [];
  const stream = name => ({ getTracks: () => [{ stop: () => events.push(name) }] });
  engine.localStream = stream('processed-track');
  engine.rawStream = stream('capture-track');
  engine.stopBackgroundMeter = () => events.push('background-meter');
  engine.cleanupProcessedStream = async () => events.push('rust-worklet-dispose');
  engine.terminateVadWorker = () => events.push('vad-terminate');
  engine.outputBusGain = { disconnect: () => events.push('output-gain') };
  engine.outputPeakGuard = { disconnect: () => events.push('output-guard') };
  engine.outputMixContext = { state: 'running', close: async () => events.push('output-close') };
  engine.leaveAll = () => { throw new Error('network teardown is forbidden'); };
  await engine.releaseIdleAudio();
  assert.deepEqual(events, ['processed-track', 'background-meter', 'capture-track', 'rust-worklet-dispose', 'vad-terminate', 'output-gain', 'output-guard', 'output-close']);
  assert.equal(engine.localStream, null);
  assert.equal(engine.rawStream, null);
  assert.equal(engine.outputMixContext, null);
});

test('visible level meter preserves raw capture but releases the heavy DSP', async () => {
  const { engine } = fixture();
  let stopped = 0;
  let meters = 0;
  engine.rawStream = { getTracks: () => [{ stop: () => stopped++ }], getAudioTracks: () => [{ readyState: 'live' }] };
  engine.micLevelListeners.add(() => {});
  engine.stopBackgroundMeter = () => {};
  engine.cleanupProcessedStream = async () => {};
  engine.startBackgroundMeter = () => meters++;
  await engine.releaseIdleAudio();
  assert.equal(stopped, 0);
  assert.equal(meters, 1);
});

test('a receiver arriving during asynchronous idle cleanup keeps its output mixer', async () => {
  const { engine } = fixture();
  let resolve;
  const closed = new Promise(done => { resolve = done; });
  let outputClosed = 0;
  engine.stopBackgroundMeter = () => {};
  engine.cleanupProcessedStream = () => closed;
  engine.outputMixContext = { state: 'running', close: async () => outputClosed++ };
  const release = engine.releaseIdleAudio();
  engine.peerConnections.set('new-viewer', {});
  resolve();
  await release;
  assert.equal(outputClosed, 0);
  assert.notEqual(engine.outputMixContext, null);
});

test('background microphone startup is single-flight', async () => {
  const { engine } = fixture();
  let resolve;
  const pending = new Promise(done => { resolve = done; });
  let captures = 0;
  engine.captureBackgroundMic = () => { captures++; return pending; };
  const first = engine.startBackgroundMic();
  const second = engine.startBackgroundMic();
  await Promise.resolve();
  assert.equal(captures, 1);
  resolve(true);
  assert.deepEqual(await Promise.all([first, second]), [true, true]);
  assert.equal(engine.backgroundMicPromise, null);
});

test('settings-only warmup never initializes models without an audio consumer', () => {
  const { engine } = fixture({ preloadNoiseAssets: () => { throw new Error('unexpected model preload'); } });
  engine.warmUpSmartNoiseSuppression(true);
  assert.equal(engine.vadWorker, null);
});

test('late cancelled VAD startup cannot terminate or mark ready a replacement worker', async () => {
  const workers = [];
  const loads = [];
  class Worker {
    constructor() { this.messages = []; this.terminated = false; workers.push(this); }
    postMessage(message) {
      this.messages.push(message);
      if (message.type === 'init') this.onmessage({ data: { type: 'ready' } });
    }
    terminate() { this.terminated = true; }
  }
  const { engine, timers } = fixture({
    VadWorker: Worker,
    getSileroModel: () => new Promise(resolve => loads.push(resolve))
  });
  engine.micNode = {};
  const old = engine.ensureVadWorker();
  const cancelled = assert.rejects(old, error => error.name === 'AbortError');
  engine.terminateVadWorker();
  const current = engine.ensureVadWorker();
  loads[0](new Uint8Array([1]));
  await cancelled;
  assert.equal(engine.vadWorker, workers[1]);
  assert.equal(workers[1].terminated, false);
  loads[1](new Uint8Array([1]));
  await current;
  assert.equal(engine.vadWorkerReady, true);
  workers[0].onmessage({ data: { type: 'error', phase: 'initialization', error: 'stale' } });
  assert.equal(engine.vadWorker, workers[1]);
  assert.equal(engine.vadWorkerReady, true);
  assert.equal(timers.size, 0);
});

test('processed cleanup explicitly disposes Rust, clears retained gain/context and preserves level seeds', async () => {
  const { engine } = fixture();
  const messages = [];
  let disconnected = 0;
  const seed = { alcGain: 1.25 };
  engine.micLevelSeeds = new Map([['fixture', seed]]);
  engine.calibratedPreGainNode = { disconnect: () => disconnected++ };
  engine.micNode = { disconnect() {}, port: { postMessage: value => messages.push(value.type), close() {} } };
  engine.processedContext = { state: 'running', close: async () => {} };
  await engine.cleanupProcessedStream();
  assert.deepEqual(messages, ['dispose']);
  assert.equal(disconnected, 1);
  assert.equal(engine.calibratedPreGainNode, null);
  assert.equal(engine.processedContext, null);
  assert.equal(engine.micLevelSeeds.get('fixture'), seed);
});
