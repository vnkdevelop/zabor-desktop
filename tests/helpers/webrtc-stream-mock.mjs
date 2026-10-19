import { EventEmitter } from 'node:events';

/**
 * Mock MediaStreamTrack implementing W3C MediaStreamTrack event semantics.
 */
export class MockMediaStreamTrack extends EventEmitter {
  constructor({
    id = `track_${Math.random().toString(36).slice(2, 9)}`,
    kind = 'video',
    readyState = 'live',
    muted = true
  } = {}) {
    super();
    this.id = id;
    this.kind = kind;
    this.readyState = readyState;
    this._muted = muted;
    this.enabled = true;
    this.contentHint = '';
    this.streamId = null;
    this.onmute = null;
    this.onunmute = null;
    this.onended = null;
  }

  get muted() {
    return this._muted;
  }

  set muted(val) {
    const nextVal = Boolean(val);
    if (this._muted === nextVal) return;
    this._muted = nextVal;
    if (nextVal) {
      if (typeof this.onmute === 'function') this.onmute();
      this.emit('mute');
    } else {
      if (typeof this.onunmute === 'function') this.onunmute();
      this.emit('unmute');
    }
  }

  stop() {
    if (this.readyState === 'ended') return;
    this.readyState = 'ended';
    this.enabled = false;
    if (typeof this.onended === 'function') this.onended();
    this.emit('ended');
  }
}

/**
 * Mock MediaStream implementing W3C MediaStream container interface.
 */
export class MockMediaStream {
  constructor(tracks = []) {
    this.id = `stream_${Math.random().toString(36).slice(2, 9)}`;
    this._tracks = [...tracks];
  }

  getTracks() {
    return [...this._tracks];
  }

  getVideoTracks() {
    return this._tracks.filter(t => t.kind === 'video');
  }

  getAudioTracks() {
    return this._tracks.filter(t => t.kind === 'audio');
  }

  addTrack(track) {
    if (track && !this._tracks.includes(track)) {
      this._tracks.push(track);
    }
  }

  removeTrack(track) {
    this._tracks = this._tracks.filter(t => t !== track);
  }
}

/**
 * Mock RTCRtpReceiver representing inbound RTP stream.
 */
export class MockRTCRtpReceiver {
  constructor(track) {
    this.track = track;
    this.transport = { state: 'connected' };
    this.playoutDelayHint = 0;
  }

  getParameters() {
    return { encodings: [{}] };
  }
}

/**
 * Mock RTCRtpSender representing outbound RTP stream.
 */
export class MockRTCRtpSender {
  constructor(track = null) {
    this.track = track;
    this.transport = { state: 'connected' };
    this._params = { encodings: [{}] };
    this._streams = [];
  }

  async replaceTrack(newTrack) {
    this.track = newTrack;
  }

  getParameters() {
    return JSON.parse(JSON.stringify(this._params));
  }

  async setParameters(params) {
    this._params = JSON.parse(JSON.stringify(params));
  }

  setStreams(...streams) {
    this._streams = streams;
  }
}

/**
 * Mock RTCRtpTransceiver representing paired sender and receiver.
 */
export class MockRTCRtpTransceiver {
  constructor(kind, direction = 'sendrecv', track = null) {
    this.kind = kind;
    this.direction = direction;
    this.currentDirection = direction;
    this.stopped = false;
    const receiverTrack = track && track.kind === kind ? track : new MockMediaStreamTrack({ kind, muted: true });
    this.receiver = new MockRTCRtpReceiver(receiverTrack);
    this.sender = new MockRTCRtpSender(track || null);
  }

  stop() {
    this.stopped = true;
    this.currentDirection = 'stopped';
  }
}

