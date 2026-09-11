import * as signalR from '@microsoft/signalr';
import { useAppStore, User, VoiceChannel, ChannelUpdate, IncomingCall, ChannelInvite } from '../store/useAppStore';
import { webrtc } from './webrtc';
import { chatPeer, configureChatSignaling } from './chatPeer';
import i18n from '../i18n';
import callRingSound from '../assets/sounds/call.mp3';
import channelJoinSound from '../assets/sounds/join.mp3';
import channelLeaveSound from '../assets/sounds/leave.mp3';
import achievementSound from '../assets/sounds/achievement.mp3';
import { registerHiddenAchievement } from '../achievements';

const SERVER_URL = "https://vnkboltik.ru:8080/zabor_v3";

const CLIENT_REJECTION_PREFIX = 'ZABOR_CLIENT_REJECTED';
const AUTH_THROTTLE_PREFIX = 'ZABOR_AUTH_THROTTLED';

export interface PingStats {
  ping: number;
  jitter: number;
  loss: number;
  missed: number;
}

const OFFLINE_PING_STATS: PingStats = { ping: -1, jitter: 0, loss: 0, missed: 0 };

const RECONNECT_GIVE_UP_MS = 95000;
const RECONNECT_MAX_STEP_MS = 30000;

const RECONNECT_POLICY: signalR.IRetryPolicy = {
  nextRetryDelayInMilliseconds(ctx) {
    if (ctx.elapsedMilliseconds > RECONNECT_GIVE_UP_MS) return null;
    const step = Math.min(1000 * 2 ** ctx.previousRetryCount, RECONNECT_MAX_STEP_MS);
    return Math.round(step * (0.5 + Math.random()));
  }
};

export interface IceServerConfig {
  urls: string[];
  username?: string;
  credential?: string;
}

export interface IceConfig {
  iceServers: IceServerConfig[];
  expiresAtUnixMs: number;
  ttlSeconds: number;
}

class SignalRService {
  private connection: signalR.HubConnection | null = null;
  private listenersAttached = false;
  private reconnectAttempts = 0;
  private reconnectTimer: NodeJS.Timeout | null = null;
  private isReconnecting = false;
  private intentionalDisconnect = false;
  private sessionReady = false;
  private pingTimer: NodeJS.Timeout | null = null;
  private pingInFlight = false;
  private pingWindow: (number | null)[] = [];
  private currentPingStats: PingStats = OFFLINE_PING_STATS;
  private lastSpeakingState: boolean | null = null;
  private lastReportedUtcOffset: number | null = null;
  private timeZoneUnsupported = false;
  private wasInChannel: string | null = null;
  private wasMuted = false;
  private wasDeafened = false;
  private wasStreaming = false;
  private wasStreamQuality = 'low';
  private sfxContext: AudioContext | null = null;
  private sfxElements: Map<string, HTMLAudioElement> = new Map();
  private streamDropTimers: Map<string, NodeJS.Timeout> = new Map();
  private speakingDebounceTimer: NodeJS.Timeout | null = null;

  private isJoiningChannel = false;
  private voiceOperationId = 0;
  private voiceOperationDone: Promise<void> | null = null;
  private voiceOperationResolve: (() => void) | null = null;
  private voiceOperationTimer: NodeJS.Timeout | null = null;

  private isStartingCall = false;
  private isAcceptingCall = false;
  private isPreparingToQuit = false;

  private static readonly CONNECT_WAIT_TIMEOUT_MS = 15000;
  private static readonly CONNECT_START_TIMEOUT_MS = 35000;
  private static readonly INVOKE_TIMEOUT_MS = 20000;
  private static readonly VOICE_OPERATION_TIMEOUT_MS = 75000;
  private static readonly PING_INTERVAL_MS = 5000;
  private static readonly PING_TIMEOUT_MS = 10000;
  private static readonly PING_WINDOW_SIZE = 10;
  private static readonly PING_MISSES_BEFORE_OFFLINE = 2;
  private static readonly PEER_CONNECT_STAGGER_MS = 60;
  private static readonly PEER_CONNECT_DELAY_MS = 150;
  private static readonly STREAM_DROP_GRACE_MS = 30000;
  private static readonly SPEAKING_HANGOVER_MS = 350;

  private playSfx(src: string, volume = 0.5) {
    try {
      let audio = this.sfxElements.get(src);
      if (!audio) {
        audio = new Audio(src);
        this.sfxElements.set(src, audio);
      }
      audio.volume = volume;
      audio.currentTime = 0;
      audio.play().catch(() => { });
    } catch { }
  }

  private stopSfx(src: string) {
    const audio = this.sfxElements.get(src);
    if (audio) {
      audio.pause();
      audio.currentTime = 0;
    }
  }

  private clearStreamDropTimer(userId: string) {
    const timer = this.streamDropTimers.get(userId);
    if (timer) {
      clearTimeout(timer);
      this.streamDropTimers.delete(userId);
    }
  }

  private pingCallbacks: Set<(ping: number) => void> = new Set();
  private connectionCallbacks: Set<(isConnected: boolean) => void> = new Set();
  private forceLogoutCallbacks: Set<() => void> = new Set();
  private lastNotifiedConnected: boolean | null = null;

  public isConnected(): boolean {
    return this.connection?.state === signalR.HubConnectionState.Connected;
  }

  public getPing(): number {
    return this.currentPingStats.ping;
  }

  public getPingStats(): PingStats {
    return this.currentPingStats;
  }

  public onPingUpdate(callback: (ping: number) => void): () => void {
    this.pingCallbacks.add(callback);
    callback(this.currentPingStats.ping);
    return () => this.pingCallbacks.delete(callback);
  }

  public onConnectionUpdate(callback: (isConnected: boolean) => void): () => void {
    this.connectionCallbacks.add(callback);
    callback(this.isConnected());
    return () => this.connectionCallbacks.delete(callback);
  }

  public onForceLogout(callback: () => void): () => void {
    this.forceLogoutCallbacks.add(callback);
    return () => this.forceLogoutCallbacks.delete(callback);
  }

  private publishPingStats(stats: PingStats) {
    this.currentPingStats = stats;
    this.pingCallbacks.forEach(cb => cb(stats.ping));
  }

  private notifyConnectionUpdate(isConnected: boolean) {
    if (this.lastNotifiedConnected === isConnected) return;
    this.lastNotifiedConnected = isConnected;
    this.connectionCallbacks.forEach(cb => cb(isConnected));
  }

  private getSfxContext(masterGain: number): { ctx: AudioContext; master: GainNode } | null {
    try {
      if (!this.sfxContext) {
        try {
          this.sfxContext = new AudioContext({ sampleRate: 48000 });
        } catch (e) {
          console.warn('[SignalR] Failed to create sfxContext at 48000Hz, falling back to default:', e);
          this.sfxContext = new AudioContext();
        }
      }
      if (this.sfxContext.state === 'suspended') this.sfxContext.resume().catch(() => { });
      const ctx = this.sfxContext;
      const master = ctx.createGain();
      master.gain.value = masterGain;
      master.connect(ctx.destination);
      return { ctx, master };
    } catch { return null; }
  }

  private missedPings = 0;

  private startPingMeasurement() {
    this.stopPingMeasurement();
    this.missedPings = 0;

    const scheduleNext = () => {
      if (this.pingTimer) clearTimeout(this.pingTimer);
      this.pingTimer = setTimeout(measurePing, SignalRService.PING_INTERVAL_MS);
    };

    const measurePing = async () => {
      if (this.pingInFlight) { scheduleNext(); return; }
      if (!this.isConnected()) {
        this.resetPingStats();
        scheduleNext();
        return;
      }
      this.pingInFlight = true;
      const start = performance.now();
      try {
        await this.invokeWithTimeout<number>("Ping", SignalRService.PING_TIMEOUT_MS);
        this.missedPings = 0;
        this.recordPingSample(Math.round(performance.now() - start));
      } catch {
        this.missedPings++;
        this.recordPingSample(null);
      } finally {
        this.pingInFlight = false;
        scheduleNext();
        void this.reportTimeZone();
      }
    };

    void measurePing();
  }

  private recordPingSample(rtt: number | null) {
    this.pingWindow.push(rtt);
    if (this.pingWindow.length > SignalRService.PING_WINDOW_SIZE) this.pingWindow.shift();
    this.publishPingStats(this.analyzePing());
  }

