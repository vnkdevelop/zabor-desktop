import { EventEmitter } from 'node:events';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

class MockIpcMain extends EventEmitter {
  constructor() {
    super();
    this.handlers = new Map();
  }

  handle(channel, listener) {
    this.handlers.set(channel, listener);
  }

  handleOnce(channel, listener) {
    this.handlers.set(channel, (...args) => {
      this.handlers.delete(channel);
      return listener(...args);
    });
  }

  removeHandler(channel) {
    this.handlers.delete(channel);
  }

  async invoke(channel, ...args) {
    const handler = this.handlers.get(channel);
    if (!handler) {
      throw new Error(`No handler registered for ipc channel: ${channel}`);
    }
    const fakeEvent = {
      sender: {
        id: 1,
        send: (ch, ...p) => this.emit(ch, ...p),
        isDestroyed: () => false
      }
    };
    return handler(fakeEvent, ...args);
  }

  reset() {
    this.handlers.clear();
    this.removeAllListeners();
  }
}

class MockIpcRenderer extends EventEmitter {
  constructor(ipcMainRef) {
    super();
    this.ipcMain = ipcMainRef;
  }

  async invoke(channel, ...args) {
    return this.ipcMain.invoke(channel, ...args);
  }

  send(channel, ...args) {
    this.ipcMain.emit(channel, { sender: { id: 1 } }, ...args);
  }
}

class MockWebContents extends EventEmitter {
  constructor(id = 1) {
    super();
    this.id = id;
    this._destroyed = false;
    this.sentEvents = [];
  }

  isDestroyed() {
    return this._destroyed;
  }

  send(channel, ...args) {
    if (this._destroyed) return;
    this.sentEvents.push({ channel, args, timestamp: Date.now() });
    this.emit(channel, ...args);
  }

  destroy() {
    this._destroyed = true;
  }
}

class MockBrowserWindow extends EventEmitter {
  constructor(options = {}) {
    super();
    this.options = options;
    this.webContents = new MockWebContents();
    this._destroyed = false;
    this._visible = false;
    this._minimized = false;
    this._maximized = false;
    MockBrowserWindow.instances.push(this);
  }

  isDestroyed() {
    return this._destroyed;
  }

  isVisible() {
    return this._visible;
  }

  show() {
    this._visible = true;
    this.emit('show');
  }

  hide() {
    this._visible = false;
    this.emit('hide');
  }

  focus() {
    this.emit('focus');
  }

  restore() {
    this._minimized = false;
    this.emit('restore');
  }

  minimize() {
    this._minimized = true;
    this.emit('minimize');
  }

  isMinimized() {
    return this._minimized;
  }

  maximize() {
    this._maximized = true;
    this.emit('maximize');
  }

  unmaximize() {
    this._maximized = false;
    this.emit('unmaximize');
  }

  isMaximized() {
    return this._maximized;
  }

  close() {
    this._destroyed = true;
    this.webContents.destroy();
    this.emit('close');
    this.emit('closed');
  }

  destroy() {
    this.close();
  }

  static getAllWindows() {
    return MockBrowserWindow.instances.filter(w => !w.isDestroyed());
  }

  static getFocusedWindow() {
    return MockBrowserWindow.instances.find(w => !w.isDestroyed()) || null;
  }

  static reset() {
    MockBrowserWindow.instances = [];
  }
}
MockBrowserWindow.instances = [];

import { mkdtempSync, rmSync, existsSync } from 'node:fs';

class MockApp extends EventEmitter {
  constructor() {
    super();
    this._version = '3.6.4';
    this.initTempDir();
    this._hasSingleInstanceLock = true;
    this.exitCode = null;
    this.isQuit = false;
    this.releasedLock = false;
  }

  initTempDir() {
    try {
      this._tempDir = mkdtempSync(join(tmpdir(), 'zabor-updater-test-'));
      this._userDataDir = mkdtempSync(join(tmpdir(), 'zabor-userdata-test-'));
    } catch {
      const id = `${Date.now()}-${Math.random().toString(36).slice(2)}`;
      this._tempDir = join(tmpdir(), `zabor-test-${id}`);
      this._userDataDir = join(tmpdir(), `zabor-user-${id}`);
    }
    return this._tempDir;
  }

  cleanupTempDir() {
    try {
      if (this._tempDir && existsSync(this._tempDir)) {
        rmSync(this._tempDir, { recursive: true, force: true });
      }
      if (this._userDataDir && existsSync(this._userDataDir)) {
        rmSync(this._userDataDir, { recursive: true, force: true });
      }
    } catch {}
  }

  getVersion() {
    return this._version;
  }

  setVersion(v) {
    this._version = v;
  }

  getPath(name) {
    if (name === 'temp') return this._tempDir;
    if (name === 'userData') return this._userDataDir;
    if (name === 'exe') return join(this._userDataDir, 'ZABOR.exe');
    return this._tempDir;
  }

  getAppPath() {
    return process.cwd();
  }

  requestSingleInstanceLock() {
    return this._hasSingleInstanceLock;
  }

  releaseSingleInstanceLock() {
    this.releasedLock = true;
  }

  exit(code = 0) {
    this.exitCode = code;
    this.isQuit = true;
    this.emit('quit');
  }

  quit() {
    this.exit(0);
  }

  reset() {
    this.cleanupTempDir();
    this.initTempDir();
    this._version = '3.6.4';
    this.exitCode = null;
    this.isQuit = false;
    this.releasedLock = false;
    this._hasSingleInstanceLock = true;
    this.removeAllListeners();
  }
}

const app = new MockApp();
const ipcMain = new MockIpcMain();
const ipcRenderer = new MockIpcRenderer(ipcMain);
const BrowserWindow = MockBrowserWindow;

const shell = {
  openedUrls: [],
  openedPaths: [],
  async openExternal(url) {
    shell.openedUrls.push(url);
    return true;
  },
  async openPath(path) {
    shell.openedPaths.push(path);
    return '';
  },
  reset() {
    shell.openedUrls = [];
    shell.openedPaths = [];
  }
};

const dialog = {
  showErrorBox(title, content) {
    dialog.errors.push({ title, content });
  },
  errors: [],
  reset() {
    dialog.errors = [];
  }
};

export {
  app,
  ipcMain,
  ipcRenderer,
  BrowserWindow,
  shell,
  dialog,
  MockBrowserWindow,
  MockApp,
  MockIpcMain
};

export default {
  app,
  ipcMain,
  ipcRenderer,
  BrowserWindow,
  shell,
  dialog
};
