export type ChatDelivery = 'pending' | 'sent' | 'delivered' | 'read' | 'failed';

export interface ChatFileAttachment {
  name: string;
  size: number;
  sha256: string;
  storedName: string | null;
  sourceAvailable: boolean;
  transferState: 'available' | 'missing' | 'transferring' | 'failed';
  progress: number;
}

export interface ChatMessage {
  id: string;
  ownerId: string;
  conversationId: string;
  senderId: string;
  targetId: string;
  kind: 'text' | 'file';
  text: string;
  createdAt: number;
  sequence: number;
  delivery: ChatDelivery;
  file: ChatFileAttachment | null;
}

export interface ChatWireMessage extends Omit<ChatMessage, 'ownerId' | 'conversationId' | 'delivery' | 'file'> {
  file: Pick<ChatFileAttachment, 'name' | 'size' | 'sha256'> | null;
}

export interface PickedChatFile {
  storedName: string;
  name: string;
  size: number;
  sha256: string;
}

export interface ChatControlPacket {
  version: 1;
  senderId: string;
  targetId: string;
  type: 'key' | 'encrypted' | 'message' | 'ack' | 'ack-through' | 'read' | 'read-through' | 'sync-request' | 'sync-state' | 'sync-complete' | 'ping' | 'pong' | 'file-request' | 'file-begin' | 'file-complete' | 'file-unavailable' | 'message-delete' | 'file-delete';
  messageId?: string;
  transferId?: string;
  message?: ChatWireMessage;
  sequence?: number;
  missing?: Array<[number, number]>;
  sentAt?: number;
  publicKey?: JsonWebKey;
  payload?: string;
}

export interface ChatChunkHeader {
  version: 1;
  senderId: string;
  targetId: string;
  type: 'file-chunk';
  messageId: string;
  transferId: string;
  offset: number;
}