/**
 * Mock RTCPeerConnection supporting W3C states, rollback, transceivers and stats.
 */
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
    this.stats = new Map();
  }

  getReceivers() {
    return this.transceivers.map(t => t.receiver);
  }

  getSenders() {
    return this.transceivers.map(t => t.sender);
  }

  getTransceivers() {
    return [...this.transceivers];
  }

  addTransceiver(trackOrKind, init = {}) {
    const kind = typeof trackOrKind === 'string' ? trackOrKind : trackOrKind.kind;
    const track = typeof trackOrKind === 'object' ? trackOrKind : null;
    const direction = init.direction || 'sendrecv';
    const transceiver = new MockRTCRtpTransceiver(kind, direction, track);
    this.transceivers.push(transceiver);
    return transceiver;
  }

  addTrack(track, stream) {
    let transceiver = this.transceivers.find(t => t.kind === track.kind && !t.sender.track && !t.stopped);
    if (!transceiver) {
      transceiver = this.addTransceiver(track, { direction: 'sendrecv' });
    } else {
      transceiver.sender.track = track;
      transceiver.direction = 'sendrecv';
    }
    if (stream) {
      transceiver.sender.setStreams(stream);
    }
    return transceiver.sender;
  }

  async createOffer(opts = {}) {
    return {
      type: 'offer',
      sdp: `v=0\r\no=mock 123 123 IN IP4 127.0.0.1\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\n`
    };
  }

  async createAnswer(opts = {}) {
    return {
      type: 'answer',
      sdp: `v=0\r\no=mock 123 123 IN IP4 127.0.0.1\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\n`
    };
  }

  async setLocalDescription(desc) {
    if (!desc) return;
    if (desc.type === 'rollback') {
      this.signalingState = 'stable';
      this.localDescription = null;
      return;
    }
    this.localDescription = { type: desc.type, sdp: desc.sdp || '' };
    this.signalingState = desc.type === 'offer' ? 'have-local-offer' : 'stable';
  }

  async setRemoteDescription(desc) {
    if (!desc) return;
    this.remoteDescription = { type: desc.type, sdp: desc.sdp || '' };
    this.signalingState = desc.type === 'offer' ? 'have-remote-offer' : 'stable';
  }

  async getStats() {
    return this.stats;
  }

  close() {
    this.signalingState = 'closed';
    this.connectionState = 'closed';
    this.iceConnectionState = 'closed';
    if (typeof this.onconnectionstatechange === 'function') {
      this.onconnectionstatechange();
    }
    this.emit('connectionstatechange');
  }
}

/**
 * Base Mock AudioNode tracking connections.
 */
export class MockAudioNode {
  constructor() {
    this.connectedTo = [];
    this.isDisconnected = false;
  }

  connect(destination) {
    this.connectedTo.push(destination);
    this.isDisconnected = false;
    return destination;
  }

  disconnect() {
    this.connectedTo = [];
    this.isDisconnected = true;
  }
}

/**
 * Mock AudioParam with linear ramp scheduling.
 */
export class MockAudioParam {
  constructor(defaultValue = 1) {
    this.value = defaultValue;
    this.ramps = [];
  }

  linearRampToValueAtTime(val, time) {
    this.value = val;
    this.ramps.push({ val, time });
  }

  setValueAtTime(val, time) {
    this.value = val;
  }
}

export class MockGainNode extends MockAudioNode {
  constructor(defaultValue = 1) {
    super();
    this.gain = new MockAudioParam(defaultValue);
  }
}

export class MockDelayNode extends MockAudioNode {
  constructor(opts = {}) {
    super();
    this.delayTime = new MockAudioParam(opts.delayTime || 0);
    this.maxDelayTime = opts.maxDelayTime || 1.0;
  }
}

export class MockDynamicsCompressorNode extends MockAudioNode {
  constructor() {
    super();
    this.threshold = new MockAudioParam(-12);
    this.knee = new MockAudioParam(2);
    this.ratio = new MockAudioParam(20);
    this.attack = new MockAudioParam(0.002);
    this.release = new MockAudioParam(0.080);
  }
}

export class MockMediaStreamAudioSourceNode extends MockAudioNode {
  constructor(mediaStream) {
    super();
    this.mediaStream = mediaStream;
  }
}

export class MockAudioContext {
  constructor() {
    this.state = 'running';
    this.currentTime = 0;
    this.sampleRate = 48000;
    this.destination = new MockAudioNode();
  }

  createMediaStreamSource(stream) {
    return new MockMediaStreamAudioSourceNode(stream);
  }

  createGain() {
    return new MockGainNode(1);
  }