  private analyzePing(): PingStats {
    const answered = this.pingWindow.filter((value): value is number => value !== null);
    const loss = this.pingWindow.length
      ? Math.round(((this.pingWindow.length - answered.length) / this.pingWindow.length) * 100)
      : 0;
    const missed = this.missedPings;

    if (answered.length === 0 || missed >= SignalRService.PING_MISSES_BEFORE_OFFLINE) {
      return { ...OFFLINE_PING_STATS, loss, missed };
    }

    const sorted = [...answered].sort((a, b) => a - b);
    const ping = sorted[Math.floor(sorted.length / 2)];
    const jitter = Math.round(
      answered.reduce((sum, value) => sum + Math.abs(value - ping), 0) / answered.length
    );

    return { ping, jitter, loss, missed };
  }

  private resetPingStats() {
    this.pingWindow = [];
    this.missedPings = 0;
    this.publishPingStats(OFFLINE_PING_STATS);
  }

  private stopPingMeasurement() {
    if (this.pingTimer) { clearTimeout(this.pingTimer); this.pingTimer = null; }
    this.pingWindow = [];
  }

  public lastConnectionError: string | null = null;

  private clientRejectedReason: string | null = null;
  private authThrottleMessage: string | null = null;
  private registerError: string | null = null;

  public get lastAuthThrottleMessage(): string | null {
    return this.authThrottleMessage;
  }

  public get lastRegisterError(): string | null {
    return this.registerError;
  }

  private parseAuthThrottle(error: unknown): string | null {
    const raw = error instanceof Error ? error.message : String(error ?? '');
    const index = raw.indexOf(AUTH_THROTTLE_PREFIX);
    if (index === -1) return null;
    const seconds = Number(raw.slice(index + AUTH_THROTTLE_PREFIX.length + 1).match(/^\d+/)?.[0] ?? 0);
    const minutes = Math.max(1, Math.ceil(seconds / 60));
    return i18n.t('validation.authThrottled', { minutes });
  }

  public get isClientRejected(): boolean {
    return this.clientRejectedReason !== null;
  }

  private parseClientRejection(rawError: string): { reason: string; message: string } | null {
    const index = rawError.indexOf(CLIENT_REJECTION_PREFIX);
    if (index === -1) return null;
    const tail = rawError.slice(index + CLIENT_REJECTION_PREFIX.length + 1);
    const reason = tail.match(/^[a-z_]+/)?.[0] || 'unknown';
    const key = `main.connection.clientRejected.${reason}`;
    return {
      reason,
      message: i18n.exists(key) ? i18n.t(key) : i18n.t('main.connection.clientRejected.unknown')
    };
  }

  private applyClientRejection(rejection: { reason: string; message: string }) {
    this.clientRejectedReason = rejection.reason;
    this.lastConnectionError = rejection.message;
    this.isReconnecting = false;
    this.sessionReady = false;
    if (this.reconnectTimer) { clearTimeout(this.reconnectTimer); this.reconnectTimer = null; }
    this.stopPingMeasurement();
    this.resetPingStats();
    this.notifyConnectionUpdate(false);
  }

  public async connect(): Promise<boolean> {
    if (this.isConnected()) {
      this.isReconnecting = false;
      this.reconnectAttempts = 0;
      if (!this.pingTimer) this.startPingMeasurement();
      this.notifyConnectionUpdate(true);
      return true;
    }
    if (this.clientRejectedReason) return false;
    if (this.isReconnecting) {
      const deadline = Date.now() + SignalRService.CONNECT_WAIT_TIMEOUT_MS;
      while (this.isReconnecting && Date.now() < deadline) {
        await new Promise<void>(resolve => setTimeout(resolve, 200));
      }
      return this.isConnected();
    }
    this.intentionalDisconnect = false;
    this.isReconnecting = true;
    this.sessionReady = false;
    this.lastConnectionError = null;
    this.timeZoneUnsupported = false;
    this.lastReportedUtcOffset = null;
    let connection: signalR.HubConnection | null = null;
    try {
      if (this.connection) {
        try {

          await Promise.race([
            this.connection.stop(),
            new Promise<void>(resolve => setTimeout(resolve, 2000))
          ]);
        } catch { }
      }
      this.listenersAttached = false;
      connection = new signalR.HubConnectionBuilder()
        .withUrl(SERVER_URL, {
          skipNegotiation: false,
          transport: signalR.HttpTransportType.WebSockets | signalR.HttpTransportType.LongPolling,

          accessTokenFactory: async () => {
            try {
              return (await window.windowControls?.getClientAttestation?.()) ?? '';
            } catch {
              return '';
            }
          }
        })
        .withAutomaticReconnect(RECONNECT_POLICY)
        .build();
      this.connection = connection;
      connection.serverTimeoutInMilliseconds = 45000;
      connection.keepAliveIntervalInMilliseconds = 8000;
      this.setupListeners();
      this.setupReconnectionHandlers(connection);

      const startWithTimeout = Promise.race([
        connection.start(),
        new Promise<never>((_, reject) =>
          setTimeout(() => reject(new Error('Connection start timeout')), SignalRService.CONNECT_START_TIMEOUT_MS)
        )
      ]);
      await startWithTimeout;

      if (this.connection !== connection) {
        await connection.stop().catch(() => { });
        return this.isConnected();
      }

      this.reconnectAttempts = 0;
      this.isReconnecting = false;
      this.startPingMeasurement();

      this.notifyConnectionUpdate(true);
      return true;
    } catch (err: any) {
      if (connection && this.connection !== connection) {
        connection.stop().catch(() => { });
        return this.isConnected();
      }
      if (connection && this.connection === connection) {
        this.connection = null;
        connection.stop().catch(() => { });
      }
      this.isReconnecting = false;
      const rawError = err?.message || String(err);
      const rejection = this.parseClientRejection(rawError);
      if (rejection) {
        this.applyClientRejection(rejection);
        return false;
      }
      this.lastConnectionError = rawError;
      this.resetPingStats();
      this.notifyConnectionUpdate(false);
      this.scheduleReconnect();
      return false;
    }
  }

  private saveReconnectionState() {
    const store = useAppStore.getState();
    if (store.currentChannelId) {
      this.wasInChannel = store.currentChannelId;
    }
    if (store.currentUser) {
      this.wasMuted = store.currentUser.isMuted || store.currentUser.isServerMuted || false;
      this.wasDeafened = store.currentUser.isDeafened || store.currentUser.isServerDeafened || false;
      this.wasStreaming = webrtc.localVideoStream !== null;
      this.wasStreamQuality = store.currentUser.streamQuality || 'low';
    }
  }

  private setupReconnectionHandlers(connection: signalR.HubConnection) {
    connection.onreconnecting(() => {
      if (this.connection !== connection) return;
      if (this.intentionalDisconnect) return;
      this.isReconnecting = true;
      this.sessionReady = false;
      this.resetPingStats();
      this.saveReconnectionState();
      this.notifyConnectionUpdate(false);
    });
    connection.onreconnected(async () => {
      if (this.connection !== connection) return;
      this.reconnectAttempts = 0;
      this.isReconnecting = false;
      this.timeZoneUnsupported = false;
      this.lastReportedUtcOffset = null;
      this.startPingMeasurement();
      this.notifyConnectionUpdate(true);
    });
    connection.onclose((error) => {
      if (this.connection !== connection) return;
      this.isReconnecting = false;
      this.sessionReady = false;
      this.resetPingStats();
      this.stopPingMeasurement();

      const rejection = error ? this.parseClientRejection(error.message || String(error)) : null;
      if (rejection) {
        this.saveReconnectionState();
        this.applyClientRejection(rejection);
        return;
      }
      if (!this.intentionalDisconnect) {
        this.saveReconnectionState();
        this.notifyConnectionUpdate(false);
        this.scheduleReconnect();
      }
    });
  }

  private scheduleReconnect() {
    if (this.reconnectTimer || this.intentionalDisconnect || this.clientRejectedReason) return;
    this.reconnectAttempts++;
    const base = Math.min(2000 * 2 ** Math.min(this.reconnectAttempts - 1, 4), 30000);
    const delay = base + Math.floor(Math.random() * 1000);
    this.reconnectTimer = setTimeout(async () => {
      this.reconnectTimer = null;
      await this.connect();
    }, delay);
  }

  public disconnect() {
    this.intentionalDisconnect = true;
    this.isReconnecting = false;
    this.sessionReady = false;
    this.reconnectAttempts = 0;
    this.lastNotifiedConnected = null;
    this.lastSpeakingState = null;
    this.wasInChannel = null;
    this.wasMuted = false;
    this.wasDeafened = false;
    this.wasStreaming = false;
    this.wasStreamQuality = 'low';
    this.stopPingMeasurement();
    if (this.reconnectTimer) { clearTimeout(this.reconnectTimer); this.reconnectTimer = null; }
    if (this.connection) {
      const c = this.connection;
      this.connection = null;
      this.listenersAttached = false;
      Promise.race([c.stop(), new Promise<void>(resolve => setTimeout(resolve, 2000))]).catch(() => { });
    }
    if (this.sfxContext) { this.sfxContext.close().catch(() => { }); this.sfxContext = null; }
    this.sfxElements.forEach(audio => { audio.pause(); audio.srcObject = null; });
    this.sfxElements.clear();
    this.streamDropTimers.forEach(timer => clearTimeout(timer));
    this.streamDropTimers.clear();
  }

