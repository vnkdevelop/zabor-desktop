const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const { pathToFileURL } = require('node:url');
const { app, BrowserWindow, safeStorage, session } = require('electron');
const root = path.resolve(process.argv[2] || '');
if (process.argv.length !== 3 || !path.basename(root).startsWith('zabor-electron-copy-') || fs.readFileSync(path.join(root, '.native-copy-snapshot'), 'utf8') !== 'zabor-electron-copy-v1') app.exit(1);
app.setPath('appData', root);
app.setPath('userData', root);
app.setPath('sessionData', root);
app.setPath('crashDumps', root);
app.setAppLogsPath(path.join(root, 'logs'));
app.disableHardwareAcceleration();
app.commandLine.appendSwitch('disable-background-networking');
app.commandLine.appendSwitch('disable-component-update');

app.whenReady().then(async () => {
  let window;
  const html = path.join(root, '.seed.html');
  try {
    if (!safeStorage.isEncryptionAvailable()) throw new Error('fixture-protection-unavailable');
    const owner = 'fixture-owner';
    const peer = 'fixture-peer';
    const scalar = crypto.createHash('sha256').update('zabor-native-profile-synthetic-key-v1').digest();
    const key = crypto.createECDH('prime256v1');
    key.setPrivateKey(scalar);
    const point = key.getPublicKey(null, 'uncompressed');
    const publicKey = { kty: 'EC', crv: 'P-256', x: point.subarray(1, 33).toString('base64url'), y: point.subarray(33).toString('base64url'), key_ops: [], ext: true };
    const privateKey = { ...publicKey, d: scalar.toString('base64url'), key_ops: ['deriveKey', 'deriveBits'] };
    scalar.fill(0);
    const saved = { userId: owner, login: 'fixture-login', password: 'fixture-private-only', settings: { language: 'en', inputVolume: 83, openAtLogin: false, minimizeToTray: true } };
    fs.writeFileSync(path.join(root, 'session.enc'), safeStorage.encryptString(JSON.stringify(saved)), { flag: 'wx' });
    fs.writeFileSync(path.join(root, 'chat-identity.enc'), safeStorage.encryptString(JSON.stringify({ publicKey, privateKey })), { flag: 'wx' });
    fs.writeFileSync(path.join(root, 'app-settings.json'), JSON.stringify({ openAtLogin: false, minimizeToTray: true }), { flag: 'wx' });
    fs.mkdirSync(path.join(root, 'chat-files'));
    const content = Buffer.from('synthetic attachment only');
    const sha256 = crypto.createHash('sha256').update(content).digest('hex');
    fs.writeFileSync(path.join(root, 'chat-files', 'fixture-src.txt'), content, { flag: 'wx' });
    const now = Date.now();
    const base = { ownerId: owner, conversationId: peer, senderId: owner, targetId: peer, kind: 'text', text: 'synthetic history only', createdAt: now, sequence: 1, delivery: 'sent', file: null };
    const messages = [
      { ...base, id: 'fixture-fresh' },
      { ...base, id: 'fixture-expired', createdAt: now - 14 * 24 * 60 * 60 * 1000 - 1000 },
      { ...base, id: 'fixture-deleted' },
      { ...base, id: 'fixture-foreign', ownerId: 'foreign-owner' },
      { ...base, id: 'fixture-file', kind: 'file', file: { name: 'fixture.txt', size: content.length, sha256, storedName: 'fixture-src.txt', savedPath: 'C:\\never-read\\fixture.txt', sourceAvailable: true, transferState: 'available', progress: 0 } }
    ];
    const fingerprint = JSON.stringify({ kty: publicKey.kty, crv: publicKey.crv, x: publicKey.x, y: publicKey.y });
    session.defaultSession.webRequest.onBeforeRequest((details, callback) => callback({ cancel: !details.url.startsWith('file:') }));
    session.defaultSession.setPermissionRequestHandler((_contents, _permission, callback) => callback(false));
    session.defaultSession.setPermissionCheckHandler(() => false);
    fs.writeFileSync(html, '<!doctype html><meta http-equiv="Content-Security-Policy" content="default-src \'none\'; connect-src \'none\'">', { flag: 'wx' });
    window = new BrowserWindow({ show: false, webPreferences: { sandbox: true, contextIsolation: true, nodeIntegration: false, webSecurity: true } });
    window.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
    await window.loadURL(pathToFileURL(html).href);
    await window.webContents.executeJavaScript(`(async () => {
      localStorage.setItem('zabor_chat_key_fixture-owner_fixture-peer', ${JSON.stringify(fingerprint)});
      localStorage.setItem('zabor_chat_key_foreign-owner_fixture-peer', 'must-not-import');
      localStorage.setItem('zabor_chat_deleted_incoming_fixture-owner_fixture-peer', '[{"type":"message-delete","messageId":"fixture-deleted"}]');
      localStorage.setItem('zabor_relay_only_ice', 'true');
      const database = await new Promise((resolve, reject) => {
        const request = indexedDB.open('zabor-chat-v1', 1);
        request.onupgradeneeded = () => {
          const store = request.result.createObjectStore('messages', { keyPath: ['ownerId', 'id'] });
          store.createIndex('ownerConversation', ['ownerId', 'conversationId', 'createdAt']);
          store.createIndex('ownerCreatedAt', ['ownerId', 'createdAt']);
        };
        request.onerror = () => reject(request.error);
        request.onsuccess = () => resolve(request.result);
      });
      try {
        await new Promise((resolve, reject) => {
          const transaction = database.transaction('messages', 'readwrite');
          transaction.oncomplete = resolve;
          transaction.onabort = () => reject(transaction.error);
          transaction.onerror = () => reject(transaction.error);
          for (const message of ${JSON.stringify(messages)}) transaction.objectStore('messages').put(message);
        });
      } finally { database.close(); }
    })()`);
    window.destroy();
    window = null;
    fs.unlinkSync(html);
    process.stdout.write(JSON.stringify({ fixture: true, now }), (error) => app.exit(error ? 1 : 0));
  } catch {
    try {
      if (window && !window.isDestroyed()) window.destroy();
      if (fs.existsSync(html)) fs.unlinkSync(html);
    } finally { app.exit(1); }
  }
}, () => app.exit(1));
