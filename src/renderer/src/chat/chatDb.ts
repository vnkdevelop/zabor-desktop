import type { ChatMessage } from './types';

const DB_NAME = 'zabor-chat-v1';
const DB_VERSION = 1;
const STORE_NAME = 'messages';
const RETENTION_MS = 14 * 24 * 60 * 60 * 1000;

let openPromise: Promise<IDBDatabase> | null = null;

function openDb(): Promise<IDBDatabase> {
  if (openPromise) return openPromise;
  openPromise = new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, DB_VERSION);
    request.onupgradeneeded = () => {
      const db = request.result;
      const store = db.createObjectStore(STORE_NAME, { keyPath: ['ownerId', 'id'] });
      store.createIndex('ownerConversation', ['ownerId', 'conversationId', 'createdAt']);
      store.createIndex('ownerCreatedAt', ['ownerId', 'createdAt']);
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
  return openPromise;
}

function requestValue<T>(request: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

export async function loadConversation(ownerId: string, conversationId: string): Promise<ChatMessage[]> {
  const db = await openDb();
  const tx = db.transaction(STORE_NAME, 'readonly');
  const index = tx.objectStore(STORE_NAME).index('ownerConversation');
  const range = IDBKeyRange.bound([ownerId, conversationId, 0], [ownerId, conversationId, Number.MAX_SAFE_INTEGER]);
  return requestValue(index.getAll(range));
}

export async function saveMessage(message: ChatMessage): Promise<void> {
  const db = await openDb();
  const tx = db.transaction(STORE_NAME, 'readwrite');
  await requestValue(tx.objectStore(STORE_NAME).put(message));
}

export async function deleteMessage(ownerId: string, id: string): Promise<void> {
  const db = await openDb();
  const tx = db.transaction(STORE_NAME, 'readwrite');
  await requestValue(tx.objectStore(STORE_NAME).delete([ownerId, id]));
}

export async function loadPending(ownerId: string): Promise<ChatMessage[]> {
  const db = await openDb();
  const tx = db.transaction(STORE_NAME, 'readonly');
  const index = tx.objectStore(STORE_NAME).index('ownerCreatedAt');
  const range = IDBKeyRange.bound([ownerId, 0], [ownerId, Number.MAX_SAFE_INTEGER]);
  const messages = await requestValue(index.getAll(range));
  return messages.filter(message => message.senderId === ownerId && message.delivery !== 'delivered');
}

export async function pruneExpired(ownerId: string): Promise<void> {
  const db = await openDb();
  const tx = db.transaction(STORE_NAME, 'readwrite');
  const store = tx.objectStore(STORE_NAME);
  const index = store.index('ownerCreatedAt');
  const range = IDBKeyRange.bound([ownerId, 0], [ownerId, Date.now() - RETENTION_MS]);
  const expired = await requestValue(index.getAll(range));
  const storedNames = expired.flatMap(message => message.file?.storedName ? [message.file.storedName] : []);
  const uniqueStoredNames = [...new Set(storedNames)];
  if (uniqueStoredNames.length) {
    const removed = await window.windowControls.chatFileDeleteMany(uniqueStoredNames);
    if (removed !== uniqueStoredNames.length) throw new Error('chat-file-cleanup-failed');
  }
  await Promise.all(expired.map(message => requestValue(store.delete([ownerId, message.id]))));
}
