import { app, BrowserWindow, shell, ipcMain } from 'electron';
import { join } from 'path';
import { createWriteStream, mkdirSync, existsSync, rmSync } from 'fs';
import { spawn } from 'child_process';

export interface UpdateInfo {
  version: string;
  currentVersion: string;
  releaseName: string;
  releaseNotes: string;
  publishedAt: string;
  downloadUrl: string;
  releaseUrl: string;
  fileSize: number;
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

let downloadedInstallerPath: string | null = null;
let isDownloading = false;
let downloadAbortController: AbortController | null = null;

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

    const updateInfo: UpdateInfo = {
      version: tagName.replace(/^v/i, ''),
      currentVersion,
      releaseName: data.name || `zabor ${tagName}`,
      releaseNotes: data.body || '',
      publishedAt: data.published_at || '',
      downloadUrl,
      releaseUrl: data.html_url || `https://github.com/${GITHUB_REPO}/releases`,
      fileSize
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

export function setupUpdater(getMainWindow: () => BrowserWindow | null): void {
  cleanUpdateDirectory();

  ipcMain.handle('check-for-updates', async () => {
    return await checkGitHubRelease();
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
    if (isDownloading) return { success: false, message: 'Already downloading' };
    const allowedPrefix = `https://github.com/${GITHUB_REPO}/releases/download/`;
    if (!downloadUrl || typeof downloadUrl !== 'string' || !downloadUrl.startsWith(allowedPrefix)) {
      return { success: false, message: 'Invalid download url' };
    }

    cleanUpdateDirectory();

    const window = getMainWindow();
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

    isDownloading = true;
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
        if (window && !window.isDestroyed()) {
          window.webContents.send('update-download-progress', {
            percent,
            transferred,
            total
          } as UpdateProgress);
        }
      }

      await new Promise<void>((resolve, reject) => {
        fileStream.end(() => resolve());
        fileStream.on('error', reject);
      });

      downloadedInstallerPath = targetPath;
      isDownloading = false;
      downloadAbortController = null;

      if (window && !window.isDestroyed()) {
        window.webContents.send('update-downloaded', { filePath: targetPath });
      }

      return { success: true, filePath: targetPath };
    } catch (err: any) {
      isDownloading = false;
      downloadAbortController = null;
      downloadedInstallerPath = null;
      cleanUpdateDirectory();
      if (window && !window.isDestroyed()) {
        window.webContents.send('update-error', err?.message || 'Download error');
      }
      return { success: false, error: err?.message };
    }
  });

  ipcMain.handle('cancel-update-download', () => {
    if (downloadAbortController) {
      downloadAbortController.abort();
      downloadAbortController = null;
      isDownloading = false;
      downloadedInstallerPath = null;
      cleanUpdateDirectory();
      return true;
    }
    return false;
  });

  ipcMain.handle('install-update', async () => {
    if (!downloadedInstallerPath || !existsSync(downloadedInstallerPath)) {
      return { success: false, message: 'Installer not found' };
    }

    const installerPath = downloadedInstallerPath;

    try {
      if (process.platform === 'win32') {
        const prodElevate = join(process.resourcesPath, 'elevate.exe');
        const devElevate = join(app.getAppPath(), 'release', 'win-unpacked', 'resources', 'elevate.exe');
        const elevatePath = existsSync(prodElevate) ? prodElevate : existsSync(devElevate) ? devElevate : null;

        if (elevatePath) {
          const child = spawn(elevatePath, [installerPath, '--updated'], {
            detached: true,
            stdio: 'ignore',
            windowsHide: true
          });
          child.on('error', () => {});
          child.unref();
        } else {
          const psScript = `Start-Process -FilePath "${installerPath.replace(/"/g, '`"')}" -ArgumentList '--updated' -Verb RunAs`;
          const child = spawn('powershell.exe', ['-NoProfile', '-WindowStyle', 'Hidden', '-Command', psScript], {
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

      const window = getMainWindow();
      if (window && !window.isDestroyed()) {
        try {
          window.hide();
        } catch {}
      }

      setTimeout(() => {
        app.exit(0);
      }, 100);

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
      if (window && !window.isDestroyed()) {
        window.webContents.send('update-available', result.updateInfo);
      }
    }
  }, 4000);
}
