import { create } from 'zustand';
import { deleteMessage, loadConversation, pruneExpired, saveMessage } from '../chat/chatDb';
import type { ChatMessage } from '../chat/types';

interface ChatState {
  selectedFriendId: string | null;
  messages: Record<string, ChatMessage[]>;
  loaded: Record<string, boolean>;
  connections: Record<string, boolean>;
  unread: Record<string, boolean>;
  selectFriend: (friendId: string | null) => void;
  markUnread: (friendId: string) => void;
  markRead: (friendId: string) => void;
  load: (ownerId: string, friendId: string) => Promise<void>;
  upsert: (message: ChatMessage) => Promise<void>;
  updateDeliveries: (ownerId: string, friendId: string, messageIds: string[], delivery: ChatMessage['delivery']) => Promise<void>;
  remove: (ownerId: string, friendId: string, messageId: string) => Promise<void>;
  setConnection: (friendId: string, connected: boolean) => void;
  cleanup: (ownerId: string) => Promise<void>;
  reset: () => void;
}

function ordered(messages: ChatMessage[]): ChatMessage[] {
  return [...messages].sort((a, b) => a.sequence - b.sequence || a.createdAt - b.createdAt || a.id.localeCompare(b.id));
}

export const useChatStore = create<ChatState>((set, get) => ({
  selectedFriendId: null,
  messages: {},
  loaded: {},
  connections: {},
  unread: {},
  selectFriend: selectedFriendId => set(state => ({
    selectedFriendId,
    unread: selectedFriendId ? { ...state.unread, [selectedFriendId]: false } : state.unread
  })),
  markUnread: friendId => set(state => ({ unread: { ...state.unread, [friendId]: true } })),
  markRead: friendId => set(state => ({ unread: { ...state.unread, [friendId]: false } })),
  load: async (ownerId, friendId) => {
    const key = `${ownerId}:${friendId}`;
    if (get().loaded[key]) return;
    const messages = await loadConversation(ownerId, friendId);
    set(state => ({ messages: { ...state.messages, [friendId]: ordered(messages) }, loaded: { ...state.loaded, [key]: true } }));
  },
  upsert: async message => {
    set(state => {
      const current = state.messages[message.conversationId] ?? [];
      const index = current.findIndex(item => item.id === message.id);
      const next = index < 0 ? [...current, message] : current.map(item => item.id === message.id ? message : item);
      return { messages: { ...state.messages, [message.conversationId]: ordered(next) } };
    });
    await saveMessage(message);
  },
  updateDeliveries: async (ownerId, friendId, messageIds, delivery) => {
    const ids = new Set(messageIds);
    const current = get().messages[friendId] ?? [];
    const updates = current.filter(message => ids.has(message.id) && message.senderId === ownerId && message.delivery !== delivery).map(message => ({ ...message, delivery }));
    if (!updates.length) return;
    set(state => ({ messages: { ...state.messages, [friendId]: (state.messages[friendId] ?? []).map(message => ids.has(message.id) && message.senderId === ownerId ? { ...message, delivery } : message) } }));
    await Promise.all(updates.map(saveMessage));
  },
  remove: async (ownerId, friendId, messageId) => {
    set(state => ({ messages: { ...state.messages, [friendId]: (state.messages[friendId] ?? []).filter(item => item.id !== messageId) } }));
    await deleteMessage(ownerId, messageId);
  },
  setConnection: (friendId, connected) => set(state => ({ connections: { ...state.connections, [friendId]: connected } })),
  cleanup: async ownerId => {
    await pruneExpired(ownerId);
    set(state => {
      const cutoff = Date.now() - 14 * 24 * 60 * 60 * 1000;
      return { messages: Object.fromEntries(Object.entries(state.messages).map(([id, items]) => [id, items.filter(item => item.createdAt > cutoff)])) };
    });
  },
  reset: () => set({ selectedFriendId: null, messages: {}, loaded: {}, connections: {}, unread: {} })
}));