  createDelay(maxDelayTime = 1.0) {
    return new MockDelayNode({ maxDelayTime });
  }

  createDynamicsCompressor() {
    return new MockDynamicsCompressorNode();
  }

  async close() {
    this.state = 'closed';
  }
}

export class MockAudioElement {
  constructor() {
    this.autoplay = false;
    this.muted = false;
    this.srcObject = null;
    this.paused = true;
    this.played = false;
  }

  async play() {
    this.paused = false;
    this.played = true;
  }

  pause() {
    this.paused = true;
  }
}

/**
 * Create a mock Zustand store matching useAppStore.ts stream contracts.
 */
export function createMockAppStore(initialState = {}) {
  let state = {
    currentUser: { id: 'local_user' },
    voiceUsers: [],
    currentCallUser: null,
    remoteVideoStreams: {},
    activeStreamId: null,
    webRTCConnectionStatus: {},
    ...initialState
  };

  const listeners = new Set();

  const api = {
    setRemoteVideoStream(userId, stream) {
      const current = { ...state.remoteVideoStreams };
      if (!stream) {
        delete current[userId];
        const nextActive = state.activeStreamId === userId ? null : state.activeStreamId;
        store.setState({ remoteVideoStreams: current, activeStreamId: nextActive });
      } else {
        current[userId] = stream;
        store.setState({ remoteVideoStreams: current });
      }
    },
    setActiveStreamId(id) {
      store.setState({ activeStreamId: id });
    },
    updateUserStatus(userId, updates) {
      let nextCurrentUser = state.currentUser;
      if (state.currentUser?.id === userId) {
        nextCurrentUser = { ...state.currentUser, ...updates };
      }
      let nextCallUser = state.currentCallUser;
      if (state.currentCallUser?.id === userId) {
        nextCallUser = { ...state.currentCallUser, ...updates };
      }
      const nextVoiceUsers = state.voiceUsers.map(u => (u.id === userId ? { ...u, ...updates } : u));
      store.setState({
        currentUser: nextCurrentUser,
        currentCallUser: nextCallUser,
        voiceUsers: nextVoiceUsers
      });
    },
    setWebRTCConnectionStatus(userId, status) {
      store.setState({
        webRTCConnectionStatus: { ...state.webRTCConnectionStatus, [userId]: status }
      });
    }
  };

  const store = {
    getState() {
      return { ...state, ...api };
    },
    setState(updater) {
      if (typeof updater === 'function') {
        state = { ...state, ...updater(state) };
      } else {
        state = { ...state, ...updater };
      }
      listeners.forEach(cb => cb(store.getState()));
    },
    subscribe(cb) {
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
    ...api
  };

  return store;
}

/**
 * StreamEngine binds the mocks to exercise genuine logic paths of
 * WebRTC stream negotiation, receiver sync, fallback and teardown.
 */
export class StreamEngine {
  static MAX_ROLLBACKS = 3;

  constructor(store, options = {}) {
    this.store = store;
    this.userId = options.userId || 'local_user';

    this.peerConnections = new Map();
    this.userVoiceTrackIds = new Map();
    this.userStreamAudioTrackIds = new Map();
    this.userScreenStreamIds = new Map();

    this.streamAudioElements = new Map();
    this.streamSourceNodes = new Map();
    this.streamDelayNodes = new Map();
    this.streamGainNodes = new Map();
    this.streamLimiterNodes = new Map();
    this.streamSyncOffsetEma = new Map();
    this.streamSyncSkew = new Map();

    this.userGainNodes = new Map();
    this.userSpeaking = new Map();

    this.outputMixContext = new MockAudioContext();
    this.outputBusGain = this.outputMixContext.createGain();

    this.peerSignalingQueues = new Map();
    this.isMakingOffer = new Map();
    this.isSettingRemoteAnswerPending = new Map();
    this.pendingRenegotiation = new Set();
    this.pendingIceRestart = new Set();
    this.offerEpoch = new Map();
    this.rollbackCount = new Map();

    this.localVideoStream = null;
    this.statsMonitoringActive = false;

    this.sentOffers = [];
    this.sentAnswers = [];
    this.signalRService = {
      sendWebRTCOffer: (target, offer) => this.sentOffers.push({ target, offer }),
      sendWebRTCAnswer: (target, answer) => this.sentAnswers.push({ target, answer })
    };
  }

  initPeer(userId, pc) {
    this.peerConnections.set(userId, pc);
    this.store.getState().setWebRTCConnectionStatus(userId, true);
  }

  syncRemoteVideoStream(userId) {
    const pc = this.peerConnections.get(userId);
    if (!pc || pc.connectionState === 'closed') return;

    const storeState = this.store.getState();
    const user = storeState.voiceUsers.find(u => u.id === userId) ||
      (storeState.currentCallUser?.id === userId ? storeState.currentCallUser : null);
    const isUserStreaming = Boolean(user?.isStreaming);

    for (const receiver of pc.getReceivers()) {
      const track = receiver.track;
      if (track && track.kind === 'video' && track.readyState !== 'ended') {
        if (!track.onunmute) {
          track.onunmute = () => {
            this.syncRemoteVideoStream(userId);
          };
        }
        if (isUserStreaming || !track.muted) {
          this.attachRemoteVideo(userId, track);
        }
      }
    }

    const knownVoiceTrackId = this.userVoiceTrackIds.get(userId);
    const audioReceivers = pc.getReceivers().filter(r => r.track && r.track.kind === 'audio');
    const screenAudioReceiver = audioReceivers.find(r => r.track && r.track.id !== knownVoiceTrackId && r.track.readyState !== 'ended');
    if (screenAudioReceiver?.track) {
      if (!screenAudioReceiver.track.onunmute) {
        screenAudioReceiver.track.onunmute = () => {
          this.syncRemoteVideoStream(userId);
        };
      }
      if (isUserStreaming || !screenAudioReceiver.track.muted) {
        if (this.userStreamAudioTrackIds.get(userId) !== screenAudioReceiver.track.id) {
          const remoteVideoStream = this.store.getState().remoteVideoStreams[userId];
          this.attachRemoteStreamAudio(userId, screenAudioReceiver.track, screenAudioReceiver, remoteVideoStream);
        }
      }
    }
  }

  attachRemoteVideo(userId, track, stream) {
    if (track.readyState === 'ended') return;
    const currentStream = this.store.getState().remoteVideoStreams[userId];
    if (currentStream && currentStream.getVideoTracks().includes(track)) return;

    const videoStream = stream || new MockMediaStream([track]);
    this.store.getState().setRemoteVideoStream(userId, videoStream);
    track.onended = () => {
      const activeStream = this.store.getState().remoteVideoStreams[userId];
      if (activeStream && activeStream.getVideoTracks().includes(track)) {
        this.cleanupRemoteStream(userId);
      }
    };
  }

  attachRemoteStreamAudio(userId, track, receiver, stream) {
    if (track.readyState === 'ended') return;
    if (this.userStreamAudioTrackIds.get(userId) === track.id) return;

    this.userStreamAudioTrackIds.set(userId, track.id);
    const streamId = stream?.id || track.streamId;
    if (streamId) this.userScreenStreamIds.set(userId, streamId);
    track.onended = () => {
      if (this.userStreamAudioTrackIds.get(userId) === track.id) {
        this.cleanupRemoteStreamAudio(userId);
      }
    };
    track.contentHint = 'music';

    let streamPull = this.streamAudioElements.get(userId);
    if (!streamPull) {
      streamPull = new MockAudioElement();
      streamPull.autoplay = true;
      streamPull.muted = true;
      this.streamAudioElements.set(userId, streamPull);
    }
    streamPull.srcObject = stream || new MockMediaStream([track]);
    void streamPull.play().catch(() => {});

    if (this.streamSourceNodes.has(userId)) {
      try { this.streamSourceNodes.get(userId)?.disconnect(); } catch {}
      try { this.streamDelayNodes.get(userId)?.disconnect(); } catch {}
      try { this.streamGainNodes.get(userId)?.disconnect(); } catch {}
      try { this.streamLimiterNodes.get(userId)?.disconnect(); } catch {}
    }

    const source = this.outputMixContext.createMediaStreamSource(new MockMediaStream([track]));
    const delay = this.outputMixContext.createDelay(1.0);
    const gain = this.outputMixContext.createGain();
    const limiter = this.outputMixContext.createDynamicsCompressor();

    source.connect(delay);
    delay.connect(limiter);
    limiter.connect(gain);
    gain.connect(this.outputBusGain);

    this.streamSourceNodes.set(userId, source);
    this.streamDelayNodes.set(userId, delay);
    this.streamGainNodes.set(userId, gain);
    this.streamLimiterNodes.set(userId, limiter);
    this.streamSyncOffsetEma.delete(userId);
    this.streamSyncSkew.delete(userId);
    this.statsMonitoringActive = true;
  }

  cleanupRemoteStreamAudio(userId) {
    this.userStreamAudioTrackIds.delete(userId);
    this.userScreenStreamIds.delete(userId);

    const streamPull = this.streamAudioElements.get(userId);
    if (streamPull) {
      try {
        streamPull.pause();
        streamPull.srcObject = null;
      } catch {}
      this.streamAudioElements.delete(userId);
    }

    const streamSource = this.streamSourceNodes.get(userId);
    if (streamSource) {
      try { streamSource.disconnect(); } catch {}
      this.streamSourceNodes.delete(userId);
    }

    const streamGain = this.streamGainNodes.get(userId);
    if (streamGain) {
      try { streamGain.disconnect(); } catch {}
      this.streamGainNodes.delete(userId);
    }

    const streamLimiter = this.streamLimiterNodes.get(userId);
    if (streamLimiter) {
      try { streamLimiter.disconnect(); } catch {}
      this.streamLimiterNodes.delete(userId);
    }

    const streamDelay = this.streamDelayNodes.get(userId);
    if (streamDelay) {
      try { streamDelay.disconnect(); } catch {}
      this.streamDelayNodes.delete(userId);
    }

    this.streamSyncOffsetEma.delete(userId);
    this.streamSyncSkew.delete(userId);
    if (this.streamGainNodes.size === 0) {
      this.statsMonitoringActive = false;
    }
  }

  cleanupRemoteStream(userId) {
    this.cleanupRemoteStreamAudio(userId);
    this.store.getState().setRemoteVideoStream(userId, null);
  }

  disconnectFromPeer(userId, options = {}) {
    this.store.getState().setWebRTCConnectionStatus(userId, false);
    const pc = this.peerConnections.get(userId);
    if (pc) {
      pc.ontrack = null;
      pc.onicecandidate = null;
      pc.onconnectionstatechange = null;
      pc.oniceconnectionstatechange = null;
      pc.close();
      this.peerConnections.delete(userId);
    }

    const voiceGain = this.userGainNodes.get(userId);
    if (voiceGain) {
      try { voiceGain.disconnect(); } catch {}
      this.userGainNodes.delete(userId);
    }

    this.cleanupRemoteStreamAudio(userId);
    if (!options.preserveRemoteVideo) {
      this.store.getState().setRemoteVideoStream(userId, null);
    }

    this.pendingRenegotiation.delete(userId);
    this.peerSignalingQueues.delete(userId);
    this.isMakingOffer.delete(userId);
    this.isSettingRemoteAnswerPending.delete(userId);
    this.offerEpoch.delete(userId);
    this.rollbackCount.delete(userId);
  }

  leaveAll() {
    this.statsMonitoringActive = false;
    const peerIds = Array.from(this.peerConnections.keys());
    for (const userId of peerIds) {
      this.disconnectFromPeer(userId);
    }
    const audioIds = Array.from(this.streamAudioElements.keys());
    for (const userId of audioIds) {
      this.cleanupRemoteStreamAudio(userId);
    }
    const streamUsers = Object.keys(this.store.getState().remoteVideoStreams);
    for (const userId of streamUsers) {
      this.store.getState().setRemoteVideoStream(userId, null);
    }
  }

  enqueueSignalingTask(userId, task) {
    const currentQueue = this.peerSignalingQueues.get(userId) || Promise.resolve();
    const nextQueue = currentQueue
      .then(async () => {
        await task();
      })
      .catch(err => {
        // Log or handle signaling task error
      });
    this.peerSignalingQueues.set(userId, nextQueue);
    return nextQueue;
  }

  async renegotiatePeer(pc, userId, iceRestart = false) {
    return this.enqueueSignalingTask(userId, async () => {
      if (this.peerConnections.get(userId) !== pc || pc.signalingState === 'closed' || pc.connectionState === 'closed') {
        this.pendingRenegotiation.delete(userId);
        this.pendingIceRestart.delete(userId);
        return;
      }
      if (pc.signalingState !== 'stable' || this.isMakingOffer.get(userId)) {
        this.pendingRenegotiation.add(userId);
        if (iceRestart) this.pendingIceRestart.add(userId);
        return;
      }
      this.pendingRenegotiation.delete(userId);
      const needsIceRestart = iceRestart || this.pendingIceRestart.has(userId);
      this.isMakingOffer.set(userId, true);
      try {
        const offer = await pc.createOffer();
        if (this.peerConnections.get(userId) !== pc || pc.signalingState === 'closed' || pc.connectionState === 'closed') {
          this.pendingRenegotiation.delete(userId);
          this.pendingIceRestart.delete(userId);
          return;
        }
        if (pc.signalingState !== 'stable') {
          this.pendingRenegotiation.add(userId);
          if (needsIceRestart) this.pendingIceRestart.add(userId);
          return;
        }
        await pc.setLocalDescription(offer);
        const epoch = (this.offerEpoch.get(userId) ?? 0) + 1;
        this.offerEpoch.set(userId, epoch);
        this.signalRService.sendWebRTCOffer(userId, JSON.stringify({ type: pc.localDescription.type, sdp: pc.localDescription.sdp, epoch }));
        this.pendingIceRestart.delete(userId);
      } catch (e) {
        // error handling
      } finally {
        this.isMakingOffer.set(userId, false);
      }
    });
  }

  flushPendingRenegotiation(userId) {
    if (!this.pendingRenegotiation.has(userId)) return;
    const pc = this.peerConnections.get(userId);
    if (!pc || pc.signalingState !== 'stable' || this.isMakingOffer.get(userId)) return;
    this.pendingRenegotiation.delete(userId);
    void this.renegotiatePeer(pc, userId);
  }

  async recoverRejectedAnswer(userId, pc) {
    if (this.peerConnections.get(userId) !== pc || pc.connectionState === 'closed') {
      this.rollbackCount.delete(userId);
      void this.renegotiatePeer(pc, userId);
      return;
    }
    if (pc.signalingState !== 'have-local-offer') return;
    const attempts = (this.rollbackCount.get(userId) ?? 0) + 1;
    this.rollbackCount.set(userId, attempts);
    if (attempts > StreamEngine.MAX_ROLLBACKS) {
      this.rollbackCount.delete(userId);
      void this.renegotiatePeer(pc, userId);
      return;
    }
    try {
      await pc.setLocalDescription({ type: 'rollback' });
    } catch {
      void this.renegotiatePeer(pc, userId);
      return;
    }
    this.isMakingOffer.set(userId, false);
    this.pendingRenegotiation.add(userId);
    void this.renegotiatePeer(pc, userId);
  }

  async handleOffer(senderId, offerStr) {
    return this.enqueueSignalingTask(senderId, async () => {
      const pc = this.peerConnections.get(senderId);
      if (!pc || pc.connectionState === 'closed') return;
      try {
        const offer = JSON.parse(offerStr);
        const isMakingOffer = this.isMakingOffer.get(senderId) ?? false;
        const readyForOffer = !isMakingOffer && (pc.signalingState === 'stable' || (this.isSettingRemoteAnswerPending.get(senderId) ?? false));
        const offerCollision = !readyForOffer;

        const me = this.store.getState().currentUser?.id ?? this.userId;
        const isPolitePeer = me > senderId;

        if (offerCollision) {
          if (!isPolitePeer) {
            return;
          }
          if (pc.signalingState === 'have-local-offer') {
            await pc.setLocalDescription({ type: 'rollback' });
          }
          this.pendingRenegotiation.add(senderId);
        }

        await pc.setRemoteDescription(offer);
        const answer = await pc.createAnswer();
        await pc.setLocalDescription(answer);
        this.signalRService.sendWebRTCAnswer(senderId, JSON.stringify({ type: answer.type, sdp: answer.sdp, epoch: offer.epoch }));
        this.flushPendingRenegotiation(senderId);
        this.syncRemoteVideoStream(senderId);
      } catch (e) {
        // error handling
      }
    });
  }

  async handleAnswer(senderId, answerStr) {
    return this.enqueueSignalingTask(senderId, async () => {
      const pc = this.peerConnections.get(senderId);
      if (!pc || pc.connectionState === 'closed') return;
      this.isSettingRemoteAnswerPending.set(senderId, true);
      try {
        const answer = JSON.parse(answerStr);
        const currentEpoch = this.offerEpoch.get(senderId);
        const answerEpoch = answer.epoch;
        if (answerEpoch != null && currentEpoch != null && answerEpoch !== currentEpoch) {
          return;
        }
        if (pc.signalingState === 'have-local-offer') {
          await pc.setRemoteDescription(answer);
          this.rollbackCount.delete(senderId);
          this.flushPendingRenegotiation(senderId);
          this.syncRemoteVideoStream(senderId);
        }
      } catch (e) {
        await this.recoverRejectedAnswer(senderId, pc);
      } finally {
        this.isSettingRemoteAnswerPending.set(senderId, false);
      }
    });
  }

  async startScreenShare(videoTrack, audioTrack = null) {
    const stream = new MockMediaStream([videoTrack, ...(audioTrack ? [audioTrack] : [])]);
    this.localVideoStream = stream;
    for (const [userId, pc] of this.peerConnections.entries()) {
      if (videoTrack) {
        const videoTransceiver = pc.getTransceivers().find(t => t.receiver.track.kind === 'video' && !t.stopped);
        if (videoTransceiver) {
          videoTransceiver.direction = 'sendrecv';
          await videoTransceiver.sender.replaceTrack(videoTrack);
          videoTransceiver.sender.setStreams(stream);
        } else {
          pc.addTrack(videoTrack, stream);
        }
      }
      if (audioTrack) {
        const audioTransceivers = pc.getTransceivers().filter(t => t.receiver.track.kind === 'audio' && !t.stopped);
        const screenAudioTransceiver = audioTransceivers.length > 1 ? audioTransceivers[1] : null;
        if (screenAudioTransceiver) {
          screenAudioTransceiver.direction = 'sendrecv';
          await screenAudioTransceiver.sender.replaceTrack(audioTrack);
          screenAudioTransceiver.sender.setStreams(stream);
        } else {
          pc.addTrack(audioTrack, stream);
        }
      }
      await this.renegotiatePeer(pc, userId);
    }
  }

  stopScreenShare() {
    const localVideo = this.localVideoStream;
    const videoTracks = localVideo ? localVideo.getVideoTracks() : [];
    const screenAudioTracks = localVideo ? localVideo.getAudioTracks() : [];

    for (const [userId, pc] of this.peerConnections.entries()) {
      try {
        const transceivers = pc.getTransceivers();
        const audioTransceivers = transceivers.filter(t => t.receiver.track.kind === 'audio' && !t.stopped);
        for (const t of transceivers) {
          if (t.stopped) continue;
          const track = t.sender.track;
          const isScreenVideo = t.receiver.track.kind === 'video' || (track && track.kind === 'video') || (track && videoTracks.includes(track));
          const isScreenAudio = (audioTransceivers.length > 1 && audioTransceivers[1] === t) || (track && screenAudioTracks.includes(track));
          if (isScreenVideo || isScreenAudio) {
            void t.sender.replaceTrack(null);
            t.direction = 'recvonly';
          }
        }
        void this.renegotiatePeer(pc, userId);
      } catch {}
    }

    if (this.localVideoStream) {
      this.localVideoStream.getTracks().forEach(track => {
        try {
          track.enabled = false;
          track.stop();
        } catch {}
      });
      this.localVideoStream = null;
    }
  }
}
