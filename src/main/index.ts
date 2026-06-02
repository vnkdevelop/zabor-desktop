import { app, shell, BrowserWindow, ipcMain, Tray, Menu, nativeImage } from 'electron';
import { join } from 'path';
import { existsSync, rmSync, readFileSync, writeFileSync, promises as fsPromises } from 'fs';


if (app) {
  app.disableHardwareAcceleration();

  app.commandLine.appendSwitch('force-color-profile', 'srgb');
  app.commandLine.appendSwitch('disable-backgrounding-occluded-windows');
  app.commandLine.appendSwitch('disable-renderer-backgrounding');
  app.commandLine.appendSwitch('disable-background-timer-throttling');
  app.commandLine.appendSwitch('ignore-certificate-errors');


  app.commandLine.appendSwitch('disable-features', 'WebRtcHideLocalIpsWithMdns');

  if (!app.isPackaged) {
    const port = process.env.REMOTE_DEBUGGING_PORT || '9222';
    app.commandLine.appendSwitch('remote-debugging-port', port);
  }
}

const isDev = !app.isPackaged;

let mainWindow: BrowserWindow | null = null;
let tray: Tray | null = null;
let isQuitting = false;




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
      click: () => {
        if (mainWindow && !mainWindow.isDestroyed()) {
          try {
            mainWindow.webContents.send('before-quit');
          } catch {}
          setTimeout(() => {
            isQuitting = true;
            app.quit();
          }, 800);
        } else {
          isQuitting = true;
          app.quit();
        }
      }
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
    backgroundColor: '#0C0C0E',
    ...stateOptions,
    webPreferences: {
      preload: join(__dirname, '../preload/index.js'),
      contextIsolation: true,
      nodeIntegration: false,
      backgroundThrottling: false
    }
  });

  if (isAutoStart && savedState?.isMaximized) {
    mainWindow.maximize();
  }


  mainWindow.once('ready-to-show', () => {
    mainWindow?.show();
  });


  setTimeout(() => {
    if (mainWindow && !mainWindow.isVisible()) mainWindow.show();
  }, 3000);

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
    shell.openExternal(url);
    return { action: 'deny' };
  });

  if (isDev && process.env['ELECTRON_RENDERER_URL']) {
    mainWindow.loadURL(process.env['ELECTRON_RENDERER_URL']);
  } else {
    mainWindow.loadFile(join(__dirname, '../renderer/index.html'));
  }
}




app.whenReady().then(() => {
  const settings = loadAppSettings();
  applyAutoLaunch(settings.openAtLogin);


  ipcMain.on('window-minimize', () => {
    BrowserWindow.getFocusedWindow()?.minimize();
  });

  ipcMain.on('window-maximize', () => {
    const win = BrowserWindow.getFocusedWindow();
    if (win) {
      win.isMaximized() ? win.unmaximize() : win.maximize();
    }
  });

  ipcMain.on('window-close', () => {
    const settings = loadAppSettings();
    if (settings.minimizeToTray) {
      if (mainWindow && !mainWindow.isDestroyed()) {
        mainWindow.hide();
      }
    } else {
      isQuitting = true;
      app.quit();
    }
  });

  ipcMain.on('app-quit', () => {
    isQuitting = true;
    app.quit();
  });


  ipcMain.handle('wipe-app-data', async () => {
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
      const fullPath = join(userDataPath, dir);
      try {
        if (existsSync(fullPath)) {
          rmSync(fullPath, { recursive: true, force: true });
        }
      } catch {}
    }
    try {
      if (mainWindow && !mainWindow.isDestroyed()) {
        const ses = mainWindow.webContents.session;
        await ses.clearStorageData();
        await ses.clearCache();
      }
    } catch {}
    return true;
  });


  const SESSION_PATH = join(app.getPath('userData'), 'session.json');

  ipcMain.handle('save-session', async (_event, data: string) => {
    try { await fsPromises.writeFile(SESSION_PATH, data, 'utf-8'); return true; } catch { return false; }
  });

  ipcMain.handle('load-session', async () => {
    try {
      if (existsSync(SESSION_PATH)) return await fsPromises.readFile(SESSION_PATH, 'utf-8');
    } catch {}
    return null;
  });

  ipcMain.handle('clear-session', async () => {
    try { if (existsSync(SESSION_PATH)) await fsPromises.rm(SESSION_PATH, { force: true }); } catch {}
    return true;
  });

  ipcMain.handle('get-userdata-path', () => {
    return app.getPath('userData');
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

  createWindow();
  createTray();

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
  if (!isQuitting) {
    if (mainWindow && !mainWindow.isDestroyed()) {
      event.preventDefault();
      try {
        mainWindow.webContents.send('before-quit');
      } catch {}
      setTimeout(() => {
        isQuitting = true;
        app.quit();
      }, 800);
    } else {
      isQuitting = true;
    }
  }
});

app.on('quit', () => {
  if (tray) {
    tray.destroy();
    tray = null;
  }
});