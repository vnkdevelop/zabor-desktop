import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';
import ts from 'typescript';
import { MediaHealthTracker } from '../../src/renderer/src/services/media-health.ts';
import { RC_OP_KEYFRAME_REQUEST } from '../../src/renderer/src/services/remoteControl.ts';
import { MockMediaStream } from './webrtc-stream-mock.mjs';

const source = readFileSync(new URL('../../src/renderer/src/services/webrtc.ts', import.meta.url), 'utf8');
const parsed = ts.createSourceFile('webrtc.ts', source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
const manager = parsed.statements.find(node => ts.isClassDeclaration(node) && node.name?.text === 'WebRTCManager');
const methodNames = [
  'attachRemoteVideo',
  'findRemoteVideoReceiver',
  'syncRemoteVideoStream',
  'setupPeerHandlers',
  'checkMediaHealth',
  'enqueueSignalingTask',
  'renegotiatePeer',
  'flushPendingRenegotiation',
  'handleOffer',
  'handleAnswer'
];
const members = manager.members.filter(node => methodNames.includes(node.name?.getText(parsed)));
if (members.length !== methodNames.length) throw new Error('Required production WebRTC methods are missing');
const compiled = ts.transpileModule(`class WebRTCManager { ${members.map(node => node.getText(parsed)).join('\n')} }`, {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.None }
}).outputText;

export function createProductionStreamEngine(store) {
  let now = 0;
  const signals = [];
  const diagnostics = [];
  const signalRService = {
    isConnected: () => true,
    isSessionReady: () => true,
    sendWebRTCOffer: (id, offer) => signals.push({ type: 'offer', id, payload: JSON.parse(offer) }),
    sendWebRTCAnswer: (id, answer) => signals.push({ type: 'answer', id, payload: JSON.parse(answer) }),
    sfuRequestKeyframe: async id => signals.push({ type: 'sfu-keyframe', id }),
    sfuReportFailure: async id => signals.push({ type: 'sfu-failure', id })
  };
  const Engine = runInNewContext(`${compiled}\nWebRTCManager`, {
    useAppStore: store,
    signalRService,
    MediaStream: MockMediaStream,
    RTCSessionDescription: class { constructor(description) { Object.assign(this, description); } },
    performance: { now: () => now },
    logDiagnostic: record => diagnostics.push(record),
    optimizeSDP: sdp => sdp,
    SILENCE_SUPPRESSION_OFF: {},
    SILENCE_SUPPRESSION_OFF_WITH_ICE_RESTART: { iceRestart: true },
    ULTRA_FRAME_MS_FAST: 10,
    ULTRA_FRAME_MS_SAFE: 20,
    RC_OP_KEYFRAME_REQUEST,
    console
  });
  const engine = new Engine();
  for (const name of [
    'peerConnections', 'sfuSubConnections', 'sfuSubEpochs', 'p2pStreamsBackup',
    'p2pAudioBackup', 'userScreenStreamIds', 'userVoiceTrackIds', 'userStreamAudioTrackIds',
    'streamSourceNodes', 'streamAudioElements', 'reportedViewStates', 'peerSignalingQueues',
    'isMakingOffer', 'isSettingRemoteAnswerPending', 'offerEpoch', 'remoteControlVersions',
    'rcDataChannels', 'rollbackCount'
  ]) engine[name] = new Map();
  engine.pendingRenegotiation = new Set();
  engine.pendingIceRestart = new Set();
  engine.diagnosticConnections = new WeakMap();
  engine.nextDiagnosticConnection = 0;
  engine.mediaHealth = new MediaHealthTracker();
  engine.mediaSessionGeneration = 0;
  engine.localVideoStream = null;
  engine.localStream = null;
  engine.controllingStreamerId = null;
  engine.outputMixContext = null;
  engine.scheduleStreamViewInterestReport = () => {};
  engine.applyVideoZeroPlayoutDelay = () => {};
  engine.initOutputMixer = () => {};
  engine.ensureRemoteControlChannel = () => {};
  engine.ensureUltraSendChannel = () => {};
  engine.ultraPtimeFor = () => 20;
  engine.isPeerRelevant = id => store.getState().voiceUsers.some(user => user.id === id);
  engine.awaitPeerRelevance = async id => engine.isPeerRelevant(id);
  engine.drainPendingCandidates = async () => {};
  engine.startIceTimeout = () => {};
  engine.sendViewerSignal = (id, op) => {
    if (op !== RC_OP_KEYFRAME_REQUEST) throw new Error('Unexpected stream recovery opcode');
    signals.push({ type: 'keyframe', id });
  };
  engine.switchViewerFromSfuToP2P = async id => signals.push({ type: 'p2p-fallback', id });
  engine.cleanupRemoteStream = id => store.setRemoteVideoStream(id, null);
  return { engine, signals, diagnostics, setTime: value => { now = value; } };
}
