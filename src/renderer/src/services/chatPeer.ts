import { useAppStore } from '../store/useAppStore';
import { useChatStore } from '../store/useChatStore';
import { loadPending } from '../chat/chatDb';
import type { ChatChunkHeader, ChatControlPacket, ChatMessage, PickedChatFile } from '../chat/types';
import { chatPublicKey, decryptChatPayload, deriveChatKey, encryptChatPayload } from '../chat/chatCrypto';

const CHAT_SDP_PREFIX = 'zabor-chat:';
const CHUNK_BYTES = 48 * 1024;
const BUFFER_LIMIT = 512 * 1024;
const MAX_CONTROL_BYTES = 64 * 1024;
const MAX_FILE_BYTES = 2 * 1024 * 1024 * 1024;
const RETENTION_MS = 14 * 24 * 60 * 60 * 1000;
const MAX_FUTURE_SKEW_MS = 5 * 60 * 1000;
const CONNECT_TIMEOUT_MS = 8_000;
const FALLBACK_DELAY_MS = 1_500;
const RECONNECT_DELAY_MS = 1_000;
const MAX_RECONNECT_DELAY_MS = 30_000;
const WARM_CONNECTION_MS = 5 * 60 * 1000;
const PING_INTERVAL_MS = 20_000;
const MAX_SYNC_RANGES = 64;
const DELIVERY_RETRY_BASE_MS = 5_000;
const MAX_DELIVERY_RETRIES = 5;

export interface ChatPeerDiagnostics {
  connectedAt: number | null;
  lastRttMs: number | null;
  reconnects: number;
  route: 'p2p' | 'turn' | 'signalr' | 'unknown';
}

interface ChatSignaling {
  fetchIceServers: () => Promise<{ iceServers: RTCIceServer[]; expiresAtUnixMs: number } | null>;
  sendOffer: (targetId: string, offer: string) => void;
  sendAnswer: (targetId: string, answer: string) => void;
  sendIceCandidate: (targetId: string, candidate: string) => void;
  sendFallback?: (targetId: string, packet: string) => Promise<boolean>;
}

let signaling: ChatSignaling | null = null;

export function configureChatSignaling(value: ChatSignaling): void {
  signaling = value;
}

interface IncomingTransfer {
  messageId: string;
  transferId: string;
  expected: number;
  written: number;
  storedName: string;
  sha256: string;
}

class ChatPeerManager {
  private peers = new Map<string, RTCPeerConnection>();
  private channels = new Map<string, RTCDataChannel>();
  private fileChannels = new Map<string, RTCDataChannel>();
  private incoming = new Map<string, IncomingTransfer>();
  private pendingCandidates = new Map<string, RTCIceCandidateInit[]>();
  private ignoredOffers = new Set<string>();
  private connecting = new Map<string, Promise<void>>();
  private flushing = new Set<string>();
  private initializedOwnerId: string | null = null;
  private sequences = new Map<string, number>();
  private iceConfig: RTCConfiguration = { iceServers: [{ urls: 'stun:stun.l.google.com:19302' }] };
  private iceExpiresAt = 0;
  private retentionTimer: ReturnType<typeof setInterval> | null = null;
  private reconnectTimers = new Map<string, ReturnType<typeof setTimeout>>();
  private reconnectAttempts = new Map<string, number>();
  private disconnectTimers = new Map<string, ReturnType<typeof setTimeout>>();
  private pingTimer: ReturnType<typeof setInterval> | null = null;
  private cryptoKeys = new Map<string, CryptoKey>();
  private pendingSecurePackets = new Map<string, Array<Omit<ChatControlPacket, 'version' | 'senderId' | 'targetId'>>>();
  private deliveryTimers = new Map<string, ReturnType<typeof setTimeout>>();
  private deliveryAttempts = new Map<string, number>();
  private diagnostics = new Map<string, ChatPeerDiagnostics>();

  public isChatSignal(payload: string): boolean {
    return payload.startsWith(CHAT_SDP_PREFIX);
  }

  public getDiagnostics(friendId: string): ChatPeerDiagnostics {
    return this.diagnostics.get(friendId) ?? { connectedAt: null, lastRttMs: null, reconnects: 0, route: 'unknown' };
  }

  public async initialize(): Promise<void> {
    const ownerId = useAppStore.getState().currentUser?.id;
    if (!ownerId) return;
    if (this.initializedOwnerId === ownerId) return;
    this.initializedOwnerId = ownerId;
    await useChatStore.getState().cleanup(ownerId);
    for (const friend of useAppStore.getState().friends) {
      if (friend.isOnline) void this.connect(friend.id);
    }
    if (!this.retentionTimer) {
      this.retentionTimer = setInterval(() => {
        const currentOwnerId = useAppStore.getState().currentUser?.id;
        if (currentOwnerId) void useChatStore.getState().cleanup(currentOwnerId);
      }, 60 * 60 * 1000);
    }
    if (!this.pingTimer) this.pingTimer = setInterval(() => this.pingOpenChannels(), PING_INTERVAL_MS);
  }

  public async open(friendId: string): Promise<void> {
    const state = useAppStore.getState();
    const ownerId = state.currentUser?.id;
    if (!ownerId || !state.friends.some(friend => friend.id === friendId)) return;
    await useChatStore.getState().load(ownerId, friendId);
    void this.markVisibleAsRead(friendId);
    if (!state.friends.find(friend => friend.id === friendId)?.isOnline) return;
    await this.connect(friendId);
  }

  public preconnect(friendIds: string[]): void {
    for (const friendId of friendIds.slice(0, 5)) if (this.isFriend(friendId)) void this.connect(friendId);
  }

  public async markVisibleAsRead(friendId: string): Promise<void> {
    const ownerId = this.ownerId();
    if (!ownerId || useChatStore.getState().selectedFriendId !== friendId || document.visibilityState !== 'visible') return;
    const sequence = Math.max(0, ...(useChatStore.getState().messages[friendId] ?? []).filter(message => message.senderId === friendId).map(message => message.sequence));
    if (sequence) await this.sendControl(friendId, { type: 'read-through', sequence });
    useChatStore.getState().markRead(friendId);
  }

