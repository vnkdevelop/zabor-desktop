import { promises as fs } from 'node:fs';
import { join } from 'node:path';
import { sanitizeDiagnostic } from '../shared/diagnostics';

export class DiagnosticLog {
  private queue = Promise.resolve();
  private pending = 0;
  constructor(public readonly directory: string, private readonly maxBytes = 512 * 1024) {}

  public write(value: unknown): boolean {
    const record = sanitizeDiagnostic(value);
    if (!record || this.pending >= 256) return false;
    const line = JSON.stringify({ time: new Date().toISOString(), ...record }) + '\n';
    this.pending++;
    this.queue = this.queue.then(async () => {
      await fs.mkdir(this.directory, { recursive: true });
      const path = join(this.directory, 'diagnostics.jsonl');
      const size = await fs.stat(path).then(stat => stat.size).catch(() => 0);
      if (size + Buffer.byteLength(line) > this.maxBytes) {
        await fs.rm(path + '.2', { force: true });
        await fs.rename(path + '.1', path + '.2').catch(() => {});
        await fs.rename(path, path + '.1').catch(() => {});
      }
      await fs.appendFile(path, line, 'utf8');
    }).catch(() => { console.warn('[Diagnostics] local log write failed'); }).finally(() => { this.pending--; });
    return true;
  }

  public flush(): Promise<void> { return this.queue; }
}
