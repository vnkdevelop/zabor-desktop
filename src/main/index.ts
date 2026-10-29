import { app, shell, BrowserWindow, ipcMain, Tray, Menu, nativeImage, dialog, safeStorage, session, screen, desktopCapturer } from 'electron';

if (!app || typeof app.whenReady !== 'function') {
  console.error('\n[ZABOR] Ошибка запуска: проект ZABOR является Electron-приложением и не может быть запущен через обычный Node.js ("node out/main/index.js")!');
  console.error('[ZABOR] Для запуска используйте команду в терминале: npm run dev (или скрипт sync.bat)\n');
  process.exit(1);
}
import { join } from 'path';
import { existsSync, rmSync, readFileSync, writeFileSync, promises as fsPromises } from 'fs';
import { createHmac, randomBytes } from 'crypto';
import { setupUpdater } from './updater';
import { disposeChatFileWriters, registerChatFileHandlers } from './chatFiles';
import { RemoteControlSimulatorManager } from './remoteControlSimulator';

declare const __ZABOR_CLIENT_SECRET__: string;
declare const __ZABOR_CLIENT_CHANNEL__: string;

const CLIENT_SECRET = typeof __ZABOR_CLIENT_SECRET__ === 'string' ? __ZABOR_CLIENT_SECRET__ : '';
const CLIENT_CHANNEL = typeof __ZABOR_CLIENT_CHANNEL__ === 'string' ? __ZABOR_CLIENT_CHANNEL__ : 'unofficial';
const ATTESTATION_SCHEME = 'v1';

function buildClientAttestation(): string | null {
  if (!CLIENT_SECRET) return null;
  const timestamp = Math.floor(Date.now() / 1000);
  const nonce = randomBytes(8).toString('hex');
  const payload = `${ATTESTATION_SCHEME}:${CLIENT_CHANNEL}:${app.getVersion()}:${timestamp}:${nonce}`;
  const signature = createHmac('sha256', CLIENT_SECRET).update(payload).digest('base64url');
  return `${payload}:${signature}`;
}

interface StreamAudioMetadata {
  sampleRate: number;
  channels: number;
  bitsPerSample: number;
  isFloat: boolean;
}

interface NativeScreenShareAudio {
  startCapture: (
    processId: number,
    isIncludeMode: boolean,
    onData: (data: Buffer, metadata: StreamAudioMetadata) => void
  ) => boolean;
  stopCapture: () => boolean;
  getPidFromWindowHandle: (windowHandle: number) => number;
  isAvailable: () => boolean;
  getLoadError: () => string | null;
}

let nativeScreenShareAudio: NativeScreenShareAudio | null = null;
try {
  nativeScreenShareAudio = require('electron-native-screenshare') as NativeScreenShareAudio;
} catch (e) {
  console.warn('[Audio] Failed to load electron-native-screenshare:', e);
}

const MAX_GPU_FALLBACK_TIER = 2;

let gpuFallbackTier = 0;
let gpuFallbackEscalated = false;

function gpuFallbackPath(): string {
  return join(app.getPath('userData'), 'gpu-fallback.json');
}

function clampGpuTier(value: unknown): number {
  const tier = Number(value);
  if (!Number.isFinite(tier)) return 0;
  return Math.max(0, Math.min(MAX_GPU_FALLBACK_TIER, Math.trunc(tier)));
}

function readGpuFallbackTier(): number {
  if (process.env.ZABOR_GPU_TIER !== undefined) return clampGpuTier(process.env.ZABOR_GPU_TIER);
  try {
    const state = JSON.parse(readFileSync(gpuFallbackPath(), 'utf-8'));
    if (state?.version !== app.getVersion()) return 0;
    return clampGpuTier(state?.tier);
  } catch {
    return 0;
  }
}

function writeGpuFallbackTier(tier: number, reason: string) {
  try {
    writeFileSync(
      gpuFallbackPath(),
      JSON.stringify({ tier, reason, version: app.getVersion(), updatedAt: new Date().toISOString() }, null, 2)
    );
  } catch (error) {
    console.warn('[GPU] Could not persist the acceleration tier:', error);
  }
}

function webrtcIpPolicyPath(): string {
  return join(app.getPath('userData'), 'webrtc-ip-policy.json');
}

function readWebRtcRelayOnly(): boolean {
  if (process.env.ZABOR_WEBRTC_IP_POLICY !== undefined) {
    return process.env.ZABOR_WEBRTC_IP_POLICY === 'relay';
  }
  try {
    const state = JSON.parse(readFileSync(webrtcIpPolicyPath(), 'utf-8'));
    return state?.relayOnly === true;
  } catch {
    return false;
  }
}

function writeWebRtcRelayOnly(relayOnly: boolean): void {
  try {
    writeFileSync(
      webrtcIpPolicyPath(),
      JSON.stringify({ relayOnly, updatedAt: new Date().toISOString() }, null, 2)
    );
  } catch (error) {
    console.warn('[WebRTC] Could not persist the IP handling policy:', error);
  }
}

