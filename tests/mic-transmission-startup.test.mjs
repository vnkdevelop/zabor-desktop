import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';
import ts from 'typescript';
import { createPipeline, signalQuantum } from './helpers/client-dsp-fixture.mjs';

const text = readFileSync('src/renderer/src/services/webrtc.ts', 'utf8');
const source = ts.createSourceFile('webrtc.ts', text, ts.ScriptTarget.Latest, true);
const manager = source.statements.find(node => ts.isClassDeclaration(node) && node.name?.text === 'WebRTCManager');
const names = ['micTransmissionReadyNode', 'micTransmissionReadyPromise', 'cancelMicTransmissionReady',
  'micTransmissionRequestId', 'idleAudioGeneration', 'idleAudioTimer', 'cancelIdleAudioRelease',
  'waitForMicTransmissionReady', 'cancelPendingMicrophoneStart', 'startLocalStream', 'toggleMute', 'micGraphSignature', 'usesSmartWorklet'];
const members = names.map(name => manager.members.find(node => node.name?.getText(source) === name).getText(source)).join('\n');
const compiled = ts.transpileModule(`class Engine { ${members} }`, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;

class Target {
  listeners = new Map();
  addEventListener(name, fn) { const listeners = this.listeners.get(name) ?? new Set(); listeners.add(fn); this.listeners.set(name, listeners); }
  removeEventListener(name, fn) { this.listeners.get(name)?.delete(fn); }
  emit(name, data) { for (const fn of [...(this.listeners.get(name) ?? [])]) fn({ data }); }
}

function fixture() {
  const timers = new Map();
  let timerId = 0;
  const state = { currentUser: { id: 'me', isMuted: false, isServerMuted: false }, currentChannelId: 'a',
    currentCallUser: null, callStatus: 'idle', voiceUsers: [{ id: 'me' }], channelUsersMap: {} };
  const Engine = runInNewContext(`${compiled}\nEngine`, {
    DOMException, Error, console, performance, MIC_TRANSMISSION_READY_TIMEOUT_MS: 20_000,
    setTimeout: (fn, delay) => { const id = ++timerId; timers.set(id, { fn, delay }); return id; },
    clearTimeout: id => timers.delete(id), useAppStore: { getState: () => state },
    safeResumeContext: async context => { context.state = 'running'; },
    describeMediaError: error => error.message, createSilentAudioStream: () => { throw new Error('unexpected fallback'); }
  });
  const node = new Target();
  const requests = [];
  let resolvePreparationRequest;
  const preparationRequested = new Promise(resolve => { resolvePreparationRequest = resolve; });
  node.port = new Target();
  node.port.start = () => {};
  node.port.postMessage = message => {
    requests.push(message);
    if (message.type === 'prepareTransmission') resolvePreparationRequest(message);
  };
  const context = { state: 'running', sampleRate: 48000 };
  const track = { readyState: 'live', enabled: false, contentHint: '', stop() { this.readyState = 'ended'; } };
  const local = { getAudioTracks: () => [track], getTracks: () => [track] };
  const rawTrack = { readyState: 'live', enabled: true, label: 'fixture', contentHint: '', getSettings: () => ({ sampleRate: 48000, deviceId: 'default' }), stop() {} };
  const raw = { getAudioTracks: () => [rawTrack], getTracks: () => [rawTrack] };
  const engine = Object.assign(new Engine(), {
    micNode: node, processedContext: context, vadEpoch: 0, microphoneGeneration: 0,
    idleAudioReleasePromise: null, backgroundMicPromise: null, activeStartPromise: null,
    ultraLowLatency: false, currentDeviceId: 'default', noiseSuppression: true,
    localStream: null, rawStream: null, rawStreamIsFallback: false, activeRawDeviceId: null,
    activeGraphSignature: null, manualGateNode: null, webRtcVadSidecar: null, ultraSendClonedTrack: null,
    thresholdMode: 'auto', smartModel: 'rnnoise', analyzerFeedsWorklet: () => false,
    ensureVadWorker: async () => {}, terminateVadWorker() {}, initOutputMixer() {}, stopBackgroundMeter() {},
    startSilenceMonitor() {}, loadCalibration() {}, applyLocalSpeaking() {},
    updateVadBranch: async () => {}, captureRawMicStream: async () => raw,
    createProcessedStream: async () => { engine.micNode = node; engine.processedContext = context; return local; },
    cleanupProcessedStream: async () => { engine.cancelMicTransmissionReady?.(); engine.micTransmissionReadyNode = null; },
    reportMicCaptureError() {}
  });
  return { engine, node, context, track, local, raw, state, requests, timers, preparationRequested };
}

const readyMessage = requestId => ({ type: 'transmissionReady', requestId, sampleRate: 48000,
  frameSize: 480, preparedFrames: 32, inputQueued: 32, outputQueued: 960, clockSpanMs: 310, clockAudioMs: 310 });
const flush = async () => { for (let index = 0; index < 12; index++) await Promise.resolve(); };

test('cold start does not resolve or enable sending before the worklet confirms preparation', async () => {
  const { engine, node, track, requests } = fixture();
  let completed = false;
  const started = engine.startLocalStream().then(result => { completed = true; return result; });
  await flush();
  const request = requests.find(message => message.type === 'prepareTransmission');
  assert.ok(request);
  engine.toggleMute(false);
  assert.equal(track.enabled, false);
  assert.equal(completed, false);
  node.port.emit('message', readyMessage(request.requestId + 1));
  await flush();
  assert.equal(completed, false);
  node.port.emit('message', readyMessage(request.requestId));
  assert.equal(await started, true);
  assert.equal(track.enabled, true);
});

test('cold start clock mismatch fails closed without announcing a ready microphone', async () => {
  const { engine, node, track, requests } = fixture();
  const started = engine.startLocalStream();
  await flush();
  const request = requests.find(message => message.type === 'prepareTransmission');
  node.port.emit('message', { ...readyMessage(request.requestId), sampleRate: 44100 });
  assert.equal(await started, false);
  assert.equal(track.enabled, false);
  assert.equal(engine.micTransmissionReadyNode, null);
  assert.equal(engine.micEngineError, 'MIC_DSP_CLOCK_INVALID');
});

test('cold preparation timeout is bounded and does not unmute an unready track', async () => {
  const { engine, track, timers } = fixture();
  const started = engine.startLocalStream();
  await flush();
  const timeout = [...timers.values()].find(timer => timer.delay === 20_000);
  assert.ok(timeout);
  timeout.fn();
  assert.equal(await started, false);
  assert.equal(track.enabled, false);
  assert.equal(engine.micEngineError, 'MIC_DSP_PREPARATION_TIMEOUT');
});

test('leaving during VAD loading cancels microphone preparation before waiting for the model', async () => {
  const { engine, track, state, requests } = fixture();
  let loading;
  let resolveModel;
  const requested = new Promise(resolve => { loading = resolve; });
  engine.analyzerFeedsWorklet = () => true;
  engine.ensureVadWorker = () => {
    loading();
    return new Promise(resolve => { resolveModel = resolve; });
  };
  const started = engine.startLocalStream();
  await requested;
  const signalText = readFileSync('src/renderer/src/services/signalr.ts', 'utf8');
  const signalSource = ts.createSourceFile('signalr.ts', signalText, ts.ScriptTarget.Latest, true);
  const signalClass = signalSource.statements.find(node => ts.isClassDeclaration(node) && node.name?.text === 'SignalRService');
  const member = signalClass.members.find(node => node.name?.getText(signalSource) === 'leaveChannel');
  const signalCode = ts.transpileModule(`class Signal { ${member.getText(signalSource)} }`, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
  Object.assign(state, {
    setIsJoiningChannel() {}, removeUserFromChannelMap() {}, setActiveStreamId() {},
    clearVoiceChannel() { this.currentChannelId = null; }
  });
  const actions = [];
  const Signal = runInNewContext(`${signalCode}\nSignal`, {
    useAppStore: { getState: () => state }, webrtc: Object.assign(engine, {
      localVideoStream: null, leaveAll() { actions.push('leave-peers'); }, enterBackgroundMode: async () => {}
    }), channelLeaveSound: 'leave', clearTimeout
  });
  const signal = Object.assign(new Signal(), {
    voiceOperationDone: started, voiceOperationId: 1, playSfx() {}, cancelChannelRejoinRetry() {},
    safeInvoke: async method => actions.push(method)
  });
  await signal.leaveChannel();
  assert.equal(await started, false);
  assert.equal(track.enabled, false);
  assert.equal(engine.micTransmissionReadyNode, null);
  assert.equal(requests.some(message => message.type === 'prepareTransmission'), false);
  assert.deepEqual(actions, ['leave-peers', 'LeaveChannel']);
  resolveModel();
  await flush();
  assert.equal(engine.micTransmissionReadyNode, null);
});

test('an old cancelled preparation cannot enable or mark a replacement graph ready', async () => {
  const { engine, node, requests } = fixture();
  const first = engine.waitForMicTransmissionReady();
  const cancelled = assert.rejects(first, error => error.name === 'AbortError');
  const request = requests.find(message => message.type === 'prepareTransmission');
  engine.cancelMicTransmissionReady();
  await cancelled;
  const replacement = new Target();
  replacement.port = new Target();
  replacement.port.start = () => {};
  const nextRequests = [];
  replacement.port.postMessage = message => nextRequests.push(message);
  engine.micNode = replacement;
  engine.processedContext = { state: 'running', sampleRate: 48000 };
  engine.vadEpoch++;
  const next = engine.waitForMicTransmissionReady();
  node.port.emit('message', readyMessage(request.requestId));
  assert.equal(engine.micTransmissionReadyNode, null);
  replacement.port.emit('message', readyMessage(nextRequests[0].requestId));
  await next;
  assert.equal(engine.micTransmissionReadyNode, replacement);
});

test('A to B to C channel switches retain the ready microphone, context and DSP state', async () => {
  const { engine, node, context, track, local, raw, state, requests } = fixture();
  engine.localStream = local;
  engine.rawStream = raw;
  engine.activeRawDeviceId = 'default';
  engine.activeGraphSignature = engine.micGraphSignature();
  engine.micTransmissionReadyNode = node;
  track.enabled = true;
  engine.captureRawMicStream = async () => { throw new Error('channel hop recaptured the device'); };
  engine.createProcessedStream = async () => { throw new Error('channel hop rebuilt DSP'); };
  engine.cleanupProcessedStream = async () => { throw new Error('channel hop closed DSP'); };
  const signalText = readFileSync('src/renderer/src/services/signalr.ts', 'utf8');
  const signalSource = ts.createSourceFile('signalr.ts', signalText, ts.ScriptTarget.Latest, true);
  const signalClass = signalSource.statements.find(node => ts.isClassDeclaration(node) && node.name?.text === 'SignalRService');
  const member = signalClass.members.find(node => node.name?.getText(signalSource) === 'switchChannel');
  const signalCode = ts.transpileModule(`class Signal { ${member.getText(signalSource)} }`, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
  const actions = [];
  const sounds = [];
  Object.assign(state, {
    removeUserFromChannelMap() {}, setActiveStreamId() {}, setIsJoiningChannel() {},
    commitVoiceChannel(channelId, users) { this.currentChannelId = channelId; this.voiceUsers = users; },
    setCallStatus(value) { this.callStatus = value; }, setCurrentCallUser(value) { this.currentCallUser = value; },
    clearVoiceChannel() { throw new Error('unexpected rollback'); }
  });
  const Signal = runInNewContext(`${signalCode}\nSignal`, {
    useAppStore: { getState: () => state }, webrtc: Object.assign(engine, { localVideoStream: null, reconcilePeers() {} }),
    SignalRService: { INVOKE_TIMEOUT_MS: 1000, PEER_CONNECT_DELAY_MS: 100 },
    channelLeaveSound: 'leave', channelJoinSound: 'join', setTimeout: () => 1
  });
  const signal = Object.assign(new Signal(), {
    isJoiningChannel: false, voiceOperationDone: null, voiceOperationId: 0,
    beginVoiceOperation() { return ++this.voiceOperationId; }, finishVoiceOperation() {},
    cancelChannelRejoinRetry() {}, playSfx(sound) { if (sound === 'join') sounds.push(actions.at(-1)); },
    async invokeWithTimeout(method) { actions.push(method); return method === 'JoinChannel' ? { users: [{ id: 'me' }] } : undefined; }
  });
  assert.equal(await signal.switchChannel('b'), 'ok');
  assert.equal(await signal.switchChannel('c'), 'ok');
  assert.deepEqual(actions, ['LeaveChannel', 'JoinChannel', 'LeaveChannel', 'JoinChannel']);
  assert.deepEqual(sounds, ['JoinChannel', 'JoinChannel']);
  assert.equal(engine.micNode, node);
  assert.equal(engine.processedContext, context);
  assert.equal(engine.localStream, local);
  assert.equal(engine.rawStream, raw);
  assert.equal(track.enabled, true);
  assert.equal(requests.some(message => message.type === 'prepareTransmission'), false);
});

test('cold channel join sound follows DSP preparation and accepted membership, not the initial click', async () => {
  const { engine, node, track, state, preparationRequested } = fixture();
  state.currentChannelId = null;
  const sounds = [];
  const commands = [];
  Object.assign(state, {
    removeUserFromChannelMap() {},
    commitVoiceChannel(channelId, users) { this.currentChannelId = channelId; this.voiceUsers = users; },
    setCallStatus(value) { this.callStatus = value; }, setCurrentCallUser(value) { this.currentCallUser = value; },
    clearVoiceChannel() { throw new Error('unexpected rollback'); }
  });
  const signalText = readFileSync('src/renderer/src/services/signalr.ts', 'utf8');
  const signalSource = ts.createSourceFile('signalr.ts', signalText, ts.ScriptTarget.Latest, true);
  const signalClass = signalSource.statements.find(node => ts.isClassDeclaration(node) && node.name?.text === 'SignalRService');
  const member = signalClass.members.find(node => node.name?.getText(signalSource) === '_joinChannelImpl');
  const signalCode = ts.transpileModule(`class Signal { ${member.getText(signalSource)} }`, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
  const Signal = runInNewContext(`${signalCode}\nSignal`, {
    useAppStore: { getState: () => state }, webrtc: Object.assign(engine, { warmUpConnectivity() {}, reconcilePeers() {} }),
    SignalRService: { INVOKE_TIMEOUT_MS: 1000, PEER_CONNECT_DELAY_MS: 100 },
    channelJoinSound: 'join', setTimeout: () => 1, performance, console
  });
  const signal = Object.assign(new Signal(), {
    voiceOperationId: 1, ensureSessionReady: async () => true,
    playSfx(sound) { sounds.push({ sound, lastCommand: commands.at(-1), prepared: engine.micTransmissionReadyNode === node }); },
    async invokeWithTimeout(method) { commands.push(method); return { users: [{ id: 'me' }] }; }
  });
  const joined = signal._joinChannelImpl('b', 1);
  const request = await preparationRequested;
  assert.equal(sounds.length, 0);
  assert.equal(commands.length, 0);
  assert.equal(track.enabled, false);
  node.port.emit('message', readyMessage(request.requestId));
  assert.equal(await joined, 'ok');
  assert.deepEqual(commands, ['JoinChannel']);
  assert.equal(sounds.length, 1);
  assert.equal(sounds[0].prepared, true);
  assert.equal(sounds[0].lastCommand, 'JoinChannel');
});

test('transmission preparation leaves unmuted PCM unchanged sample for sample', () => {
  let now = 1;
  const clock = { now: () => now };
  const reference = createPipeline(true, null, clock);
  const prepared = createPipeline(true, null, clock);
  prepared.pipeline.port.onmessage({ data: { type: 'prepareTransmission', requestId: 7 } });
  for (let quantum = 0; quantum < 750; quantum++) {
    const input = signalQuantum(quantum * 128);
    const before = new Float32Array(128);
    const after = new Float32Array(128);
    now += 128 / 48;
    reference.pipeline.process([[input]], [[before]]);
    prepared.pipeline.process([[input]], [[after]]);
    assert.deepEqual(after, before);
  }
  const ready = prepared.messages.filter(message => message.type === 'transmissionReady');
  assert.equal(ready.length, 1);
  assert.equal(ready[0].requestId, 7);
  assert.equal(ready[0].sampleRate, 48000);
  assert.equal(ready[0].frameSize, 480);
  assert.ok(ready[0].preparedFrames >= 32);
});

test('muted cold preparation runs locally, remains silent and returns to the cheap muted path', () => {
  let now = 1;
  let processed = 0;
  const engine = { label: 'fixture', supportsAttenuationLimit: false, isReady: true, frame: 480,
    stageName: 'ready', start: async () => {}, process(frame) { processed++; return frame; },
    setAttenuationLimit() {}, setPostFilterBeta() {} };
  const { pipeline, messages } = createPipeline(true, engine, { now: () => now });
  pipeline.port.onmessage({ data: { type: 'setConfig', isMuted: true } });
  pipeline.port.onmessage({ data: { type: 'prepareTransmission', requestId: 8 } });
  for (let quantum = 0; quantum < 500; quantum++) {
    const output = new Float32Array(128);
    now += 128 / 48;
    pipeline.process([[signalQuantum(quantum * 128)]], [[output]]);
    assert.equal(output.some(sample => sample !== 0), false);
  }
  assert.ok(messages.some(message => message.type === 'transmissionReady'));
  const count = processed;
  pipeline.process([[signalQuantum(0)]], [[new Float32Array(128)]]);
  assert.equal(processed, count);
});

test('startup catch-up bursts cannot announce readiness until wall time matches the audio cadence', () => {
  let now = 1;
  const { pipeline, messages } = createPipeline(true, null, { now: () => now });
  pipeline.port.onmessage({ data: { type: 'prepareTransmission', requestId: 11 } });
  for (let quantum = 0; quantum < 500; quantum++) {
    pipeline.process([[signalQuantum(quantum * 128)]], [[new Float32Array(128)]]);
  }
  assert.equal(messages.some(message => message.type === 'transmissionReady'), false);
  for (let quantum = 500; quantum < 1000; quantum++) {
    now += 128 / 48;
    pipeline.process([[signalQuantum(quantum * 128)]], [[new Float32Array(128)]]);
  }
  const ready = messages.find(message => message.type === 'transmissionReady');
  assert.ok(ready);
  assert.ok(Math.abs(ready.clockSpanMs - ready.clockAudioMs) <= 15 + ready.clockAudioMs * 0.05);
});

test('a startup stall resets preparation and empty denoiser output never announces readiness', () => {
  let now = 1;
  const { pipeline, messages } = createPipeline(true, null, { now: () => now });
  pipeline.port.onmessage({ data: { type: 'prepareTransmission', requestId: 9 } });
  for (let quantum = 0; quantum < 70; quantum++) {
    now += 128 / 48;
    pipeline.process([[signalQuantum(quantum * 128)]], [[new Float32Array(128)]]);
  }
  assert.ok(pipeline.transmissionPreparedFrames > 0);
  now += 200;
  pipeline.process([[signalQuantum(70 * 128)]], [[new Float32Array(128)]]);
  assert.equal(pipeline.transmissionPreparedFrames, 0);
  assert.equal(messages.some(message => message.type === 'transmissionReady'), false);
  const bad = createPipeline(true, { label: 'empty', supportsAttenuationLimit: false, isReady: true,
    frame: 480, stageName: 'ready', start: async () => {}, process: () => null,
    setAttenuationLimit() {}, setPostFilterBeta() {} }, { now: () => now });
  bad.pipeline.port.onmessage({ data: { type: 'prepareTransmission', requestId: 10 } });
  for (let quantum = 0; quantum < 300; quantum++) {
    now += 128 / 48;
    bad.pipeline.process([[signalQuantum(quantum * 128)]], [[new Float32Array(128)]]);
  }
  assert.equal(bad.messages.some(message => message.type === 'transmissionReady'), false);
});
