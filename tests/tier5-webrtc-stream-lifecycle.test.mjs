import { test, describe, beforeEach } from 'node:test';
import assert from 'node:assert/strict';
import {
  MockMediaStreamTrack,
  MockMediaStream,
  MockRTCPeerConnection,
  MockGainNode,
  createMockAppStore,
  StreamEngine
} from './helpers/webrtc-stream-mock.mjs';

describe('Tier 5: WebRTC Remote Stream & Dynamic Negotiation Lifecycle', () => {
  let store;
  let engine;

  beforeEach(() => {
    store = createMockAppStore();
    engine = new StreamEngine(store);
  });

  // --------------------------------------------------------------------------
  // Suite 1: syncRemoteVideoStream Receiver Binding & Store Synchronization
  // --------------------------------------------------------------------------
  describe('Suite 1: syncRemoteVideoStream Receiver Binding & Store Synchronization', () => {
    test('S1-01: pre-existing video receiver binds immediately when user starts streaming', () => {
      const pc = new MockRTCPeerConnection();
      const videoTrack = new MockMediaStreamTrack({ kind: 'video', muted: false });
      const transceiver = pc.addTransceiver('video', { direction: 'recvonly' });
      transceiver.receiver.track = videoTrack;
      engine.initPeer('user_alpha', pc);

      store.setState({
        voiceUsers: [{ id: 'user_alpha', isStreaming: true, streamQuality: 'high' }]
      });

      engine.syncRemoteVideoStream('user_alpha');

      const stream = store.getState().remoteVideoStreams['user_alpha'];
      assert.ok(stream, 'Remote video stream should be attached to store');
      assert.equal(stream.getVideoTracks().length, 1);
      assert.equal(stream.getVideoTracks()[0].id, videoTrack.id);
      assert.equal(typeof videoTrack.onended, 'function', 'track.onended cleanup should be hooked');
    });

    test('S1-02: asynchronous receiver arrival via handleAnswer / handleOffer', () => {
      const pc = new MockRTCPeerConnection();
      engine.initPeer('user_alpha', pc);

      store.setState({
        voiceUsers: [{ id: 'user_alpha', isStreaming: true }]
      });

      // Receiver does not exist yet upon initial SignalR notification
      engine.syncRemoteVideoStream('user_alpha');
      assert.equal(
        store.getState().remoteVideoStreams['user_alpha'],
        undefined,
        'No stream should be bound before receiver arrives'
      );

      // Offer/answer exchange finishes, transceiver added asynchronously
      const videoTrack = new MockMediaStreamTrack({ kind: 'video', muted: false });
      const transceiver = pc.addTransceiver('video', { direction: 'recvonly' });
      transceiver.receiver.track = videoTrack;

      // Secondary sync invoked by handleAnswer / handleOffer
      engine.syncRemoteVideoStream('user_alpha');

      const stream = store.getState().remoteVideoStreams['user_alpha'];
      assert.ok(stream, 'Stream should be bound cleanly after transceiver arrival');
      assert.equal(stream.getVideoTracks()[0].id, videoTrack.id);
    });

    test('S1-03: idempotency on repeated sync invocations', () => {
      const pc = new MockRTCPeerConnection();
      const videoTrack = new MockMediaStreamTrack({ kind: 'video', muted: false });
      const transceiver = pc.addTransceiver('video', { direction: 'recvonly' });
      transceiver.receiver.track = videoTrack;
      engine.initPeer('user_alpha', pc);

      store.setState({
        voiceUsers: [{ id: 'user_alpha', isStreaming: true }]
      });

      engine.syncRemoteVideoStream('user_alpha');
      const initialStreamRef = store.getState().remoteVideoStreams['user_alpha'];

      let stateUpdates = 0;
      const unsubscribe = store.subscribe(() => {
        stateUpdates++;
      });

      for (let i = 0; i < 5; i++) {
        engine.syncRemoteVideoStream('user_alpha');
      }
      unsubscribe();

      const finalStreamRef = store.getState().remoteVideoStreams['user_alpha'];
      assert.equal(
        finalStreamRef,
        initialStreamRef,
        'Stream instance reference must remain identical across duplicate sync calls'
      );
      assert.equal(stateUpdates, 0, 'No redundant store mutations should be emitted');
    });

    test('S1-04: track replacement when streamer changes source', () => {
      const pc = new MockRTCPeerConnection();
      const trackA = new MockMediaStreamTrack({ id: 'track_A', kind: 'video', muted: false });
      const transceiver = pc.addTransceiver('video', { direction: 'recvonly' });
      transceiver.receiver.track = trackA;
      engine.initPeer('user_alpha', pc);

      store.setState({
        voiceUsers: [{ id: 'user_alpha', isStreaming: true }]
      });

      engine.syncRemoteVideoStream('user_alpha');
      assert.equal(
        store.getState().remoteVideoStreams['user_alpha'].getVideoTracks()[0].id,
        'track_A'
      );

      // Broadcaster switches from window capture to screen capture (track replaced)
      const trackB = new MockMediaStreamTrack({ id: 'track_B', kind: 'video', muted: false });
      transceiver.receiver.track = trackB;
      engine.syncRemoteVideoStream('user_alpha');

      assert.equal(
        store.getState().remoteVideoStreams['user_alpha'].getVideoTracks()[0].id,
        'track_B',
        'Store stream should reflect the newly replaced video track'
      );

      // When the decommissioned track A stops, it must NOT tear down the new stream
      trackA.stop();
      assert.ok(
        store.getState().remoteVideoStreams['user_alpha'],
        'Active stream must not be disrupted by old track termination'
      );
      assert.equal(
        store.getState().remoteVideoStreams['user_alpha'].getVideoTracks()[0].id,
        'track_B'
      );
    });

    test('S1-05: closed peer connection rejection', () => {
      const pc = new MockRTCPeerConnection();
      pc.connectionState = 'closed';
      const track = new MockMediaStreamTrack({ kind: 'video', muted: false });
      const transceiver = pc.addTransceiver('video', { direction: 'recvonly' });
      transceiver.receiver.track = track;
      engine.initPeer('user_alpha', pc);

      store.setState({
        voiceUsers: [{ id: 'user_alpha', isStreaming: true }]
      });

      engine.syncRemoteVideoStream('user_alpha');
      assert.equal(
        store.getState().remoteVideoStreams['user_alpha'],
        undefined,
        'Should not bind or attach when peer connection is closed'
      );
      assert.equal(track.onunmute, null);
      assert.equal(track.onended, null);
    });
  });

  // --------------------------------------------------------------------------
  // Suite 2: Complete Resource Cleanup & Call Teardown
  // --------------------------------------------------------------------------
  describe('Suite 2: Complete Resource Cleanup & Call Teardown', () => {
    test('S2-01: cleanupRemoteStream deletes remote video stream from store', () => {
      const track = new MockMediaStreamTrack({ kind: 'video', muted: false });
      const stream = new MockMediaStream([track]);
      store.setRemoteVideoStream('user_alpha', stream);
      store.setActiveStreamId('user_alpha');

      assert.ok(store.getState().remoteVideoStreams['user_alpha']);
      assert.equal(store.getState().activeStreamId, 'user_alpha');

      engine.cleanupRemoteStream('user_alpha');

      assert.equal(
        store.getState().remoteVideoStreams['user_alpha'],
        undefined,
        'Stream should be deleted from remoteVideoStreams dictionary'
      );
      assert.equal(
        store.getState().activeStreamId,
        null,
        'activeStreamId should be reset when the active stream is cleaned up'
      );
    });

    test('S2-02: screen audio Web Audio nodes cleanly disconnected', () => {
      const audioTrack = new MockMediaStreamTrack({ kind: 'audio', muted: false });
      engine.attachRemoteStreamAudio('user_alpha', audioTrack);

      const sourceNode = engine.streamSourceNodes.get('user_alpha');
      const delayNode = engine.streamDelayNodes.get('user_alpha');
      const gainNode = engine.streamGainNodes.get('user_alpha');
      const limiterNode = engine.streamLimiterNodes.get('user_alpha');

      assert.ok(sourceNode && delayNode && gainNode && limiterNode);
      assert.equal(sourceNode.isDisconnected, false);

      engine.cleanupRemoteStreamAudio('user_alpha');

      assert.equal(sourceNode.isDisconnected, true, 'MediaStreamSourceNode should be disconnected');
      assert.equal(delayNode.isDisconnected, true, 'DelayNode should be disconnected');
      assert.equal(gainNode.isDisconnected, true, 'GainNode should be disconnected');
      assert.equal(limiterNode.isDisconnected, true, 'DynamicsCompressorNode should be disconnected');
      assert.equal(engine.streamSourceNodes.has('user_alpha'), false);
      assert.equal(engine.streamDelayNodes.has('user_alpha'), false);
      assert.equal(engine.streamGainNodes.has('user_alpha'), false);
      assert.equal(engine.streamLimiterNodes.has('user_alpha'), false);
    });

    test('S2-03: HTML5 audio element paused and disassociated', () => {
      const audioTrack = new MockMediaStreamTrack({ kind: 'audio', muted: false });
      engine.attachRemoteStreamAudio('user_alpha', audioTrack);

      const audioElement = engine.streamAudioElements.get('user_alpha');
      assert.ok(audioElement);
      assert.equal(audioElement.paused, false);
      assert.ok(audioElement.srcObject);

      engine.cleanupRemoteStreamAudio('user_alpha');

      assert.equal(audioElement.paused, true, 'Audio element must be paused');
      assert.equal(audioElement.srcObject, null, 'srcObject must be disassociated');
      assert.equal(engine.streamAudioElements.has('user_alpha'), false);
    });

    test('S2-04: track.onended triggers automatic remote stream cleanup', () => {
      const pc = new MockRTCPeerConnection();
      const videoTrack = new MockMediaStreamTrack({ kind: 'video', muted: false });
      const transceiver = pc.addTransceiver('video', { direction: 'recvonly' });
      transceiver.receiver.track = videoTrack;
      engine.initPeer('user_alpha', pc);

      store.setState({
        voiceUsers: [{ id: 'user_alpha', isStreaming: true }]
      });

      engine.syncRemoteVideoStream('user_alpha');
      assert.ok(store.getState().remoteVideoStreams['user_alpha']);

      // Broadcaster terminates sharing at OS level
      videoTrack.stop();

      assert.equal(
        store.getState().remoteVideoStreams['user_alpha'],
        undefined,
        'Remote stream must be cleaned up when track ends'
      );
    });

    test('S2-05: channel exit (leaveAll) cleans all active streaming peers', () => {
      const pc1 = new MockRTCPeerConnection();
      const pc2 = new MockRTCPeerConnection();
      const pc3 = new MockRTCPeerConnection();
      engine.initPeer('peer_1', pc1);
      engine.initPeer('peer_2', pc2);
      engine.initPeer('peer_3', pc3);

      const track1 = new MockMediaStreamTrack({ kind: 'video', muted: false });
      const track2 = new MockMediaStreamTrack({ kind: 'video', muted: false });
      pc1.addTransceiver('video').receiver.track = track1;
      pc2.addTransceiver('video').receiver.track = track2;

      store.setState({
        voiceUsers: [
          { id: 'peer_1', isStreaming: true },
          { id: 'peer_2', isStreaming: true },
          { id: 'peer_3', isStreaming: false }
        ]
      });

      engine.syncRemoteVideoStream('peer_1');
      engine.syncRemoteVideoStream('peer_2');
      assert.ok(store.getState().remoteVideoStreams['peer_1']);
      assert.ok(store.getState().remoteVideoStreams['peer_2']);

      engine.leaveAll();

      assert.equal(pc1.connectionState, 'closed');
      assert.equal(pc2.connectionState, 'closed');
      assert.equal(pc3.connectionState, 'closed');
      assert.deepEqual(store.getState().remoteVideoStreams, {});
      assert.equal(engine.streamAudioElements.size, 0);
      assert.equal(engine.statsMonitoringActive, false);
    });
  });

  // --------------------------------------------------------------------------
  // Suite 3: Track Mute State, Fallback Logic & Inbound RTP Transitions
  // --------------------------------------------------------------------------
  describe('Suite 3: Track Mute State, Fallback Logic & Inbound RTP Transitions', () => {
    test('S3-01: signaling-first arrival with muted track attaches immediately', () => {
      const pc = new MockRTCPeerConnection();
      // Track arrives muted by Chromium engine default before first RTP packet
      const videoTrack = new MockMediaStreamTrack({ kind: 'video', muted: true });
      const transceiver = pc.addTransceiver('video', { direction: 'recvonly' });
      transceiver.receiver.track = videoTrack;
      engine.initPeer('user_alpha', pc);

      store.setState({
        voiceUsers: [{ id: 'user_alpha', isStreaming: true }]
      });

      engine.syncRemoteVideoStream('user_alpha');

      assert.ok(
        store.getState().remoteVideoStreams['user_alpha'],
        'Track must attach immediately when isUserStreaming is true even if muted'
      );
    });

    test('S3-02: RTP-first arrival: unmuted track attaches without signaling', () => {
      const pc = new MockRTCPeerConnection();
      // Inbound RTP started before SignalR notification was received
      const videoTrack = new MockMediaStreamTrack({ kind: 'video', muted: false });
      const transceiver = pc.addTransceiver('video', { direction: 'recvonly' });
      transceiver.receiver.track = videoTrack;
      engine.initPeer('user_alpha', pc);

      store.setState({
        voiceUsers: [{ id: 'user_alpha', isStreaming: false }]
      });

      engine.syncRemoteVideoStream('user_alpha');

      assert.ok(
        store.getState().remoteVideoStreams['user_alpha'],
        'Track must attach immediately when !track.muted is true even if isUserStreaming is false'
      );
    });

    test('S3-03: delayed signaling: muted track un-mutes and triggers fallback', () => {
      const pc = new MockRTCPeerConnection();
      const videoTrack = new MockMediaStreamTrack({ kind: 'video', muted: true });
      const transceiver = pc.addTransceiver('video', { direction: 'recvonly' });
      transceiver.receiver.track = videoTrack;
      engine.initPeer('user_alpha', pc);

      store.setState({
        voiceUsers: [{ id: 'user_alpha', isStreaming: false }]
      });

      engine.syncRemoteVideoStream('user_alpha');

      // Neither condition met yet
      assert.equal(store.getState().remoteVideoStreams['user_alpha'], undefined);
      assert.equal(typeof videoTrack.onunmute, 'function', 'track.onunmute must be registered');

      // First inbound RTP arrives, Chromium transitions track.muted to false
      videoTrack.muted = false;

      assert.ok(
        store.getState().remoteVideoStreams['user_alpha'],
        'Stream must attach once track un-mutes'
      );
    });

    test('S3-04: screen audio muted fallback', () => {
      const pc = new MockRTCPeerConnection();
      engine.initPeer('user_alpha', pc);

      const audioTrack = new MockMediaStreamTrack({ kind: 'audio', muted: true });
      const transceiver = pc.addTransceiver('audio', { direction: 'recvonly' });
      transceiver.receiver.track = audioTrack;

      store.setState({
        voiceUsers: [{ id: 'user_alpha', isStreaming: false }]
      });

      engine.syncRemoteVideoStream('user_alpha');
      assert.equal(engine.streamSourceNodes.has('user_alpha'), false);
      assert.equal(typeof audioTrack.onunmute, 'function');

      // Inbound audio RTP arrives
      audioTrack.muted = false;

      assert.equal(engine.streamSourceNodes.has('user_alpha'), true);
      assert.equal(engine.userStreamAudioTrackIds.get('user_alpha'), audioTrack.id);

      // Redundant unmute should not duplicate audio nodes
      audioTrack.emit('unmute');
      assert.equal(engine.streamSourceNodes.size, 1);
    });

    test('S3-05: premature track end while muted', () => {
      const pc = new MockRTCPeerConnection();
      const videoTrack = new MockMediaStreamTrack({ kind: 'video', muted: true });
      const transceiver = pc.addTransceiver('video', { direction: 'recvonly' });
      transceiver.receiver.track = videoTrack;
      engine.initPeer('user_alpha', pc);

      store.setState({
        voiceUsers: [{ id: 'user_alpha', isStreaming: true }]
      });

      videoTrack.stop(); // transitioned to 'ended'
      engine.syncRemoteVideoStream('user_alpha');

      assert.equal(
        store.getState().remoteVideoStreams['user_alpha'],
        undefined,
        'Ended track must be rejected immediately'
      );
    });
  });

  // --------------------------------------------------------------------------
  // Suite 4: Renegotiation Concurrency, Queueing & Perfect Negotiation Collision
  // --------------------------------------------------------------------------
  describe('Suite 4: Renegotiation Concurrency, Queueing & Perfect Negotiation Collision', () => {
    test('S4-01: rapid start/stop/restart screen sharing burst', async () => {
      const pc = new MockRTCPeerConnection();
      pc.addTransceiver('video', { direction: 'recvonly' });
      engine.initPeer('user_alpha', pc);

      const track1 = new MockMediaStreamTrack({ kind: 'video', muted: false });
      const track2 = new MockMediaStreamTrack({ kind: 'video', muted: false });

      // Simulate user rapid clicking: start -> stop -> restart in <50ms
      const p1 = engine.startScreenShare(track1);
      engine.stopScreenShare();
      const p2 = engine.startScreenShare(track2);

      await Promise.all([p1, p2]);

      const videoTransceiver = pc.getTransceivers().find(t => t.kind === 'video');
      assert.equal(
        videoTransceiver.direction,
        'sendrecv',
        'Final transceiver direction must match final user action'
      );
      assert.equal(videoTransceiver.sender.track, track2);
    });

    test('S4-02: simultaneous offer collision: polite peer rolls back', async () => {
      // Local peer is polite ('user_999' > 'user_111')
      store.setState({ currentUser: { id: 'user_999' } });
      const pc = new MockRTCPeerConnection();
      engine.initPeer('user_111', pc);

      // Local peer creates offer in flight
      pc.signalingState = 'have-local-offer';
      pc.localDescription = { type: 'offer', sdp: 'local offer' };
      engine.isMakingOffer.set('user_111', true);

      // Remote colliding offer arrives from user_111
      const remoteOffer = JSON.stringify({ type: 'offer', sdp: 'remote offer', epoch: 1 });
      await engine.handleOffer('user_111', remoteOffer);

      assert.equal(
        pc.signalingState,
        'stable',
        'Polite peer rolls back local offer, accepts remote offer, and returns answer to reach stable'
      );
      assert.ok(engine.sentAnswers.length > 0, 'Polite peer must dispatch answer to colliding remote offer');
    });

    test('S4-03: simultaneous offer collision: impolite peer ignores offer', async () => {
      // Local peer is impolite ('user_111' < 'user_999')
      store.setState({ currentUser: { id: 'user_111' } });
      const pc = new MockRTCPeerConnection();
      engine.initPeer('user_999', pc);

      // Local peer has local offer in flight
      pc.signalingState = 'have-local-offer';
      pc.localDescription = { type: 'offer', sdp: 'local offer' };
      engine.isMakingOffer.set('user_999', true);

      // Remote colliding offer arrives from user_999
      const remoteOffer = JSON.stringify({ type: 'offer', sdp: 'remote offer', epoch: 1 });
      await engine.handleOffer('user_999', remoteOffer);

      assert.equal(
        pc.signalingState,
        'have-local-offer',
        'Impolite peer must retain have-local-offer and ignore colliding offer'
      );
      assert.equal(
        engine.sentAnswers.length,
        0,
        'Impolite peer must not send answer to colliding offer'
      );

      // When remote polite peer yields and answers local offer
      const remoteAnswer = JSON.stringify({ type: 'answer', sdp: 'remote answer', epoch: 1 });
      engine.offerEpoch.set('user_999', 1);
      await engine.handleAnswer('user_999', remoteAnswer);

      assert.equal(pc.signalingState, 'stable', 'Both peers converge to stable');
    });

    test('S4-04: outdated answer epoch rejection', async () => {
      const pc = new MockRTCPeerConnection();
      pc.signalingState = 'have-local-offer';
      engine.initPeer('user_beta', pc);
      engine.offerEpoch.set('user_beta', 2); // local offer epoch is 2

      // Stale answer arrives from prior negotiation epoch 1
      const staleAnswer = JSON.stringify({ type: 'answer', sdp: 'stale answer', epoch: 1 });
      await engine.handleAnswer('user_beta', staleAnswer);

      assert.equal(
        pc.signalingState,
        'have-local-offer',
        'Stale answer with epoch mismatch must be dropped without corrupting signaling state'
      );

      // Matching answer arrives
      const validAnswer = JSON.stringify({ type: 'answer', sdp: 'valid answer', epoch: 2 });
      await engine.handleAnswer('user_beta', validAnswer);

      assert.equal(pc.signalingState, 'stable', 'Matching answer epoch is accepted');
    });

    test('S4-05: offer rejection recovery within rollback limit', async () => {
      const pc = new MockRTCPeerConnection();
      pc.signalingState = 'have-local-offer';
      engine.initPeer('user_beta', pc);

      // Simulate consecutive answer rejections up to MAX_ROLLBACKS (3)
      for (let attempt = 1; attempt <= 3; attempt++) {
        await engine.recoverRejectedAnswer('user_beta', pc);
        assert.equal(engine.rollbackCount.get('user_beta'), attempt);
        pc.signalingState = 'have-local-offer'; // reset for next simulation
      }

      // 4th failure exceeds MAX_ROLLBACKS (3) -> triggers full renegotiation and resets counter
      await engine.recoverRejectedAnswer('user_beta', pc);
      assert.equal(engine.rollbackCount.has('user_beta'), false);
    });
  });

  // --------------------------------------------------------------------------
  // Suite 5: Topology Isolation (1-on-1 Direct Calls vs Group Channel Voice Streams)
  // --------------------------------------------------------------------------
  describe('Suite 5: Topology Isolation (1-on-1 Direct Calls vs Group Channel Voice Streams)', () => {
    test('S5-01: 1-on-1 direct call stream resolution via currentCallUser', () => {
      const pc = new MockRTCPeerConnection();
      const videoTrack = new MockMediaStreamTrack({ kind: 'video', muted: false });
      pc.addTransceiver('video', { direction: 'recvonly' }).receiver.track = videoTrack;
      engine.initPeer('partner_direct', pc);

      // Partner is not in voiceUsers, but is active in currentCallUser
      store.setState({
        voiceUsers: [],
        currentCallUser: { id: 'partner_direct', isStreaming: true }
      });

      engine.syncRemoteVideoStream('partner_direct');

      const stream = store.getState().remoteVideoStreams['partner_direct'];
      assert.ok(stream, 'Direct call partner stream should be resolved and attached');
      assert.equal(stream.getVideoTracks()[0].id, videoTrack.id);
    });

    test('S5-02: direct call teardown (endCall) isolation', () => {
      const pc = new MockRTCPeerConnection();
      const videoTrack = new MockMediaStreamTrack({ kind: 'video', muted: false });
      const audioTrack = new MockMediaStreamTrack({ kind: 'audio', muted: false });
      pc.addTransceiver('video', { direction: 'recvonly' }).receiver.track = videoTrack;
      engine.initPeer('partner_direct', pc);

      store.setState({
        voiceUsers: [],
        currentCallUser: { id: 'partner_direct', isStreaming: true }
      });

      engine.syncRemoteVideoStream('partner_direct');
      engine.attachRemoteStreamAudio('partner_direct', audioTrack);
      assert.ok(store.getState().remoteVideoStreams['partner_direct']);
      assert.ok(engine.streamSourceNodes.has('partner_direct'));

      // Direct call ends: disconnectFromPeer called for partner only
      engine.disconnectFromPeer('partner_direct');
      store.setState({ currentCallUser: null });

      assert.equal(store.getState().remoteVideoStreams['partner_direct'], undefined);
      assert.equal(engine.streamSourceNodes.has('partner_direct'), false);
      assert.equal(engine.peerConnections.has('partner_direct'), false);
      assert.equal(store.getState().currentCallUser, null);
    });

    test('S5-03: group channel multi-peer stream isolation', () => {
      const pcA = new MockRTCPeerConnection();
      const pcB = new MockRTCPeerConnection();
      engine.initPeer('user_A', pcA);
      engine.initPeer('user_B', pcB);

      const trackA = new MockMediaStreamTrack({ kind: 'video', muted: false });
      pcA.addTransceiver('video').receiver.track = trackA;

      const userBGain = new MockGainNode(1.0);
      engine.userGainNodes.set('user_B', userBGain);
      engine.userSpeaking.set('user_B', true);

      store.setState({
        voiceUsers: [
          { id: 'user_A', isStreaming: true },
          { id: 'user_B', isStreaming: false, isSpeaking: true },
          { id: 'user_C', isStreaming: false },
          { id: 'user_D', isStreaming: false }
        ]
      });

      engine.syncRemoteVideoStream('user_A');
      assert.ok(store.getState().remoteVideoStreams['user_A']);

      // User C changes User A's stream volume
      const audioTrackA = new MockMediaStreamTrack({ kind: 'audio', muted: false });
      engine.attachRemoteStreamAudio('user_A', audioTrackA);
      const streamGainA = engine.streamGainNodes.get('user_A');
      streamGainA.gain.linearRampToValueAtTime(0.45, 0);

      assert.equal(streamGainA.gain.value, 0.45);
      assert.equal(userBGain.gain.value, 1.0, 'User B voice volume must remain untouched');
      assert.equal(engine.userSpeaking.get('user_B'), true, 'User B speaking state must remain intact');
      assert.equal(store.getState().remoteVideoStreams['user_B'], undefined);
    });

    test('S5-04: direct call to group channel switch', () => {
      const pcDirect = new MockRTCPeerConnection();
      const directVideoTrack = new MockMediaStreamTrack({ kind: 'video', muted: false });
      const directAudioTrack = new MockMediaStreamTrack({ kind: 'audio', muted: false });
      pcDirect.addTransceiver('video').receiver.track = directVideoTrack;
      engine.initPeer('direct_partner', pcDirect);

      store.setState({
        currentCallUser: { id: 'direct_partner', isStreaming: true },
        voiceUsers: []
      });

      engine.syncRemoteVideoStream('direct_partner');
      engine.attachRemoteStreamAudio('direct_partner', directAudioTrack);
      assert.ok(store.getState().remoteVideoStreams['direct_partner']);
      assert.ok(engine.streamAudioElements.has('direct_partner'));

      // Transition: leave direct call and join group channel
      engine.disconnectFromPeer('direct_partner');
      store.setState({ currentCallUser: null });

      assert.equal(store.getState().remoteVideoStreams['direct_partner'], undefined);
      assert.equal(engine.streamAudioElements.has('direct_partner'), false);

      // Join group channel with peers X and Y
      const pcX = new MockRTCPeerConnection();
      const pcY = new MockRTCPeerConnection();
      engine.initPeer('peer_X', pcX);
      engine.initPeer('peer_Y', pcY);

      store.setState({
        voiceUsers: [
          { id: 'peer_X', isStreaming: false },
          { id: 'peer_Y', isStreaming: false }
        ]
      });

      assert.equal(engine.peerConnections.has('peer_X'), true);
      assert.equal(engine.peerConnections.has('peer_Y'), true);
      assert.equal(engine.peerConnections.has('direct_partner'), false);
      assert.deepEqual(store.getState().remoteVideoStreams, {});
    });
  });
});