function reportGpuStatus() {
  try {
    const status = app.getGPUFeatureStatus() as unknown as Record<string, string>;
    const compositing = status?.gpu_compositing ?? 'unknown';
    const canvas = status?.['2d_canvas'] ?? 'unknown';
    const video = status?.video_decode ?? 'unknown';
    console.log(`[GPU] tier=${gpuFallbackTier} compositing=${compositing} canvas=${canvas} video=${video}`);
  } catch (error) {
    console.warn('[GPU] Could not read the feature status:', error);
  }
}

if (app) {
  gpuFallbackTier = process.env.ZABOR_DISABLE_GPU === '1' ? MAX_GPU_FALLBACK_TIER : readGpuFallbackTier();

  app.commandLine.appendSwitch('high-dpi-support', '1');
  app.commandLine.appendSwitch('force-color-profile', 'srgb');
  app.commandLine.appendSwitch('enable-font-antialiasing');
  app.commandLine.appendSwitch('enable-subpixel-font-rendering');
  app.commandLine.appendSwitch('disable-backgrounding-occluded-windows');
  app.commandLine.appendSwitch('disable-renderer-backgrounding');
  app.commandLine.appendSwitch('disable-background-timer-throttling');
  app.commandLine.appendSwitch('enable-webrtc-srtp-aes-gcm');
  app.commandLine.appendSwitch(
    'force-webrtc-ip-handling-policy',
    readWebRtcRelayOnly() ? 'disable_non_proxied_udp' : 'default'
  );
  if (process.env.ZABOR_SCALE_FACTOR) {
    app.commandLine.appendSwitch('force-device-scale-factor', process.env.ZABOR_SCALE_FACTOR);
  }

  if (gpuFallbackTier === 0) {
    app.commandLine.appendSwitch('enable-gpu-rasterization');
    app.commandLine.appendSwitch('enable-zero-copy');
    app.commandLine.appendSwitch('enable-accelerated-video-encode');
    app.commandLine.appendSwitch('enable-accelerated-video-decode');
    app.commandLine.appendSwitch('video-capture-use-gpu-memory-buffer');

    app.commandLine.appendSwitch(
      'enable-features',
      'WebRtcAllowWgcScreenCapturer,WebRtcAllowWgcWindowCapturer,AcceleratedVideoEncoder'
    );
  }

  if (gpuFallbackTier >= 1) {
    app.commandLine.appendSwitch('disable-accelerated-2d-canvas');
    app.commandLine.appendSwitch('disable-accelerated-video-decode');
    app.commandLine.appendSwitch('disable-gpu-rasterization');
  }

  if (gpuFallbackTier >= MAX_GPU_FALLBACK_TIER) {
    app.commandLine.appendSwitch('disable-gpu-compositing');
    app.disableHardwareAcceleration();
  }

  app.on('child-process-gone', (_event, details) => {
    if (details.type !== 'GPU') return;
    console.warn(`[GPU] GPU process gone: ${details.reason} (exit ${details.exitCode})`);
    if (details.reason === 'clean-exit') return;
    if (gpuFallbackEscalated || gpuFallbackTier >= MAX_GPU_FALLBACK_TIER) return;
    gpuFallbackEscalated = true;
    const nextTier = gpuFallbackTier + 1;
    writeGpuFallbackTier(nextTier, `gpu-${details.reason}`);
    console.warn(`[GPU] Acceleration tier ${nextTier} will be used on the next launch`);
  });

  if (!app.isPackaged) {
    const port = process.env.REMOTE_DEBUGGING_PORT || '9222';
    app.commandLine.appendSwitch('remote-debugging-port', port);
  }
}

const isDev = !app.isPackaged;

const DEEPFILTER_ASSETS = new Set(['pkg/df_bg.wasm', 'models/DeepFilterNet3_onnx.tar.gz']);
const MIN_DEEPFILTER_ASSET_BYTES = 100 * 1024;

let bundledAssetBase: string | null = null;

function bundledAssetBases(): string[] {
  return [
    join(__dirname, '../renderer'),
    join(app.getAppPath(), 'out/renderer'),
    join(app.getAppPath(), 'src/renderer/public'),
    join(process.resourcesPath || '', 'renderer'),
    process.resourcesPath || ''
  ];
}

async function readBundledAsset(relativePath: string): Promise<Uint8Array | null> {
  const candidates = bundledAssetBase
    ? [bundledAssetBase, ...bundledAssetBases()]
    : bundledAssetBases();

  const tried: string[] = [];
  for (const base of candidates) {
    if (!base) continue;
    const full = join(base, relativePath);
    if (tried.includes(full)) continue;
    tried.push(full);
    try {
      if (!existsSync(full)) continue;
      const bytes = await fsPromises.readFile(full);
      if (bytes.byteLength < MIN_DEEPFILTER_ASSET_BYTES) continue;
      bundledAssetBase = base;
      return bytes;
    } catch {
    }
  }

  console.warn(`[Assets] ${relativePath} not found. Tried:\n  ${tried.join('\n  ')}`);
  return null;
}

let mainWindow: BrowserWindow | null = null;
let updaterWindow: BrowserWindow | null = null;
let overlayPillWindow: BrowserWindow | null = null;
let remoteControlSimulator: RemoteControlSimulatorManager | null = null;
let tray: Tray | null = null;
let isQuitting = false;
let quitRequested = false;
let streamAudioOwnerId: number | null = null;

