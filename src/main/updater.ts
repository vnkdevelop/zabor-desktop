import { app, BrowserWindow, shell, ipcMain } from 'electron';
import { join } from 'path';
import { createWriteStream, mkdirSync, existsSync, rmSync, createReadStream, writeFileSync } from 'fs';
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
  const cleanRemote = (remote || '').trim().replace(/^v/i, '');
  const cleanCurrent = (current || '').trim().replace(/^v/i, '');

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
      if (updateState.status !== 'downloading' && updateState.status !== 'installing') {
        updateState.status = 'idle';
        updateState.updateInfo = null;
        updateState.progress = null;
        updateState.error = null;
        updateState.installerPath = null;
      }
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
  return true;
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

  if (process.platform === 'win32' && UPDATE_SIGNER) {
    const sig = await verifyAuthenticode(filePath);
    if (!sig.valid) return { ok: false, reason: `signature-${sig.status}` };
    const subjectMatch = sig.subject.toLowerCase().includes(UPDATE_SIGNER.toLowerCase());
    if (!subjectMatch) return { ok: false, reason: 'signer-mismatch' };
  }

  return { ok: true, reason: expectedSha256 ? 'sha256+signature' : 'signature-only' };
}

const INSTALL_SPLASH_SCRIPT = `
[CmdletBinding()]
param(
    [int]$ParentPid = 0
)

Add-Type -AssemblyName PresentationCore,PresentationFramework,WindowsBase

$xaml = @'
<Window xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
        xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
        Title="ZaborUpdater"
        WindowStyle="None"
        AllowsTransparency="True"
        Background="Transparent"
        ShowInTaskbar="False"
        Topmost="True"
        WindowStartupLocation="CenterScreen"
        Width="380" Height="140">
    <Grid VerticalAlignment="Center" HorizontalAlignment="Center">
        <Border Width="240" Height="110" CornerRadius="16"
                Background="#D90A0A0E" BorderBrush="#24FFFFFF" BorderThickness="1">
            <StackPanel HorizontalAlignment="Center" VerticalAlignment="Center">
                <TextBlock Text="zabor" FontSize="26" FontWeight="Black" Foreground="#FFFFFF" HorizontalAlignment="Center" Margin="0,0,0,12"/>
                <Grid Height="4" Width="160" HorizontalAlignment="Center">
                    <Border CornerRadius="2" Background="#2BFFFFFF"/>
                    <Canvas ClipToBounds="True">
                        <Border Name="Indicator" CornerRadius="2" Height="4" Width="50" Background="#FFFFFF">
                            <Border.Triggers>
                                <EventTrigger RoutedEvent="Loaded">
                                    <BeginStoryboard>
                                        <Storyboard RepeatBehavior="Forever">
                                            <DoubleAnimation Storyboard.TargetProperty="(Canvas.Left)"
                                                             From="-50" To="160"
                                                             Duration="0:0:1.1"/>
                                        </Storyboard>
                                    </BeginStoryboard>
                                </EventTrigger>
                            </Border.Triggers>
                        </Border>
                    </Canvas>
                </Grid>
            </StackPanel>
        </Border>
    </Grid>
</Window>
'@

try {
    $reader = [System.Xml.XmlNodeReader]::new([xml]$xaml)
    $window = [System.Windows.Markup.XamlReader]::Load($reader)

    $window.Add_MouseLeftButtonDown({ $window.DragMove() })
    $window.Add_KeyDown({ if ($_.Key -eq [System.Windows.Input.Key]::Escape) { $window.Close() } })

    $timer = New-Object System.Windows.Threading.DispatcherTimer
    $timer.Interval = [TimeSpan]::FromMilliseconds(500)
    $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()

    $timer.Add_Tick({
        if ($stopwatch.Elapsed.TotalSeconds -gt 600) {
            $window.Close()
            return
        }

        if ($stopwatch.Elapsed.TotalSeconds -gt 6) {
            try {
                $setupProcs = Get-Process -Name "ZABOR-Setup*" -ErrorAction SilentlyContinue
                if ($setupProcs) {
                    return
                }

                if ($stopwatch.Elapsed.TotalSeconds -gt 10) {
                    $procs = Get-Process -Name "ZABOR" -ErrorAction SilentlyContinue
                    if ($procs) {
                        $newProc = if ($ParentPid -gt 0) {
                            $procs | Where-Object { $_.Id -ne $ParentPid }
                        } else {
                            $procs
                        }
                        if ($newProc) {
                            $window.Close()
                            return
                        }
                    }

                    if ($stopwatch.Elapsed.TotalSeconds -gt 15) {
                        $window.Close()
                        return
                    }
                }
            } catch {}
        }
    })

    $timer.Start()
    [void]$window.ShowDialog()
} catch {
}
`;