  public async handleOnline(friendId: string): Promise<void> {
    const ownerId = this.ownerId();
    if (!ownerId || !this.isFriend(friendId)) return;
    await loadPending(ownerId);
    await this.connect(friendId);
  }

  public handleOffline(friendId: string): void {
    const timer = this.reconnectTimers.get(friendId);
    if (timer) clearTimeout(timer);
    this.reconnectTimers.delete(friendId);
    this.disconnect(friendId);
    useChatStore.getState().setConnection(friendId, false);
  }

  public keepWarm(friendId: string): void {
    const previous = this.disconnectTimers.get(friendId);
    if (previous) clearTimeout(previous);
    const timer = setTimeout(() => {
      this.disconnectTimers.delete(friendId);
      if (useChatStore.getState().selectedFriendId !== friendId) this.disconnect(friendId);
    }, WARM_CONNECTION_MS);
    this.disconnectTimers.set(friendId, timer);
  }

  public async sendText(friendId: string, text: string): Promise<void> {
    const value = text.trim();
    if (!value || value.length > 4000) return;
    const ownerId = this.ownerId();
    if (!ownerId || !this.isFriend(friendId)) return;
    const message = this.createMessage(ownerId, friendId, 'text', value, null);
    await useChatStore.getState().upsert(message);
    await this.sendMessage(message);
  }

  public async sendFiles(friendId: string): Promise<void> {
    const ownerId = this.ownerId();
    if (!ownerId || !this.isFriend(friendId)) return;
    const files = await window.windowControls.chatFilePick();
    await this.sendPickedFiles(friendId, files);
  }

  public async sendDroppedFiles(friendId: string, filePaths: string[]): Promise<void> {
    const ownerId = this.ownerId();
    if (!ownerId || !this.isFriend(friendId) || filePaths.length === 0) return;
    const files = await window.windowControls.chatFileImport(filePaths);
    await this.sendPickedFiles(friendId, files);
  }

  private async sendPickedFiles(friendId: string, files: PickedChatFile[]): Promise<void> {
    const ownerId = this.ownerId();
    if (!ownerId || !this.isFriend(friendId)) return;
    for (const picked of files) {
      if (picked.size > MAX_FILE_BYTES) continue;
      const message = this.createMessage(ownerId, friendId, 'file', '', {
        name: picked.name,
        size: picked.size,
        sha256: picked.sha256,
        storedName: picked.storedName,
        sourceAvailable: true,
        transferState: 'available',
        progress: 1
      });
      await useChatStore.getState().upsert(message);
      await this.sendMessage(message);
    }
  }

  public async requestFile(message: ChatMessage): Promise<void> {
    if (message.kind !== 'file' || message.senderId === this.ownerId()) return;
    const next = { ...message, file: message.file ? { ...message.file, transferState: 'transferring' as const, progress: 0 } : null };
    await useChatStore.getState().upsert(next);
    await this.sendControl(message.senderId, { type: 'file-request', messageId: message.id });
  }

  public async clearFile(message: ChatMessage): Promise<void> {
    if (!message.file?.storedName) return;
    if (!await window.windowControls.chatFileDelete(message.file.storedName)) return;
    await useChatStore.getState().upsert({ ...message, file: { ...message.file, storedName: null, transferState: 'missing', progress: 0 } });
    await this.sendOrQueueDeletion(message.conversationId, { type: 'file-delete', messageId: message.id });
  }

  public async deleteLocal(message: ChatMessage): Promise<void> {
    if (message.file?.storedName && !await window.windowControls.chatFileDelete(message.file.storedName)) return;
    await useChatStore.getState().remove(message.ownerId, message.conversationId, message.id);
    await this.sendOrQueueDeletion(message.conversationId, { type: 'message-delete', messageId: message.id });
  }

  public async handleOffer(senderId: string, payload: string): Promise<void> {
    if (!this.isFriend(senderId)) return;
    const existing = this.peers.get(senderId);
    const pc = existing ?? await this.createPeer(senderId, false);
    const offer = JSON.parse(payload.slice(CHAT_SDP_PREFIX.length)) as RTCSessionDescriptionInit;
    const collision = pc.signalingState !== 'stable';
    const polite = (this.ownerId() ?? '').localeCompare(senderId) > 0;
    if (collision && !polite) {
      this.ignoredOffers.add(senderId);
      return;
    }
    this.ignoredOffers.delete(senderId);
    if (collision) await pc.setLocalDescription({ type: 'rollback' });
    await pc.setRemoteDescription(offer);
    await this.flushCandidates(senderId, pc);
    await pc.setLocalDescription(await pc.createAnswer());
    signaling?.sendAnswer(senderId, CHAT_SDP_PREFIX + JSON.stringify(pc.localDescription));
  }

  public async handleAnswer(senderId: string, payload: string): Promise<void> {
    if (!this.isFriend(senderId)) return;
    const pc = this.peers.get(senderId);
    if (!pc) return;
    await pc.setRemoteDescription(JSON.parse(payload.slice(CHAT_SDP_PREFIX.length)));
    await this.flushCandidates(senderId, pc);
  }

  public async restartIce(friendId: string): Promise<void> {
    const pc = this.peers.get(friendId);
    if (!pc || pc.signalingState !== 'stable' || !this.isFriend(friendId)) return;
    await pc.setLocalDescription(await pc.createOffer({ iceRestart: true }));
    signaling?.sendOffer(friendId, CHAT_SDP_PREFIX + JSON.stringify(pc.localDescription));
  }

  public async handleIceCandidate(senderId: string, payload: string): Promise<void> {
    if (!this.isFriend(senderId)) return;
    if (this.ignoredOffers.has(senderId)) return;
    const candidate = JSON.parse(payload.slice(CHAT_SDP_PREFIX.length)) as RTCIceCandidateInit;
    const pc = this.peers.get(senderId);
    if (!pc || !pc.remoteDescription) {
      const pending = this.pendingCandidates.get(senderId) ?? [];
      pending.push(candidate);
      this.pendingCandidates.set(senderId, pending.slice(-256));
      return;
    }
    await pc.addIceCandidate(candidate);
  }

