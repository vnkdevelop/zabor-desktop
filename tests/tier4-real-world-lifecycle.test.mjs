import { test, describe, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, writeFileSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import {
  app,
  ipcMain,
  ipcRenderer,
  MockBrowserWindow
} from './helpers/electron-mock.mjs';
import {
  spawnedProcesses,
  resetSpawnedProcesses
} from './helpers/child-process-mock.mjs';
import {
  setupUpdater,
  checkGitHubRelease,
  cleanUpdateDirectory,
  isNewerVersion
} from '../src/main/updater.ts';
import {
  CURRENT_APP_VERSION,
  NEW_APP_VERSION,
  ALLOWED_DOWNLOAD_PREFIX,
  createMockReleasePayload,
  createMockFetchResponse,
  calculateSha256
} from './helpers/fixtures.mjs';

describe('Tier 4: Real-World Scenarios', () => {
  let originalFetch;

  beforeEach(() => {
    app.reset();
    ipcMain.reset();
    MockBrowserWindow.reset();
    resetSpawnedProcesses();
    originalFetch = globalThis.fetch;
    const testTempDir = app.getPath('temp');
    if (!existsSync(testTempDir)) {
      mkdirSync(testTempDir, { recursive: true });
    }
  });

  afterEach(() => {
    globalThis.fetch = originalFetch;
    cleanUpdateDirectory();
  });

  describe('1. Full Update Lifecycle Simulation (Check -> Download -> Verify -> Install -> Relaunch)', () => {
    test('executes complete end-to-end update lifecycle cleanly', async () => {
      // Phase 0: Setup window and updater
      const updaterWin = new MockBrowserWindow({ title: 'Updater' });
      setupUpdater(() => updaterWin);

      // Verify initial state
      app.setVersion(CURRENT_APP_VERSION);
      const initialStatus = await ipcRenderer.invoke('get-update-status');
      assert.strictEqual(initialStatus.status, 'idle');
      assert.strictEqual(initialStatus.updateInfo, null);
      assert.strictEqual(initialStatus.progress, null);

      // Prepare simulated installer file content & valid SHA-256
      const installerBytes = Buffer.from('MZ\x90\x00' + 'A'.repeat(64 * 1024)); // simulated PE executable header
      const expectedSha256 = calculateSha256(installerBytes);
      const downloadUrl = `${ALLOWED_DOWNLOAD_PREFIX}v${NEW_APP_VERSION}/ZABOR-Setup-${NEW_APP_VERSION}.exe`;

      // Phase 1: Mock GitHub API & file download responses
      globalThis.fetch = async (url) => {
        if (url.includes('api.github.com')) {
          return createMockFetchResponse({
            status: 200,
            jsonBody: createMockReleasePayload({
              tagName: `v${NEW_APP_VERSION}`,
              fileSize: installerBytes.length,
              sha256: expectedSha256
            })
          });
        }

        // Chunked file stream
        const chunkSize = 16 * 1024;
        let offset = 0;
        const stream = new ReadableStream({
          pull(controller) {
            if (offset < installerBytes.length) {
              const end = Math.min(offset + chunkSize, installerBytes.length);
              const chunk = installerBytes.subarray(offset, end);
              offset = end;
              controller.enqueue(new Uint8Array(chunk));
            } else {
              controller.close();
            }
          }
        });

        return createMockFetchResponse({
          status: 200,
          contentLength: installerBytes.length,
          bodyStream: stream
        });
      };

      // Phase 2: Check for updates
      const checkResult = await ipcRenderer.invoke('check-for-updates');
      assert.strictEqual(checkResult.updateAvailable, true);
      assert.strictEqual(checkResult.updateInfo.version, NEW_APP_VERSION);
      assert.strictEqual(checkResult.updateInfo.sha256, expectedSha256);

      const statusAfterCheck = await ipcRenderer.invoke('get-update-status');
      assert.strictEqual(statusAfterCheck.status, 'available');
      assert.ok(statusAfterCheck.updateInfo);

      // Phase 3: Start download with progress
      const downloadResult = await ipcRenderer.invoke('start-update-download', downloadUrl, NEW_APP_VERSION);
      assert.strictEqual(downloadResult.success, true);
      assert.ok(existsSync(downloadResult.filePath));

      // Verify progress events were broadcast to window
      const progressEvents = updaterWin.webContents.sentEvents.filter(
        e => e.channel === 'update-download-progress'
      );
      assert.ok(progressEvents.length >= 4, 'Should have multiple incremental progress events');
      const finalProgress = progressEvents[progressEvents.length - 1].args[0];
      assert.strictEqual(finalProgress.percent, 100);
      assert.strictEqual(finalProgress.transferred, installerBytes.length);

      // Verify downloaded event
      const downloadedEvt = updaterWin.webContents.sentEvents.find(e => e.channel === 'update-downloaded');
      assert.ok(downloadedEvt, 'update-downloaded event should be emitted');

      // Verify status is now 'downloaded'
      const statusAfterDownload = await ipcRenderer.invoke('get-update-status');
      assert.strictEqual(statusAfterDownload.status, 'downloaded');

      // Phase 4: Install update (verify SHA256 -> spawn -> emit update-installing)
      const installResult = await ipcRenderer.invoke('install-update');
      assert.strictEqual(installResult.success, true);

      // Verify installer process was spawned detached and unrefed
      const installerProc = spawnedProcesses.find(p =>
        (p.args && p.args.some(a => String(a).includes('/S'))) || (p.command && p.command.includes('/S'))
      );
      assert.ok(installerProc, 'Installer process should be spawned');
      assert.ok(installerProc.unrefCalled, 'Child installer process must be unrefed to detach cleanly');
      assert.ok(
        installerProc.args.some(a => String(a).includes('/S')) || (installerProc.command && installerProc.command.includes('/S')),
        'Installer must be launched in silent mode (/S)'
      );

      // Verify splash process was spawned on win32
      if (process.platform === 'win32') {
        const splashProc = spawnedProcesses.find(p =>
          (p.command && (p.command.includes('ZaborUpdater') || p.command.includes('zabor-splash'))) ||
          (p.args && (p.args.includes('-ParentPid') || p.args.some(a => String(a).includes('ZaborUpdater') || String(a).includes('zabor-splash'))))
        );
        assert.ok(splashProc, 'Installation splash window process should be spawned');
        assert.ok(splashProc.unrefCalled, 'Splash process must be unrefed');
      }

      // Verify installing event emitted
      const installingEvt = updaterWin.webContents.sentEvents.find(e => e.channel === 'update-installing');
      assert.ok(installingEvt, 'update-installing event should be emitted');

      const statusInstalling = await ipcRenderer.invoke('get-update-status');
      assert.strictEqual(statusInstalling.status, 'installing');

      // Phase 5: Relaunch simulation (app version is now updated to 3.6.5)
      app.setVersion(NEW_APP_VERSION);
      assert.strictEqual(app.getVersion(), NEW_APP_VERSION);

      // Query GitHub again with the same release
      const postUpdateCheck = await checkGitHubRelease();
      assert.strictEqual(postUpdateCheck.updateAvailable, false, 'Should report no updates after installation');
      assert.strictEqual(postUpdateCheck.currentVersion, NEW_APP_VERSION);
      assert.strictEqual(postUpdateCheck.updateInfo, undefined);
    });
  });

  describe('2. Cyclic Update Loop Elimination & State Recovery', () => {
    test('subsequent checks on latest version remain up-to-date and do not loop', async () => {
      app.setVersion(NEW_APP_VERSION);

      globalThis.fetch = async () =>
        createMockFetchResponse({
          status: 200,
          jsonBody: createMockReleasePayload({
            tagName: `v${NEW_APP_VERSION}`
          })
        });

      // Perform multiple back-to-back checks
      for (let i = 0; i < 3; i++) {
        const check = await checkGitHubRelease();
        assert.strictEqual(check.updateAvailable, false);
      }
    });

    test('recovers from download failure on retry without getting stuck', async () => {
      const mockWin = new MockBrowserWindow();
      setupUpdater(() => mockWin);

      const downloadUrl = `${ALLOWED_DOWNLOAD_PREFIX}v${NEW_APP_VERSION}/ZABOR-Setup-${NEW_APP_VERSION}.exe`;

      // First attempt: Server returns 503 Service Unavailable
      globalThis.fetch = async () =>
        createMockFetchResponse({ status: 503, statusText: 'Service Unavailable' });

      const attempt1 = await ipcRenderer.invoke('start-update-download', downloadUrl, NEW_APP_VERSION);
      assert.strictEqual(attempt1.success, false);

      const statusErr = await ipcRenderer.invoke('get-update-status');
      assert.strictEqual(statusErr.status, 'error');

      // Second attempt: Server recovers and provides file
      const validPayload = Buffer.from('Valid-Recovery-Payload');
      globalThis.fetch = async () =>
        createMockFetchResponse({
          status: 200,
          contentLength: validPayload.length,
          bodyStream: new ReadableStream({
            start(c) {
              c.enqueue(new Uint8Array(validPayload));
              c.close();
            }
          })
        });

      const attempt2 = await ipcRenderer.invoke('start-update-download', downloadUrl, NEW_APP_VERSION);
      assert.strictEqual(attempt2.success, true);

      const statusRecovered = await ipcRenderer.invoke('get-update-status');
      assert.strictEqual(statusRecovered.status, 'downloaded');
      assert.strictEqual(statusRecovered.error, null);
    });
  });
});
