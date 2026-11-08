const fs = require('node:fs');
const path = require('node:path');
const { pathToFileURL } = require('node:url');
const { app, BrowserWindow, safeStorage, session } = require('electron');

const MAX_EXPORT = 64 * 1024 * 1024;
const root = path.resolve(process.argv[2] || '');
const assertPath = (file) => {
  let current = path.parse(file).root;
  for (const part of file.slice(current.length).split(path.sep).filter(Boolean)) {
    current = path.join(current, part);
    const stat = fs.lstatSync(current);
    if (stat.isSymbolicLink() || (!stat.isDirectory() && !stat.isFile())) throw new Error('unsafe-snapshot');
  }
};
const read = (name, optional = false) => {
  const file = path.join(root, name);
  if (optional && !fs.existsSync(file)) return null;
  assertPath(file);
  if (fs.statSync(file).size > 2 * 1024 * 1024) throw new Error('oversized-snapshot-file');
  return fs.readFileSync(file);
};

try {
  if (process.platform !== 'win32' || process.argv.length !== 3 || !path.basename(root).startsWith('zabor-electron-copy-')) throw new Error('snapshot-only');
  assertPath(root);
  if (read('.native-copy-snapshot').toString() !== 'zabor-electron-copy-v1') throw new Error('snapshot-only');
  app.setPath('appData', root);
  app.setPath('userData', root);
  app.setPath('sessionData', root);
  app.setPath('crashDumps', root);
  app.setAppLogsPath(path.join(root, 'logs'));
  app.disableHardwareAcceleration();
  app.commandLine.appendSwitch('disable-background-networking');
  app.commandLine.appendSwitch('disable-component-update');
} catch {
  process.exitCode = 1;
  app.exit(1);
}

app.whenReady().then(async () => {
  let window;
  let renderer;
  try {
    if (!safeStorage.isEncryptionAvailable()) throw new Error('safe-storage-unavailable');
    const encryptedSession = read('session.enc', true);
    const legacy = encryptedSession ? null : read('session.json', true);
    if (!encryptedSession && !legacy) throw new Error('missing-session');
    const saved = JSON.parse(encryptedSession ? safeStorage.decryptString(encryptedSession) : legacy.toString('utf8'));
    if (typeof saved.userId !== 'string' || typeof saved.login !== 'string' || typeof saved.password !== 'string' || !saved.settings || typeof saved.settings !== 'object' || Array.isArray(saved.settings)) throw new Error('invalid-session');
    if (!/^[A-Za-z0-9.:-]{1,512}$/.test(saved.userId)) throw new Error('invalid-owner');
    const identity = read('chat-identity.enc', true);
    const appSettings = read('app-settings.json', true);
    const indexedDirectory = path.join(root, 'IndexedDB');
    const hadIndexedStorage = fs.existsSync(indexedDirectory) && fs.readdirSync(indexedDirectory).length > 0;
    session.defaultSession.setPermissionRequestHandler((_contents, _permission, callback) => callback(false));
    session.defaultSession.setPermissionCheckHandler(() => false);
    session.defaultSession.webRequest.onBeforeRequest((details, callback) => callback({ cancel: !details.url.startsWith('file:') }));
    renderer = path.join(root, '.native-export.html');
    fs.writeFileSync(renderer, '<!doctype html><meta http-equiv="Content-Security-Policy" content="default-src \'none\'; connect-src \'none\'">', { flag: 'wx' });
    window = new BrowserWindow({ show: false, webPreferences: { sandbox: true, contextIsolation: true, nodeIntegration: false, webSecurity: true, backgroundThrottling: false } });
    window.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
    await window.loadURL(pathToFileURL(renderer).href);
    const browser = await window.webContents.executeJavaScript(`(async () => {
      const owner = ${JSON.stringify(saved.userId)};
      const localStorageCopy = {};
      for (let index = 0; index < localStorage.length; index++) {
        const key = localStorage.key(index);
        if (key === 'zabor_relay_only_ice' || ['zabor_chat_key_', 'zabor_chat_deletions_', 'zabor_chat_deleted_incoming_'].some(prefix => key.startsWith(prefix + owner + '_'))) localStorageCopy[key] = localStorage.getItem(key);
        if (Object.keys(localStorageCopy).length > 50000) throw new Error('oversized-storage');
      }
      const databases = await indexedDB.databases();
      const existing = databases.find(database => database.name === 'zabor-chat-v1');
      if (!existing) {
        if (${JSON.stringify(hadIndexedStorage)}) throw new Error('missing-copied-chat-database');
        return { localStorage: localStorageCopy, messages: [] };
      }
      if (existing.version !== 1) throw new Error('unknown-chat-version');
      const database = await new Promise((resolve, reject) => {
        const request = indexedDB.open('zabor-chat-v1', 1);
        request.onupgradeneeded = () => { request.transaction.abort(); reject(new Error('unexpected-upgrade')); };
        request.onerror = () => reject(request.error);
        request.onblocked = () => reject(new Error('blocked-chat'));
        request.onsuccess = () => resolve(request.result);
      });
      try {
        const messages = await new Promise((resolve, reject) => {
          const transaction = database.transaction('messages', 'readonly');
          const store = transaction.objectStore('messages');
          if (JSON.stringify(store.keyPath) !== JSON.stringify(['ownerId', 'id'])) throw new Error('unknown-chat-schema');
          const request = store.openCursor();
          const result = [];
          let bytes = 0;
          request.onsuccess = () => {
            try {
              const cursor = request.result;
              if (!cursor) return;
              if (cursor.value.ownerId === owner) {
                bytes += JSON.stringify(cursor.value).length;
                if (result.length >= 50000 || bytes > ${MAX_EXPORT}) throw new Error('oversized-history');
                result.push(cursor.value);
              }
              cursor.continue();
            } catch (error) {
              transaction.abort();
              reject(error);
            }
          };
          request.onerror = () => reject(request.error);
          transaction.onabort = () => reject(transaction.error || new Error('aborted-chat'));
          transaction.onerror = () => reject(transaction.error || new Error('failed-chat'));
          transaction.oncomplete = () => resolve(result);
        });
        return { localStorage: localStorageCopy, messages };
      } finally { database.close(); }
    })()`);
    const output = JSON.stringify({ version: 1, session: saved, identity: identity ? safeStorage.decryptString(identity) : null, appSettings: appSettings ? JSON.parse(appSettings.toString('utf8')) : {}, ...browser });
    if (Buffer.byteLength(output) > MAX_EXPORT) throw new Error('oversized-export');
    window.destroy();
    window = null;
    fs.unlinkSync(renderer);
    renderer = null;
    process.stdout.write(output, (error) => app.exit(error ? 1 : 0));
  } catch {
    try {
      if (window && !window.isDestroyed()) window.destroy();
      if (renderer && fs.existsSync(renderer)) fs.unlinkSync(renderer);
    } finally {
      app.exit(1);
    }
  }
}, () => app.exit(1));
