import { app, BrowserWindow, shell, ipcMain } from 'electron';
import { join } from 'path';
import { createWriteStream, mkdirSync, existsSync, rmSync, createReadStream } from 'fs';
import { spawn, execFile } from 'child_process';
import { createHash } from 'crypto';

declare const __ZABOR_UPDATE_SIGNER__: string;
const UPDATE_SIGNER = typeof __ZABOR_UPDATE_SIGNER__ === 'string' ? __ZABOR_UPDATE_SIGNER__ : '';

export interface UpdateInfo {
  version: string;
  currentVersion: string;
  releaseName: string;
  releaseNotes: string;
  publishedAt: string;
  downloadUrl: string;
  releaseUrl: string;
  fileSize: number;
  sha256: string;
}

export interface UpdateProgress {
  percent: number;
  transferred: number;
  total: number;
}

export interface UpdateCheckResult {
  updateAvailable: boolean;
  currentVersion: string;
  updateInfo?: UpdateInfo;
  error?: string;
}

export type UpdateStatus = 'idle' | 'available' | 'downloading' | 'downloaded' | 'installing' | 'error';

export interface UpdateSnapshot {
  status: UpdateStatus;
  updateInfo: UpdateInfo | null;
  progress: UpdateProgress | null;
  error: string | null;
}

const GITHUB_REPO = 'vnkdevelop/zabor-desktop';
const GITHUB_API_URL = `https://api.github.com/repos/${GITHUB_REPO}/releases/latest`;

function getUpdateTempDir(): string {
  return join(app.getPath('temp'), 'zabor-update');
}

export function cleanUpdateDirectory(): void {
  try {
    const tempDir = getUpdateTempDir();
    if (existsSync(tempDir)) {
      rmSync(tempDir, { recursive: true, force: true });
    }
  } catch {}
}

export function isNewerVersion(remote: string, current: string): boolean {
  const cleanRemote = remote.replace(/^v/i, '').trim();
  const cleanCurrent = current.replace(/^v/i, '').trim();

  const rParts = cleanRemote.split(/[-+]/)[0].split('.').map(n => parseInt(n, 10) || 0);
  const cParts = cleanCurrent.split(/[-+]/)[0].split('.').map(n => parseInt(n, 10) || 0);

  const maxLen = Math.max(rParts.length, cParts.length);
  for (let i = 0; i < maxLen; i++) {
    const r = rParts[i] ?? 0;
    const c = cParts[i] ?? 0;
    if (r > c) return true;
    if (r < c) return false;
  }
  return false;
}

let downloadAbortController: AbortController | null = null;

const updateState: {
  status: UpdateStatus;
  updateInfo: UpdateInfo | null;
  progress: UpdateProgress | null;
  error: string | null;
  installerPath: string | null;
  expectedSha256: string;
} = {
  status: 'idle',
  updateInfo: null,
  progress: null,
  error: null,
  installerPath: null,
  expectedSha256: ''
};

function getSnapshot(): UpdateSnapshot {
  return {
    status: updateState.status,
    updateInfo: updateState.updateInfo,
    progress: updateState.progress,
    error: updateState.error
  };
}

