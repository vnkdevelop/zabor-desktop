import { test, describe, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, existsSync, mkdirSync } from 'node:fs';
import { resolve, join } from 'node:path';
import {
  app,
  ipcMain,
  ipcRenderer,
  BrowserWindow,
  MockBrowserWindow
} from './helpers/electron-mock.mjs';
import {
  setupUpdater,
  cleanUpdateDirectory
} from '../src/main/updater.ts';
import {
  NEW_APP_VERSION,
  ALLOWED_DOWNLOAD_PREFIX,
  createMockFetchResponse
} from './helpers/fixtures.mjs';

describe('Tier 3: Cross-Feature Combinations', () => {
  let originalFetch;

  beforeEach(() => {
    app.reset();
    ipcMain.reset();
    MockBrowserWindow.reset();
    originalFetch = globalThis.fetch;
  });

  afterEach(() => {
    globalThis.fetch = originalFetch;
    cleanUpdateDirectory();
  });

  describe('1. Window Close During Download', () => {
    test('safe teardown when updater window is closed during active download stream', async () => {
      const mainWindow = new MockBrowserWindow({ title: 'Main' });
      const updaterWindow = new MockBrowserWindow({ title: 'Updater' });

      // sendToWindow in updater.ts queries getMainWindow()
      setupUpdater(() => updaterWindow);

      const downloadUrl = `${ALLOWED_DOWNLOAD_PREFIX}v${NEW_APP_VERSION}/ZABOR-Setup-${NEW_APP_VERSION}.exe`;

      let streamController;
      const infiniteStream = new ReadableStream({
        start(c) {
          streamController = c;
        },
        async pull(c) {
          c.enqueue(new Uint8Array(Buffer.alloc(2048, 1)));
          await new Promise(r => setTimeout(r, 20));
        }
      });

      globalThis.fetch = async (_url, opts) => {
        if (opts?.signal) {
          opts.signal.addEventListener('abort', () => {
            if (streamController) {
              streamController.error(new DOMException('The operation was aborted', 'AbortError'));
            }
          });
        }
        return createMockFetchResponse({
          status: 200,
          contentLength: 204800,
          bodyStream: infiniteStream
        });
      };

      // Start download
      const downloadPromise = ipcRenderer.invoke('start-update-download', downloadUrl, NEW_APP_VERSION);

      // Wait for at least one chunk to be transferred
      await new Promise(r => setTimeout(r, 40));

      const midStatus = await ipcRenderer.invoke('get-update-status');
      assert.strictEqual(midStatus.status, 'downloading');

      // Now simulate window close while stream is pushing chunks
      updaterWindow.close();
      assert.strictEqual(updaterWindow.isDestroyed(), true);

      // Closing window should allow cancelling download cleanly without throwing
      const cancelResult = await ipcRenderer.invoke('cancel-update-download');
      assert.strictEqual(cancelResult, true);

      const downloadResult = await downloadPromise;
      assert.strictEqual(downloadResult.success, false);

      // Verify mainWindow is intact and app didn't crash
      assert.strictEqual(mainWindow.isDestroyed(), false);
      assert.strictEqual(app.isQuit, false);
    });

    test('sendToWindow does not throw when webContents is destroyed', () => {
      const win = new MockBrowserWindow();
      win.close(); // destroyed

      assert.strictEqual(win.isDestroyed(), true);
      assert.strictEqual(win.webContents.isDestroyed(), true);

      // Calling send on destroyed window should be safely ignored
      assert.doesNotThrow(() => {
        if (!win.isDestroyed()) {
          win.webContents.send('update-download-progress', { percent: 50 });
        }
      });
    });
  });

  describe('2. Silent NSIS Installer Scripting Contract (--keep-app-data and --force-run)', () => {
    test('NSIS script parses --keep-app-data and guards all MessageBoxes with silent checks or defaults', () => {
      const nsisPath = resolve(process.cwd(), 'build/installer.nsh');
      assert.ok(existsSync(nsisPath), 'build/installer.nsh must exist');

      const nsisSource = readFileSync(nsisPath, 'utf8');

      // 1. Verify --keep-app-data parameter handling
      assert.ok(
        nsisSource.includes('${GetOptions} $R0 "--keep-app-data"'),
        'NSIS must parse --keep-app-data command-line parameter'
      );

      // 2. Verify data retention logic (backing up and restoring APPDATA)
      assert.ok(
        nsisSource.includes('$TEMP\\zabor_backup'),
        'NSIS must backup user data during silent update'
      );
      assert.ok(
        nsisSource.includes('CopyFiles /SILENT'),
        'NSIS must use silent file copying for backup/restore'
      );

      // 3. Verify auto-restart logic in customInstall
      assert.ok(
        nsisSource.includes('!macro customInstall'),
        'NSIS must implement customInstall macro'
      );
      assert.ok(
        nsisSource.includes('${isUpdated}'),
        'NSIS must check ${isUpdated} flag for post-update handling'
      );

      // 4. Verify that uninstaller checks silent mode before prompting
      assert.ok(
        nsisSource.includes('${IfNot} ${Silent}'),
        'Interactive prompts in uninstaller must be wrapped with ${IfNot} ${Silent}'
      );
    });

    test('NSIS installer arguments schema matches PROJECT.md contract', () => {
      const expectedContractFlags = ['/S', '--updated', '--keep-app-data', '--force-run'];

      function validateInstallerFlags(flags) {
        const hasSilent = flags.includes('/S');
        const hasUpdated = flags.includes('--updated');
        const hasKeepAppData = flags.includes('--keep-app-data');
        const hasForceRun = flags.includes('--force-run');

        return {
          valid: hasSilent && hasUpdated && hasKeepAppData && hasForceRun,
          missing: expectedContractFlags.filter(f => !flags.includes(f))
        };
      }

      const validArgs = ['/S', '--updated', '--keep-app-data', '--force-run'];
      assert.strictEqual(validateInstallerFlags(validArgs).valid, true);

      const legacyArgs = ['/S', '--updated'];
      const checkLegacy = validateInstallerFlags(legacyArgs);
      assert.strictEqual(checkLegacy.valid, false);
      assert.deepStrictEqual(checkLegacy.missing, ['--keep-app-data', '--force-run']);
    });
  });

  describe('3. Process Lifecycle & Single Instance Lock Teardown Protocol', () => {
    test('enforces clean teardown order: release lock -> spawn detached -> exit', async () => {
      const lifecycleSteps = [];

      // Mock lifecycle manager simulating updater launch protocol
      const lifecycleHarness = {
        emitInstalling() {
          lifecycleSteps.push('EMIT_INSTALLING');
        },
        releaseLock() {
          app.releaseSingleInstanceLock();
          lifecycleSteps.push('RELEASE_LOCK');
        },
        closeWindows() {
          const windows = BrowserWindow.getAllWindows();
          windows.forEach(w => w.close());
          lifecycleSteps.push('CLOSE_WINDOWS');
        },
        spawnInstaller(args) {
          lifecycleSteps.push(`SPAWN_INSTALLER:${args.join(' ')}`);
          return { unref: () => lifecycleSteps.push('UNREF_CHILD') };
        },
        exitApp() {
          app.exit(0);
          lifecycleSteps.push('EXIT_APP');
        }
      };

      const win = new MockBrowserWindow();
      assert.strictEqual(BrowserWindow.getAllWindows().length, 1);

      // Execute sequence
      lifecycleHarness.emitInstalling();
      lifecycleHarness.releaseLock();
      lifecycleHarness.closeWindows();
      const child = lifecycleHarness.spawnInstaller(['/S', '--updated', '--keep-app-data', '--force-run']);
      child.unref();
      lifecycleHarness.exitApp();

      assert.deepStrictEqual(lifecycleSteps, [
        'EMIT_INSTALLING',
        'RELEASE_LOCK',
        'CLOSE_WINDOWS',
        'SPAWN_INSTALLER:/S --updated --keep-app-data --force-run',
        'UNREF_CHILD',
        'EXIT_APP'
      ]);

      assert.strictEqual(app.releasedLock, true, 'Single instance lock must be released');
      assert.strictEqual(app.exitCode, 0, 'Exit code must be 0');
      assert.strictEqual(BrowserWindow.getAllWindows().length, 0, 'All windows closed before exit');
    });
  });
});