function showPillOverlay(controllerName: string): void {
  try {
    const primary = screen.getPrimaryDisplay();
    const width = 160;
    const height = 32;
    const x = Math.round(primary.bounds.x + (primary.bounds.width - width) / 2);
    const y = primary.bounds.y + 10;

    if (!overlayPillWindow || overlayPillWindow.isDestroyed()) {
      overlayPillWindow = new BrowserWindow({
        width,
        height,
        x,
        y,
        frame: false,
        transparent: true,
        alwaysOnTop: true,
        skipTaskbar: true,
        resizable: false,
        focusable: false,
        hasShadow: false,
        show: false,
        webPreferences: {
          contextIsolation: true,
          nodeIntegration: false
        }
      });
      overlayPillWindow.setAlwaysOnTop(true, 'screen-saver');
      overlayPillWindow.setVisibleOnAllWorkspaces(true);
      overlayPillWindow.setIgnoreMouseEvents(true);

      overlayPillWindow.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
    }

    const safeName = (controllerName || 'viewer').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
    const htmlContent = `<!DOCTYPE html><html><head><meta charset="utf-8"><style>
*{margin:0;padding:0;box-sizing:border-box;}
body{width:100vw;height:100vh;display:flex;align-items:center;justify-content:center;background:transparent;user-select:none;font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif;}
.pill{display:flex;align-items:center;gap:7px;padding:4px 12px;background:rgba(18,18,20,0.88);backdrop-filter:blur(16px);-webkit-backdrop-filter:blur(16px);border:1px solid rgba(255,255,255,0.08);border-top:1px solid rgba(255,255,255,0.14);border-radius:9999px;color:rgba(255,255,255,0.9);font-size:11px;font-weight:600;letter-spacing:0.02em;pointer-events:none;}
.dot{width:6px;height:6px;border-radius:50%;background:#ffffff;flex-shrink:0;}
.name{max-width:115px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;text-transform:lowercase;}
</style></head><body>
<div class="pill">
<div class="dot"></div>
<span class="name">${safeName}</span>
</div>
</body></html>`;

    overlayPillWindow.loadURL(`data:text/html;charset=utf-8,${encodeURIComponent(htmlContent)}`);
    overlayPillWindow.showInactive();
  } catch (err) {
    console.warn('[OverlayPill] Failed to show pill overlay:', err);
  }
}

function hidePillOverlay(): void {
  try {
    if (overlayPillWindow && !overlayPillWindow.isDestroyed()) {
      overlayPillWindow.hide();
    }
  } catch {}
}

function stopStreamAudioCapture(): void {
  if (streamAudioOwnerId === null) return;
  streamAudioOwnerId = null;
  try {
    if (nativeScreenShareAudio?.isAvailable?.()) nativeScreenShareAudio.stopCapture();
  } catch (error) {
    console.warn('[StreamAudio] Failed to stop native audio capture:', error);
  }
}

function getWindowHandle(sourceId: string): number | null {
  const match = /^window:(\d+):\d+$/.exec(sourceId);
  if (!match) return null;
  const handle = Number(match[1]);
  return Number.isSafeInteger(handle) && handle > 0 ? handle : null;
}

function getMainWindowHandle(): string | null {
  if (!mainWindow || mainWindow.isDestroyed()) return null;
  const handle = mainWindow.getNativeWindowHandle();
  if (handle.length >= 8) return handle.readBigUInt64LE().toString();
  if (handle.length >= 4) return String(handle.readUInt32LE());
  return null;
}

function requestQuit(): void {
  if (isQuitting || quitRequested) return;
  quitRequested = true;
  if (updaterWindow && !updaterWindow.isDestroyed()) {
    try {
      updaterWindow.destroy();
    } catch {}
    updaterWindow = null;
  }
  if (mainWindow && !mainWindow.isDestroyed()) {
    try {
      mainWindow.webContents.send('before-quit');
    } catch {}
    setTimeout(() => {
      isQuitting = true;
      app.quit();
    }, 1500);
    return;
  }
  isQuitting = true;
  app.quit();
}

const gotLock = app.requestSingleInstanceLock();

if (!gotLock) {
  app.quit();
} else {
  app.on('second-instance', () => {
    if (mainWindow && !mainWindow.isDestroyed()) {
      if (!mainWindow.isVisible()) mainWindow.show();
      if (mainWindow.isMinimized()) mainWindow.restore();
      mainWindow.focus();
    }
  });
}

interface AppSettings {
  openAtLogin: boolean;
  minimizeToTray: boolean;
}

function getSettingsPath(): string {
  return join(app.getPath('userData'), 'app-settings.json');
}

function loadAppSettings(): AppSettings {
  let parsed: Partial<AppSettings> = {};
  try {
    const filePath = getSettingsPath();
    if (existsSync(filePath)) {
      parsed = JSON.parse(readFileSync(filePath, 'utf-8'));
    }
  } catch {}

  return {
    openAtLogin: parsed.openAtLogin !== undefined
      ? parsed.openAtLogin
      : (isDev ? false : app.getLoginItemSettings({ args: ['--autostart'] }).openAtLogin),
    minimizeToTray: parsed.minimizeToTray !== undefined
      ? parsed.minimizeToTray
      : true
  };
}