export async function checkGitHubRelease(): Promise<UpdateCheckResult> {
  const currentVersion = app.getVersion();
  try {
    const response = await fetch(GITHUB_API_URL, {
      headers: {
        'User-Agent': 'zabor-desktop',
        'Accept': 'application/vnd.github.v3+json'
      }
    });

    if (!response.ok) {
      return {
        updateAvailable: false,
        currentVersion,
        error: `GitHub API error: ${response.status} ${response.statusText}`
      };
    }

    const data = await response.json() as any;
    const tagName = (data.tag_name || '').toString();
    const isNewer = isNewerVersion(tagName, currentVersion);

    if (!isNewer) {
      return {
        updateAvailable: false,
        currentVersion
      };
    }

    const assets = Array.isArray(data.assets) ? data.assets : [];
    const setupAsset = assets.find((a: any) => typeof a.name === 'string' && a.name.toLowerCase().endsWith('.exe'));

    const downloadUrl = setupAsset ? setupAsset.browser_download_url : data.html_url;
    const fileSize = setupAsset ? setupAsset.size : 0;
    const digest = setupAsset && typeof setupAsset.digest === 'string' ? setupAsset.digest : '';
    const sha256 = digest.toLowerCase().startsWith('sha256:') ? digest.slice(7).toLowerCase() : '';

    const updateInfo: UpdateInfo = {
      version: tagName.replace(/^v/i, ''),
      currentVersion,
      releaseName: data.name || `zabor ${tagName}`,
      releaseNotes: data.body || '',
      publishedAt: data.published_at || '',
      downloadUrl,
      releaseUrl: data.html_url || `https://github.com/${GITHUB_REPO}/releases`,
      fileSize,
      sha256
    };

    return {
      updateAvailable: true,
      currentVersion,
      updateInfo
    };
  } catch (err: any) {
    return {
      updateAvailable: false,
      currentVersion,
      error: err?.message || 'Failed to fetch release'
    };
  }
}

function installerNeedsElevation(): boolean {
  if (process.platform !== 'win32') return false;
  try {
    const exePath = app.getPath('exe').toLowerCase();
    const programFiles = (process.env['PROGRAMFILES'] || 'c:\\program files').toLowerCase();
    const programFilesX86 = (process.env['PROGRAMFILES(X86)'] || 'c:\\program files (x86)').toLowerCase();
    return exePath.startsWith(programFiles) || exePath.startsWith(programFilesX86);
  } catch {
    return false;
  }
}

function hashFile(filePath: string): Promise<string> {
  return new Promise((resolve, reject) => {
    const hash = createHash('sha256');
    const stream = createReadStream(filePath);
    stream.on('error', reject);
    stream.on('data', chunk => hash.update(chunk));
    stream.on('end', () => resolve(hash.digest('hex').toLowerCase()));
  });
}

function verifyAuthenticode(filePath: string): Promise<{ valid: boolean; subject: string; status: string }> {
  return new Promise(resolve => {
    if (process.platform !== 'win32') {
      resolve({ valid: false, subject: '', status: 'not-windows' });
      return;
    }
    const psScript =
      `$ErrorActionPreference='Stop';` +
      `$s=Get-AuthenticodeSignature -LiteralPath $args[0];` +
      `$o=[ordered]@{status=$s.Status.ToString();subject=''};` +
      `if($s.SignerCertificate){$o.subject=$s.SignerCertificate.Subject};` +
      `ConvertTo-Json -Compress -InputObject $o`;
    execFile(
      'powershell.exe',
      ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-Command', psScript, filePath],
      { timeout: 30000, windowsHide: true },
      (err, stdout) => {
        if (err) {
          resolve({ valid: false, subject: '', status: 'verify-failed' });
          return;
        }
        try {
          const parsed = JSON.parse(String(stdout).trim());
          const status = String(parsed.status || '');
          const subject = String(parsed.subject || '');
          resolve({ valid: status === 'Valid', subject, status });
        } catch {
          resolve({ valid: false, subject: '', status: 'parse-failed' });
        }
      }
    );
  });
}

async function verifyInstaller(filePath: string, expectedSha256: string): Promise<{ ok: boolean; reason: string }> {
  if (expectedSha256) {
    let actual: string;
    try {
      actual = await hashFile(filePath);
    } catch {
      return { ok: false, reason: 'hash-error' };
    }
    if (actual !== expectedSha256) {
      return { ok: false, reason: 'sha256-mismatch' };
    }
  }

  if (process.platform === 'win32') {
    const sig = await verifyAuthenticode(filePath);
    if (UPDATE_SIGNER) {
      if (!sig.valid) return { ok: false, reason: `signature-${sig.status}` };
      const subjectMatch = sig.subject.toLowerCase().includes(UPDATE_SIGNER.toLowerCase());
      if (!subjectMatch) return { ok: false, reason: 'signer-mismatch' };
    } else if (sig.valid) {
      console.warn('[updater] installer carries a valid Authenticode signature but no expected signer is pinned; set ZABOR_UPDATE_SIGNER at build time to enforce it');
    } else {
      console.warn('[updater] installer is unsigned and no signer is pinned; only SHA-256 (if published) was checked');
    }
  }

  return { ok: true, reason: expectedSha256 ? 'sha256+signature' : 'signature-only' };
}

