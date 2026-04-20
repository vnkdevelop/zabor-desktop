import * as signalR from '@microsoft/signalr';
import { useAppStore, User, VoiceChannel, ChannelUpdate, IncomingCall } from '../store/useAppStore';
import { webrtc } from './webrtc';
import callRingSound from '../assets/sounds/call.mp3';
import channelJoinSound from '../assets/sounds/join.mp3';
import channelLeaveSound from '../assets/sounds/leave.mp3';
import achievementSound from '../assets/sounds/achievement.mp3';

const SERVER_URL = "https://vnkboltik.ru:8080/zabor_v3";

class SignalRService {
  private connection: signalR.HubConnection | null = null;
  private listenersAttached = false;
  private reconnectAttempts = 0;
  private reconnectTimer: NodeJS.Timeout | null = null;
  private reconnectGraceTimer: NodeJS.Timeout | null = null;
  private isReconnecting = false;
  private intentionalDisconnect = false;
  private pingInterval: NodeJS.Timeout | null = null;
  private currentPing = 0;
  private lastSpeakingState: boolean | null = null;
  private wasInChannel: string | null = null;
  private sfxContext: AudioContext | null = null;
  private sfxElements: Map<string, HTMLAudioElement> = new Map();

private playSfx(src: string, volume = 0.5) {
  try {
    let audio = this.sfxElements.get(src);
    if (!audio) {
      audio = new Audio(src);
      this.sfxElements.set(src, audio);
    }
    audio.volume = volume;
    audio.currentTime = 0;
    audio.play().catch(() => {});
  } catch {}
}

private stopSfx(src: string) {
  const audio = this.sfxElements.get(src);
  if (audio) {
    audio.pause();
    audio.currentTime = 0;
  }
}

  private pingCallbacks: Set<(ping: number) => void> = new Set();
  private connectionCallbacks: Set<(isConnected: boolean) => void> = new Set();

  public isConnected(): boolean {
    return this.connection?.state === signalR.HubConnectionState.Connected;
  }

  public getPing(): number {
    return this.currentPing;
  }

  public onPingUpdate(callback: (ping: number) => void): () => void {
    this.pingCallbacks.add(callback);
    return () => this.pingCallbacks.delete(callback);
  }

  public onConnectionUpdate(callback: (isConnected: boolean) => void): () => void {
    this.connectionCallbacks.add(callback);
    callback(this.isConnected());
    return () => this.connectionCallbacks.delete(callback);
  }

  private notifyPingUpdate(ping: number) {
    this.currentPing = ping;
    this.pingCallbacks.forEach(cb => cb(ping));
  }

  private notifyConnectionUpdate(isConnected: boolean) {
    this.connectionCallbacks.forEach(cb => cb(isConnected));
  }

  private getSfxContext(masterGain: number): { ctx: AudioContext; master: GainNode } | null {
    try {
      if (!this.sfxContext) this.sfxContext = new AudioContext();
      if (this.sfxContext.state === 'suspended') this.sfxContext.resume().catch(() => {});
      const ctx = this.sfxContext;
      const master = ctx.createGain();
      master.gain.value = masterGain;
      master.connect(ctx.destination);
      return { ctx, master };
    } catch { return null; }
  }

  private missedPings = 0;

  private startPingMeasurement() {
    if (this.pingInterval) clearInterval(this.pingInterval);
    this.missedPings = 0;
    const measurePing = async () => {
      if (!this.isConnected()) { this.notifyPingUpdate(-1); return; }
      try {
        const start = performance.now();
        await this.connection!.invoke("Ping");
        this.missedPings = 0;
        this.notifyPingUpdate(Math.round(performance.now() - start));
      } catch { 
        this.missedPings++;
        this.notifyPingUpdate(-1);
        if (this.missedPings >= 2 && this.connection) {
          try { this.connection.stop(); } catch {}
        }
      }
    };
    measurePing();
    this.pingInterval = setInterval(measurePing, 5000);
  }

  private stopPingMeasurement() {
    if (this.pingInterval) { clearInterval(this.pingInterval); this.pingInterval = null; }
  }

  public lastConnectionError: string | null = null;