  public async handleFallback(senderId: string, payload: string): Promise<void> {
    if (!this.isFriend(senderId) || payload.length > MAX_CONTROL_BYTES) return;
    const packet = JSON.parse(payload) as ChatControlPacket;
    if (!this.validPacket(senderId, packet)) return;
    await this.receiveControl(senderId, packet);
  }

  public close(): void {
    for (const channel of this.channels.values()) channel.close();
    for (const channel of this.fileChannels.values()) channel.close();
    for (const peer of this.peers.values()) peer.close();
    this.channels.clear();
    this.fileChannels.clear();
    this.peers.clear();
    this.incoming.clear();
    this.pendingCandidates.clear();
    this.ignoredOffers.clear();
    this.connecting.clear();
    this.flushing.clear();
    for (const timer of this.reconnectTimers.values()) clearTimeout(timer);
    for (const timer of this.disconnectTimers.values()) clearTimeout(timer);
    this.reconnectTimers.clear();
    this.disconnectTimers.clear();
    this.reconnectAttempts.clear();
    this.cryptoKeys.clear();
    this.pendingSecurePackets.clear();
    for (const timer of this.deliveryTimers.values()) clearTimeout(timer);
    this.deliveryTimers.clear();
    this.deliveryAttempts.clear();
    this.diagnostics.clear();
    if (this.pingTimer) clearInterval(this.pingTimer);
    this.pingTimer = null;
    this.initializedOwnerId = null;
    if (this.retentionTimer) clearInterval(this.retentionTimer);
    this.retentionTimer = null;
    useChatStore.getState().reset();
  }

  private async connect(friendId: string): Promise<void> {
    if (this.channels.get(friendId)?.readyState === 'open') return;
    const existing = this.peers.get(friendId);
    if (existing?.connectionState === 'connected') return;
    if (existing) this.disconnect(friendId);
    const active = this.connecting.get(friendId);
    if (active) return active;
    const attempt = (async () => {
      const pc = await this.createPeer(friendId, true);
      await pc.setLocalDescription(await pc.createOffer());
      signaling?.sendOffer(friendId, CHAT_SDP_PREFIX + JSON.stringify(pc.localDescription));
      await this.waitForChannel(friendId);
      if (this.channels.get(friendId)?.readyState !== 'open') {
        this.disconnect(friendId);
        this.scheduleReconnect(friendId);
      }
    })().finally(() => this.connecting.delete(friendId));
    this.connecting.set(friendId, attempt);
    return attempt;
  }

  private async createPeer(friendId: string, initiator: boolean): Promise<RTCPeerConnection> {
    this.disconnect(friendId);
    await this.refreshIce();
    const pc = new RTCPeerConnection({ ...this.iceConfig, iceCandidatePoolSize: 4 });
    this.peers.set(friendId, pc);
    pc.onicecandidate = event => {
      if (!event.candidate) return;
      if (localStorage.getItem('zabor_relay_only_ice') === 'true'
        && !/\styp\s+relay(?:\s|$)/.test(event.candidate.candidate)) return;
      signaling?.sendIceCandidate(friendId, CHAT_SDP_PREFIX + JSON.stringify(event.candidate));
    };
    pc.onconnectionstatechange = () => {
      if (pc.connectionState === 'failed') {
        useChatStore.getState().setConnection(friendId, false);
        void this.restartIce(friendId).catch(() => {
          this.disconnect(friendId);
          this.scheduleReconnect(friendId);
        });
        return;
      }
      if (pc.connectionState === 'closed' || pc.connectionState === 'disconnected') {
        useChatStore.getState().setConnection(friendId, false);
        this.disconnect(friendId);
        this.scheduleReconnect(friendId);
      }
    };
    pc.ondatachannel = event => this.attachChannel(friendId, event.channel);
    if (initiator) {
      this.attachChannel(friendId, pc.createDataChannel('zabor-chat-v1', { ordered: true }));
      this.attachChannel(friendId, pc.createDataChannel('zabor-chat-files-v1', { ordered: true }));
    }
    return pc;
  }

  private attachChannel(friendId: string, channel: RTCDataChannel): void {
    if (channel.label === 'zabor-chat-files-v1') {
      channel.binaryType = 'arraybuffer';
      channel.bufferedAmountLowThreshold = BUFFER_LIMIT / 2;
      channel.onopen = () => this.fileChannels.set(friendId, channel);
      channel.onclose = () => this.fileChannels.delete(friendId);
      let fileQueue = Promise.resolve();
      channel.onmessage = event => { fileQueue = fileQueue.then(() => this.receive(friendId, event.data)).catch(() => { }); };
      return;
    }
    channel.binaryType = 'arraybuffer';
    channel.bufferedAmountLowThreshold = BUFFER_LIMIT / 2;
    channel.onopen = () => {
      this.channels.set(friendId, channel);
      this.reconnectAttempts.delete(friendId);
      this.diagnostics.set(friendId, { ...this.getDiagnostics(friendId), connectedAt: Date.now() });
      void this.refreshRoute(friendId);
      useChatStore.getState().setConnection(friendId, true);
      void this.flushDeletionQueue(friendId);
      void this.flushPending(friendId);
      void this.sendControl(friendId, { type: 'sync-request' });
      void this.sendKey(friendId);
    };
    channel.onclose = () => {
      useChatStore.getState().setConnection(friendId, false);
      this.scheduleReconnect(friendId);
    };
    channel.onerror = () => {
      useChatStore.getState().setConnection(friendId, false);
      this.scheduleReconnect(friendId);
    };
    let receiveQueue = Promise.resolve();
    channel.onmessage = event => {
      receiveQueue = receiveQueue.then(() => this.receive(friendId, event.data)).catch(() => { });
    };
  }

