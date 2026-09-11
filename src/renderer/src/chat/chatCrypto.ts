interface ChatIdentity {
  publicKey: JsonWebKey;
  privateKey: JsonWebKey;
}

let identityPromise: Promise<CryptoKeyPair> | null = null;

async function loadIdentity(): Promise<CryptoKeyPair> {
  const stored = await window.windowControls.loadChatIdentity();
  if (stored) {
    try {
      const identity = JSON.parse(stored) as ChatIdentity;
      return {
        publicKey: await crypto.subtle.importKey('jwk', identity.publicKey, { name: 'ECDH', namedCurve: 'P-256' }, true, []),
        privateKey: await crypto.subtle.importKey('jwk', identity.privateKey, { name: 'ECDH', namedCurve: 'P-256' }, true, ['deriveKey'])
      };
    } catch { }
  }
  const pair = await crypto.subtle.generateKey({ name: 'ECDH', namedCurve: 'P-256' }, true, ['deriveKey', 'deriveBits']);
  await window.windowControls.saveChatIdentity(JSON.stringify({ publicKey: await crypto.subtle.exportKey('jwk', pair.publicKey), privateKey: await crypto.subtle.exportKey('jwk', pair.privateKey) }));
  return pair;
}

export function chatIdentity(): Promise<CryptoKeyPair> {
  identityPromise ??= loadIdentity();
  return identityPromise;
}

export async function chatPublicKey(): Promise<JsonWebKey> {
  return crypto.subtle.exportKey('jwk', (await chatIdentity()).publicKey);
}

export async function deriveChatKey(remotePublicKey: JsonWebKey): Promise<CryptoKey> {
  const remote = await crypto.subtle.importKey('jwk', remotePublicKey, { name: 'ECDH', namedCurve: 'P-256' }, false, []);
  return crypto.subtle.deriveKey({ name: 'ECDH', public: remote }, (await chatIdentity()).privateKey, { name: 'AES-GCM', length: 256 }, false, ['encrypt', 'decrypt']);
}

export async function encryptChatPayload(key: CryptoKey, value: string): Promise<string> {
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const encrypted = new Uint8Array(await crypto.subtle.encrypt({ name: 'AES-GCM', iv }, key, new TextEncoder().encode(value)));
  return JSON.stringify({ iv: Array.from(iv), data: Array.from(encrypted) });
}

export async function decryptChatPayload(key: CryptoKey, value: string): Promise<string> {
  const packet = JSON.parse(value) as { iv: number[]; data: number[] };
  return new TextDecoder().decode(await crypto.subtle.decrypt({ name: 'AES-GCM', iv: new Uint8Array(packet.iv) }, key, new Uint8Array(packet.data)));
}