  public async connect(): Promise<boolean> {
    if (this.isConnected()) return true;
    if (this.isReconnecting) {
      await new Promise<void>(resolve => setTimeout(resolve, 500));
      return this.isConnected();
    }
    this.intentionalDisconnect = false;
    this.isReconnecting = true;
    this.lastConnectionError = null;
    try {
      if (this.connection) { try { await this.connection.stop(); } catch {} }
      this.connection = new signalR.HubConnectionBuilder()
        .withUrl(SERVER_URL, {
          skipNegotiation: false,
          transport: signalR.HttpTransportType.WebSockets | signalR.HttpTransportType.LongPolling
        })
        .withAutomaticReconnect([0, 1000, 2000, 5000, 5000, 10000, 10000, 30000, 30000])
        .build();
      this.connection.serverTimeoutInMilliseconds = 15000;
      this.connection.keepAliveIntervalInMilliseconds = 5000;
      this.setupListeners();
      this.setupReconnectionHandlers();
      await this.connection.start();
      this.reconnectAttempts = 0;
      this.isReconnecting = false;
      this.startPingMeasurement();
      this.notifyConnectionUpdate(true);
      return true;
    } catch (err: any) {
      this.isReconnecting = false;
      this.lastConnectionError = err?.message || String(err);
      this.notifyPingUpdate(-1);
      this.notifyConnectionUpdate(false);
      this.scheduleReconnect();
      return false;
    }
  }

  private setupReconnectionHandlers() {
    if (!this.connection) return;
    this.connection.onreconnecting(() => {
      if (this.intentionalDisconnect) return;
      this.isReconnecting = true;
      this.notifyPingUpdate(-1);
      const store = useAppStore.getState();
      if (store.currentChannelId) this.wasInChannel = store.currentChannelId;
      if (!this.reconnectGraceTimer) {
        this.reconnectGraceTimer = setTimeout(() => {
          this.reconnectGraceTimer = null;
          if (this.isReconnecting) this.notifyConnectionUpdate(false);
        }, 5000);
      }
    });
    this.connection.onreconnected(async () => {
      this.reconnectAttempts = 0;
      this.isReconnecting = false;
      if (this.reconnectGraceTimer) { clearTimeout(this.reconnectGraceTimer); this.reconnectGraceTimer = null; }
      this.startPingMeasurement();

      const store = useAppStore.getState();
      if (store.currentUser) {
        try {
          const raw = await window.windowControls.loadSession();
          if (raw) {
            const parsed = JSON.parse(raw);
            if (parsed.login && parsed.password) {
              await this.connection!.invoke("Login", parsed.login, parsed.password);
              await this.loadData();
            }
          }
        } catch {}
      }
      const channelToRejoin = this.wasInChannel || store.currentChannelId;
      this.wasInChannel = null;
      if (channelToRejoin) await this.rejoinChannel(channelToRejoin);

      this.notifyConnectionUpdate(true);
    });
    this.connection.onclose(() => {
      this.isReconnecting = false;
      this.notifyPingUpdate(-1);
      this.stopPingMeasurement();
      if (!this.intentionalDisconnect) {
        const store = useAppStore.getState();
        if (store.currentChannelId) this.wasInChannel = store.currentChannelId;
        if (!this.reconnectGraceTimer) {
          this.reconnectGraceTimer = setTimeout(() => {
            this.reconnectGraceTimer = null;
            this.notifyConnectionUpdate(false);
          }, 5000);
        }
        this.scheduleReconnect();
      }
    });
  }

  private scheduleReconnect() {
    if (this.reconnectTimer || this.intentionalDisconnect) return;
    this.reconnectAttempts++;
    this.reconnectTimer = setTimeout(async () => {
      this.reconnectTimer = null;
      await this.connect();
    }, 5000);
  }

  private async rejoinChannel(channelId: string) {
    try {
      await this.safeInvoke("LeaveChannel");
      await this.joinChannel(channelId);
    } catch {}
  }

  public disconnect() {
    this.intentionalDisconnect = true;
    this.isReconnecting = false;
    this.reconnectAttempts = 0;
    this.lastSpeakingState = null;
    this.wasInChannel = null;
    this.stopPingMeasurement();
    if (this.reconnectGraceTimer) { clearTimeout(this.reconnectGraceTimer); this.reconnectGraceTimer = null; }
    if (this.reconnectTimer) { clearTimeout(this.reconnectTimer); this.reconnectTimer = null; }
    if (this.connection) { this.connection.stop(); this.connection = null; this.listenersAttached = false; }
    if (this.sfxContext) { this.sfxContext.close().catch(() => {}); this.sfxContext = null; }
    this.sfxElements.forEach(audio => { audio.pause(); audio.srcObject = null; });
this.sfxElements.clear();
  }

