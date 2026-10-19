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
  isNewerVersion,
  checkGitHubRelease,
  setupUpdater,
  cleanUpdateDirectory
} from '../src/main/updater.ts';
import {
  CURRENT_APP_VERSION,
  NEW_APP_VERSION,
  ALLOWED_DOWNLOAD_PREFIX,
  createMockReleasePayload,
  createMockFetchResponse,
  createMockChunkStream,
  calculateSha256
} from './helpers/fixtures.mjs';

describe('Tier 2: Boundary & Corner Cases', () => {
  let originalFetch;
  let testTempDir;

  beforeEach(() => {
    app.reset();
    ipcMain.reset();
    MockBrowserWindow.reset();
    originalFetch = globalThis.fetch;
    testTempDir = app.getPath('temp');
    if (!existsSync(testTempDir)) {
      mkdirSync(testTempDir, { recursive: true });
    }
  });

  afterEach(() => {
    globalThis.fetch = originalFetch;
    cleanUpdateDirectory();
  });

  describe('1. Network Drop During Download', () => {
    test('transitions to error state and cleans partial temp file on network socket drop', async () => {
      const mockWin = new MockBrowserWindow();
      setupUpdater(() => mockWin);

      const downloadUrl = `${ALLOWED_DOWNLOAD_PREFIX}v${NEW_APP_VERSION}/ZABOR-Setup-${NEW_APP_VERSION}.exe`;
      const streamHelper = createMockChunkStream({
        totalBytes: 500000,
        chunkCount: 4,
        failAtChunk: 2,
        failError: new Error('ECONNRESET: Connection dropped by peer')
      });

      globalThis.fetch = async () =>
        createMockFetchResponse({
          status: 200,
          contentLength: 500000,
          bodyStream: streamHelper.stream
        });

      const result = await ipcRenderer.invoke('start-update-download', downloadUrl, NEW_APP_VERSION);

      assert.strictEqual(result.success, false);
      assert.ok(result.error.includes('ECONNRESET'));

      const status = await ipcRenderer.invoke('get-update-status');
      assert.strictEqual(status.status, 'error');
      assert.ok(status.error.includes('ECONNRESET'));
      assert.strictEqual(status.progress, null);

      // Verify update-error event was emitted to window
      const errorEvent = mockWin.webContents.sentEvents.find(e => e.channel === 'update-error');
      assert.ok(errorEvent, 'Window should receive update-error notification');
      assert.ok(errorEvent.args[0].includes('ECONNRESET'));

      // Verify temp dir was cleaned and no locked partial installer remains
      const updateTemp = join(app.getPath('temp'), 'zabor-update');
      const installerFile = join(updateTemp, `ZABOR-Setup-${NEW_APP_VERSION}.exe`);
      assert.strictEqual(existsSync(installerFile), false, 'Partial file must be deleted on error');
    });
  });

  describe('2. Cancellation Mid-Download', () => {
    test('cancels active download, clears controller, and resets status', async () => {
      const mockWin = new MockBrowserWindow();
      setupUpdater(() => mockWin);

      // Verify initial cancel returns false when not downloading
      const initialCancel = await ipcRenderer.invoke('cancel-update-download');
      assert.strictEqual(initialCancel, false);

      const downloadUrl = `${ALLOWED_DOWNLOAD_PREFIX}v${NEW_APP_VERSION}/ZABOR-Setup-${NEW_APP_VERSION}.exe`;

      globalThis.fetch = async (_url, opts) => {
        let streamController;
        const slowStream = new ReadableStream({
          start(c) {
            streamController = c;
          },
          async pull(c) {
            c.enqueue(new Uint8Array(Buffer.alloc(1024, 1)));
            await new Promise(r => setTimeout(r, 50));
          }
        });

        if (opts?.signal) {
          opts.signal.addEventListener('abort', () => {
            if (streamController) {
              streamController.error(new DOMException('The operation was aborted', 'AbortError'));
            }
          });
        }

        return createMockFetchResponse({
          status: 200,
          contentLength: 1048576,
          bodyStream: slowStream
        });
      };

      // Start download asynchronously
      const downloadPromise = ipcRenderer.invoke('start-update-download', downloadUrl, NEW_APP_VERSION);

      // Small tick to ensure download starts
      await new Promise(r => setTimeout(r, 30));

      const midStatus = await ipcRenderer.invoke('get-update-status');
      assert.strictEqual(midStatus.status, 'downloading');

      // Now cancel
      const cancelResult = await ipcRenderer.invoke('cancel-update-download');
      assert.strictEqual(cancelResult, true);

      // Wait for start-update-download promise to settle
      const downloadResult = await downloadPromise;
      assert.strictEqual(downloadResult.success, false);

      const postStatus = await ipcRenderer.invoke('get-update-status');
      assert.ok(postStatus.status === 'idle' || postStatus.status === 'available');
      assert.strictEqual(postStatus.progress, null);
    });
  });

  describe('3. Corrupted SHA-256 Digest Verification', () => {
    test('rejects installer when sha256 mismatches and cleans corrupted file', async () => {
      const mockWin = new MockBrowserWindow();
      setupUpdater(() => mockWin);

      const downloadUrl = `${ALLOWED_DOWNLOAD_PREFIX}v${NEW_APP_VERSION}/ZABOR-Setup-${NEW_APP_VERSION}.exe`;
      const fileContent = Buffer.from('Fake executable binary content for test');
      const actualHash = calculateSha256(fileContent);
      const expectedCorruptedHash = '0000000000000000000000000000000000000000000000000000000000000000';

      // First simulate check-for-updates discovering release with expected corrupted sha256
      globalThis.fetch = async (url) => {
        if (url.includes('api.github.com')) {
          return createMockFetchResponse({
            status: 200,
            jsonBody: createMockReleasePayload({
              tagName: `v${NEW_APP_VERSION}`,
              sha256: expectedCorruptedHash
            })
          });
        }
        // File download returns actual content whose hash will NOT match
        return createMockFetchResponse({
          status: 200,
          contentLength: fileContent.length,
          bodyStream: new ReadableStream({
            start(controller) {
              controller.enqueue(new Uint8Array(fileContent));
              controller.close();
            }
          })
        });
      };

      const check = await ipcRenderer.invoke('check-for-updates');
      assert.strictEqual(check.updateAvailable, true);

      // Download file
      const downloadResult = await ipcRenderer.invoke('start-update-download', downloadUrl, NEW_APP_VERSION);
      assert.strictEqual(downloadResult.success, true);

      // Now attempt installation -> must verify sha256 and fail
      const installResult = await ipcRenderer.invoke('install-update');
      assert.strictEqual(installResult.success, false);
      assert.ok(
        installResult.message.includes('sha256-mismatch'),
        `Expected sha256-mismatch but got: ${installResult.message}`
      );

      // Verify status is error
      const status = await ipcRenderer.invoke('get-update-status');
      assert.strictEqual(status.status, 'error');
      assert.ok(status.error.includes('sha256-mismatch'));

      // Verify update-error was broadcasted to window
      const errorEvt = mockWin.webContents.sentEvents.find(e => e.channel === 'update-error');
      assert.ok(errorEvt);
      assert.ok(errorEvt.args[0].includes('sha256-mismatch'));
    });
  });

  describe('4. Invalid Release Payload Handling', () => {
    test('handles HTTP 404 release gracefully', async () => {
      globalThis.fetch = async () =>
        createMockFetchResponse({ status: 404, statusText: 'Not Found' });

      const result = await checkGitHubRelease();
      assert.strictEqual(result.updateAvailable, false);
      assert.ok(result.error.includes('404'));
      assert.strictEqual(result.currentVersion, app.getVersion());
    });

    test('handles HTTP 500 server error gracefully', async () => {
      globalThis.fetch = async () =>
        createMockFetchResponse({ status: 500, statusText: 'Internal Server Error' });

      const result = await checkGitHubRelease();
      assert.strictEqual(result.updateAvailable, false);
      assert.ok(result.error.includes('500'));
    });

    test('handles release payload with empty or missing assets array', async () => {
      globalThis.fetch = async () =>
        createMockFetchResponse({
          status: 200,
          jsonBody: {
            tag_name: `v${NEW_APP_VERSION}`,
            name: 'No assets release',
            body: 'Notes',
            assets: []
          }
        });

      const result = await checkGitHubRelease();
      assert.strictEqual(result.updateAvailable, true);
      assert.strictEqual(result.updateInfo.version, NEW_APP_VERSION);
      assert.strictEqual(result.updateInfo.fileSize, 0);
      assert.strictEqual(result.updateInfo.sha256, '');
    });

    test('handles non-array assets field in release payload', async () => {
      globalThis.fetch = async () =>
        createMockFetchResponse({
          status: 200,
          jsonBody: {
            tag_name: `v${NEW_APP_VERSION}`,
            assets: null
          }
        });

      const result = await checkGitHubRelease();
      assert.strictEqual(result.updateAvailable, true);
      assert.strictEqual(result.updateInfo.version, NEW_APP_VERSION);
    });
  });

  describe('5. Empty Versions and URL Validation', () => {
    test('isNewerVersion handles empty and malformed strings', () => {
      assert.strictEqual(isNewerVersion('', '3.6.4'), false);
      assert.strictEqual(isNewerVersion('   ', '3.6.4'), false);
      assert.strictEqual(isNewerVersion('invalid-text', '3.6.4'), false);
      assert.strictEqual(isNewerVersion('3.6.4', ''), true);
      assert.strictEqual(isNewerVersion('', ''), false);
    });

    test('start-update-download rejects unauthorized download domains', async () => {
      const mockWin = new MockBrowserWindow();
      setupUpdater(() => mockWin);

      const maliciousUrls = [
        'http://evil.com/setup.exe',
        'https://attacker.org/ZABOR-Setup-3.6.5.exe',
        'ftp://github.com/releases/download/v3.6.5.exe',
        'javascript:alert(1)',
        'file:///c:/windows/system32/calc.exe'
      ];

      for (const url of maliciousUrls) {
        const res = await ipcRenderer.invoke('start-update-download', url, '3.6.5');
        assert.strictEqual(res.success, false);
        assert.strictEqual(res.message, 'Invalid download url');
      }
    });

    test('start-update-download rejects directory traversal in version identifier', async () => {
      const mockWin = new MockBrowserWindow();
      setupUpdater(() => mockWin);

      const safeUrl = `${ALLOWED_DOWNLOAD_PREFIX}v3.6.5/setup.exe`;
      const dangerousVersions = [
        '../../evil',
        '..\\..\\windows\\system32',
        '../../../payload',
        '///etc/passwd'
      ];

      for (const ver of dangerousVersions) {
        // Safe version regex in updater: String(version).replace(/[^a-zA-Z0-9.-]/g, '')
        // If it reduces to empty or is sanitized, verify it doesn't escape
        const res = await ipcRenderer.invoke('start-update-download', safeUrl, ver);
        if (res.success) {
          // If allowed, ensure filename does not contain path separators
          assert.ok(!res.filePath.includes('..'));
        } else {
          assert.strictEqual(res.success, false);
        }
      }
    });
  });

  describe('6. Same Version vs Newer Version Checking', () => {
    test('returns updateAvailable: false when remote tag matches installed version', async () => {
      globalThis.fetch = async () =>
        createMockFetchResponse({
          status: 200,
          jsonBody: createMockReleasePayload({
            tagName: `v${CURRENT_APP_VERSION}`
          })
        });

      const result = await checkGitHubRelease();
      assert.strictEqual(result.updateAvailable, false);
      assert.strictEqual(result.currentVersion, CURRENT_APP_VERSION);
      assert.strictEqual(result.updateInfo, undefined);
    });

    test('returns updateAvailable: false when remote tag is older than installed version', async () => {
      globalThis.fetch = async () =>
        createMockFetchResponse({
          status: 200,
          jsonBody: createMockReleasePayload({
            tagName: 'v3.5.0'
          })
        });

      const result = await checkGitHubRelease();
      assert.strictEqual(result.updateAvailable, false);
    });

    test('returns updateAvailable: true only when remote tag is strictly newer', async () => {
      globalThis.fetch = async () =>
        createMockFetchResponse({
          status: 200,
          jsonBody: createMockReleasePayload({
            tagName: `v${NEW_APP_VERSION}`
          })
        });

      const result = await checkGitHubRelease();
      assert.strictEqual(result.updateAvailable, true);
      assert.strictEqual(result.updateInfo.version, NEW_APP_VERSION);
    });
  });
});