  private async receive(friendId: string, data: string | ArrayBuffer): Promise<void> {
    if (!this.isFriend(friendId)) return;
    if (typeof data === 'string') {
      if (data.length > MAX_CONTROL_BYTES) return;
      const packet = JSON.parse(data) as ChatControlPacket;
      if (!this.validPacket(friendId, packet)) return;
      await this.receiveControl(friendId, packet);
      return;
    }
    await this.receiveChunk(friendId, data);
  }

  private async receiveControl(friendId: string, packet: ChatControlPacket): Promise<void> {
    const ownerId = this.ownerId();
    if (!ownerId) return;
    if (packet.type === 'key' && packet.publicKey) {
      try {
        const fingerprint = JSON.stringify({ kty: packet.publicKey.kty, crv: packet.publicKey.crv, x: packet.publicKey.x, y: packet.publicKey.y });
        const fingerprintKey = `zabor_chat_key_${ownerId}_${friendId}`;
        const trusted = localStorage.getItem(fingerprintKey);
        if (trusted && trusted !== fingerprint) return;
        if (!trusted) localStorage.setItem(fingerprintKey, fingerprint);
        const firstKey = !this.cryptoKeys.has(friendId);
        this.cryptoKeys.set(friendId, await deriveChatKey(packet.publicKey));
        if (firstKey) await this.sendKey(friendId);
        await this.flushSecurePackets(friendId);
      } catch { }
      return;
    }
    if (packet.type === 'encrypted' && packet.payload) {
      const key = this.cryptoKeys.get(friendId);
      if (!key) {
        await this.sendKey(friendId);
        return;
      }
      try {
        const inner = JSON.parse(await decryptChatPayload(key, packet.payload)) as ChatControlPacket;
        if (!this.validPacket(friendId, inner) || inner.type === 'encrypted' || inner.type === 'key') return;
        await this.receiveControl(friendId, inner);
      } catch { }
      return;
    }
    if (packet.type === 'message' && packet.message) {
      const raw = packet.message;
      if (raw.senderId !== friendId || raw.targetId !== ownerId || typeof raw.id !== 'string' || raw.id.length > 128) return;
      if ((raw.kind !== 'text' && raw.kind !== 'file') || typeof raw.text !== 'string' || raw.text.length > 4000) return;
      if (!Number.isSafeInteger(raw.createdAt) || raw.createdAt <= 0 || !Number.isSafeInteger(raw.sequence) || raw.sequence <= 0) return;
      const now = Date.now();
      if (raw.createdAt < now - RETENTION_MS || raw.createdAt > now + MAX_FUTURE_SKEW_MS) return;
      if (raw.kind === 'text' && (raw.file !== null || raw.text.trim().length === 0)) return;
      if (raw.kind === 'file') {
        if (!raw.file || raw.text.length > 0 || typeof raw.file.name !== 'string' || raw.file.name.length === 0 || raw.file.name.length > 255) return;
        if (!Number.isSafeInteger(raw.file.size) || raw.file.size <= 0 || raw.file.size > MAX_FILE_BYTES) return;
        if (!/^[a-f0-9]{64}$/.test(raw.file.sha256)) return;
      }
      const tombstones = this.readDeletionQueue(this.incomingDeletionKey(ownerId, friendId));
      if (tombstones.some(item => item.type === 'message-delete' && item.messageId === raw.id)) return;
      const existing = this.findMessage(friendId, raw.id);
      if (existing) {
        if (existing.senderId === friendId) await this.sendControl(friendId, { type: 'ack', messageId: existing.id });
        if (existing.senderId === friendId && useChatStore.getState().selectedFriendId === friendId && document.visibilityState === 'visible') void this.markVisibleAsRead(friendId);
        return;
      }
      const fileDeleted = tombstones.some(item => item.type === 'file-delete' && item.messageId === raw.id);
      const message: ChatMessage = { ...raw, ownerId, conversationId: friendId, delivery: 'delivered', file: raw.file ? { ...raw.file, storedName: null, sourceAvailable: false, transferState: fileDeleted ? 'failed' : 'missing', progress: 0 } : null };
      this.sequences.set(friendId, Math.max(this.sequences.get(friendId) ?? 0, message.sequence));
      await useChatStore.getState().upsert(message);
      await this.sendControl(friendId, { type: 'ack', messageId: message.id });
      if (useChatStore.getState().selectedFriendId === friendId && document.visibilityState === 'visible') void this.markVisibleAsRead(friendId);
      else useChatStore.getState().markUnread(friendId);
      return;
    }
    if (packet.type === 'ack' && packet.messageId) {
      this.clearDeliveryRetry(packet.messageId);
      const message = this.findMessage(friendId, packet.messageId);
      if (message && message.senderId === ownerId && message.delivery !== 'read') await useChatStore.getState().upsert({ ...message, delivery: 'delivered' });
      return;
    }
    if (packet.type === 'ack-through' && packet.sequence) {
      const ids = (useChatStore.getState().messages[friendId] ?? []).filter(message => message.senderId === ownerId && message.sequence <= packet.sequence && message.delivery !== 'read').map(message => message.id);
      ids.forEach(messageId => this.clearDeliveryRetry(messageId));
      await useChatStore.getState().updateDeliveries(ownerId, friendId, ids, 'delivered');
      return;
    }
    if (packet.type === 'read' && packet.messageId) {
      await useChatStore.getState().updateDeliveries(ownerId, friendId, [packet.messageId], 'read');
      return;
    }
    if (packet.type === 'read-through' && packet.sequence) {
      const ids = (useChatStore.getState().messages[friendId] ?? []).filter(message => message.senderId === ownerId && message.sequence <= packet.sequence).map(message => message.id);
      await useChatStore.getState().updateDeliveries(ownerId, friendId, ids, 'read');
      return;
    }
    if (packet.type === 'sync-request') {
      await this.sendSyncState(friendId);
      return;
    }
    if (packet.type === 'sync-state') {
      const requested = packet.missing?.slice(0, MAX_SYNC_RANGES) ?? [];
      const messages = useChatStore.getState().messages[friendId] ?? [];
      for (const message of messages) {
        if (message.senderId !== ownerId || message.delivery === 'pending' || message.delivery === 'failed') continue;
        if (message.sequence > (packet.sequence ?? 0) || requested.some(([start, end]) => message.sequence >= start && message.sequence <= end)) await this.sendControl(friendId, { type: 'message', message: this.toWire(message) });
      }
      await this.sendControl(friendId, { type: 'sync-complete' });
      return;
    }
    if (packet.type === 'sync-complete') {
      if (useChatStore.getState().selectedFriendId === friendId && document.visibilityState === 'visible') void this.markVisibleAsRead(friendId);
      return;
    }
    if (packet.type === 'ping') {
      await this.sendControl(friendId, { type: 'pong', sentAt: packet.sentAt });
      return;
    }
    if (packet.type === 'pong') {
      if (packet.sentAt) this.diagnostics.set(friendId, { ...this.getDiagnostics(friendId), lastRttMs: Math.max(0, Date.now() - packet.sentAt) });
      return;
    }
    if (packet.type === 'message-delete' && packet.messageId) {
      const message = this.findMessage(friendId, packet.messageId);
      if (!message) {
        this.storeIncomingDeletion(ownerId, friendId, packet);
        return;
      }
      if (message.file?.storedName && !await window.windowControls.chatFileDelete(message.file.storedName)) return;
      await useChatStore.getState().remove(ownerId, friendId, packet.messageId);
      return;
    }
    if (packet.type === 'file-delete' && packet.messageId) {
      const message = this.findMessage(friendId, packet.messageId);
      if (!message) {
        this.storeIncomingDeletion(ownerId, friendId, packet);
        return;
      }
      if (message.file) {
        if (message.file.storedName && !await window.windowControls.chatFileDelete(message.file.storedName)) return;
        await useChatStore.getState().upsert({ ...message, file: { ...message.file, storedName: null, transferState: 'missing', progress: 0 } });
      }
      return;
    }
    if (packet.type === 'file-request' && packet.messageId) {
      const message = this.findMessage(friendId, packet.messageId);
      if (message?.senderId === ownerId && message.file?.storedName) {
        const actualHash = await window.windowControls.chatFileHash(message.file.storedName);
        if (actualHash === message.file.sha256) void this.sendFile(friendId, message);
        else {
          await useChatStore.getState().upsert({ ...message, file: { ...message.file, sourceAvailable: false, transferState: 'failed', progress: 0 } });
          await this.sendControl(friendId, { type: 'file-unavailable', messageId: packet.messageId });
        }
      } else await this.sendControl(friendId, { type: 'file-unavailable', messageId: packet.messageId });
      return;
    }
    if (packet.type === 'file-begin' && packet.messageId && packet.transferId) {
      const message = this.findMessage(friendId, packet.messageId);
      if (!message?.file || message.senderId !== friendId || message.file.size > MAX_FILE_BYTES || this.incoming.has(packet.transferId)) return;
      const result = await window.windowControls.chatFileBegin(packet.transferId, message.file.name, message.file.size);
      if (result.ok) this.incoming.set(packet.transferId, { messageId: message.id, transferId: packet.transferId, expected: message.file.size, written: 0, storedName: result.storedName, sha256: message.file.sha256 });
      else await useChatStore.getState().upsert({ ...message, file: { ...message.file, transferState: 'failed', progress: 0 } });
      return;
    }
    if (packet.type === 'file-complete' && packet.transferId) await this.completeIncoming(friendId, packet.transferId);
    if (packet.type === 'file-unavailable' && packet.messageId) {
      const message = this.findMessage(friendId, packet.messageId);
      if (message?.file) await useChatStore.getState().upsert({ ...message, file: { ...message.file, transferState: 'failed', progress: 0 } });
    }
  }