  private setupListeners() {
    if (!this.connection || this.listenersAttached) return;
    this.listenersAttached = true;
    const store = useAppStore.getState;

    this.connection.on("SyncFullChannelState", (stateMap: Record<string, User[]>) => {
      store().setFullChannelState(stateMap);
    });

    this.connection.on("UserJoined", (user: User) => {
      store().updateUserStatus(user.id, { ...user, isOnline: true });
    });

    this.connection.on("UserLeft", (userId: string) => {
      store().updateUserStatus(userId, { isOnline: false, currentChannelId: null, currentCallUserId: null, isSpeaking: false });
      store().removeUserFromChannelMap('', userId);
      webrtc.disconnectFromPeer(userId);
    });

    this.connection.on("UserUpdated", (user: User) => {
      if (store().currentUser?.id === user.id) store().setCurrentUser({ ...store().currentUser!, ...user });
      store().updateUserStatus(user.id, user);
    });

    this.connection.on("UserJoinedChannel", (user: User, channelId?: string) => {
      if (!channelId) return;
      store().removeUserFromChannelMap('', user.id);
      store().updateUserStatus(user.id, { ...user, currentChannelId: channelId, currentCallUserId: null, isOnline: true });
      store().addUserToChannelMap(channelId, { ...user, currentChannelId: channelId, currentCallUserId: null });
      if (store().currentChannelId === channelId && user.id !== store().currentUser?.id) {
        webrtc.connectToPeer(user.id);
        this.playSfx(channelJoinSound, 0.3);
      }
    });

    this.connection.on("UserLeftChannel", (userId: string, channelId?: string) => {
      store().updateUserStatus(userId, { currentChannelId: null, isSpeaking: false });
      store().removeUserFromChannelMap('', userId);
      webrtc.disconnectFromPeer(userId);
      if (store().currentChannelId === channelId && userId !== store().currentUser?.id) {
        this.playSfx(channelLeaveSound, 0.3);
      }
    });

    this.connection.on("ChannelCreated", (channel: VoiceChannel) => {
      const channels = store().channels;
      // Удаляем optimistic-версию если есть, добавляем реальную
      const withoutOptimistic = channels.filter(c => !c.id.startsWith('__opt_') || c.name !== channel.name);
      if (!withoutOptimistic.find(c => c.id === channel.id)) {
        store().setChannels([...withoutOptimistic, channel]);
      } else {
        store().setChannels(withoutOptimistic);
      }
    });

    this.connection.on("ChannelUpdated", (channel: VoiceChannel) => {
      store().setChannels(store().channels.map(c => c.id === channel.id ? channel : c));
      if (store().selectedChannelForMembers?.id === channel.id) store().setSelectedChannelForMembers(channel);
    });

    this.connection.on("ChannelDeleted", (channelId: string) => {
      store().setChannels(store().channels.filter(c => c.id !== channelId));
      if (store().currentChannelId === channelId) this.leaveChannel();
      if (store().selectedChannelForMembers?.id === channelId) store().setModal('channelMembers', false);
    });

    this.connection.on("ForceLeaveVoice", async () => { await this.leaveChannel(); });

    this.connection.on("UserStateChanged", (update: any) => {
      const nextUpdates: Partial<User> = {
        isMuted: update.isMuted ?? false,
        isDeafened: update.isDeafened ?? false,
        isSpeaking: update.isSpeaking ?? false,
        isServerMuted: update.isServerMuted ?? false,
        isServerDeafened: update.isServerDeafened ?? false
      };
      store().updateUserStatus(update.userId, nextUpdates);
      const currentUser = store().currentUser;
      if (currentUser && update.userId === currentUser.id) {
        const effectiveMuted = (update.isMuted ?? currentUser.isMuted) || (update.isServerMuted ?? currentUser.isServerMuted ?? false);
        const effectiveDeafened = (update.isDeafened ?? currentUser.isDeafened) || (update.isServerDeafened ?? currentUser.isServerDeafened ?? false);
        webrtc.toggleMute(effectiveMuted);
        webrtc.setDeafened(effectiveDeafened);
        store().setCurrentUser({
          ...currentUser,
          isMuted: update.isMuted ?? currentUser.isMuted,
          isDeafened: update.isDeafened ?? currentUser.isDeafened,
          isServerMuted: update.isServerMuted ?? currentUser.isServerMuted ?? false,
          isServerDeafened: update.isServerDeafened ?? currentUser.isServerDeafened ?? false,
          isSpeaking: update.isSpeaking ?? currentUser.isSpeaking
        });
      }
    });

    this.connection.on("UserSpeaking", (userId: string, isSpeaking: boolean) => {
      store().setSpeakingStatus(userId, isSpeaking);
    });

    this.connection.on("FriendRequestReceived", (user: User) => {
      if (!store().friendRequests.find((r: User) => r.id === user.id)) {
        store().setFriendRequests([...store().friendRequests, user]);
        this.playNotificationSound();
      }
    });

    this.connection.on("FriendRequestAccepted", (user: User) => {
      if (!store().friends.find((f: User) => f.id === user.id)) store().setFriends([...store().friends, user]);
    });

    this.connection.on("FriendAdded", (user: User) => {
      if (!store().friends.find((f: User) => f.id === user.id)) store().setFriends([...store().friends, user]);
      store().setFriendRequests(store().friendRequests.filter((r: User) => r.id !== user.id));
    });

    this.connection.on("FriendRemoved", (userId: string) => {
      store().setFriends(store().friends.filter((f: User) => f.id !== userId));
    });

    this.connection.on("ReceiveChannelInvite", async (senderId: string, senderName: string, channelId: string, channelName: string) => {
      this.getChannelMembersList(channelId).then(users => {
        if (users && users.length > 0) {
          store().setChannelUsers(channelId, users);
        }
      }).catch(() => {});
      
      store().setIncomingChannelInvite({ senderId, senderName, channelId, channelName });
      store().setModal('incomingChannelInvite', true);
      this.playRingtone();
    });

    this.connection.on("IncomingCall", (call: IncomingCall) => {
      store().setIncomingCall(call);
      store().setModal('incomingCall', true);
      this.playRingtone();
    });

    this.connection.on("CallAccepted", (user: User) => {
      store().setCurrentCallUser(user);
      store().setCallStatus('connected');
      store().setIncomingCall(null);
      store().setModal('incomingCall', false);
      this.stopRingtone();
      webrtc.connectToPeer(user.id);
    });

    this.connection.on("CallDeclined", () => {
      store().setCallStatus('idle');
      store().setIncomingCall(null);
      store().setModal('incomingCall', false);
      store().setCurrentCallUser(null);
      webrtc.stopLocalStream();
      this.stopRingtone();
    });

    this.connection.on("CallEnded", () => {
      const callUser = store().currentCallUser;
      if (callUser) webrtc.disconnectFromPeer(callUser.id);
      webrtc.stopLocalStream();
      store().setIncomingCall(null);
      store().setModal('incomingCall', false);
      store().setCurrentCallUser(null);
      store().setCallStatus('idle');
      this.stopRingtone();
    });

    this.connection.on("CallStarted", (user: User) => {
      // CallStarted приходит принимающей стороне (звонок успешно поднят).
      // Принимающая сторона НЕ создаёт оффер — она ждёт ReceiveWebRTCOffer от вызывающего.
      // Нам важно установить currentCallUser и callStatus СЕЙЧАС, чтобы handleOffer
      // не отбросил входящий оффер из-за отсутствия callUser в store.
      store().setCurrentCallUser(user);
      store().setCallStatus('connected');
      store().setIncomingCall(null);
      store().setModal('incomingCall', false);
      this.stopRingtone();
      // Принимающая сторона НЕ вызывает connectToPeer — ждёт ReceiveWebRTCOffer
    });

    this.connection.on("AchievementUnlocked", (achievementId: string) => {
      store().setAchievementToast(achievementId);
      setTimeout(() => store().setAchievementToast('__hiding__' + achievementId), 4500);
      setTimeout(() => store().setAchievementToast(null), 5000);
      this.playSfx(achievementSound, 0.4);
    });

    this.connection.on("ReceiveWebRTCOffer", async (sId: string, o: string) => { await webrtc.handleOffer(sId, o); });
    this.connection.on("ReceiveWebRTCAnswer", async (sId: string, a: string) => { await webrtc.handleAnswer(sId, a); });
    this.connection.on("ReceiveIceCandidate", async (sId: string, c: string) => { await webrtc.handleIceCandidate(sId, c); });

    this.connection.on("ForceLogout", async () => {
      try { await window.windowControls.clearSession(); await window.windowControls.wipeAppData(); } catch {}
      const appStore = useAppStore.getState();
      appStore.setCurrentUser(null); appStore.setChannels([]); appStore.setFriends([]);
      appStore.setFriendRequests([]); appStore.setChannelInvites([]); appStore.setVoiceUsers([]);
      appStore.setCurrentChannelId(null); appStore.setCallStatus('idle'); appStore.setCurrentCallUser(null);
      appStore.setFullChannelState({});
      window.location.reload();
    });

    window.windowControls?.onBeforeQuit?.(() => { this.disconnect(); });
  }