  private setupListeners() {
    if (!this.connection || this.listenersAttached) return;
    const connection = this.connection;
    this.listenersAttached = true;
    const store = useAppStore.getState;
    const on = (methodName: string, handler: (...args: any[]) => void | Promise<void>) => {
      connection.on(methodName, (...args: any[]) => {
        if (this.connection !== connection) return;
        return handler(...args);
      });
    };

    on("SyncFullChannelState", (stateMap: Record<string, User[]>) => {
      store().setFullChannelState(stateMap);
    });

    on("UserJoined", (user: User) => {
      store().updateUserStatus(user.id, { ...user, isOnline: true });
      void chatPeer.handleOnline(user.id);
    });

    on("UserLeft", (userId: string) => {
      chatPeer.handleOffline(userId);
      this.clearStreamDropTimer(userId);
      store().updateUserStatus(userId, { isOnline: false, currentChannelId: null, currentCallUserId: null, isSpeaking: false });
      store().removeUserFromChannelMap('', userId);
      webrtc.disconnectFromPeer(userId);
    });

    on("UserUpdated", (user: User) => {
      if (store().currentUser?.id === user.id) store().setCurrentUser({ ...store().currentUser!, ...user });
      store().updateUserStatus(user.id, user);
    });

    on("UserJoinedChannel", (user: User, channelId?: string) => {
      if (!channelId) return;
      const state = store();
      if (user.id === state.currentUser?.id) {
        if (state.isJoiningChannel) return;
        if (state.currentChannelId !== channelId) return;
      }
      store().removeUserFromChannelMap('', user.id);
      store().updateUserStatus(user.id, { ...user, currentChannelId: channelId, currentCallUserId: null, isOnline: true });
      store().addUserToChannelMap(channelId, { ...user, currentChannelId: channelId, currentCallUserId: null });
      if (store().currentChannelId === channelId && user.id !== store().currentUser?.id) {
        webrtc.disconnectFromPeer(user.id);
        webrtc.connectToPeer(user.id);
        this.playSfx(channelJoinSound, 0.3);
      }
    });

    on("UserLeftChannel", (userId: string, channelId?: string) => {
      if (!channelId) return;
      const state = store();
      if (userId === state.currentUser?.id) {
        if (state.isJoiningChannel) return;
        if (state.currentChannelId && state.currentChannelId !== channelId) return;
      }
      const knownChannelId = state.friends.find(user => user.id === userId)?.currentChannelId
        ?? Object.entries(state.channelUsersMap).find(([, users]) => users.some(user => user.id === userId))?.[0];
      state.removeUserFromChannelMap(channelId, userId);
      if (knownChannelId === channelId) {
        state.updateUserStatus(userId, { currentChannelId: null, isSpeaking: false });
      }
      if (state.currentChannelId === channelId) webrtc.disconnectFromPeer(userId);
      if (store().currentChannelId === channelId && userId !== store().currentUser?.id) {
        this.playSfx(channelLeaveSound, 0.3);
      }
    });

    on("ChannelCreated", (channel: VoiceChannel) => {
      const channels = store().channels;

      const withoutOptimistic = channels.filter(c => !c.id.startsWith('__opt_') || c.name !== channel.name);
      if (!withoutOptimistic.find(c => c.id === channel.id)) {
        store().setChannels([...withoutOptimistic, channel]);
      } else {
        store().setChannels(withoutOptimistic);
      }
    });

    on("ChannelUpdated", (channel: VoiceChannel) => {
      store().setChannels(store().channels.map(c => c.id === channel.id ? channel : c));
      if (store().selectedChannelForMembers?.id === channel.id) store().setSelectedChannelForMembers(channel);
    });

    on("ChannelDeleted", (channelId: string) => {
      store().setChannels(store().channels.filter(c => c.id !== channelId));
      if (store().currentChannelId === channelId) this.leaveChannel();
      if (store().selectedChannelForMembers?.id === channelId) store().setModal('channelMembers', false);
    });

    on("ForceLeaveVoice", async () => { await this.leaveChannel(); });

    on("UserStateChanged", (update: any) => {
      const uId = update.userId || update.UserId;
      if (!uId) return;

      const nextUpdates: Partial<User> = {
        isMuted: update.isMuted ?? update.IsMuted ?? false,
        isDeafened: update.isDeafened ?? update.IsDeafened ?? false,
        isSpeaking: update.isSpeaking ?? update.IsSpeaking ?? false,
        isServerMuted: update.isServerMuted ?? update.IsServerMuted ?? false,
        isServerDeafened: update.isServerDeafened ?? update.IsServerDeafened ?? false
      };
      store().updateUserStatus(uId, nextUpdates);

      const currentUser = store().currentUser;
      if (currentUser && uId === currentUser.id) {

        const newServerMuted = update.isServerMuted ?? update.IsServerMuted ?? currentUser.isServerMuted ?? false;
        const newServerDeafened = update.isServerDeafened ?? update.IsServerDeafened ?? currentUser.isServerDeafened ?? false;

        const effectiveMuted = currentUser.isMuted || newServerMuted;
        const effectiveDeafened = currentUser.isDeafened || newServerDeafened;
        webrtc.toggleMute(effectiveMuted);
        webrtc.setDeafened(effectiveDeafened);

        store().setCurrentUser({
          ...currentUser,

          isServerMuted: newServerMuted,
          isServerDeafened: newServerDeafened,
          isSpeaking: update.isSpeaking ?? update.IsSpeaking ?? currentUser.isSpeaking
        });
      }
    });

    on("UserSpeaking", (userId: string, isSpeaking: boolean) => {
      if (store().currentUser?.id === userId) store().setSpeakingStatus(userId, isSpeaking);
    });

    on("UserStartedStreaming", (userId: string, streamQuality: string) => {
      this.clearStreamDropTimer(userId);
      store().updateUserStatus(userId, { isStreaming: true, streamQuality });
    });

    on("UserStoppedStreaming", (userId: string) => {
      this.clearStreamDropTimer(userId);
      const appStore = store();
      if (appStore.activeStreamId === userId) {
        appStore.setActiveStreamId(null);
      }
      appStore.updateUserStatus(userId, { isStreaming: false, streamQuality: undefined });
      webrtc.cleanupRemoteStream(userId);
    });

    on("StreamDropped", (userId: string) => {
      this.clearStreamDropTimer(userId);
      const timer = setTimeout(() => {
        this.streamDropTimers.delete(userId);
        const appStore = store();
        if (appStore.activeStreamId === userId) {
          appStore.setActiveStreamId(null);
        }
        appStore.updateUserStatus(userId, { isStreaming: false, streamQuality: undefined });
        webrtc.cleanupRemoteStream(userId);
      }, SignalRService.STREAM_DROP_GRACE_MS);
      this.streamDropTimers.set(userId, timer);
    });

    on("FriendRequestReceived", (user: User) => {
      if (!store().friendRequests.find((r: User) => r.id === user.id)) {
        store().setFriendRequests([...store().friendRequests, user]);
        this.playNotificationSound();
      }
    });

    on("FriendRequestAccepted", (user: User) => {
      if (!store().friends.find((f: User) => f.id === user.id)) store().setFriends([...store().friends, user]);
    });

    on("FriendAdded", (user: User) => {
      if (!store().friends.find((f: User) => f.id === user.id)) store().setFriends([...store().friends, user]);
      store().setFriendRequests(store().friendRequests.filter((r: User) => r.id !== user.id));
    });

    on("FriendRemoved", (userId: string) => {
      store().setFriends(store().friends.filter((f: User) => f.id !== userId));
    });

    on("ReceiveChannelInvite", async (invite: ChannelInvite) => {
      const invites = store().channelInvites;
      if (!invites.find(i => i.channelId === invite.channelId)) {
        store().setChannelInvites([...invites, invite]);
        this.playNotificationSound();
      }
    });

    on("IncomingChannelCall", (call: IncomingCall, channelId: string, channelName: string) => {
      store().setIncomingChannelInvite({ senderId: call.callerId, senderName: call.callerName, channelId, channelName });
      store().setModal('incomingChannelInvite', true);
      this.playRingtone();
    });

    on("IncomingCall", async (call: IncomingCall) => {
      const state = store();
      if (state.callStatus === 'calling' && state.currentCallUser?.id === call.callerId) {
        await this.acceptCall(call.callerId);
        return;
      }
      state.setIncomingCall(call);
      state.setModal('incomingCall', true);
      this.playRingtone();
    });

    on("CallAccepted", (user: User) => {
      store().setCurrentCallUser(user);
      store().setCallStatus('connected');
      store().setIncomingCall(null);
      store().setModal('incomingCall', false);
      this.stopRingtone();
      webrtc.connectToPeer(user.id);
      this.playSfx(channelJoinSound, 0.3);
    });

    on("CallDeclined", () => {
      store().setCallStatus('idle');
      store().setIncomingCall(null);
      store().setModal('incomingCall', false);
      store().setCurrentCallUser(null);
      this.stopRingtone();
      webrtc.enterBackgroundMode();
    });

    on("CallEnded", () => {
      const callStatus = store().callStatus;
      const callUser = store().currentCallUser;
      if (store().currentUser?.isStreaming || webrtc.localVideoStream) {
        webrtc.stopScreenShare();
        this.safeInvoke("StopStream");
        const currentUser = store().currentUser;
        if (currentUser) {
          store().updateUserStatus(currentUser.id, { isStreaming: false, streamQuality: undefined });
        }
      }
      if (callUser) webrtc.disconnectFromPeer(callUser.id);
      store().setIncomingCall(null);
      store().setModal('incomingCall', false);
      store().setCurrentCallUser(null);
      store().setCallStatus('idle');
      store().setActiveStreamId(null);
      store().setStreamFullscreen(false);
      this.stopRingtone();

      if (callStatus === 'connected') {
        this.playSfx(channelLeaveSound, 0.3);
      }
      webrtc.enterBackgroundMode();
    });

    on("CallStarted", (user: User) => {

      store().setCurrentCallUser(user);
      store().setCallStatus('connected');
      store().setIncomingCall(null);
      store().setModal('incomingCall', false);
      this.stopRingtone();

      this.playSfx(channelJoinSound, 0.3);
    });

    on("AchievementUnlocked", (achievementId: string, hiddenDef?: any) => {
      if (hiddenDef) {
        registerHiddenAchievement(hiddenDef, true);
      }
      store().setAchievementToast(achievementId);
      setTimeout(() => store().setAchievementToast('__hiding__' + achievementId), 4500);
      setTimeout(() => store().setAchievementToast(null), 5000);
      this.playSfx(achievementSound, 0.4);
    });

    on("ReceiveWebRTCOffer", async (sId: string, o: string) => {
      if (chatPeer.isChatSignal(o)) { try { await chatPeer.handleOffer(sId, o); } catch { } }
      else await webrtc.handleOffer(sId, o);
    });
    on("ReceiveWebRTCAnswer", async (sId: string, a: string) => {
      if (chatPeer.isChatSignal(a)) { try { await chatPeer.handleAnswer(sId, a); } catch { } }
      else await webrtc.handleAnswer(sId, a);
    });
    on("ReceiveIceCandidate", async (sId: string, c: string) => {
      if (chatPeer.isChatSignal(c)) { try { await chatPeer.handleIceCandidate(sId, c); } catch { } }
      else await webrtc.handleIceCandidate(sId, c);
    });
    on("ReceiveChatRelay", async (sId: string, packet: string) => {
      try { await chatPeer.handleFallback(sId, packet); } catch { }
    });
    on("ReceiveStreamViewState", (sId: string, state: string) => {
      webrtc.applyViewerState(sId, state === 'preview' ? 'preview' : 'watching');
    });

    on("ForceLogout", async () => {
      try { await window.windowControls?.clearSession?.(); await window.windowControls?.wipeAppData?.(); } catch { }
      const appStore = useAppStore.getState();
      appStore.logout();
      appStore.setFullChannelState({});
      appStore.clearChannelMemberData();
      this.disconnect();
      this.forceLogoutCallbacks.forEach(cb => {
        try { cb(); } catch (e) { console.error('[SignalR] onForceLogout callback error:', e); }
      });
    });

    window.windowControls?.onBeforeQuit?.(() => { void this.prepareForQuit(); });
  }