  private async receiveChunk(friendId: string, data: ArrayBuffer): Promise<void> {
    if (data.byteLength < 4) return;
    const view = new DataView(data);
    const headerLength = view.getUint32(0);
    if (headerLength <= 0 || headerLength > MAX_CONTROL_BYTES || 4 + headerLength > data.byteLength) return;
    const header = JSON.parse(new TextDecoder().decode(new Uint8Array(data, 4, headerLength))) as ChatChunkHeader;
    if (header.type !== 'file-chunk' || header.senderId !== friendId || header.targetId !== this.ownerId()) return;
    const transfer = this.incoming.get(header.transferId);
    if (!transfer || transfer.messageId !== header.messageId || header.offset !== transfer.written) return;
    const chunk = new Uint8Array(data, 4 + headerLength);
    const result = await window.windowControls.chatFileChunk(header.transferId, chunk);
    if (!result.ok) {
      await window.windowControls.chatFileAbort(header.transferId);
      this.incoming.delete(header.transferId);
      const failed = this.findMessage(friendId, transfer.messageId);
      if (failed?.file) await useChatStore.getState().upsert({ ...failed, file: { ...failed.file, transferState: 'failed', progress: 0 } });
      return;
    }
    transfer.written = result.written;
    const message = this.findMessage(friendId, transfer.messageId);
    if (message?.file) await useChatStore.getState().upsert({ ...message, file: { ...message.file, transferState: 'transferring', progress: transfer.expected ? transfer.written / transfer.expected : 1 } });
  }

  private async completeIncoming(friendId: string, transferId: string): Promise<void> {
    const transfer = this.incoming.get(transferId);
    if (!transfer) return;
    if (transfer.written !== transfer.expected) {
      await window.windowControls.chatFileAbort(transferId);
      this.incoming.delete(transferId);
      const incomplete = this.findMessage(friendId, transfer.messageId);
      if (incomplete?.file) await useChatStore.getState().upsert({ ...incomplete, file: { ...incomplete.file, transferState: 'failed', progress: 0 } });
      return;
    }
    const committed = await window.windowControls.chatFileCommit(transferId);
    this.incoming.delete(transferId);
    if (!committed.ok) {
      const failed = this.findMessage(friendId, transfer.messageId);
      if (failed?.file) await useChatStore.getState().upsert({ ...failed, file: { ...failed.file, transferState: 'failed', progress: 0 } });
      return;
    }
    const hash = await window.windowControls.chatFileHash(committed.storedName);
    const message = this.findMessage(friendId, transfer.messageId);
    if (!message?.file) return;
    if (hash !== transfer.sha256) {
      await window.windowControls.chatFileDelete(committed.storedName);
      await useChatStore.getState().upsert({ ...message, file: { ...message.file, transferState: 'failed', progress: 0 } });
      return;
    }
    await useChatStore.getState().upsert({ ...message, file: { ...message.file, storedName: committed.storedName, transferState: 'available', progress: 1 } });
  }