  // ── SFX ───────────────────────────────────────────────────────

  private notificationAudio: HTMLAudioElement | null = null;
  private ringtoneInterval: NodeJS.Timeout | null = null;

  private playNotificationSound() {
  // this.playSfx(channelJoinSound, 0.4);
}

  public playRingtone() {
  this.stopRingtone();
  this.playSfx(callRingSound, 0.3);
  const audio = this.sfxElements.get(callRingSound);
  if (audio) audio.loop = true;
}

public stopRingtone() {
  if (this.ringtoneInterval) { clearInterval(this.ringtoneInterval); this.ringtoneInterval = null; }
  this.stopSfx(callRingSound);
  const audio = this.sfxElements.get(callRingSound);
  if (audio) audio.loop = false;
}

  // ── Network helpers ───────────────────────────────────────────

  private async ensureConnected(): Promise<boolean> {
    if (this.isConnected()) return true;
    for (let i = 0; i < 3; i++) {
      if (await this.connect()) return true;
      await new Promise<void>(resolve => setTimeout(resolve, 1000));
    }
    return false;
  }

  private async safeInvoke<T>(method: string, ...args: any[]): Promise<T | null> {
    if (!await this.ensureConnected()) return null;
    try { return await this.connection!.invoke<T>(method, ...args); }
    catch { return null; }
  }

