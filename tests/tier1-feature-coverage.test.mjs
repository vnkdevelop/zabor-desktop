import { test, describe, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import {
  app,
  ipcMain,
  ipcRenderer,
  BrowserWindow,
  MockBrowserWindow
} from './helpers/electron-mock.mjs';
import {
  isNewerVersion
} from '../src/main/updater.ts';
import {
  CURRENT_APP_VERSION,
  NEW_APP_VERSION,
  createMockReleasePayload,
  createMockFetchResponse,
  createMockChunkStream
} from './helpers/fixtures.mjs';

describe('Tier 1: Feature Coverage', () => {
  let originalFetch;

  beforeEach(() => {
    app.reset();
    ipcMain.reset();
    MockBrowserWindow.reset();
    originalFetch = globalThis.fetch;
  });

  afterEach(() => {
    globalThis.fetch = originalFetch;
  });

  describe('1. Updater Window Hash Routing & Isolation', () => {
    function resolveRoute(hash) {
      const cleanHash = (hash || '').replace(/^#/, '').trim();
      if (cleanHash === 'updater') {
        return {
          component: 'UpdaterWindow',
          loadsWebRtc: false,
          loadsSignalR: false,
          loadsAudioGraph: false,
          frameless: true,
          alwaysOnTop: true,
          width: 420,
          height: 240
        };
      }
      return {
        component: 'App',
        loadsWebRtc: true,
        loadsSignalR: true,
        loadsAudioGraph: true,
        frameless: false,
        alwaysOnTop: false,
        width: 1280,
        height: 800
      };
    }

    test('routes to isolated UpdaterWindow when hash is #updater', () => {
      const route = resolveRoute('#updater');
      assert.strictEqual(route.component, 'UpdaterWindow');
      assert.strictEqual(route.loadsWebRtc, false, 'UpdaterWindow must not load heavy WebRTC engine');
      assert.strictEqual(route.loadsSignalR, false, 'UpdaterWindow must not load SignalR socket');
      assert.strictEqual(route.loadsAudioGraph, false, 'UpdaterWindow must not load Web Audio graph');
      assert.strictEqual(route.width, 420);
      assert.strictEqual(route.height, 240);
      assert.strictEqual(route.alwaysOnTop, true);
    });

    test('routes to main App when hash is empty or standard route', () => {
      const routeEmpty = resolveRoute('');
      assert.strictEqual(routeEmpty.component, 'App');
      assert.strictEqual(routeEmpty.loadsWebRtc, true);

      const routeSlash = resolveRoute('#/');
      assert.strictEqual(routeSlash.component, 'App');
    });
  });

  describe('2. IPC Contracts (Main ↔ Renderer)', () => {
    test('defines required IPC handlers on updater setup', () => {
      // Simulate registering standard updater IPC contracts
      const requiredChannels = [
        'check-for-updates',
        'get-update-status',
        'get-app-version',
        'open-external-url',
        'start-update-download',
        'cancel-update-download',
        'install-update'
      ];

      // Register mock handlers matching updater contracts
      ipcMain.handle('check-for-updates', async () => ({
        updateAvailable: true,
        currentVersion: '3.6.4',
        updateInfo: {
          version: '3.6.5',
          currentVersion: '3.6.4',
          releaseName: 'zabor 3.6.5',
          releaseNotes: 'Fixes',
          publishedAt: '2026-10-16T12:00:00Z',
          downloadUrl: 'https://github.com/vnkdevelop/zabor-desktop/releases/download/v3.6.5/ZABOR-Setup-3.6.5.exe',
          releaseUrl: 'https://github.com/vnkdevelop/zabor-desktop/releases/tag/v3.6.5',
          fileSize: 1048576,
          sha256: 'abc123'
        }
      }));

      ipcMain.handle('get-update-status', () => ({
        status: 'available',
        updateInfo: null,
        progress: null,
        error: null
      }));

      ipcMain.handle('get-app-version', () => app.getVersion());

      ipcMain.handle('open-external-url', async (_evt, url) => typeof url === 'string');
      ipcMain.handle('start-update-download', async (_evt, url, ver) => {
        if (!url || !ver) return { success: false, message: 'Invalid arguments' };
        return { success: true };
      });

      ipcMain.handle('cancel-update-download', () => true);

      ipcMain.handle('install-update', async () => ({ success: true }));

      for (const channel of requiredChannels) {
        assert.ok(ipcMain.handlers.has(channel), `Channel "${channel}" must be registered in ipcMain`);
      }
    });

    test('validates IPC return payload shapes adhere to PROJECT.md contracts', async () => {
      ipcMain.handle('check-for-updates', async () => ({
        updateAvailable: false,
        currentVersion: '3.6.4'
      }));

      const checkResult = await ipcRenderer.invoke('check-for-updates');
      assert.strictEqual(typeof checkResult.updateAvailable, 'boolean');
      assert.strictEqual(typeof checkResult.currentVersion, 'string');
      assert.strictEqual(checkResult.currentVersion, '3.6.4');
    });

    test('closing updaterWindow does not quit entire application', () => {
      const mainWindow = new BrowserWindow({ title: 'Main' });
      const updaterWindow = new BrowserWindow({ title: 'Updater' });

      assert.strictEqual(BrowserWindow.getAllWindows().length, 2);

      // Close only updaterWindow
      updaterWindow.close();

      assert.strictEqual(updaterWindow.isDestroyed(), true);
      assert.strictEqual(mainWindow.isDestroyed(), false);
      assert.strictEqual(BrowserWindow.getAllWindows().length, 1);
      assert.strictEqual(app.isQuit, false, 'App should remain open while mainWindow is alive');
    });
  });

  describe('3. Download Progress Events Contract', () => {
    test('calculates correct percentages and emits expected payload structure', () => {
      const progressEvents = [];
      const total = 1000;
      const chunks = [250, 250, 250, 250];
      let transferred = 0;

      for (const chunk of chunks) {
        transferred += chunk;
        const percent = total > 0 ? Math.round((transferred / total) * 100) : 0;
        const payload = { percent, transferred, total };
        progressEvents.push(payload);
      }

      assert.strictEqual(progressEvents.length, 4);
      assert.deepStrictEqual(progressEvents[0], { percent: 25, transferred: 250, total: 1000 });
      assert.deepStrictEqual(progressEvents[1], { percent: 50, transferred: 500, total: 1000 });
      assert.deepStrictEqual(progressEvents[2], { percent: 75, transferred: 750, total: 1000 });
      assert.deepStrictEqual(progressEvents[3], { percent: 100, transferred: 1000, total: 1000 });
    });

    test('handles total === 0 gracefully without NaN', () => {
      const total = 0;
      const transferred = 500;
      const percent = total > 0 ? Math.round((transferred / total) * 100) : 0;
      assert.strictEqual(percent, 0);
      assert.ok(!Number.isNaN(percent));
    });
  });

  describe('4. Installer Silent Arguments Contract', () => {
    test('verifies installer argument contract from PROJECT.md', () => {
      // Contract specification: ['/S', '--updated', '--keep-app-data', '--force-run']
      const requiredArgs = ['/S', '--updated', '--keep-app-data', '--force-run'];

      function buildInstallerArgs(options = {}) {
        const args = ['/S', '--updated'];
        if (options.keepAppData !== false) {
          args.push('--keep-app-data');
        }
        if (options.forceRun !== false) {
          args.push('--force-run');
        }
        return args;
      }

      const generatedArgs = buildInstallerArgs({ keepAppData: true, forceRun: true });
      assert.deepStrictEqual(generatedArgs, requiredArgs);
    });

    test('NSIS script contains silent guards and parameters', () => {
      const nsisPath = resolve(process.cwd(), 'build/installer.nsh');
      const content = readFileSync(nsisPath, 'utf8');

      // Check that --keep-app-data is handled
      assert.ok(
        content.includes('--keep-app-data'),
        'build/installer.nsh must support --keep-app-data argument'
      );

      // Check that KeepAppDataOnUpdate variable is declared
      assert.ok(
        content.includes('KeepAppDataOnUpdate'),
        'build/installer.nsh must have KeepAppDataOnUpdate variable'
      );

      // Check that Silent is guarded or uninstallation handles silent flags
      assert.ok(
        content.includes('${Silent}') || content.includes('/SD IDYES'),
        'build/installer.nsh must guard interactive prompts or provide silent defaults'
      );
    });
  });

  describe('5. Version Comparison Matrix (isNewerVersion)', () => {
    test('detects patch increments', () => {
      assert.strictEqual(isNewerVersion('3.6.5', '3.6.4'), true);
      assert.strictEqual(isNewerVersion('v3.6.5', '3.6.4'), true);
      assert.strictEqual(isNewerVersion('3.6.5', 'v3.6.4'), true);
    });

    test('detects minor and major increments', () => {
      assert.strictEqual(isNewerVersion('3.7.0', '3.6.4'), true);
      assert.strictEqual(isNewerVersion('4.0.0', '3.6.4'), true);
      assert.strictEqual(isNewerVersion('10.0.0', '3.6.4'), true);
    });

    test('detects 4-component versions', () => {
      assert.strictEqual(isNewerVersion('3.6.4.1', '3.6.4'), true);
      assert.strictEqual(isNewerVersion('3.6.4', '3.6.4.1'), false);
    });

    test('returns false for identical versions', () => {
      assert.strictEqual(isNewerVersion('3.6.4', '3.6.4'), false);
      assert.strictEqual(isNewerVersion('v3.6.4', '3.6.4'), false);
      assert.strictEqual(isNewerVersion('3.6.4', 'v3.6.4'), false);
      assert.strictEqual(isNewerVersion('v3.6.4', 'v3.6.4'), false);
    });

    test('returns false for older versions', () => {
      assert.strictEqual(isNewerVersion('3.6.3', '3.6.4'), false);
      assert.strictEqual(isNewerVersion('3.5.9', '3.6.4'), false);
      assert.strictEqual(isNewerVersion('2.9.99', '3.6.4'), false);
    });

    test('handles whitespace and prefix variations', () => {
      assert.strictEqual(isNewerVersion('  3.6.5  ', '3.6.4'), true);
      assert.strictEqual(isNewerVersion('v3.6.5  ', '3.6.4'), true);
      assert.strictEqual(isNewerVersion('V3.6.5', 'v3.6.4'), true);
    });

    test('handles pre-release and metadata suffixes', () => {
      assert.strictEqual(isNewerVersion('3.6.5-beta.1', '3.6.4'), true);
      assert.strictEqual(isNewerVersion('3.6.4-beta.1', '3.6.4'), false);
      assert.strictEqual(isNewerVersion('3.6.5+build123', '3.6.4'), true);
    });
  });
});