function saveAppSettings(settings: AppSettings): void {
  try {
    writeFileSync(getSettingsPath(), JSON.stringify(settings, null, 2), 'utf-8');
  } catch {}
}

function applyAutoLaunch(enabled: boolean): void {
  if (isDev) {
    app.setLoginItemSettings({ openAtLogin: false });
    return;
  }
  app.setLoginItemSettings({
    openAtLogin: enabled,
    args: enabled ? ['--autostart'] : [],
  });
}

interface WindowState {
  x?: number;
  y?: number;
  width?: number;
  height?: number;
  isMaximized?: boolean;
}

function getWindowStatePath(): string {
  return join(app.getPath('userData'), 'window-state.json');
}

function loadWindowState(): WindowState | null {
  try {
    const filePath = getWindowStatePath();
    if (existsSync(filePath)) {
      return JSON.parse(readFileSync(filePath, 'utf-8'));
    }
  } catch {}
  return null;
}

function saveWindowState(state: WindowState): void {
  try {
    writeFileSync(getWindowStatePath(), JSON.stringify(state, null, 2), 'utf-8');
  } catch {}
}

let stateSaveTimer: NodeJS.Timeout | null = null;
function scheduleWindowStateSave() {
  if (stateSaveTimer) clearTimeout(stateSaveTimer);
  stateSaveTimer = setTimeout(() => {
    if (!mainWindow) return;
    try {
      const state: WindowState = {
        isMaximized: mainWindow.isMaximized(),
      };
      const bounds = mainWindow.getNormalBounds();
      state.x = bounds.x;
      state.y = bounds.y;
      state.width = bounds.width;
      state.height = bounds.height;
      saveWindowState(state);
    } catch {}
  }, 500);
}

function createTray(): void {
  const iconPath = isDev
    ? join(__dirname, '../../build/icon.ico')
    : join(process.resourcesPath, 'icon.ico');

  let trayIcon: Electron.NativeImage;

  if (existsSync(iconPath)) {
    trayIcon = nativeImage.createFromPath(iconPath);
    trayIcon = trayIcon.resize({ width: 16, height: 16 });
  } else {
    trayIcon = nativeImage.createEmpty();
  }

  tray = new Tray(trayIcon);
  tray.setToolTip('ZABOR');

  const contextMenu = Menu.buildFromTemplate([
    {
      label: 'Открыть ZABOR',
      click: () => {
        if (mainWindow && !mainWindow.isDestroyed()) {
          mainWindow.show();
          mainWindow.focus();
        } else {
          createWindow();
        }
      }
    },
    { type: 'separator' },
    {
      label: 'Выйти',
      click: requestQuit
    }
  ]);

  tray.setContextMenu(contextMenu);

  tray.on('click', () => {
    if (mainWindow && !mainWindow.isDestroyed()) {
      if (mainWindow.isVisible()) {
        mainWindow.focus();
      } else {
        mainWindow.show();
        mainWindow.focus();
      }
    } else {
      createWindow();
    }
  });
}