  // ── Auth ──────────────────────────────────────────────────────

  public async checkUserExists(username: string): Promise<boolean> {
    return await this.safeInvoke<boolean>("CheckUserExists", username) ?? false;
  }

  public async login(username: string, password: string): Promise<boolean> {
    const user = await this.safeInvoke<User>("Login", username, password);
    if (user) { useAppStore.getState().setCurrentUser(user); await this.loadData(); return true; }
    return false;
  }

  public async register(username: string, password: string, displayName: string, avatarBase64: string | null, avatarColor: string): Promise<boolean> {
    const user = await this.safeInvoke<User>("Register", username, password, displayName, avatarBase64, avatarColor);
    if (user) { useAppStore.getState().setCurrentUser(user); await this.loadData(); return true; }
    return false;
  }

  public async updateProfile(displayName: string, avatarBase64: string | null, avatarColor: string): Promise<void> {
    await this.safeInvoke("UpdateProfile", displayName, avatarBase64, avatarColor);
  }

  public async changePassword(newPassword: string): Promise<boolean> {
    return await this.safeInvoke<boolean>("UpdateUserPassword", newPassword) ?? false;
  }

  // ── Settings ──────────────────────────────────────────────────

  public async saveAudioSettings(settings: { inputVolume: number; outputVolume: number; selectedInput: string; selectedOutput: string; noiseSuppression: boolean; }): Promise<void> {
    await this.safeInvoke("SaveAudioSettings", JSON.stringify(settings));
  }

  public async loadAudioSettings(): Promise<{ inputVolume: number; outputVolume: number; selectedInput: string; selectedOutput: string; noiseSuppression: boolean; } | null> {
    const json = await this.safeInvoke<string>("GetAudioSettings");
    if (!json) return null;
    try { return JSON.parse(json); } catch { return null; }
  }

  // ── Achievements ──────────────────────────────────────────────

