import { create } from 'zustand';
import { deleteMessage, loadConversation, pruneExpired, saveMessage } from '../chat/chatDb';
import type { ChatMessage } from '../chat/types';
import { useAppStore } from './useAppStore';

export type ChatRoute = 'p2p' | 'turn' | 'signalr' | 'unknown';

interface ChatState {
  selectedFriendId: string | null;
  messages: Record<string, ChatMessage[]>;
  loaded: Record<string, boolean>;
  connections: Record<string, boolean>;
  routes: Record<string, ChatRoute>;
  unread: Record<string, boolean>;
  selectFriend: (friendId: string | null) => void;
  markUnread: (friendId: string) => void;
  markRead: (friendId: string) => void;
  load: (ownerId: string, friendId: string) => Promise<void>;
  upsert: (message: ChatMessage) => Promise<void>;
  updateFileProgress: (conversationId: string, messageId: string, progress: number) => void;
  updateDeliveries: (ownerId: string, friendId: string, messageIds: string[], delivery: ChatMessage['delivery']) => Promise<void>;
  remove: (ownerId: string, friendId: string, messageId: string) => Promise<void>;
  setConnection: (friendId: string, connected: boolean) => void;
  setRoute: (friendId: string, route: ChatRoute) => void;
  cleanup: (ownerId: string) => Promise<void>;
  reset: () => void;
}

function ordered(messages: ChatMessage[]): ChatMessage[] {
  return [...messages].sort((a, b) => a.sequence - b.sequence || a.createdAt - b.createdAt || a.id.localeCompare(b.id));
}

const inFlightLoads = new Map<string, Promise<void>>();
let storeEpoch = 0;
const removedMessages = new Set<string>();

export const useChatStore = create<ChatState>((set, get) => ({
  selectedFriendId: null,
  messages: {},
  loaded: {},
  connections: {},
  routes: {},
  unread: {},
  selectFriend: selectedFriendId => set(state => {
    const clearsUnread = Boolean(selectedFriendId) && state.unread[selectedFriendId!] === true;
    if (state.selectedFriendId === selectedFriendId && !clearsUnread) return state;
    return {
      selectedFriendId,
      unread: clearsUnread ? { ...state.unread, [selectedFriendId!]: false } : state.unread
    };
  }),
  markUnread: friendId => set(state => state.unread[friendId] === true
    ? state
    : { unread: { ...state.unread, [friendId]: true } }),
  markRead: friendId => set(state => state.unread[friendId] !== true
    ? state
    : { unread: { ...state.unread, [friendId]: false } }),
  load: async (ownerId, friendId) => {
    if (useAppStore.getState().currentUser?.id !== ownerId) return;
    const epoch = storeEpoch;
    const key = `${ownerId}:${friendId}`;
    if (get().loaded[key]) return;
    const active = inFlightLoads.get(key);
    if (active) return active;
    const pending = (async () => {
      const messages = await loadConversation(ownerId, friendId);
      if (epoch !== storeEpoch || useAppStore.getState().currentUser?.id !== ownerId) return;
      set(state => {
        const merged = new Map(messages.filter(message => !removedMessages.has(`${ownerId}:${message.id}`)).map(message => [message.id, message]));
        for (const message of state.messages[friendId] ?? []) merged.set(message.id, message);
        return { messages: { ...state.messages, [friendId]: ordered([...merged.values()]) }, loaded: { ...state.loaded, [key]: true } };
      });
    })().finally(() => { if (inFlightLoads.get(key) === pending) inFlightLoads.delete(key); });
    inFlightLoads.set(key, pending);
    return pending;
  },
  upsert: async message => {
    if (useAppStore.getState().currentUser?.id !== message.ownerId) return;
    set(state => {
      const current = state.messages[message.conversationId] ?? [];
      const index = current.findIndex(item => item.id === message.id);
      const next = index < 0 ? [...current, message] : current.map(item => item.id === message.id ? message : item);
      return { messages: { ...state.messages, [message.conversationId]: ordered(next) } };
    });
    await saveMessage(message);
  },
  updateFileProgress: (conversationId, messageId, progress) => set(state => {
    const current = state.messages[conversationId];
    if (!current) return state;
    let changed = false;
    const next = current.map(item => {
      if (item.id !== messageId || !item.file) return item;
      if (item.file.progress === progress && item.file.transferState === 'transferring') return item;
      changed = true;
      return { ...item, file: { ...item.file, progress, transferState: 'transferring' as const } };
    });
    return changed ? { messages: { ...state.messages, [conversationId]: next } } : state;
  }),
  updateDeliveries: async (ownerId, friendId, messageIds, delivery) => {
    if (useAppStore.getState().currentUser?.id !== ownerId) return;
    const ids = new Set(messageIds);
    const current = get().messages[friendId] ?? [];
    const updates = current.filter(message => ids.has(message.id) && message.senderId === ownerId && message.delivery !== delivery).map(message => ({ ...message, delivery }));
    if (!updates.length) return;
    set(state => ({ messages: { ...state.messages, [friendId]: (state.messages[friendId] ?? []).map(message => ids.has(message.id) && message.senderId === ownerId ? { ...message, delivery } : message) } }));
    await Promise.all(updates.map(saveMessage));
  },
  remove: async (ownerId, friendId, messageId) => {
    if (useAppStore.getState().currentUser?.id !== ownerId) return;
    removedMessages.add(`${ownerId}:${messageId}`);
    set(state => ({ messages: { ...state.messages, [friendId]: (state.messages[friendId] ?? []).filter(item => item.id !== messageId) } }));
    await deleteMessage(ownerId, messageId);
  },
  setConnection: (friendId, connected) => set(state => (state.connections[friendId] ?? false) === connected
    ? state
    : { connections: { ...state.connections, [friendId]: connected } }),
  setRoute: (friendId, route) => set(state => (state.routes[friendId] ?? 'unknown') === route
    ? state
    : { routes: { ...state.routes, [friendId]: route } }),
  cleanup: async ownerId => {
    const epoch = storeEpoch;
    await pruneExpired(ownerId);
    if (epoch !== storeEpoch || useAppStore.getState().currentUser?.id !== ownerId) return;
    set(state => {
      const cutoff = Date.now() - 14 * 24 * 60 * 60 * 1000;
      return { messages: Object.fromEntries(Object.entries(state.messages).map(([id, items]) => [id, items.filter(item => item.createdAt > cutoff)])) };
    });
  },
  reset: () => {
    storeEpoch++;
    removedMessages.clear();
    inFlightLoads.clear();
    set({ selectedFriendId: null, messages: {}, loaded: {}, connections: {}, routes: {}, unread: {} });
  }
}));