function createWindow(): void {
  const isAutoStart = process.argv.includes('--autostart');

  let stateOptions: Partial<Electron.BrowserWindowConstructorOptions> = {};
  let savedState: WindowState | null = null;

  if (isAutoStart) {
    savedState = loadWindowState();
    if (savedState) {
      if (savedState.x !== undefined && savedState.y !== undefined) {
        const { screen } = require('electron');
        const displays = screen.getAllDisplays();
        const isVisible = displays.some((display: Electron.Display) => {
          const bounds = display.bounds;
          return (
            savedState!.x! >= bounds.x &&
            savedState!.y! >= bounds.y &&
            savedState!.x! < bounds.x + bounds.width &&
            savedState!.y! < bounds.y + bounds.height
          );
        });

        if (isVisible) {
          stateOptions = {
            x: savedState.x,
            y: savedState.y,
            width: savedState.width || 1280,
            height: savedState.height || 800,
          };
        } else if (savedState.width && savedState.height) {
          stateOptions = {
            width: savedState.width,
            height: savedState.height,
          };
        }
      } else {
        stateOptions = {
          width: savedState.width || 1280,
          height: savedState.height || 800,
        };
      }
    }
  }

  mainWindow = new BrowserWindow({
    width: 1280,
    height: 800,
    minWidth: 900,
    minHeight: 600,
    frame: false,
    show: false,
    backgroundColor: '#131313',
    ...stateOptions,
    webPreferences: {
      preload: join(__dirname, '../preload/index.js'),
      contextIsolation: true,
      nodeIntegration: false,
      backgroundThrottling: false,
      devTools: isDev
    }
  });

  if (isAutoStart && savedState?.isMaximized) {
    mainWindow.maximize();
  }

  const showMainWindow = () => {
    if (!mainWindow || mainWindow.isDestroyed()) return;
    if (mainWindow.isMinimized()) mainWindow.restore();
    mainWindow.show();
    mainWindow.focus();
  };

  mainWindow.once('ready-to-show', () => {
    clearTimeout(showFallbackTimer);
    showMainWindow();
    try {
      const pid = mainWindow?.webContents.getOSProcessId();
      if (pid) {
        const os = require('os');
        os.setPriority(pid, os.constants.priority.PRIORITY_HIGH);
      }
    } catch {}
  });

  const showFallbackTimer = setTimeout(showMainWindow, 3000);
  mainWindow.webContents.on('did-fail-load', (_event, errorCode, errorDescription, validatedURL, isMainFrame) => {
    if (!isMainFrame) return;
    clearTimeout(showFallbackTimer);
    showMainWindow();
    dialog.showErrorBox(
      'ZABOR не удалось запустить',
      `Не удалось загрузить интерфейс (${errorCode}): ${errorDescription}\n${validatedURL}`
    );
  });
  mainWindow.webContents.on('render-process-gone', (_event, details) => {
    dialog.showErrorBox('ZABOR аварийно завершил работу', `Причина: ${details.reason}`);
  });

  mainWindow.on('resize', scheduleWindowStateSave);
  mainWindow.on('move', scheduleWindowStateSave);

  mainWindow.on('maximize', () => {
    scheduleWindowStateSave();
    mainWindow?.webContents.send('window-maximized');
  });

  mainWindow.on('unmaximize', () => {
    scheduleWindowStateSave();
    mainWindow?.webContents.send('window-unmaximized');
  });

  mainWindow.on('blur', () => {
    remoteControlSimulator?.unclipCursor();
  });

  mainWindow.on('minimize', () => {
    remoteControlSimulator?.unclipCursor();
  });

  mainWindow.on('close', (event) => {
    const settings = loadAppSettings();
    if (!isQuitting && settings.minimizeToTray) {
      event.preventDefault();
      scheduleWindowStateSave();
      mainWindow?.hide();
    } else {
      scheduleWindowStateSave();
    }
  });

  mainWindow.on('closed', () => {
    mainWindow = null;
  });

  mainWindow.webContents.on('will-navigate', (event) => {
    event.preventDefault();
  });

  mainWindow.webContents.setWindowOpenHandler(({ url }) => {
    try {
      const scheme = new URL(url).protocol;
      if (scheme === 'https:' || scheme === 'http:') shell.openExternal(url);
    } catch {}
    return { action: 'deny' };
  });

  if (!isDev) {
    mainWindow.webContents.on('before-input-event', (event, input) => {
      const key = input.key.toLowerCase();
      if (
        (input.control && input.shift && (key === 'i' || key === 'j' || key === 'c')) ||
        ((input.meta || input.control) && input.alt && (key === 'i' || key === 'j')) ||
        key === 'f12'
      ) {
        event.preventDefault();
      }
    });

    mainWindow.webContents.on('devtools-opened', () => {
      mainWindow?.webContents.closeDevTools();
    });
  }

  const rendererLoad = isDev && process.env['ELECTRON_RENDERER_URL']
    ? mainWindow.loadURL(process.env['ELECTRON_RENDERER_URL'])
    : mainWindow.loadFile(join(__dirname, '../renderer/index.html'));
  rendererLoad.catch(error => {
    clearTimeout(showFallbackTimer);
    showMainWindow();
    dialog.showErrorBox('ZABOR не удалось запустить', error instanceof Error ? error.message : String(error));
  });
}

function createUpdaterWindow(): BrowserWindow {
  if (updaterWindow && !updaterWindow.isDestroyed()) {
    if (updaterWindow.isMinimized()) updaterWindow.restore();
    updaterWindow.show();
    updaterWindow.focus();
    return updaterWindow;
  }

  updaterWindow = new BrowserWindow({
    width: 420,
    height: 240,
    frame: false,
    alwaysOnTop: true,
    resizable: false,
    minimizable: false,
    maximizable: false,
    center: true,
    backgroundColor: '#09090B',
    show: false,
    webPreferences: {
      preload: join(__dirname, '../preload/index.js'),
      contextIsolation: true,
      nodeIntegration: false,
      backgroundThrottling: false,
      devTools: isDev
    }
  });

  updaterWindow.once('ready-to-show', () => {
    if (!updaterWindow || updaterWindow.isDestroyed()) return;
    updaterWindow.show();
    updaterWindow.focus();
  });

  updaterWindow.on('closed', () => {
    updaterWindow = null;
  });

  updaterWindow.webContents.on('will-navigate', (event) => {
    event.preventDefault();
  });

  updaterWindow.webContents.setWindowOpenHandler(({ url }) => {
    try {
      const scheme = new URL(url).protocol;
      if (scheme === 'https:' || scheme === 'http:') shell.openExternal(url);
    } catch {}
    return { action: 'deny' };
  });

  if (!isDev) {
    updaterWindow.webContents.on('before-input-event', (event, input) => {
      const key = input.key.toLowerCase();
      if (
        (input.control && input.shift && (key === 'i' || key === 'j' || key === 'c')) ||
        ((input.meta || input.control) && input.alt && (key === 'i' || key === 'j')) ||
        key === 'f12'
      ) {
        event.preventDefault();
      }
    });

    updaterWindow.webContents.on('devtools-opened', () => {
      updaterWindow?.webContents.closeDevTools();
    });
  }

  const updaterLoad = isDev && process.env['ELECTRON_RENDERER_URL']
    ? updaterWindow.loadURL(`${process.env['ELECTRON_RENDERER_URL']}#updater`)
    : updaterWindow.loadFile(join(__dirname, '../renderer/index.html'), { hash: 'updater' });

  updaterLoad.catch((error) => {
    console.error('[UpdaterWindow] Failed to load updater window:', error);
  });

  return updaterWindow;
}