  public async getMyAchievements(): Promise<any> {
    const json = await this.safeInvoke<string>("GetMyAchievements");
    if (json) {
      try {
        const raw = JSON.parse(json);
        return { stats: raw.Stats || raw.stats || {}, unlockedIds: raw.UnlockedIds || raw.unlockedIds || [], visitedChannelIds: raw.VisitedChannelIds || raw.visitedChannelIds || [] };
      } catch {}
    }
    return { stats: {}, unlockedIds: [], visitedChannelIds: [] };
  }

  public async getUserAchievements(userId: string): Promise<any> {
    const json = await this.safeInvoke<string>("GetUserAchievements", userId);
    if (json) {
      try {
        const raw = JSON.parse(json);
        return { stats: raw.Stats || raw.stats || {}, unlockedIds: raw.UnlockedIds || raw.unlockedIds || [], visitedChannelIds: raw.VisitedChannelIds || raw.visitedChannelIds || [] };
      } catch {}
    }
    return { stats: {}, unlockedIds: [], visitedChannelIds: [] };
  }

  public async viewProfile(userId: string): Promise<void> { await this.safeInvoke("ViewProfile", userId); }
  public async getJokeOfTheDay(): Promise<string> { return await this.safeInvoke<string>("GetJokeOfTheDay") ?? ''; }

  // ── Data ──────────────────────────────────────────────────────

  public async loadData(): Promise<void> {
    const [channels, friends, requests] = await Promise.all([
      this.safeInvoke<VoiceChannel[]>("GetChannels"),
      this.safeInvoke<User[]>("GetFriends"),
      this.safeInvoke<User[]>("GetFriendRequests")
    ]);
    useAppStore.getState().setChannels(channels || []);
    useAppStore.getState().setFriends(friends || []);
    useAppStore.getState().setFriendRequests(requests || []);
  }

  // ── Channels (optimistic) ─────────────────────────────────────

  public async createChannel(name: string): Promise<void> {
    const store = useAppStore.getState();
    const currentUser = store.currentUser;
    if (!currentUser) return;

    const tempId = `__opt_${Date.now()}`;
    const optimistic: VoiceChannel = { id: tempId, name: name.trim(), ownerId: currentUser.id };
    store.setChannels([...store.channels, optimistic]);

    const result = await this.safeInvoke<VoiceChannel>("CreateChannel", name);

    if (!result) {
      // Rollback — убираем optimistic-канал
      useAppStore.getState().setChannels(useAppStore.getState().channels.filter(c => c.id !== tempId));
    } else {
      // ChannelCreated event уже мог добавить реальный канал — убираем optimistic
      const current = useAppStore.getState().channels;
      useAppStore.getState().setChannels(current.filter(c => c.id !== tempId));
    }
  }

  public async updateChannel(id: string, name: string): Promise<void> {
    const store = useAppStore.getState();
    const prevChannels = store.channels;

    // Optimistic rename
    store.setChannels(prevChannels.map(c => c.id === id ? { ...c, name: name.trim() } : c));

    const result = await this.safeInvoke<boolean>("UpdateChannel", { channelId: id, name });
    if (!result) useAppStore.getState().setChannels(prevChannels);
  }

  public async quitAccessChannel(channelId: string): Promise<void> {
    const store = useAppStore.getState();
    const prevChannels = store.channels;

    // Optimistic remove
    store.setChannels(prevChannels.filter(c => c.id !== channelId));

    if (store.currentChannelId === channelId) {
      webrtc.leaveAll();
      webrtc.stopLocalStream();
      store.setCurrentChannelId(null);
      store.setVoiceUsers([]);
    }

    const result = await this.safeInvoke("QuitAccessChannel", channelId);
    if (!result) useAppStore.getState().setChannels(prevChannels);
  }

  public async kickFromChannel(channelId: string, userId: string): Promise<void> {
    const store = useAppStore.getState();
    const prevMembers = store.channelMembers;

    // Optimistic remove from members
    store.setChannelMembers(prevMembers.filter(m => m.id !== userId));

    const result = await this.safeInvoke("KickFromChannel", channelId, userId);
    if (!result) useAppStore.getState().setChannelMembers(prevMembers);
  }

  public async getChannelMembersList(channelId: string): Promise<User[]> {
    return await this.safeInvoke<User[]>("GetChannelMembersList", channelId) || [];
  }

  public async sendChannelInvite(targetUserId: string, channelId: string, channelName: string): Promise<void> {
    await this.safeInvoke("SendChannelInvite", targetUserId, channelId, channelName);
  }