  private async sendMessage(message: ChatMessage): Promise<void> {
    if (!this.isFriend(message.targetId)) return;
    const packet = {
      type: 'message',
      message: {
        id: message.id,
        senderId: message.senderId,
        targetId: message.targetId,
        kind: message.kind,
        text: message.text,
        createdAt: message.createdAt,
        sequence: message.sequence,
        file: message.file ? { name: message.file.name, size: message.file.size, sha256: message.file.sha256 } : null
      }
    } satisfies Omit<ChatControlPacket, 'version' | 'senderId' | 'targetId'>;
    const fallbackTimer = message.kind === 'text' ? setTimeout(() => void this.sendFallback(message.targetId, packet, message.id), FALLBACK_DELAY_MS) : null;
    const sent = await this.sendControl(message.targetId, packet);
    if (fallbackTimer) clearTimeout(fallbackTimer);
    const current = this.findMessage(message.targetId, message.id);
    if (current && sent && current.delivery !== 'delivered' && current.delivery !== 'read') {
      await useChatStore.getState().upsert({ ...current, delivery: 'sent' });
      this.scheduleDeliveryRetry(message.id);
    }
  }

  private async sendFallback(friendId: string, packet: Omit<ChatControlPacket, 'version' | 'senderId' | 'targetId'>, messageId?: string): Promise<boolean> {
    const ownerId = this.ownerId();
    const fallback = signaling?.sendFallback;
    if (!ownerId || !fallback) return false;
    const key = this.cryptoKeys.get(friendId);
    if (!key && packet.type !== 'key') {
      const queued = this.pendingSecurePackets.get(friendId) ?? [];
      queued.push(packet);
      this.pendingSecurePackets.set(friendId, queued.slice(-256));
      await this.sendKey(friendId);
      return false;
    }
    const envelope = packet.type === 'key' ? packet : { type: 'encrypted' as const, payload: await encryptChatPayload(key!, JSON.stringify({ ...packet, version: 1, senderId: ownerId, targetId: friendId })) };
    const encoded = JSON.stringify({ ...envelope, version: 1, senderId: ownerId, targetId: friendId });
    if (encoded.length > MAX_CONTROL_BYTES) return false;
    const sent = await fallback(friendId, encoded);
    if (sent) {
      this.diagnostics.set(friendId, { ...this.getDiagnostics(friendId), route: 'signalr' });
    }
    return sent;
  }

  private async sendKey(friendId: string): Promise<void> {
    const fallback = signaling?.sendFallback;
    const ownerId = this.ownerId();
    if (!fallback || !ownerId) return;
    const packet: ChatControlPacket = { version: 1, senderId: ownerId, targetId: friendId, type: 'key', publicKey: await chatPublicKey() };
    await fallback(friendId, JSON.stringify(packet));
  }

  private async flushSecurePackets(friendId: string): Promise<void> {
    const queued = this.pendingSecurePackets.get(friendId) ?? [];
    this.pendingSecurePackets.delete(friendId);
    for (const packet of queued) await this.sendFallback(friendId, packet, packet.messageId ?? packet.message?.id);
  }

  private async sendFile(friendId: string, message: ChatMessage): Promise<void> {
    const file = message.file;
    if (!file?.storedName) return;
    const stat = await window.windowControls.chatFileStat(file.storedName);
    if (!stat || stat.size !== file.size) {
      await this.sendControl(friendId, { type: 'file-unavailable', messageId: message.id });
      return;
    }
    const transferId = crypto.randomUUID();
    await this.sendControl(friendId, { type: 'file-begin', messageId: message.id, transferId });
    for (let offset = 0; offset < file.size; offset += CHUNK_BYTES) {
      const chunk = await window.windowControls.chatFileReadSlice(file.storedName, offset, Math.min(CHUNK_BYTES, file.size - offset));
      if (!chunk) return;
      await this.sendBinary(friendId, { version: 1, senderId: this.ownerId()!, targetId: friendId, type: 'file-chunk', messageId: message.id, transferId, offset }, chunk);
    }
    await this.sendControl(friendId, { type: 'file-complete', messageId: message.id, transferId });
  }

  private async sendControl(friendId: string, packet: Omit<ChatControlPacket, 'version' | 'senderId' | 'targetId'>): Promise<boolean> {
    const ownerId = this.ownerId();
    if (!ownerId || !this.isFriend(friendId)) return false;
    let channel = this.channels.get(friendId);
    if (channel?.readyState !== 'open') {
      try {
        await this.connect(friendId);
        await this.waitForChannel(friendId);
      } catch {
        if (packet.type === 'file-begin' || packet.type === 'file-complete') return false;
        if (packet.message?.kind === 'file') return false;
        return this.sendFallback(friendId, packet, packet.messageId ?? packet.message?.id);
      }
      channel = this.channels.get(friendId);
    }
    if (channel?.readyState !== 'open') return false;
    channel.send(JSON.stringify({ version: 1, senderId: ownerId, targetId: friendId, ...packet }));
    return true;
  }

  private async sendOrQueueDeletion(friendId: string, packet: Pick<ChatControlPacket, 'type' | 'messageId'>): Promise<void> {
    if (await this.sendControl(friendId, packet)) return;
    const ownerId = this.ownerId();
    if (!ownerId) return;
    const key = `zabor_chat_deletions_${ownerId}_${friendId}`;
    const queued = this.readDeletionQueue(key);
    if (!queued.some(item => item.type === packet.type && item.messageId === packet.messageId)) queued.push(packet);
    localStorage.setItem(key, JSON.stringify(queued));
  }