app.whenReady().then(() => {
  if (!isDev) {
    Menu.setApplicationMenu(null);
  }
  try {
    const os = require('os');
    os.setPriority(os.constants.priority.PRIORITY_HIGH);
  } catch {}
  reportGpuStatus();
  session.defaultSession.setPermissionCheckHandler((_webContents, permission) => {
    return permission === 'media' || permission === 'display-capture';
  });
  session.defaultSession.setPermissionRequestHandler((_webContents, permission, callback) => {
    callback(permission === 'media' || permission === 'display-capture');
  });

  let selectedDisplayMediaSourceId: string | null = null;
  ipcMain.handle('set-display-media-source', (_event, sourceId: string | null) => {
    selectedDisplayMediaSourceId = sourceId;
    return true;
  });

  session.defaultSession.setDisplayMediaRequestHandler((_request, callback) => {
    desktopCapturer.getSources({ types: ['screen', 'window'], thumbnailSize: { width: 0, height: 0 } }).then((sources) => {
      let chosen = selectedDisplayMediaSourceId
        ? sources.find((s) => s.id === selectedDisplayMediaSourceId)
        : null;
      if (!chosen && sources.length > 0) {
        chosen = sources[0];
      }
      if (chosen) {
        callback({ video: chosen });
      } else {
        callback({});
      }
    }).catch(() => callback({}));
  });
  const settings = loadAppSettings();
  applyAutoLaunch(settings.openAtLogin);
  registerChatFileHandlers();

  ipcMain.on('window-minimize', () => {
    BrowserWindow.getFocusedWindow()?.minimize();
  });

  ipcMain.on('window-maximize', () => {
    const win = BrowserWindow.getFocusedWindow();
    if (win) {
      win.isMaximized() ? win.unmaximize() : win.maximize();
    }
  });

  ipcMain.on('window-close', (event) => {
    const senderWindow = BrowserWindow.fromWebContents(event.sender);
    if (senderWindow && updaterWindow && senderWindow === updaterWindow) {
      updaterWindow.close();
      return;
    }

    const settings = loadAppSettings();
    if (settings.minimizeToTray) {
      if (mainWindow && !mainWindow.isDestroyed()) {
        mainWindow.hide();
      }
    } else {
      requestQuit();
    }
  });

  ipcMain.handle('open-updater-window', () => {
    createUpdaterWindow();
    return true;
  });

  ipcMain.handle('close-updater-window', () => {
    if (updaterWindow && !updaterWindow.isDestroyed()) {
      updaterWindow.close();
      return true;
    }
    return false;
  });

  ipcMain.on('app-quit', () => {
    requestQuit();
  });

  ipcMain.handle('wipe-app-data', async (_event, options?: unknown) => {
    const preserveChat = typeof options === 'object' && options !== null && (options as { preserveChat?: unknown }).preserveChat === true;
    const userDataPath = app.getPath('userData');
    const dirsToKill = [
      'Local Storage',
      'Session Storage',
      'IndexedDB',
      'Cache',
      'Code Cache',
      'GPUCache',
      'Service Worker',
      'blob_storage'
    ];
    for (const dir of dirsToKill) {
      if (preserveChat && dir === 'IndexedDB') continue;
      const fullPath = join(userDataPath, dir);
      try {
        if (existsSync(fullPath)) {
          rmSync(fullPath, { recursive: true, force: true });
        }
      } catch {}
    }
    if (!preserveChat) {
      try {
        const chatFilesPath = join(userDataPath, 'chat-files');
        if (existsSync(chatFilesPath)) rmSync(chatFilesPath, { recursive: true, force: true });
      } catch {}
    }
    try {
      if (mainWindow && !mainWindow.isDestroyed()) {
        const ses = mainWindow.webContents.session;
        if (!preserveChat) await ses.clearStorageData();
        await ses.clearCache();
      }
    } catch {}
    return true;
  });

  const SESSION_PATH = join(app.getPath('userData'), 'session.json');
  const SESSION_ENC_PATH = join(app.getPath('userData'), 'session.enc');
  const CHAT_IDENTITY_PATH = join(app.getPath('userData'), 'chat-identity.enc');

  ipcMain.handle('save-session', async (_event, data: string) => {
    try {
      if (safeStorage.isEncryptionAvailable()) {
        await fsPromises.writeFile(SESSION_ENC_PATH, safeStorage.encryptString(data));
        if (existsSync(SESSION_PATH)) await fsPromises.rm(SESSION_PATH, { force: true });
        return true;
      }
      return false;
    } catch { return false; }
  });

  ipcMain.handle('load-session', async () => {
    try {
      if (existsSync(SESSION_ENC_PATH) && safeStorage.isEncryptionAvailable()) {
        return safeStorage.decryptString(await fsPromises.readFile(SESSION_ENC_PATH));
      }
    } catch {}
    try {
      if (existsSync(SESSION_PATH)) {
        const legacy = await fsPromises.readFile(SESSION_PATH, 'utf-8');
        if (safeStorage.isEncryptionAvailable()) {
          try {
            await fsPromises.writeFile(SESSION_ENC_PATH, safeStorage.encryptString(legacy));
            await fsPromises.rm(SESSION_PATH, { force: true });
          } catch {}
        }
        return legacy;
      }
    } catch {}
    return null;
  });

  ipcMain.handle('clear-session', async () => {
    try { if (existsSync(SESSION_PATH)) await fsPromises.rm(SESSION_PATH, { force: true }); } catch {}
    try { if (existsSync(SESSION_ENC_PATH)) await fsPromises.rm(SESSION_ENC_PATH, { force: true }); } catch {}
    return true;
  });
  ipcMain.handle('chat-identity-load', async () => {
    try {
      if (!safeStorage.isEncryptionAvailable() || !existsSync(CHAT_IDENTITY_PATH)) return null;
      return safeStorage.decryptString(await fsPromises.readFile(CHAT_IDENTITY_PATH));
    } catch { return null; }
  });
  ipcMain.handle('chat-identity-save', async (_event, data: unknown) => {
    if (typeof data !== 'string' || data.length > 16 * 1024 || !safeStorage.isEncryptionAvailable()) return false;
    try {
      await fsPromises.writeFile(CHAT_IDENTITY_PATH, safeStorage.encryptString(data));
      return true;
    } catch { return false; }
  });

  ipcMain.handle('get-client-attestation', () => {
    try { return buildClientAttestation(); } catch { return null; }
  });

  ipcMain.handle('get-userdata-path', () => {
    return app.getPath('userData');
  });

  ipcMain.handle('load-silero-model', async () => {
    const model = await readBundledAsset('silero_vad.onnx');
    if (!model) throw new Error('silero_vad.onnx not found in any known location');
    return model;
  });

  ipcMain.handle('load-deepfilter-asset', async (_event, assetPath: unknown) => {
    if (typeof assetPath !== 'string' || !DEEPFILTER_ASSETS.has(assetPath)) return null;
    return readBundledAsset(join('deepfilternet3', ...assetPath.split('/')));
  });

  ipcMain.handle('get-auto-launch', () => {
    if (isDev) return false;

    const osSetting = app.getLoginItemSettings({ args: ['--autostart'] }).openAtLogin;
    return osSetting;
  });

  ipcMain.handle('set-auto-launch', (_event, enabled: boolean) => {
    const currentSettings = loadAppSettings();
    currentSettings.openAtLogin = enabled;
    saveAppSettings(currentSettings);
    applyAutoLaunch(enabled);
    return true;
  });

  ipcMain.handle('get-minimize-to-tray', () => {
    return loadAppSettings().minimizeToTray;
  });

  ipcMain.handle('set-minimize-to-tray', (_event, enabled: boolean) => {
    const currentSettings = loadAppSettings();
    currentSettings.minimizeToTray = enabled;
    saveAppSettings(currentSettings);
    return true;
  });

  ipcMain.handle('get-webrtc-relay-only', () => {
    return readWebRtcRelayOnly();
  });

  ipcMain.handle('set-webrtc-relay-only', (_event, enabled: boolean) => {
    writeWebRtcRelayOnly(enabled === true);
    return true;
  });

  ipcMain.handle('get-desktop-sources', async (_event, options) => {
    const { desktopCapturer } = require('electron')
    const sources = await desktopCapturer.getSources(options)
    const ownWindowHandle = getMainWindowHandle()
    return sources
      .filter(src => ownWindowHandle === null || getWindowHandle(src.id)?.toString() !== ownWindowHandle)
      .map(src => ({
        id: src.id,
        name: src.name,
        thumbnail: src.thumbnail.toDataURL(),
        appIcon: src.appIcon ? src.appIcon.toDataURL() : null
      }))
  })

  ipcMain.handle('start-stream-audio-capture', (event, sourceId: unknown) => {
    if (typeof sourceId !== 'string') throw new TypeError('A desktop source id is required')
    if (!nativeScreenShareAudio || typeof nativeScreenShareAudio.isAvailable !== 'function' || !nativeScreenShareAudio.isAvailable()) {
      throw new Error(nativeScreenShareAudio?.getLoadError?.() || 'Native stream audio capture is unavailable')
    }

    stopStreamAudioCapture()

    const windowHandle = getWindowHandle(sourceId)
    const isWindow = windowHandle !== null
    if (!isWindow && !sourceId.startsWith('screen:')) {
      throw new Error(`Unsupported desktop source: ${sourceId}`)
    }

    const targetProcessId = isWindow
      ? nativeScreenShareAudio.getPidFromWindowHandle(windowHandle)
      : process.pid
    if (!targetProcessId) throw new Error(`Unable to resolve the process for source ${sourceId}`)

    const sender = event.sender
    streamAudioOwnerId = sender.id
    try {
      const started = nativeScreenShareAudio.startCapture(targetProcessId, isWindow, (data, metadata) => {
        if (streamAudioOwnerId !== sender.id || sender.isDestroyed()) return
        sender.send('stream-audio-data', data, metadata)
      })
      if (started) return true
      throw new Error('Native stream audio capture did not start')
    } catch (error) {
      try {
        if (nativeScreenShareAudio?.isAvailable?.()) nativeScreenShareAudio.stopCapture()
      } catch { }
      streamAudioOwnerId = null
      throw error
    }
  })

  ipcMain.handle('stop-stream-audio-capture', (event) => {
    if (streamAudioOwnerId === event.sender.id) stopStreamAudioCapture()
    return true
  })

  createWindow();
  createTray();
  remoteControlSimulator = new RemoteControlSimulatorManager(() => mainWindow);

  ipcMain.handle('remote-control:start', () => {
    return remoteControlSimulator.start();
  });

  ipcMain.handle('remote-control:stop', () => {
    hidePillOverlay();
  remoteControlSimulator?.unclipCursor();
    remoteControlSimulator.stop();
    return true;
  });

  ipcMain.handle('remote-control:reset', () => {
    hidePillOverlay();
  remoteControlSimulator?.unclipCursor();
    remoteControlSimulator.reset();
    return true;
  });

  ipcMain.handle('remote-control:get-screen', () => {
    return remoteControlSimulator?.getScreenResolution() || { width: 1920, height: 1080, x: 0, y: 0 };
  });

  ipcMain.handle('remote-control:clip-cursor', (event, rect: { left: number; top: number; right: number; bottom: number }) => {
    const win = BrowserWindow.fromWebContents(event.sender) || mainWindow;
    if (!win || win.isDestroyed()) {
      remoteControlSimulator?.clipCursor(rect);
      return true;
    }
    const contentBounds = win.getContentBounds();
    const dipRect = {
      x: contentBounds.x + rect.left,
      y: contentBounds.y + rect.top,
      width: Math.max(1, rect.right - rect.left),
      height: Math.max(1, rect.bottom - rect.top)
    };

    let physicalRect: { left: number; top: number; right: number; bottom: number };
    if (typeof screen.dipToScreenRect === 'function') {
      const phys = screen.dipToScreenRect(win, dipRect);
      physicalRect = {
        left: phys.x,
        top: phys.y,
        right: phys.x + phys.width,
        bottom: phys.y + phys.height
      };
    } else {
      const display = screen.getDisplayMatching(contentBounds);
      const scale = display?.scaleFactor || 1;
      physicalRect = {
        left: Math.round((contentBounds.x + rect.left) * scale),
        top: Math.round((contentBounds.y + rect.top) * scale),
        right: Math.round((contentBounds.x + rect.right) * scale),
        bottom: Math.round((contentBounds.y + rect.bottom) * scale)
      };
    }

    remoteControlSimulator?.clipCursor(physicalRect);
    return true;
  });

  ipcMain.handle('remote-control:unclip-cursor', () => {
    remoteControlSimulator?.unclipCursor();
    return true;
  });

  ipcMain.handle('remote-control:hide-host-cursor', () => {
    remoteControlSimulator?.hideHostCursor();
    return true;
  });

  ipcMain.handle('remote-control:show-host-cursor', () => {
    remoteControlSimulator?.showHostCursor();
    return true;
  });

  ipcMain.on('remote-control:show-pill', (_event, data: { controllerName: string; isControlling: boolean }) => {
    showPillOverlay(data?.controllerName || 'viewer');
  });

  ipcMain.on('remote-control:hide-pill', () => {
    hidePillOverlay();
  remoteControlSimulator?.unclipCursor();
  });

  ipcMain.on('remote-control:move', (_event, absX: number, absY: number) => {
    remoteControlSimulator?.move(absX, absY);
  });

  ipcMain.on('remote-control:move-normalized', (_event, xNorm: number, yNorm: number) => {
    remoteControlSimulator?.moveNormalized(xNorm, yNorm);
  });

  ipcMain.on('remote-control:mouse-button', (_event, button: number, down: boolean) => {
    remoteControlSimulator.mouseButton(button, down);
  });

  ipcMain.on('remote-control:mouse-wheel', (_event, delta: number) => {
    remoteControlSimulator.mouseWheel(delta);
  });

  ipcMain.on('remote-control:key', (_event, vk: number, down: boolean) => {
    remoteControlSimulator.key(vk, down);
  });

  setupUpdater(() => mainWindow, createUpdaterWindow);

  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow();
  });
});

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') {
    const settings = loadAppSettings();
    if (!settings.minimizeToTray) {
      app.quit();
    }
  }
});

app.on('before-quit', (event) => {
  hidePillOverlay();
  remoteControlSimulator?.showHostCursor();
  remoteControlSimulator?.unclipCursor();
  stopStreamAudioCapture();
  disposeChatFileWriters();
  if (!isQuitting) {
    event.preventDefault();
    requestQuit();
  }
});

app.on('quit', () => {
  if (tray) {
    tray.destroy();
    tray = null;
  }
});