  // ── Join / Leave (optimistic) ─────────────────────────────────

  public async joinChannel(channelId: string): Promise<'ok' | 'network' | 'mic_failed' | 'full'> {
    if (!await this.ensureConnected()) return 'network';
    const store = useAppStore.getState();
    const currentUser = store.currentUser;
    if (!currentUser) return 'network';

    const prevChannelId = store.currentChannelId;
    const prevVoiceUsers = store.voiceUsers;
    const prevChannelUsersMap = { ...store.channelUsersMap };

    const optimisticUser: User = { ...currentUser, currentChannelId: channelId, currentCallUserId: null, isSpeaking: false };
    
    store.removeUserFromChannelMap('', currentUser.id);

    const existingUsers = store.channelUsersMap[channelId] || [];
    const allUsers = existingUsers.find(u => u.id === currentUser.id) ? existingUsers : [...existingUsers, optimisticUser];

    store.setCurrentChannelId(channelId);
    store.setVoiceUsers(allUsers);
    store.setChannelUsers(channelId, allUsers);
    webrtc.leaveAll();
    store.setCallStatus('idle');
    store.setCurrentCallUser(null);
    this.playSfx(channelJoinSound, 0.3);

    try {
      const micStarted = await webrtc.startLocalStream();
      if (!micStarted) {
        this.rollbackChannelJoin(prevChannelId, prevVoiceUsers, prevChannelUsersMap);
        return 'mic_failed';
      }
      const update = await this.connection!.invoke<ChannelUpdate | null>("JoinChannel", { channelId });
      if (update?.users) {
        store.setVoiceUsers(update.users);
        store.setChannelUsers(channelId, update.users);
        return 'ok';
      }
      this.rollbackChannelJoin(prevChannelId, prevVoiceUsers, prevChannelUsersMap);
      webrtc.stopLocalStream();
      return 'full';
    } catch {
      this.rollbackChannelJoin(prevChannelId, prevVoiceUsers, prevChannelUsersMap);
      webrtc.stopLocalStream();
      return 'network';
    }
  }

  private rollbackChannelJoin(prevChannelId: string | null, prevVoiceUsers: User[], prevChannelUsersMap: Record<string, User[]>) {
    const store = useAppStore.getState();
    store.setCurrentChannelId(prevChannelId);
    store.setVoiceUsers(prevVoiceUsers);
    store.setFullChannelState(prevChannelUsersMap);
  }

  public async leaveChannel(): Promise<void> {
    const prevChannelId = useAppStore.getState().currentChannelId;
    const currentUser = useAppStore.getState().currentUser;
    
    if (prevChannelId) {
      this.playSfx(channelLeaveSound, 0.3);
    }

    // Optimistic: clear state immediately
    if (currentUser) {
       useAppStore.getState().removeUserFromChannelMap('', currentUser.id);
    }
    webrtc.leaveAll();
    webrtc.stopLocalStream();
    useAppStore.getState().setCurrentChannelId(null);
    useAppStore.getState().setVoiceUsers([]);

    this.safeInvoke("LeaveChannel");
  }

  // ── Friends (optimistic) ──────────────────────────────────────

  public async sendFriendRequest(username: string): Promise<boolean> {
    return await this.safeInvoke<boolean>("SendFriendRequest", username) ?? false;
  }

  public async acceptFriendRequest(userId: string): Promise<void> {
    const store = useAppStore.getState();
    const prevRequests = store.friendRequests;

    // Optimistic remove from requests
    store.setFriendRequests(prevRequests.filter((r: User) => r.id !== userId));

    const result = await this.safeInvoke("AcceptFriendRequest", userId);
    if (!result) useAppStore.getState().setFriendRequests(prevRequests);
  }

  public async declineFriendRequest(userId: string): Promise<void> {
    const store = useAppStore.getState();
    const prevRequests = store.friendRequests;

    // Optimistic
    store.setFriendRequests(prevRequests.filter((r: User) => r.id !== userId));

    const result = await this.safeInvoke("DeclineFriendRequest", userId);
    if (!result) useAppStore.getState().setFriendRequests(prevRequests);
  }

