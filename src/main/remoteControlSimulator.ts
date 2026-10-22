import { app, screen, globalShortcut, BrowserWindow } from 'electron';
import { spawn, ChildProcess } from 'child_process';
import { join } from 'path';
import { existsSync } from 'fs';

export class RemoteControlSimulatorManager {
  private child: ChildProcess | null = null;
  private isRunning = false;
  private monitorTimer: NodeJS.Timeout | null = null;
  private lastSimulatedX = -1;
  private lastSimulatedY = -1;
  private getWindow: () => BrowserWindow | null;

  // Intentional displacement threshold: 48 pixels
  private readonly DISPLACEMENT_THRESHOLD = 48;

  constructor(getWindow: () => BrowserWindow | null) {
    this.getWindow = getWindow;
  }

  private resolveExePath(): string {
    const candidates = [
      join(app.getAppPath(), 'build/ZaborInputSimulator.exe'),
      join(__dirname, '../../build/ZaborInputSimulator.exe'),
      join(process.resourcesPath || '', 'build/ZaborInputSimulator.exe'),
      join(process.resourcesPath || '', 'ZaborInputSimulator.exe'),
      join(process.cwd(), 'build/ZaborInputSimulator.exe')
    ];

    for (const p of candidates) {
      if (p && existsSync(p)) return p;
    }
    return candidates[0];
  }

  private safeWrite(line: string): void {
    if (!this.child || !this.isRunning || !this.child.stdin || this.child.killed || !this.child.stdin.writable) {
      return;
    }
    try {
      this.child.stdin.write(line, () => {});
    } catch {}
  }

  public start(): boolean {
    if (this.isRunning && this.child) return true;

    const exePath = this.resolveExePath();
    if (!existsSync(exePath)) {
      console.error('[RemoteControl] ZaborInputSimulator.exe not found at:', exePath);
      return false;
    }

    try {
      this.child = spawn(exePath, [String(process.pid)], {
        stdio: ['pipe', 'pipe', 'ignore'],
        windowsHide: true
      });

      this.child.stdin?.on('error', (err) => {
        console.warn('[RemoteControl] stdin error suppressed:', err?.message);
      });

      this.child.on('exit', () => {
        this.isRunning = false;
        this.child = null;
        this.stopMonitoring();
      });

      this.child.on('error', (err) => {
        console.error('[RemoteControl] Simulator process error:', err);
        this.stop();
      });

      this.isRunning = true;
      this.registerPanicHotkeys();
      this.startMonitoring();
      return true;
    } catch (err) {
      console.error('[RemoteControl] Failed to start simulator:', err);
      this.stop();
      return false;
    }
  }

  public stop(): void {
    this.stopMonitoring();
    this.unregisterPanicHotkeys();

    if (this.child) {
      if (this.isRunning && this.child.stdin && this.child.stdin.writable && !this.child.killed) {
        this.safeWrite('reset\nexit\n');
      }
      try {
        this.child.kill();
      } catch {}
      this.child = null;
    }
    this.isRunning = false;
    this.lastSimulatedX = -1;
    this.lastSimulatedY = -1;
  }

  public reset(): void {
    this.safeWrite('reset\n');
  }

  public move(absX: number, absY: number): void {
    this.lastSimulatedX = Math.round(absX);
    this.lastSimulatedY = Math.round(absY);
    this.safeWrite(`m ${this.lastSimulatedX} ${this.lastSimulatedY}\n`);
  }

  public mouseButton(button: number, down: boolean): void {
    const cmd = down ? 'd' : 'u';
    this.safeWrite(`${cmd} ${button}\n`);
  }

  public mouseWheel(delta: number): void {
    this.safeWrite(`w ${Math.round(delta)}\n`);
  }

  public key(vk: number, down: boolean): void {
    const cmd = down ? 'kd' : 'ku';
    this.safeWrite(`${cmd} ${vk}\n`);
  }

  public getScreenResolution(): { width: number; height: number; x: number; y: number } {
    try {
      const primary = screen.getPrimaryDisplay();
      return {
        width: primary.bounds.width,
        height: primary.bounds.height,
        x: primary.bounds.x,
        y: primary.bounds.y
      };
    } catch {
      return { width: 1920, height: 1080, x: 0, y: 0 };
    }
  }

  private registerPanicHotkeys(): void {
    try {
      // Primary: Pause key
      globalShortcut.register('Pause', () => {
        this.triggerPanicRevoke();
      });
      // Fallback: Ctrl+Alt+Backspace
      globalShortcut.register('CommandOrControl+Alt+Backspace', () => {
        this.triggerPanicRevoke();
      });
    } catch (err) {
      console.warn('[RemoteControl] Could not register panic shortcuts:', err);
    }
  }

  private unregisterPanicHotkeys(): void {
    try {
      globalShortcut.unregister('Pause');
      globalShortcut.unregister('CommandOrControl+Alt+Backspace');
    } catch {}
  }

  private triggerPanicRevoke(): void {
    this.reset();
    const win = this.getWindow();
    if (win && !win.isDestroyed()) {
      win.webContents.send('remote-control:panic-revoke');
    }
  }

  private startMonitoring(): void {
    if (this.monitorTimer) clearInterval(this.monitorTimer);

    this.monitorTimer = setInterval(() => {
      if (!this.isRunning) return;

      try {
        const cur = screen.getCursorScreenPoint();
        if (this.lastSimulatedX >= 0 && this.lastSimulatedY >= 0) {
          const dist = Math.hypot(cur.x - this.lastSimulatedX, cur.y - this.lastSimulatedY);
          if (dist >= this.DISPLACEMENT_THRESHOLD) {
            // Intentional physical movement by host detected!
            this.lastSimulatedX = cur.x;
            this.lastSimulatedY = cur.y;
            const win = this.getWindow();
            if (win && !win.isDestroyed()) {
              win.webContents.send('remote-control:host-activity', { type: 'move', dist });
            }
          }
        }
      } catch {}
    }, 40);

    // Listen to simulator stdout for physical click notification
    if (this.child?.stdout) {
      this.child.stdout.on('data', (data: Buffer) => {
        const str = data.toString().trim();
        if (str.includes('click') || str.includes('1')) {
          const win = this.getWindow();
          if (win && !win.isDestroyed()) {
            win.webContents.send('remote-control:host-activity', { type: 'click' });
          }
        }
      });
    }
  }

  private stopMonitoring(): void {
    if (this.monitorTimer) {
      clearInterval(this.monitorTimer);
      this.monitorTimer = null;
    }
  }
}