export function launchInstallSplash(): void {
  if (process.platform !== 'win32') return;

  try {
    const devPath = join(app.getAppPath(), 'build', 'ZaborUpdater.exe');
    const prodPath = typeof process.resourcesPath === 'string' ? join(process.resourcesPath, 'ZaborUpdater.exe') : '';
    const updaterExe = (prodPath && existsSync(prodPath)) ? prodPath : existsSync(devPath) ? devPath : null;

    if (updaterExe) {
      try {
        const tempExe = join(app.getPath('temp'), 'ZaborUpdater.exe');
        copyFileSync(updaterExe, tempExe);
        const child = spawn(tempExe, [String(process.pid)], {
          detached: true,
          stdio: 'ignore'
        });
        child.on('error', (err) => {
          console.warn('[installSplash] Failed to spawn ZaborUpdater from temp:', err);
        });
        child.unref();
        return;
      } catch {
        const child = spawn(updaterExe, [String(process.pid)], {
          detached: true,
          stdio: 'ignore'
        });
        child.on('error', (err) => {
          console.warn('[installSplash] Failed to spawn ZaborUpdater window:', err);
        });
        child.unref();
        return;
      }
    }

    const scriptPath = join(app.getPath('temp'), 'zabor-install-splash.ps1');
    writeFileSync(scriptPath, INSTALL_SPLASH_SCRIPT.trim(), 'utf-8');

    const currentPid = process.pid;
    const child = spawn(
      'powershell.exe',
      [
        '-NoProfile',
        '-NonInteractive',
        '-WindowStyle',
        'Hidden',
        '-ExecutionPolicy',
        'Bypass',
        '-File',
        scriptPath,
        '-ParentPid',
        String(currentPid)
      ],
      {
        detached: true,
        stdio: 'ignore',
        windowsHide: true
      }
    );

    child.on('error', (err) => {
      console.warn('[installSplash] Failed to spawn splash window:', err);
    });

    child.unref();
  } catch (err) {
    console.warn('[installSplash] Error launching splash window:', err);
  }
}

export function setupUpdater(
  getMainWindow: () => BrowserWindow | null,
  openUpdaterWindow?: () => BrowserWindow | null
): void {
  cleanUpdateDirectory();

  const sendToWindow = (channel: string, payload?: unknown): void => {
    const windows = BrowserWindow.getAllWindows();
    for (const win of windows) {
      if (!win.isDestroyed()) {
        try {
          win.webContents.send(channel, payload);
        } catch {}
      }
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
    } else if (!result.updateAvailable) {
      if (updateState.status !== 'downloading' && updateState.status !== 'installing') {
        updateState.status = 'idle';
        updateState.updateInfo = null;
        updateState.progress = null;
        updateState.error = null;
        updateState.installerPath = null;
      }
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

    if (openUpdaterWindow) {
      try {
        openUpdaterWindow();
      } catch (err) {
        console.warn('[updater] Failed to open updater window:', err);
      }
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
    let fileStream: ReturnType<typeof createWriteStream> | null = null;

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
      fileStream = createWriteStream(targetPath);
      fileStream.on('error', () => {});

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
        if (!fileStream) return resolve();
        fileStream.on('error', reject);
        fileStream.end(() => resolve());
      });

      updateState.status = 'downloaded';
      updateState.installerPath = targetPath;
      downloadAbortController = null;

      sendToWindow('update-downloaded', { filePath: targetPath });

      return { success: true, filePath: targetPath };
    } catch (err: any) {
      downloadAbortController = null;
      updateState.installerPath = null;
      if (fileStream) {
        try {
          fileStream.destroy();
        } catch {}
        fileStream = null;
      }
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

    launchInstallSplash();

    const verification = await verifyInstaller(installerPath, updateState.expectedSha256);
    if (!verification.ok) {
      try {
        exec('taskkill /F /IM ZaborUpdater.exe');
      } catch {}
      updateState.status = 'error';
      updateState.error = `installer-verification-failed:${verification.reason}`;
      updateState.installerPath = null;
      cleanUpdateDirectory();
      sendToWindow('update-error', `installer-verification-failed:${verification.reason}`);
      return { success: false, message: `Installer verification failed: ${verification.reason}` };
    }

    const installerArgs = ['/S', '--updated', '--keep-app-data', '--force-run'];

    try {
      updateState.status = 'installing';
      sendToWindow('update-installing');

      try {
        if (typeof app.releaseSingleInstanceLock === 'function') {
          app.releaseSingleInstanceLock();
        }
      } catch {}

      const windows = BrowserWindow.getAllWindows();
      for (const win of windows) {
        if (!win.isDestroyed()) {
          try {
            win.destroy();
          } catch {}
        }
      }

      if (process.platform === 'win32') {
        if (installerNeedsElevation()) {
          const prodElevate = typeof process.resourcesPath === 'string' ? join(process.resourcesPath, 'elevate.exe') : '';
          const devElevate = join(app.getAppPath(), 'release', 'win-unpacked', 'resources', 'elevate.exe');
          const elevatePath = (prodElevate && existsSync(prodElevate)) ? prodElevate : existsSync(devElevate) ? devElevate : null;

          if (elevatePath) {
            const child = spawn(elevatePath, [installerPath, ...installerArgs], {
              detached: true,
              stdio: 'ignore',
              windowsHide: true
            });
            child.on('error', () => {});
            child.unref();
          } else {
            const psScript = `Start-Process -FilePath "${installerPath.replace(/"/g, '`"')}" -ArgumentList '/S','--updated','--keep-app-data','--force-run' -Verb RunAs`;
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

      setTimeout(() => {
        app.exit(0);
      }, 150);

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
    } else if (!result.updateAvailable) {
      if (updateState.status !== 'downloading' && updateState.status !== 'installing') {
        updateState.status = 'idle';
        updateState.updateInfo = null;
        updateState.progress = null;
        updateState.error = null;
        updateState.installerPath = null;
      }
    }
  }, 4000);
}