  private async flushDeletionQueue(friendId: string): Promise<void> {
    const ownerId = this.ownerId();
    if (!ownerId) return;
    const key = `zabor_chat_deletions_${ownerId}_${friendId}`;
    const queued = this.readDeletionQueue(key);
    if (queued.length === 0) return;
    let sent = 0;
    for (const packet of queued) {
      if (!await this.sendControl(friendId, packet)) break;
      sent += 1;
    }
    const remaining = queued.slice(sent);
    if (remaining.length > 0) localStorage.setItem(key, JSON.stringify(remaining));
    else localStorage.removeItem(key);
  }

  private readDeletionQueue(key: string): Pick<ChatControlPacket, 'type' | 'messageId'>[] {
    try {
      const value = JSON.parse(localStorage.getItem(key) ?? '[]') as unknown;
      if (!Array.isArray(value)) return [];
      return value.filter((item): item is Pick<ChatControlPacket, 'type' | 'messageId'> => {
        if (!item || typeof item !== 'object') return false;
        const packet = item as Pick<ChatControlPacket, 'type' | 'messageId'>;
        return (packet.type === 'message-delete' || packet.type === 'file-delete') && typeof packet.messageId === 'string';
      });
    } catch {
      return [];
    }
  }

  private incomingDeletionKey(ownerId: string, friendId: string): string {
    return `zabor_chat_deleted_incoming_${ownerId}_${friendId}`;
  }

  private storeIncomingDeletion(ownerId: string, friendId: string, packet: Pick<ChatControlPacket, 'type' | 'messageId'>): void {
    const key = this.incomingDeletionKey(ownerId, friendId);
    const queued = this.readDeletionQueue(key);
    if (!queued.some(item => item.type === packet.type && item.messageId === packet.messageId)) queued.push(packet);
    localStorage.setItem(key, JSON.stringify(queued.slice(-1000)));
  }

  private async sendBinary(friendId: string, header: ChatChunkHeader, chunk: Uint8Array): Promise<void> {
    const channel = this.fileChannels.get(friendId) ?? this.channels.get(friendId);
    if (!channel || channel.readyState !== 'open') return;
    while (channel.bufferedAmount > BUFFER_LIMIT) await new Promise<void>(resolve => { channel.onbufferedamountlow = () => { channel.onbufferedamountlow = null; resolve(); }; });
    const encoded = new TextEncoder().encode(JSON.stringify(header));
    const packet = new Uint8Array(4 + encoded.byteLength + chunk.byteLength);
    new DataView(packet.buffer).setUint32(0, encoded.byteLength);
    packet.set(encoded, 4);
    packet.set(chunk, 4 + encoded.byteLength);
    channel.send(packet);
  }

  private async flushPending(friendId: string): Promise<void> {
    if (this.flushing.has(friendId)) return;
    const ownerId = this.ownerId();
    if (!ownerId) return;
    this.flushing.add(friendId);
    try {
      const pending = await loadPending(ownerId);
      for (const message of pending.filter(item => item.targetId === friendId)) await this.sendMessage(message);
    } finally {
      this.flushing.delete(friendId);
    }
  }

  private async waitForChannel(friendId: string): Promise<void> {
    const deadline = Date.now() + CONNECT_TIMEOUT_MS;
    while (Date.now() < deadline && this.channels.get(friendId)?.readyState !== 'open') {
      await new Promise<void>(resolve => setTimeout(resolve, 50));
    }
  }

  private createMessage(ownerId: string, friendId: string, kind: 'text' | 'file', text: string, file: ChatMessage['file']): ChatMessage {
    return { id: crypto.randomUUID(), ownerId, conversationId: friendId, senderId: ownerId, targetId: friendId, kind, text, createdAt: Date.now(), sequence: this.nextSequence(friendId), delivery: 'pending', file };
  }

  private nextSequence(friendId: string): number {
    const current = Math.max(this.sequences.get(friendId) ?? 0, ...(useChatStore.getState().messages[friendId] ?? []).map(message => message.sequence));
    const next = current + 1;
    this.sequences.set(friendId, next);
    return next;
  }

  private validPacket(friendId: string, packet: ChatControlPacket): boolean {
    if (packet.version !== 1 || packet.senderId !== friendId || packet.targetId !== this.ownerId()) return false;
    if (!['key', 'encrypted', 'message', 'ack', 'ack-through', 'read', 'read-through', 'sync-request', 'sync-state', 'sync-complete', 'ping', 'pong', 'file-request', 'file-begin', 'file-complete', 'file-unavailable', 'message-delete', 'file-delete'].includes(packet.type)) return false;
    if (packet.messageId !== undefined && (typeof packet.messageId !== 'string' || packet.messageId.length === 0 || packet.messageId.length > 128)) return false;
    if (packet.transferId !== undefined && (typeof packet.transferId !== 'string' || packet.transferId.length === 0 || packet.transferId.length > 128)) return false;
    if (packet.sequence !== undefined && (!Number.isSafeInteger(packet.sequence) || packet.sequence < 0)) return false;
    if (packet.sentAt !== undefined && (!Number.isSafeInteger(packet.sentAt) || packet.sentAt < 0)) return false;
    if (packet.missing !== undefined && (!Array.isArray(packet.missing) || packet.missing.length > MAX_SYNC_RANGES || packet.missing.some(range => !Array.isArray(range) || range.length !== 2 || !Number.isSafeInteger(range[0]) || !Number.isSafeInteger(range[1]) || range[0] < 1 || range[1] < range[0]))) return false;
    if (packet.payload !== undefined && (typeof packet.payload !== 'string' || packet.payload.length > MAX_CONTROL_BYTES)) return false;
    if (packet.publicKey !== undefined && (typeof packet.publicKey !== 'object' || packet.publicKey === null || packet.publicKey.kty !== 'EC' || packet.publicKey.crv !== 'P-256' || typeof packet.publicKey.x !== 'string' || typeof packet.publicKey.y !== 'string')) return false;
    return true;
  }