  public async removeFriend(userId: string): Promise<void> {
    const store = useAppStore.getState();
    const prevFriends = store.friends;

    // Optimistic
    store.setFriends(prevFriends.filter((f: User) => f.id !== userId));

    const result = await this.safeInvoke("RemoveFriend", userId);
    if (!result) useAppStore.getState().setFriends(prevFriends);
  }

  // ── Calls (optimistic) ────────────────────────────────────────

  public async startCall(targetUserId: string): Promise<boolean> {
    const store = useAppStore.getState();

    // Optimistic
    store.closeProfileOnly();
    store.setCallStatus('calling');
    const targetUser = store.friends.find((f: User) => f.id === targetUserId);
    if (targetUser) store.setCurrentCallUser(targetUser);

    if (store.currentChannelId) await this.leaveChannel();

    const micStarted = await webrtc.startLocalStream();
    if (!micStarted) { store.setCallStatus('idle'); store.setCurrentCallUser(null); return false; }

    const res = await this.safeInvoke<boolean>("StartCall", targetUserId);
    if (!res) { store.setCallStatus('idle'); store.setCurrentCallUser(null); webrtc.stopLocalStream(); }
    return res ?? false;
  }

  public async acceptCall(callerId: string): Promise<void> {
    if (useAppStore.getState().currentChannelId) await this.leaveChannel();
    const micStarted = await webrtc.startLocalStream();
    if (!micStarted) return;

    // BUGFIX: Устанавливаем callStatus и callUser ДО отправки AcceptCall на сервер.
    // Иначе после AcceptCall сервер шлёт WebRTC-оффер через ReceiveWebRTCOffer,
    // а handleOffer проверяет currentCallUser?.id === senderId.
    // Без этой установки оффер отвергался на устройствах где RTT высок.
    const callerUser = useAppStore.getState().incomingCall;
    if (callerUser) {
      useAppStore.getState().setCurrentCallUser({
        id: callerUser.callerId,
        displayName: callerUser.callerName,
        username: callerUser.callerName,
        avatarBase64: callerUser.callerAvatarBase64 ?? null,
        avatarColor: callerUser.callerAvatarColor ?? '#c70060',
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
    useAppStore.getState().setCallStatus('connected');

    await this.safeInvoke('AcceptCall', callerId);
    useAppStore.getState().setModal('incomingCall', false);
    this.stopRingtone();
  }

  public async declineCall(callerId: string): Promise<void> {
    // Optimistic: clear UI immediately
    useAppStore.getState().setIncomingCall(null);
    useAppStore.getState().setModal('incomingCall', false);
    useAppStore.getState().setCurrentCallUser(null);
    useAppStore.getState().setCallStatus('idle');
    webrtc.stopLocalStream();
    this.stopRingtone();

    this.safeInvoke("DeclineCall", callerId);
  }

  public async endCall(): Promise<void> {
    const callUser = useAppStore.getState().currentCallUser;
    if (callUser) webrtc.disconnectFromPeer(callUser.id);
    webrtc.stopLocalStream();

    // Optimistic: clear UI immediately
    useAppStore.getState().setIncomingCall(null);
    useAppStore.getState().setCurrentCallUser(null);
    useAppStore.getState().setCallStatus('idle');

    this.safeInvoke("EndCall");
  }

  // ── State ─────────────────────────────────────────────────────

  public toggleState(isMuted: boolean, isDeafened: boolean): void {
    webrtc.toggleMute(isMuted);
    if (this.isConnected()) this.connection?.send("UpdateUserState", { isMuted, isDeafened });
  }

  public setSpeakingState(isSpeaking: boolean): void {
    if (isSpeaking === this.lastSpeakingState) return;
    this.lastSpeakingState = isSpeaking;
    if (this.isConnected()) this.connection?.send("SetSpeakingState", isSpeaking);
  }

  // ── WebRTC signaling ──────────────────────────────────────────

  public sendWebRTCOffer(targetId: string, offer: string): void {
    if (this.isConnected()) this.connection?.send("SendWebRTCOffer", targetId, offer);
  }
  public sendWebRTCAnswer(targetId: string, answer: string): void {
    if (this.isConnected()) this.connection?.send("SendWebRTCAnswer", targetId, answer);
  }
  public sendIceCandidate(targetId: string, candidate: string): void {
    if (this.isConnected()) this.connection?.send("SendIceCandidate", targetId, candidate);
  }
}

export const signalRService = new SignalRService();