export function setupUpdater(getMainWindow: () => BrowserWindow | null): void {
  cleanUpdateDirectory();

  const sendToWindow = (channel: string, payload?: unknown): void => {
    const window = getMainWindow();
    if (window && !window.isDestroyed()) {
      window.webContents.send(channel, payload);
    }
  };

  ipcMain.handle('check-for-updates', async () => {
    const result = await checkGitHubRelease();
    if (
      result.updateAvailable &&
      result.updateInfo &&
      (updateState.status === 'idle' || updateState.status === 'available' || updateState.status === 'error')
    ) {
      updateState.status = 'available';
      updateState.updateInfo = result.updateInfo;
      updateState.progress = null;
      updateState.error = null;
    }
    return result;
  });

  ipcMain.handle('get-update-status', (): UpdateSnapshot => {
    return getSnapshot();
  });

  ipcMain.handle('get-app-version', () => {
    return app.getVersion();
  });

  ipcMain.handle('open-external-url', async (_event, url: string) => {
    if (typeof url === 'string') {
      try {
        const parsed = new URL(url);
        if (parsed.protocol === 'https:' || parsed.protocol === 'http:') {
          await shell.openExternal(url);
          return true;
        }
      } catch {}
    }
    return false;
  });

  ipcMain.handle('start-update-download', async (_event, downloadUrl: string, version: string) => {
    if (updateState.status === 'downloading') return { success: false, message: 'Already downloading' };
    const allowedPrefix = `https://github.com/${GITHUB_REPO}/releases/download/`;
    if (!downloadUrl || typeof downloadUrl !== 'string' || !downloadUrl.startsWith(allowedPrefix)) {
      return { success: false, message: 'Invalid download url' };
    }

    cleanUpdateDirectory();

    const tempDir = getUpdateTempDir();
    try {
      if (!existsSync(tempDir)) {
        mkdirSync(tempDir, { recursive: true });
      }
    } catch {}

    const safeVersion = version ? String(version).replace(/[^a-zA-Z0-9.-]/g, '') : '';
    if (!safeVersion) {
      return { success: false, message: 'Invalid version identifier' };
    }

    const fileName = `ZABOR-Setup-${safeVersion}.exe`;
    const targetPath = join(tempDir, fileName);

    updateState.status = 'downloading';
    updateState.progress = { percent: 0, transferred: 0, total: 0 };
    updateState.error = null;
    updateState.installerPath = null;
    updateState.expectedSha256 =
      updateState.updateInfo && updateState.updateInfo.version === safeVersion.replace(/^v/i, '')
        ? updateState.updateInfo.sha256
        : '';
    downloadAbortController = new AbortController();

    try {
      const response = await fetch(downloadUrl, {
        headers: { 'User-Agent': 'zabor-desktop' },
        signal: downloadAbortController.signal
      });

      if (!response.ok || !response.body) {
        throw new Error(`Download response not ok: ${response.status} ${response.statusText}`);
      }

      const total = Number(response.headers.get('content-length')) || 0;
      let transferred = 0;

      const reader = response.body.getReader();
      const fileStream = createWriteStream(targetPath);

      while (true) {
        const { done, value } = await reader.read();
        if (done) break;

        transferred += value.length;
        fileStream.write(Buffer.from(value));

        const percent = total > 0 ? Math.round((transferred / total) * 100) : 0;
        const progress: UpdateProgress = { percent, transferred, total };
        updateState.progress = progress;
        sendToWindow('update-download-progress', progress);
      }

      await new Promise<void>((resolve, reject) => {
        fileStream.end(() => resolve());
        fileStream.on('error', reject);
      });

      updateState.status = 'downloaded';
      updateState.installerPath = targetPath;
      downloadAbortController = null;

      sendToWindow('update-downloaded', { filePath: targetPath });

      return { success: true, filePath: targetPath };
    } catch (err: any) {
      downloadAbortController = null;
      updateState.installerPath = null;
      cleanUpdateDirectory();
      if (updateState.status === 'downloading') {
        updateState.status = 'error';
        updateState.error = err?.message || 'Download error';
        updateState.progress = null;
        sendToWindow('update-error', err?.message || 'Download error');
      }
      return { success: false, error: err?.message };
    }
  });

  ipcMain.handle('cancel-update-download', () => {
    if (downloadAbortController) {
      downloadAbortController.abort();
      downloadAbortController = null;
      updateState.installerPath = null;
      updateState.progress = null;
      updateState.error = null;
      updateState.status = updateState.updateInfo ? 'available' : 'idle';
      cleanUpdateDirectory();
      return true;
    }
    return false;
  });

  ipcMain.handle('install-update', async () => {
    if (!updateState.installerPath || !existsSync(updateState.installerPath)) {
      return { success: false, message: 'Installer not found' };
    }

    const installerPath = updateState.installerPath;

    const verification = await verifyInstaller(installerPath, updateState.expectedSha256);
    if (!verification.ok) {
      updateState.status = 'error';
      updateState.error = `installer-verification-failed:${verification.reason}`;
      updateState.installerPath = null;
      cleanUpdateDirectory();
      sendToWindow('update-error', `installer-verification-failed:${verification.reason}`);
      return { success: false, message: `Installer verification failed: ${verification.reason}` };
    }

    const installerArgs = ['/S', '--updated'];

    try {
      if (process.platform === 'win32') {
        if (installerNeedsElevation()) {
          const prodElevate = join(process.resourcesPath, 'elevate.exe');
          const devElevate = join(app.getAppPath(), 'release', 'win-unpacked', 'resources', 'elevate.exe');
          const elevatePath = existsSync(prodElevate) ? prodElevate : existsSync(devElevate) ? devElevate : null;

          if (elevatePath) {
            const child = spawn(elevatePath, [installerPath, ...installerArgs], {
              detached: true,
              stdio: 'ignore',
              windowsHide: true
            });
            child.on('error', () => {});
            child.unref();
          } else {
            const psScript = `Start-Process -FilePath "${installerPath.replace(/"/g, '`"')}" -ArgumentList '/S','--updated' -Verb RunAs`;
            const child = spawn('powershell.exe', ['-NoProfile', '-WindowStyle', 'Hidden', '-Command', psScript], {
              detached: true,
              stdio: 'ignore',
              windowsHide: true
            });
            child.on('error', () => {});
            child.unref();
          }
        } else {
          const child = spawn(installerPath, installerArgs, {
            detached: true,
            stdio: 'ignore',
            windowsHide: true
          });
          child.on('error', () => {});
          child.unref();
        }
      } else {
        await shell.openPath(installerPath);
      }

      updateState.status = 'installing';
      sendToWindow('update-installing');

      const window = getMainWindow();
      if (window && !window.isDestroyed()) {
        try {
          window.hide();
        } catch {}
      }

      setTimeout(() => {
        app.exit(0);
      }, 600);

      return { success: true };
    } catch (err: any) {
      return { success: false, error: err?.message };
    }
  });

  setTimeout(async () => {
    const window = getMainWindow();
    if (!window || window.isDestroyed()) return;

    const result = await checkGitHubRelease();
    if (result.updateAvailable && result.updateInfo) {
      if (updateState.status === 'idle' || updateState.status === 'available' || updateState.status === 'error') {
        updateState.status = 'available';
        updateState.updateInfo = result.updateInfo;
        updateState.progress = null;
        updateState.error = null;
      }
      sendToWindow('update-available', result.updateInfo);
    }
  }, 4000);
}
