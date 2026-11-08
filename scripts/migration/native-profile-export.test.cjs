const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const vm = require('node:vm');

const helper = fs.readFileSync(path.join(__dirname, 'native-profile-export.cjs'), 'utf8');
const temporary = process.env.ZABOR_TEST_TEMP;
if (!temporary) throw new Error('ZABOR_TEST_TEMP must identify the approved local fixture directory');

async function run({ decryptFails = false, messages = [], databaseVersion = 1, keyPath = ['ownerId', 'id'], malformedSession = false, missingDatabase = false, copiedIndexedStorage = false, stdoutFails = false } = {}) {
  const root = fs.mkdtempSync(path.join(temporary, 'zabor-electron-copy-'));
  let resolve;
  const done = new Promise(value => { resolve = value; });
  const paths = [];
  const modes = [];
  let output = '';
  let exited = false;
  let network;
  let permission;
  try {
    fs.writeFileSync(path.join(root, '.native-copy-snapshot'), 'zabor-electron-copy-v1');
    if (copiedIndexedStorage) {
      fs.mkdirSync(path.join(root, 'IndexedDB'));
      fs.writeFileSync(path.join(root, 'IndexedDB', 'fixture-metadata'), 'fixture-only');
    }
    fs.writeFileSync(path.join(root, 'session.enc'), JSON.stringify(malformedSession ? {} : { userId: 'alice', login: 'fixture', password: 'fixture-only', settings: { language: 'en' } }));
    const browserStorage = new Map([['zabor_chat_key_alice_bob', 'fixture-key'], ['zabor_chat_key_other_bob', 'foreign-key'], ['unrelated', 'not-exported']]);
    const database = {
      close() {},
      transaction(name, mode) {
        assert.equal(name, 'messages');
        modes.push(mode);
        const transaction = {
          abort() { queueMicrotask(() => transaction.onabort()); },
          objectStore() {
            return {
              keyPath,
              openCursor() {
                const request = {};
                let index = 0;
                const next = () => {
                  if (index < messages.length) {
                    request.result = { value: messages[index++], continue: () => queueMicrotask(next) };
                    request.onsuccess();
                  } else {
                    request.result = null;
                    request.onsuccess();
                    transaction.oncomplete();
                  }
                };
                queueMicrotask(next);
                return request;
              }
            };
          }
        };
        return transaction;
      }
    };
    class BrowserWindow {
      constructor(options) {
        assert.equal(options.show, false);
        assert.equal(options.webPreferences.sandbox, true);
        assert.equal(options.webPreferences.nodeIntegration, false);
        this.webContents = {
          setWindowOpenHandler(handler) { assert.equal(handler({ url: 'https://invalid.example' }).action, 'deny'); },
          async executeJavaScript(code) {
            return vm.runInNewContext(code, {
              localStorage: { length: browserStorage.size, key: index => [...browserStorage.keys()][index], getItem: key => browserStorage.get(key) },
              indexedDB: {
                databases: async () => missingDatabase ? [] : [{ name: 'zabor-chat-v1', version: databaseVersion }],
                open(name, version) {
                  assert.equal(name, 'zabor-chat-v1');
                  assert.equal(version, 1);
                  const request = { result: database };
                  queueMicrotask(() => request.onsuccess());
                  return request;
                }
              }
            });
          }
        };
      }
      async loadURL(url) { assert.ok(url.startsWith('file:')); }
      destroy() { this.destroyed = true; }
      isDestroyed() { return this.destroyed === true; }
    }
    const app = {
      setPath(name, value) { paths.push([name, value]); },
      setAppLogsPath(value) { assert.equal(path.dirname(value), root); },
      disableHardwareAcceleration() {},
      commandLine: { appendSwitch() {} },
      whenReady: () => ({ then(callback) { if (!exited) void callback(); } }),
      exit(code) { exited = true; resolve(code); }
    };
    const electron = {
      app, BrowserWindow,
      safeStorage: { isEncryptionAvailable: () => true, decryptString(value) { if (decryptFails) throw new Error('fixture decrypt denied'); return value.toString(); } },
      session: { defaultSession: { setPermissionRequestHandler(handler) { permission = handler; }, setPermissionCheckHandler(handler) { assert.equal(handler(), false); }, webRequest: { onBeforeRequest(handler) { network = handler; } } } }
    };
    vm.runInNewContext(helper, {
      require: name => name === 'electron' ? electron : require(name),
      process: { platform: 'win32', argv: ['electron', 'helper', root], stdout: { write(value, callback) { if (stdoutFails) { callback(new Error('fixture pipe failure')); return; } output += value; callback(); } } },
      Buffer
    });
    const code = await done;
    return { code, output, paths, modes, network, permission, root };
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
}

test('snapshot helper uses copied userData, readonly owner-filtered history, and denied networking', async () => {
  const result = await run({ messages: [{ ownerId: 'alice', id: 'own' }, { ownerId: 'other', id: 'foreign' }] });
  assert.equal(result.code, 0);
  const output = JSON.parse(result.output);
  assert.equal(output.session.userId, 'alice');
  assert.deepEqual(output.messages, [{ ownerId: 'alice', id: 'own' }]);
  assert.deepEqual(output.localStorage, { zabor_chat_key_alice_bob: 'fixture-key' });
  assert.deepEqual(result.modes, ['readonly']);
  for (const [, value] of result.paths) assert.equal(value, result.root);
  result.network({ url: 'https://invalid.example' }, response => assert.equal(response.cancel, true));
  result.permission(null, 'media', allowed => assert.equal(allowed, false));
  assert.equal(fs.existsSync(result.root), false);
});

test('failed safeStorage decrypt and malformed sessions emit no export', async () => {
  for (const settings of [{ decryptFails: true }, { malformedSession: true }]) {
    const result = await run(settings);
    assert.equal(result.code, 1);
    assert.equal(result.output, '');
  }
});

test('unknown IndexedDB version or message key contract is never silently accepted', async () => {
  for (const settings of [{ databaseVersion: 2 }, { keyPath: 'id' }]) {
    const result = await run(settings);
    assert.equal(result.code, 1);
    assert.equal(result.output, '');
  }
});

test('unserializable IndexedDB records abort instead of accepting a partial history', async () => {
  const cyclic = { ownerId: 'alice' };
  cyclic.self = cyclic;
  const result = await run({ messages: [cyclic] });
  assert.equal(result.code, 1);
  assert.equal(result.output, '');
});

test('copied IndexedDB storage cannot silently disappear through a wrong origin or recovery', async () => {
  const result = await run({ missingDatabase: true, copiedIndexedStorage: true });
  assert.equal(result.code, 1);
  assert.equal(result.output, '');
});

test('stdout pipe failure is a failed export, not a successful empty result', async () => {
  const result = await run({ stdoutFails: true });
  assert.equal(result.code, 1);
  assert.equal(result.output, '');
});