  private findMessage(friendId: string, messageId: string): ChatMessage | undefined {
    return (useChatStore.getState().messages[friendId] ?? []).find(message => message.id === messageId);
  }

  private ownerId(): string | null {
    return useAppStore.getState().currentUser?.id ?? null;
  }

  private isFriend(friendId: string): boolean {
    return useAppStore.getState().friends.some(friend => friend.id === friendId);
  }

  private async refreshIce(): Promise<void> {
    const relayOnly = localStorage.getItem('zabor_relay_only_ice') === 'true';
    if (Date.now() < this.iceExpiresAt - 60_000
      && this.iceConfig.iceTransportPolicy === (relayOnly ? 'relay' : undefined)) return;
    const config = await signaling?.fetchIceServers();
    if (config?.iceServers.length) {
      this.iceConfig = relayOnly
        ? {
          iceServers: config.iceServers.filter((server) => {
            const urls = Array.isArray(server.urls) ? server.urls : [server.urls];
            return urls.some((url) => url.startsWith('turn:') || url.startsWith('turns:'));
          }),
          iceTransportPolicy: 'relay'
        }
        : { iceServers: config.iceServers };
      this.iceExpiresAt = config.expiresAtUnixMs;
    }
  }

  private disconnect(friendId: string): void {
    const channel = this.channels.get(friendId);
    const fileChannel = this.fileChannels.get(friendId);
    const peer = this.peers.get(friendId);
    this.channels.delete(friendId);
    this.fileChannels.delete(friendId);
    this.peers.delete(friendId);
    this.pendingCandidates.delete(friendId);
    this.ignoredOffers.delete(friendId);
    channel?.close();
    fileChannel?.close();
    peer?.close();
  }

  private scheduleReconnect(friendId: string): void {
    if (this.reconnectTimers.has(friendId) || !this.isFriend(friendId)) return;
    const friend = useAppStore.getState().friends.find(item => item.id === friendId);
    if (!friend?.isOnline) return;
    const attempt = (this.reconnectAttempts.get(friendId) ?? 0) + 1;
    this.reconnectAttempts.set(friendId, attempt);
    this.diagnostics.set(friendId, { ...this.getDiagnostics(friendId), connectedAt: null, reconnects: this.getDiagnostics(friendId).reconnects + 1 });
    const baseDelay = Math.min(MAX_RECONNECT_DELAY_MS, RECONNECT_DELAY_MS * 2 ** Math.min(attempt - 1, 5));
    const delay = Math.round(baseDelay * (0.75 + Math.random() * 0.5));
    const timer = setTimeout(() => {
      this.reconnectTimers.delete(friendId);
      void this.connect(friendId);
    }, delay);
    this.reconnectTimers.set(friendId, timer);
  }

  private async flushCandidates(friendId: string, pc: RTCPeerConnection): Promise<void> {
    const candidates = this.pendingCandidates.get(friendId) ?? [];
    this.pendingCandidates.delete(friendId);
    for (const candidate of candidates) await pc.addIceCandidate(candidate);
  }

  private async sendSyncState(friendId: string): Promise<void> {
    const messages = (useChatStore.getState().messages[friendId] ?? []).filter(message => message.senderId === friendId).sort((a, b) => a.sequence - b.sequence);
    let sequence = 0;
    const missing: Array<[number, number]> = [];
    for (const message of messages) {
      if (message.sequence > sequence + 1) missing.push([sequence + 1, message.sequence - 1]);
      sequence = Math.max(sequence, message.sequence);
    }
    await this.sendControl(friendId, { type: 'sync-state', sequence, missing: missing.slice(-MAX_SYNC_RANGES) });
    if (sequence) await this.sendControl(friendId, { type: 'ack-through', sequence });
  }

  private pingOpenChannels(): void {
    for (const [friendId, channel] of this.channels) {
      if (channel.readyState === 'open') void this.sendControl(friendId, { type: 'ping', sentAt: Date.now() });
    }
  }

  private scheduleDeliveryRetry(messageId: string): void {
    this.clearDeliveryRetry(messageId, false);
    const attempt = (this.deliveryAttempts.get(messageId) ?? 0) + 1;
    this.deliveryAttempts.set(messageId, attempt);
    if (attempt >= MAX_DELIVERY_RETRIES) return;
    const delay = Math.round(Math.min(60_000, DELIVERY_RETRY_BASE_MS * 2 ** (attempt - 1)) * (0.8 + Math.random() * 0.4));
    this.deliveryTimers.set(messageId, setTimeout(() => {
      this.deliveryTimers.delete(messageId);
      const ownerId = this.ownerId();
      if (!ownerId) return;
      for (const messages of Object.values(useChatStore.getState().messages)) {
        const message = messages.find(item => item.id === messageId && item.senderId === ownerId && item.delivery === 'sent');
        if (!message) continue;
        void this.sendMessage(message);
        return;
      }
    }, delay));
  }

  private clearDeliveryRetry(messageId: string, resetAttempt = true): void {
    const timer = this.deliveryTimers.get(messageId);
    if (timer) clearTimeout(timer);
    this.deliveryTimers.delete(messageId);
    if (resetAttempt) this.deliveryAttempts.delete(messageId);
  }

  private async refreshRoute(friendId: string): Promise<void> {
    const pc = this.peers.get(friendId);
    if (!pc) return;
    try {
      const stats = await pc.getStats();
      let route: ChatPeerDiagnostics['route'] = 'unknown';
      stats.forEach(report => {
        if (report.type !== 'candidate-pair' || report.state !== 'succeeded' || !report.nominated) return;
        const local = stats.get(report.localCandidateId);
        const remote = stats.get(report.remoteCandidateId);
        route = local?.candidateType === 'relay' || remote?.candidateType === 'relay' ? 'turn' : 'p2p';
      });
      this.diagnostics.set(friendId, { ...this.getDiagnostics(friendId), route });
    } catch { }
  }
}

export const chatPeer = new ChatPeerManager();