  private notificationAudio: HTMLAudioElement | null = null;
  private ringtoneInterval: NodeJS.Timeout | null = null;

  private playNotificationSound() {
  }

  public playRingtone(volume = 0.3) {
    const currentUser = useAppStore.getState().currentUser;
    if (currentUser?.isDeafened || currentUser?.isServerDeafened) return;
    const audio = this.sfxElements.get(callRingSound);
    if (audio && !audio.paused) return;
    this.stopRingtone();
    this.playSfx(callRingSound, volume);
    const newAudio = this.sfxElements.get(callRingSound);
    if (newAudio) newAudio.loop = true;
  }

  public stopRingtone() {
    if (this.ringtoneInterval) { clearInterval(this.ringtoneInterval); this.ringtoneInterval = null; }
    this.stopSfx(callRingSound);
    const audio = this.sfxElements.get(callRingSound);
    if (audio) audio.loop = false;
  }

  private async ensureConnected(): Promise<boolean> {
    if (this.isConnected()) return true;
    if (this.intentionalDisconnect || this.clientRejectedReason) return false;

    if (!this.isReconnecting) {
      void this.connect();
    }

    const deadline = Date.now() + SignalRService.CONNECT_WAIT_TIMEOUT_MS;
    while (Date.now() < deadline) {
      if (this.isConnected()) return true;
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    return this.isConnected();
  }

  private async ensureSessionReady(): Promise<boolean> {
    if (this.sessionReady && this.isConnected()) return true;
    if (!await this.ensureConnected()) return false;

    const deadline = Date.now() + SignalRService.CONNECT_WAIT_TIMEOUT_MS;
    while (Date.now() < deadline) {
      if (this.sessionReady && this.isConnected()) return true;
      if (!this.isConnected() || this.intentionalDisconnect || this.clientRejectedReason) return false;
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    return this.sessionReady && this.isConnected();
  }

  private beginVoiceOperation(): number {
    this.isJoiningChannel = true;
    const operationId = ++this.voiceOperationId;
    this.voiceOperationDone = new Promise<void>(resolve => {
      this.voiceOperationResolve = resolve;
    });
    if (this.voiceOperationTimer) clearTimeout(this.voiceOperationTimer);
    this.voiceOperationTimer = setTimeout(() => {
      if (operationId !== this.voiceOperationId) return;
      console.warn('[SignalR] voice operation watchdog fired, releasing the guard');
      this.finishVoiceOperation(operationId);
    }, SignalRService.VOICE_OPERATION_TIMEOUT_MS);
    useAppStore.getState().setIsJoiningChannel(true);
    return operationId;
  }

  private finishVoiceOperation(operationId: number) {
    if (operationId === this.voiceOperationId) {
      this.isJoiningChannel = false;
      useAppStore.getState().setIsJoiningChannel(false);
      if (this.voiceOperationTimer) { clearTimeout(this.voiceOperationTimer); this.voiceOperationTimer = null; }
    }
    this.voiceOperationResolve?.();
    this.voiceOperationResolve = null;
    this.voiceOperationDone = null;
  }

  private async restoreVoiceChannel(channelId: string | null, users: User[], operationId: number): Promise<boolean> {
    if (!channelId || operationId !== this.voiceOperationId || !await this.ensureSessionReady()) {
      return false;
    }
    try {
      const update = await this.invokeWithTimeout<ChannelUpdate | null>(
        "JoinChannel", SignalRService.INVOKE_TIMEOUT_MS, { channelId }
      );
      if (operationId !== this.voiceOperationId || !update?.users) return false;
      const state = useAppStore.getState();
      webrtc.leaveAll();
      state.commitVoiceChannel(channelId, update.users.length > 0 ? update.users : users);
      state.setCallStatus('idle');
      state.setCurrentCallUser(null);
      this.connectToPeersStaggered(
        update.users.filter(user => user.id !== state.currentUser?.id).map(user => user.id),
        operationId,
        channelId
      );
      return true;
    } catch {
      return false;
    }
  }

  private invokeWithTimeout<T>(method: string, timeoutMs: number, ...args: any[]): Promise<T> {
    const connection = this.connection;
    if (!connection) return Promise.reject(new Error(`${method}: no connection`));
    let timer: NodeJS.Timeout | undefined;
    const timeout = new Promise<never>((_, reject) => {
      timer = setTimeout(() => reject(new Error(`${method} timeout after ${timeoutMs}ms`)), timeoutMs);
    });
    return Promise.race([connection.invoke<T>(method, ...args), timeout])
      .finally(() => { if (timer) clearTimeout(timer); });
  }

  private connectToPeersStaggered(userIds: string[], operationId: number, channelId: string) {
    userIds.forEach((userId, index) => {
      setTimeout(() => {
        if (operationId !== this.voiceOperationId) return;
        if (useAppStore.getState().currentChannelId !== channelId) return;
        webrtc.connectToPeer(userId);
      }, index * SignalRService.PEER_CONNECT_STAGGER_MS);
    });
  }

  private async safeInvoke<T>(method: string, ...args: any[]): Promise<T | null> {
    if (!await this.ensureSessionReady()) return null;
    try { return await this.connection!.invoke<T>(method, ...args); }
    catch (error) {
      console.warn(`[SignalR] ${method} failed`, error);
      return null;
    }
  }

  private async invokeCommand(method: string, ...args: any[]): Promise<boolean> {
    if (!await this.ensureSessionReady()) return false;
    try {
      await this.connection!.invoke(method, ...args);
      return true;
    } catch (error) {
      console.warn(`[SignalR] ${method} failed`, error);
      return false;
    }
  }

  public async fetchIceServers(): Promise<IceConfig | null> {
    if (!await this.ensureSessionReady()) return null;
    try {
      return await this.invokeWithTimeout<IceConfig>("GetIceServers", 5000);
    } catch (error) {
      console.warn("[SignalR] GetIceServers failed", error);
      return null;
    }
  }

  private getUtcOffsetMinutes(): number {
    return -new Date().getTimezoneOffset();
  }

  private async reportTimeZone(force = false): Promise<void> {
    if (this.timeZoneUnsupported) return;
    const offset = this.getUtcOffsetMinutes();
    if (!force && offset === this.lastReportedUtcOffset) return;
    if (!this.isConnected()) return;
    if (!this.sessionReady) return;
    try {
      await this.connection!.invoke("ReportTimeZone", offset);
      this.lastReportedUtcOffset = offset;
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      if (message.includes('Method does not exist')) {
        this.timeZoneUnsupported = true;
        console.info(
          '[SignalR] Сервер не поддерживает ReportTimeZone — достижения по времени' +
          ' («Ранняя птица», «Ночная сова») считаются по часам сервера. Пройдёт после его обновления.'
        );
        return;
      }
      console.warn('[SignalR] ReportTimeZone failed', error);
    }
  }

  public async checkUserExists(username: string): Promise<boolean> {
    if (!await this.ensureConnected()) return false;
    this.authThrottleMessage = null;
    try {
      return await this.connection!.invoke<boolean>("CheckUserExists", username);
    } catch (e) {
      this.authThrottleMessage = this.parseAuthThrottle(e);
      return false;
    }
  }

  public async login(username: string, password: string): Promise<'ok' | 'invalid' | 'network' | 'throttled'> {
    if (!this.isConnected()) return 'network';
    this.authThrottleMessage = null;
    try {
      const user = await this.invokeWithTimeout<User | null>(
        "Login", SignalRService.INVOKE_TIMEOUT_MS, username, password
      );
      if (user) {
        this.sessionReady = true;
        useAppStore.getState().setCurrentUser(user);
        webrtc.warmUpConnectivity();

        await this.reportTimeZone(true);

        const channelToRejoin = this.wasInChannel;
        const streamToRejoin = this.wasStreaming;
        const streamQuality = this.wasStreamQuality;
        const mutedToRestore = this.wasMuted;
        const deafenedToRestore = this.wasDeafened;

        this.wasInChannel = null;
        this.wasStreaming = false;
        this.wasMuted = false;
        this.wasDeafened = false;

        await this.loadData();

        if (mutedToRestore || deafenedToRestore) {
          const store = useAppStore.getState();
          if (store.currentUser) {
            store.setCurrentUser({
              ...store.currentUser,
              isMuted: mutedToRestore,
              isDeafened: deafenedToRestore
            });
            this.toggleState(mutedToRestore, deafenedToRestore);
          }
        }

        if (channelToRejoin) {
          const res = await this.joinChannel(channelToRejoin);
          if (res === 'ok' && streamToRejoin && webrtc.localVideoStream) {
            const isStreamActive = webrtc.localVideoStream.getTracks().some(t => t.readyState === 'live');
            if (isStreamActive) {
              await this.startStream(streamQuality);
            }
          }
        }

        return 'ok';
      }
      this.sessionReady = false;
      return 'invalid';
    } catch (e) {
      this.sessionReady = false;
      const throttled = this.parseAuthThrottle(e);
      if (throttled) {
        this.authThrottleMessage = throttled;
        console.warn('[SignalR] Login throttled by the server');
        return 'throttled';
      }
      console.error("[SignalR] Login error:", e);
      return 'network';
    }
  }

  public async getRegistrationCaptcha(): Promise<{ id: string; imageBase64: string } | null> {
    if (!await this.ensureConnected()) return null;
    try {
      const res = await this.connection!.invoke<any>("GetRegistrationCaptcha");
      if (!res) return null;
      return {
        id: String(res.id ?? res.Id ?? ''),
        imageBase64: String(res.imageBase64 ?? res.ImageBase64 ?? '')
      };
    } catch (e) {
      console.error("[SignalR] GetRegistrationCaptcha error:", e);
      return null;
    }
  }

  public async register(
    username: string,
    password: string,
    displayName: string,
    avatarBase64: string | null,
    avatarColor: string,
    captchaId?: string,
    captchaAnswer?: string
  ): Promise<boolean> {
    if (!await this.ensureConnected()) return false;
    this.authThrottleMessage = null;
    this.registerError = null;
    let user: User | null = null;
    try {
      user = await this.connection!.invoke<User>(
        "Register",
        username,
        password,
        displayName,
        avatarBase64,
        avatarColor,
        captchaId,
        captchaAnswer
      );
    } catch (e: any) {
      this.authThrottleMessage = this.parseAuthThrottle(e);
      const raw = e instanceof Error ? e.message : String(e ?? '');
      if (raw.includes("CAPTCHA_INVALID")) {
        this.registerError = "CAPTCHA_INVALID";
      } else if (raw.includes("CAPTCHA_EXPIRED")) {
        this.registerError = "CAPTCHA_EXPIRED";
      } else if (raw.includes("CAPTCHA_REQUIRED")) {
        this.registerError = "CAPTCHA_REQUIRED";
      }
      return false;
    }
    if (user) {
      this.sessionReady = true;
      useAppStore.getState().setCurrentUser(user);
      webrtc.warmUpConnectivity();
      await this.reportTimeZone(true);
      if (user.currentChannelId) {
        this.joinChannel(user.currentChannelId).catch(() => { });
      }
      await this.loadData();
      return true;
    }
    return false;
  }

  public async updateProfile(displayName: string, avatarBase64: string | null, avatarColor: string, aboutMe: string = ''): Promise<void> {
    await this.safeInvoke("UpdateProfile", displayName, avatarBase64, avatarColor, aboutMe);
  }

  public async changePassword(newPassword: string): Promise<boolean> {
    return await this.safeInvoke<boolean>("UpdateUserPassword", newPassword) ?? false;
  }

  public async deleteAccount(): Promise<boolean> {
    return await this.safeInvoke<boolean>("DeleteMyAccount") ?? false;
  }

  public async saveAudioSettings(settings: {
    inputVolume: number;
    outputVolume: number;
    selectedInput: string;
    selectedOutput: string;
    noiseSuppression: boolean;
    userVolumes?: Record<string, number>;
    language?: string;
    micThresholdMode?: 'auto' | 'manual';
    manualThresholdValue?: number;
  }): Promise<void> {
    await this.safeInvoke("SaveAudioSettings", JSON.stringify(settings));
  }

  public async loadAudioSettings(): Promise<{
    inputVolume: number;
    outputVolume: number;
    selectedInput: string;
    selectedOutput: string;
    noiseSuppression: boolean;
    userVolumes?: Record<string, number>;
    language?: string;
    micThresholdMode?: 'auto' | 'manual';
    manualThresholdValue?: number;
  } | null> {
    const json = await this.safeInvoke<string>("GetAudioSettings");
    if (!json) return null;
    try { return JSON.parse(json); } catch { return null; }
  }

  public async getMyAchievements(): Promise<any> {
    const json = await this.safeInvoke<string>("GetMyAchievements");
    if (json) {
      try {
        const raw = JSON.parse(json);
        const unlockedHiddenDefs = raw.UnlockedHiddenDefs || raw.unlockedHiddenDefs || [];
        if (Array.isArray(unlockedHiddenDefs)) {
          for (const def of unlockedHiddenDefs) {
            registerHiddenAchievement(def, true);
          }
        }
        return {
          stats: raw.Stats || raw.stats || {},
          unlockedIds: raw.UnlockedIds || raw.unlockedIds || [],
          visitedChannelIds: raw.VisitedChannelIds || raw.visitedChannelIds || [],
          totalHiddenCount: raw.TotalHiddenCount ?? raw.totalHiddenCount ?? 10,
          unlockedHiddenDefs
        };
      } catch { }
    }
    return { stats: {}, unlockedIds: [], visitedChannelIds: [], totalHiddenCount: 10, unlockedHiddenDefs: [] };
  }

  public async getUserAchievements(userId: string): Promise<any> {
    const json = await this.safeInvoke<string>("GetUserAchievements", userId);
    if (json) {
      try {
        const raw = JSON.parse(json);
        const unlockedHiddenDefs = raw.UnlockedHiddenDefs || raw.unlockedHiddenDefs || [];
        if (Array.isArray(unlockedHiddenDefs)) {
          for (const def of unlockedHiddenDefs) {
            registerHiddenAchievement(def, false);
          }
        }
        return {
          stats: raw.Stats || raw.stats || {},
          unlockedIds: raw.UnlockedIds || raw.unlockedIds || [],
          visitedChannelIds: raw.VisitedChannelIds || raw.visitedChannelIds || [],
          totalHiddenCount: raw.TotalHiddenCount ?? raw.totalHiddenCount ?? 10,
          unlockedHiddenDefs
        };
      } catch { }
    }
    return { stats: {}, unlockedIds: [], visitedChannelIds: [], totalHiddenCount: 10, unlockedHiddenDefs: [] };
  }

  public async viewProfile(userId: string): Promise<void> { await this.safeInvoke("ViewProfile", userId); }
  public async getUserByUsername(username: string): Promise<User | null> {
    return await this.safeInvoke<User>("GetUserByUsername", username);
  }
  public async getJokeOfTheDay(): Promise<string> {
    for (let attempt = 0; attempt < 3; attempt++) {
      const joke = await this.safeInvoke<string>("GetJokeOfTheDay");
      if (typeof joke === 'string' && joke.trim() && joke.trim() !== '0') return joke;
      if (attempt < 2) await new Promise(resolve => setTimeout(resolve, 1000 * (attempt + 1)));
    }
    return '';
  }

  public async loadData(): Promise<void> {
    const [channels, friends, requests, channelInvites] = await Promise.all([
      this.safeInvoke<VoiceChannel[]>("GetChannels"),
      this.safeInvoke<User[]>("GetFriends"),
      this.safeInvoke<User[]>("GetFriendRequests"),
      this.safeInvoke<ChannelInvite[]>("GetChannelInvites")
    ]);
    const store = useAppStore.getState();
    if (Array.isArray(channels)) store.setChannels(channels);
    if (Array.isArray(friends)) store.setFriends(friends);
    if (Array.isArray(requests)) store.setFriendRequests(requests);
    if (Array.isArray(channelInvites)) store.setChannelInvites(channelInvites);

  }

  public async createChannel(name: string): Promise<void> {
    const store = useAppStore.getState();
    const currentUser = store.currentUser;
    if (!currentUser) return;

    const tempId = `__opt_${Date.now()}`;
    const optimistic: VoiceChannel = { id: tempId, name: name.trim(), ownerId: currentUser.id };
    store.setChannels([...store.channels, optimistic]);

    const result = await this.safeInvoke<VoiceChannel>("CreateChannel", name);

    if (!result) {

      useAppStore.getState().setChannels(useAppStore.getState().channels.filter(c => c.id !== tempId));
    } else {

      const current = useAppStore.getState().channels;
      useAppStore.getState().setChannels(current.filter(c => c.id !== tempId));
    }
  }

  public async updateChannel(id: string, name: string): Promise<void> {
    const store = useAppStore.getState();
    const prevChannels = store.channels;

    store.setChannels(prevChannels.map(c => c.id === id ? { ...c, name: name.trim() } : c));

    const result = await this.safeInvoke<boolean>("UpdateChannel", { channelId: id, name });
    if (!result) useAppStore.getState().setChannels(prevChannels);
  }

  public async quitAccessChannel(channelId: string): Promise<void> {
    const store = useAppStore.getState();
    const prevChannels = store.channels;

    store.setChannels(prevChannels.filter(c => c.id !== channelId));

    if (store.currentChannelId === channelId) {
      webrtc.leaveAll();
      webrtc.enterBackgroundMode();
      store.setCurrentChannelId(null);
      store.setVoiceUsers([]);
    }

    const succeeded = await this.invokeCommand("QuitAccessChannel", channelId);
    if (!succeeded) useAppStore.getState().setChannels(prevChannels);
  }

  public async kickFromChannel(channelId: string, userId: string): Promise<void> {
    const store = useAppStore.getState();
    const prevMembers = store.channelMembers;
    const prevCache = store.channelMembersCache[channelId] || [];

    store.setChannelMembers(prevMembers.filter(m => m.id !== userId));
    store.setChannelMembersCache(channelId, prevCache.filter(m => m.id !== userId));

    const succeeded = await this.invokeCommand("KickFromChannel", channelId, userId);
    if (!succeeded) {
      const latestStore = useAppStore.getState();
      if (latestStore.selectedChannelForMembers?.id === channelId) latestStore.setChannelMembers(prevMembers);
      latestStore.setChannelMembersCache(channelId, prevCache);
    }
  }

  public async getChannelMembersList(channelId: string): Promise<User[] | null> {
    return await this.safeInvoke<User[]>("GetChannelMembersList", channelId);
  }

  public async sendChannelInvite(targetUserId: string, channelId: string, channelName: string): Promise<boolean> {
    return await this.invokeCommand("SendChannelInvite", targetUserId, channelId, channelName);
  }

  public async callToChannel(targetUserId: string, channelId: string, channelName: string): Promise<boolean> {
    return await this.invokeCommand("CallToChannel", targetUserId, channelId, channelName);
  }

  public async acceptChannelInvite(channelId: string): Promise<boolean> {
    const store = useAppStore.getState();
    const previousInvites = store.channelInvites;
    store.setChannelInvites(previousInvites.filter(i => i.channelId !== channelId));
    const succeeded = await this.invokeCommand("AcceptChannelInvite", channelId);
    if (!succeeded) {
      useAppStore.getState().setChannelInvites(previousInvites);
      return false;
    }
    return true;
  }

  public async declineChannelInvite(channelId: string): Promise<boolean> {
    const store = useAppStore.getState();
    const previousInvites = store.channelInvites;
    store.setChannelInvites(previousInvites.filter(i => i.channelId !== channelId));
    const succeeded = await this.invokeCommand("DeclineChannelInvite", channelId);
    if (!succeeded) {
      useAppStore.getState().setChannelInvites(previousInvites);
      return false;
    }
    return true;
  }

  public async joinChannel(channelId: string): Promise<'ok' | 'network' | 'mic_failed' | 'full'> {
    if (this.isJoiningChannel || this.voiceOperationDone) return 'network';
    const operationId = this.beginVoiceOperation();
    try {
      return await this._joinChannelImpl(channelId, operationId);
    } finally {
      this.finishVoiceOperation(operationId);
    }
  }

  private async _joinChannelImpl(channelId: string, operationId: number): Promise<'ok' | 'network' | 'mic_failed' | 'full'> {
    const startedAt = performance.now();
    const laps: string[] = [];
    let lapAt = startedAt;
    const lap = (label: string) => {
      const now = performance.now();
      laps.push(`${label} ${Math.round(now - lapAt)}ms`);
      lapAt = now;
    };
    const report = (outcome: string) => {
      console.log(`[SignalR] joinChannel ${outcome} in ${Math.round(performance.now() - startedAt)}ms: ${laps.join(', ')}`);
    };
    if (!await this.ensureSessionReady()) {
      lap('session');
      report('no-session');
      return 'network';
    }
    lap('session');
    const store = useAppStore.getState();
    const currentUser = store.currentUser;
    if (!currentUser) return 'network';
    const optimisticUser: User = { ...currentUser, currentChannelId: channelId, currentCallUserId: null, isSpeaking: false };
    const targetUsers = store.channelUsersMap[channelId] || [];
    const optimisticUsers = targetUsers.some(user => user.id === currentUser.id)
      ? targetUsers
      : [...targetUsers, optimisticUser];
    store.removeUserFromChannelMap('', currentUser.id);
    store.commitVoiceChannel(channelId, optimisticUsers);
    this.playSfx(channelJoinSound, 0.3);

    try {
      const micStarted = await webrtc.startLocalStream();
      lap('mic');
      if (!micStarted) {
        store.clearVoiceChannel(channelId);
        report('mic-failed');
        return 'mic_failed';
      }
      if (operationId !== this.voiceOperationId) return 'network';
      const update = await this.invokeWithTimeout<ChannelUpdate | null>(
        "JoinChannel", SignalRService.INVOKE_TIMEOUT_MS, { channelId }
      );
      lap('JoinChannel');
      if (operationId !== this.voiceOperationId) return 'network';
      if (update?.users) {
        webrtc.leaveAll();
        store.commitVoiceChannel(channelId, update.users);
        store.setCallStatus('idle');
        store.setCurrentCallUser(null);
        const peerIds = update.users.filter(user => user.id !== currentUser.id).map(user => user.id);
        report(`ok, ${peerIds.length} peers`);
        setTimeout(() => {
          if (operationId !== this.voiceOperationId || useAppStore.getState().currentChannelId !== channelId) return;
          this.connectToPeersStaggered(peerIds, operationId, channelId);
        }, SignalRService.PEER_CONNECT_DELAY_MS);
        return 'ok';
      }
      store.clearVoiceChannel(channelId);
      webrtc.enterBackgroundMode();
      report('full');
      return 'full';
    } catch (error) {
      store.clearVoiceChannel(channelId);
      webrtc.enterBackgroundMode();
      lap('failed');
      report(`error ${error instanceof Error ? error.message : String(error)}`);
      return 'network';
    }
  }

  public async switchChannel(channelId: string): Promise<'ok' | 'network' | 'mic_failed' | 'full'> {
    if (this.isJoiningChannel || this.voiceOperationDone) return 'network';
    const operationId = this.beginVoiceOperation();
    const sourceChannelId = useAppStore.getState().currentChannelId;
    const sourceUsers = [...useAppStore.getState().voiceUsers];
    const initialState = useAppStore.getState();
    const initialUser = initialState.currentUser;
    if (!initialUser) {
      this.finishVoiceOperation(operationId);
      return 'network';
    }
    const optimisticUser: User = {
      ...initialUser,
      currentChannelId: channelId,
      currentCallUserId: null,
      isSpeaking: false,
      isStreaming: false,
      streamQuality: undefined
    };
    const knownTargetUsers = initialState.channelUsersMap[channelId] || [];
    const optimisticUsers = knownTargetUsers.some(user => user.id === initialUser.id)
      ? knownTargetUsers
      : [...knownTargetUsers, optimisticUser];

    if (initialUser.isStreaming || webrtc.localVideoStream) {
      webrtc.stopScreenShare();
      this.safeInvoke("StopStream");
      initialState.updateUserStatus(initialUser.id, { isStreaming: false, streamQuality: undefined });
    }
    initialState.setActiveStreamId(null);

    initialState.removeUserFromChannelMap('', initialUser.id);
    if (sourceChannelId) {
      this.playSfx(channelLeaveSound, 0.3);
    }
    initialState.commitVoiceChannel(channelId, optimisticUsers);
    this.playSfx(channelJoinSound, 0.3);
    initialState.setIsJoiningChannel(true);
    try {
      const micStarted = await webrtc.startLocalStream();
      if (!micStarted) {
        useAppStore.getState().clearVoiceChannel(channelId);
        if (sourceChannelId) useAppStore.getState().commitVoiceChannel(sourceChannelId, sourceUsers);
        return 'mic_failed';
      }
      if (sourceChannelId) {
        try {
          await this.invokeWithTimeout("LeaveChannel", SignalRService.INVOKE_TIMEOUT_MS);
        } catch {
          useAppStore.getState().clearVoiceChannel(channelId);
          const restored = await this.restoreVoiceChannel(sourceChannelId, sourceUsers, operationId);
          if (!restored && sourceChannelId) {
            useAppStore.getState().commitVoiceChannel(sourceChannelId, sourceUsers);
            await webrtc.enterBackgroundMode();
          }
          return 'network';
        }
        if (operationId !== this.voiceOperationId) return 'network';
      }

      const update = await this.invokeWithTimeout<ChannelUpdate | null>(
        "JoinChannel", SignalRService.INVOKE_TIMEOUT_MS, { channelId }
      );
      if (operationId !== this.voiceOperationId) return 'network';
      if (!update?.users) {
        useAppStore.getState().clearVoiceChannel(channelId);
        const restored = await this.restoreVoiceChannel(sourceChannelId, sourceUsers, operationId);
        if (!restored) await webrtc.enterBackgroundMode();
        return 'full';
      }
      const state = useAppStore.getState();
      const currentUser = state.currentUser;
      webrtc.leaveAll();
      state.commitVoiceChannel(channelId, update.users);
      state.setCallStatus('idle');
      state.setCurrentCallUser(null);
      setTimeout(() => {
        if (operationId !== this.voiceOperationId || useAppStore.getState().currentChannelId !== channelId) return;
        this.connectToPeersStaggered(
          update.users.filter(user => user.id !== currentUser?.id).map(user => user.id),
          operationId,
          channelId
        );
      }, SignalRService.PEER_CONNECT_DELAY_MS);
      return 'ok';
    } catch {
      useAppStore.getState().clearVoiceChannel(channelId);
      const restored = sourceChannelId
        ? await this.restoreVoiceChannel(sourceChannelId, sourceUsers, operationId)
        : false;
      if (!restored) await webrtc.enterBackgroundMode();
      return 'network';
    } finally {
      this.finishVoiceOperation(operationId);
    }
  }

  public async leaveChannel(): Promise<void> {
    const pendingOperation = this.voiceOperationDone;
    if (pendingOperation) {
      this.voiceOperationId++;
      this.isJoiningChannel = false;
      useAppStore.getState().setIsJoiningChannel(false);
      await pendingOperation;
    }
    const operationId = ++this.voiceOperationId;
    const prevChannelId = useAppStore.getState().currentChannelId;
    const currentUser = useAppStore.getState().currentUser;

    if (currentUser) {
      useAppStore.getState().removeUserFromChannelMap('', currentUser.id);
    }
    if (prevChannelId) {
      this.playSfx(channelLeaveSound, 0.3);
    }

    if (useAppStore.getState().currentUser?.isStreaming || webrtc.localVideoStream) {
      webrtc.stopScreenShare();
      this.safeInvoke("StopStream");
      if (currentUser) {
        useAppStore.getState().updateUserStatus(currentUser.id, { isStreaming: false, streamQuality: undefined });
      }
    }

    if (this.speakingDebounceTimer) {
      clearTimeout(this.speakingDebounceTimer);
      this.speakingDebounceTimer = null;
    }
    this.lastSpeakingState = null;

    webrtc.leaveAll();
    const appStore = useAppStore.getState();
    appStore.setActiveStreamId(null);
    appStore.clearVoiceChannel(prevChannelId);

    await this.safeInvoke("LeaveChannel");
    if (operationId !== this.voiceOperationId) return;
    await webrtc.enterBackgroundMode();
  }

  public async sendFriendRequest(username: string): Promise<boolean> {
    return await this.safeInvoke<boolean>("SendFriendRequest", username) ?? false;
  }

  public async acceptFriendRequest(userId: string): Promise<void> {
    const store = useAppStore.getState();
    const prevRequests = store.friendRequests;

    store.setFriendRequests(prevRequests.filter((r: User) => r.id !== userId));

    const succeeded = await this.invokeCommand("AcceptFriendRequest", userId);
    if (!succeeded) useAppStore.getState().setFriendRequests(prevRequests);
  }

  public async declineFriendRequest(userId: string): Promise<void> {
    const store = useAppStore.getState();
    const prevRequests = store.friendRequests;

    store.setFriendRequests(prevRequests.filter((r: User) => r.id !== userId));

    const succeeded = await this.invokeCommand("DeclineFriendRequest", userId);
    if (!succeeded) useAppStore.getState().setFriendRequests(prevRequests);
  }

  public async removeFriend(userId: string): Promise<void> {
    const store = useAppStore.getState();
    const prevFriends = store.friends;

    store.setFriends(prevFriends.filter((f: User) => f.id !== userId));

    const succeeded = await this.invokeCommand("RemoveFriend", userId);
    if (!succeeded) useAppStore.getState().setFriends(prevFriends);
  }

  public async startCall(targetUserId: string): Promise<boolean> {

    if (this.isStartingCall) return false;
    this.isStartingCall = true;
    try {
      return await this._startCallImpl(targetUserId);
    } finally {
      this.isStartingCall = false;
    }
  }

  private async _startCallImpl(targetUserId: string): Promise<boolean> {
    const store = useAppStore.getState();
    const targetUser = store.friends.find((f: User) => f.id === targetUserId) ||
      (store.selectedProfileUser?.id === targetUserId ? store.selectedProfileUser : null);
    if (targetUser && !targetUser.isOnline) {
      return false;
    }

    store.closeProfileOnly();
    store.setCallStatus('calling');
    if (targetUser) store.setCurrentCallUser(targetUser);

    if (store.currentChannelId) await this.leaveChannel();

    const micStarted = await webrtc.startLocalStream();
    if (!micStarted) { store.setCallStatus('idle'); store.setCurrentCallUser(null); return false; }

    this.playRingtone(0.1);

    const res = await this.safeInvoke<boolean>("StartCall", targetUserId);
    if (!res) { store.setCallStatus('idle'); store.setCurrentCallUser(null); await webrtc.enterBackgroundMode(); this.stopRingtone(); }
    return res ?? false;
  }

  public async acceptCall(callerId: string): Promise<void> {
    if (this.isAcceptingCall) return;
    this.isAcceptingCall = true;
    const store = useAppStore.getState();
    const callerUser = store.incomingCall;
    store.setModal('incomingCall', false);
    store.setIncomingCall(null);
    this.stopRingtone();

    try {
      if (store.currentChannelId) await this.leaveChannel();

      if (store.callStatus !== 'idle') {
        const activeCallUser = store.currentCallUser;
        if (activeCallUser && activeCallUser.id !== callerId) {
          webrtc.disconnectFromPeer(activeCallUser.id);
          await this.invokeCommand("EndCall");
          store.setCallStatus('idle');
          store.setCurrentCallUser(null);
        }
      }

      const micStarted = await webrtc.startLocalStream();
      if (!micStarted) {
        store.setCallStatus('idle');
        store.setCurrentCallUser(null);
        void this.invokeCommand("DeclineCall", callerId);
        return;
      }

      const targetUser = store.friends.find((f: any) => f.id === callerId) || store.currentCallUser;
      if (callerUser) {
        store.setCurrentCallUser({
        id: callerUser.callerId,
        displayName: callerUser.callerName,
        username: callerUser.callerName,
        avatarBase64: callerUser.callerAvatarBase64 ?? null,
        avatarColor: callerUser.callerAvatarColor ?? '#C81E70',
        isOnline: true,
        isMuted: false,
        isDeafened: false,
        isSpeaking: false,
        isServerMuted: false,
        isServerDeafened: false,
        currentChannelId: null,
        currentCallUserId: null,
        });
      } else if (targetUser) {
        store.setCurrentCallUser({
        id: targetUser.id,
        displayName: targetUser.displayName,
        username: targetUser.username,
        avatarBase64: targetUser.avatarBase64 ?? null,
        avatarColor: targetUser.avatarColor ?? '#C81E70',
        isOnline: true,
        isMuted: false,
        isDeafened: false,
        isSpeaking: false,
        isServerMuted: false,
        isServerDeafened: false,
        currentChannelId: null,
        currentCallUserId: null,
        });
      }
      store.setCallStatus('connected');

      const accepted = await this.invokeCommand('AcceptCall', callerId);
      if (!accepted) {
        store.setCallStatus('idle');
        store.setCurrentCallUser(null);
        await webrtc.enterBackgroundMode();
      }
    } finally {
      this.isAcceptingCall = false;
    }
  }

  public async declineCall(callerId: string): Promise<void> {
    const store = useAppStore.getState();
    store.setIncomingCall(null);
    store.setModal('incomingCall', false);
    if (store.callStatus === 'calling') {
      this.playRingtone(0.1);
    } else {
      this.stopRingtone();
      if (store.callStatus !== 'connected') {
        store.setCurrentCallUser(null);
        store.setCallStatus('idle');
      }
    }
    void this.invokeCommand("DeclineCall", callerId);
  }

  public async endCall(): Promise<void> {
    const callStatus = useAppStore.getState().callStatus;
    const callUser = useAppStore.getState().currentCallUser;
    const store = useAppStore.getState();
    const currentUser = store.currentUser;
    if (currentUser?.isStreaming || webrtc.localVideoStream) {
      webrtc.stopScreenShare();
      this.safeInvoke("StopStream");
      if (currentUser) {
        store.updateUserStatus(currentUser.id, { isStreaming: false, streamQuality: undefined });
      }
    }
    if (callUser) webrtc.disconnectFromPeer(callUser.id);
    this.stopRingtone();

    useAppStore.getState().setIncomingCall(null);
    useAppStore.getState().setCurrentCallUser(null);
    useAppStore.getState().setCallStatus('idle');
    useAppStore.getState().setActiveStreamId(null);
    useAppStore.getState().setStreamFullscreen(false);

    if (callStatus === 'connected') {
      this.playSfx(channelLeaveSound, 0.3);
    }

    await this.invokeCommand("EndCall");
    await webrtc.enterBackgroundMode();
  }

  private async prepareForQuit(): Promise<void> {
    if (this.isPreparingToQuit) return;
    this.isPreparingToQuit = true;
    try {
      const state = useAppStore.getState();
      if (state.incomingCall && state.callStatus === 'idle') {
        await this.invokeCommand("DeclineCall", state.incomingCall.callerId);
      } else if (state.currentCallUser || state.callStatus !== 'idle') {
        await this.invokeCommand("EndCall");
      }
      if (state.currentChannelId) {
        await this.invokeCommand("LeaveChannel");
      }
    } finally {
      chatPeer.close();
      webrtc.leaveAll();
      this.disconnect();
    }
  }

  public toggleState(isMuted: boolean, isDeafened: boolean): void {
    webrtc.toggleMute(isMuted);
    if (this.isConnected()) {
      const currentUser = useAppStore.getState().currentUser;
      this.connection?.send("UpdateUserState", {
        userId: currentUser?.id || "",
        isMuted,
        isDeafened
      });
    }
  }

  public setSpeakingState(isSpeaking: boolean): void {
    if (isSpeaking) {
      if (this.speakingDebounceTimer) {
        clearTimeout(this.speakingDebounceTimer);
        this.speakingDebounceTimer = null;
      }
      if (this.lastSpeakingState === true) return;
      this.lastSpeakingState = true;
      if (this.isConnected()) this.connection?.send("SetSpeakingState", true);
    } else {
      if (this.lastSpeakingState === false) return;
      if (this.speakingDebounceTimer) return;
      this.speakingDebounceTimer = setTimeout(() => {
        this.speakingDebounceTimer = null;
        if (this.lastSpeakingState === true) {
          this.lastSpeakingState = false;
          if (this.isConnected()) this.connection?.send("SetSpeakingState", false);
        }
      }, SignalRService.SPEAKING_HANGOVER_MS);
    }
  }

  public async startStream(quality: string): Promise<boolean> {
    const success = await this.safeInvoke<boolean>('StartStream', quality);
    return success || false;
  }

  public async stopStream(): Promise<void> {
    await this.safeInvoke('StopStream');
  }

  public sendWebRTCOffer(targetId: string, offer: string): void {
    if (this.sessionReady && this.isConnected()) this.connection?.send("SendWebRTCOffer", targetId, offer);
  }
  public sendWebRTCAnswer(targetId: string, answer: string): void {
    if (this.sessionReady && this.isConnected()) this.connection?.send("SendWebRTCAnswer", targetId, answer);
  }
  public sendIceCandidate(targetId: string, candidate: string): void {
    if (this.sessionReady && this.isConnected()) this.connection?.send("SendIceCandidate", targetId, candidate);
  }
  public async sendChatRelay(targetId: string, packet: string): Promise<boolean> {
    if (!this.sessionReady || !this.isConnected() || packet.length > 64 * 1024) return false;
    try {
      await this.connection?.send("SendChatRelay", targetId, packet);
      return true;
    } catch {
      return false;
    }
  }
  public sendStreamViewState(targetId: string, state: 'watching' | 'preview'): void {
    if (this.sessionReady && this.isConnected()) this.connection?.send("SendStreamViewState", targetId, state).catch(() => { });
  }
}

export const signalRService = new SignalRService();

configureChatSignaling({
  fetchIceServers: () => signalRService.fetchIceServers(),
  sendOffer: (targetId, offer) => signalRService.sendWebRTCOffer(targetId, offer),
  sendAnswer: (targetId, answer) => signalRService.sendWebRTCAnswer(targetId, answer),
  sendIceCandidate: (targetId, candidate) => signalRService.sendIceCandidate(targetId, candidate),
  sendFallback: (targetId, packet) => signalRService.sendChatRelay(targetId, packet)
});
