import { app, ipcMain, dialog, shell, BrowserWindow } from 'electron';
import { join, basename, resolve, sep } from 'path';
import { createReadStream, createWriteStream, WriteStream } from 'fs';
import { existsSync, promises as fsPromises } from 'fs';
import { createHash, randomBytes } from 'crypto';

const MAX_FILE_BYTES = 2 * 1024 * 1024 * 1024;
const MAX_OPEN_WRITERS = 8;
const WRITER_IDLE_MS = 120_000;

interface ActiveWriter {
  stream: WriteStream;
  path: string;
  written: number;
  expected: number;
  lastTouched: number;
  failed: boolean;
}

const writers = new Map<string, ActiveWriter>();
let sweepTimer: NodeJS.Timeout | null = null;

function chatFilesRoot(): string {
  return join(app.getPath('userData'), 'chat-files');
}

function isInsideRoot(candidate: string): boolean {
  const root = resolve(chatFilesRoot());
  const target = resolve(candidate);
  return target === root || target.startsWith(root + sep);
}

const UNSAFE_NAME_CHARS = /[\x00-\x1f<>:"/\\|?*]/g;

function sanitizeName(name: unknown): string {
  const raw = typeof name === 'string' ? name : '';
  const flattened = basename(raw).replace(UNSAFE_NAME_CHARS, '_').trim();
  const safe = flattened.replace(/^\.+/, '').slice(0, 120);
  return safe.length > 0 ? safe : 'file';
}

function storedPath(storedName: unknown): string | null {
  if (typeof storedName !== 'string' || storedName.length === 0) return null;
  const path = join(chatFilesRoot(), basename(storedName));
  return isInsideRoot(path) ? path : null;
}

async function hashFile(path: string): Promise<string> {
  return new Promise((resolvePromise, rejectPromise) => {
    const hash = createHash('sha256');
    const stream = createReadStream(path);
    stream.on('data', chunk => hash.update(chunk));
    stream.on('error', rejectPromise);
    stream.on('end', () => resolvePromise(hash.digest('hex')));
  });
}

async function ensureRoot(): Promise<string> {
  const root = chatFilesRoot();
  await fsPromises.mkdir(root, { recursive: true });
  return root;
}

function scheduleSweep(): void {
  if (sweepTimer) return;
  sweepTimer = setInterval(() => {
    const now = Date.now();
    for (const [transferId, writer] of [...writers.entries()]) {
      if (now - writer.lastTouched < WRITER_IDLE_MS) continue;
      writers.delete(transferId);
      writer.stream.destroy();
      void fsPromises.rm(writer.path, { force: true }).catch(() => { });
    }
    if (writers.size === 0 && sweepTimer) {
      clearInterval(sweepTimer);
      sweepTimer = null;
    }
  }, 30_000);
  sweepTimer.unref?.();
}

function writeChunk(writer: ActiveWriter, chunk: Buffer): Promise<void> {
  return new Promise((resolvePromise, rejectPromise) => {
    writer.stream.write(chunk, error => {
      if (error) rejectPromise(error);
      else resolvePromise();
    });
  });
}

function closeStream(writer: ActiveWriter): Promise<void> {
  return new Promise(resolvePromise => {
    writer.stream.end(() => resolvePromise());
  });
}

export function registerChatFileHandlers(): void {
  const importFiles = async (filePaths: unknown): Promise<{ storedName: string; name: string; size: number; sha256: string }[]> => {
    if (!Array.isArray(filePaths) || filePaths.length > 20) return [];
    const picked: { storedName: string; name: string; size: number; sha256: string }[] = [];
    const root = await ensureRoot();
    for (const entry of filePaths) {
      if (typeof entry !== 'string' || entry.length === 0) continue;
      try {
        const filePath = resolve(entry);
        const stats = await fsPromises.stat(filePath);
        if (!stats.isFile() || stats.size > MAX_FILE_BYTES) continue;
        const name = sanitizeName(basename(filePath));
        const storedName = `${randomBytes(12).toString('hex')}-${name}`;
        const target = join(root, storedName);
        await fsPromises.copyFile(filePath, target);
        try {
          const sha256 = await hashFile(target);
          picked.push({ storedName, name, size: stats.size, sha256 });
        } catch {
          await fsPromises.rm(target, { force: true }).catch(() => { });
        }
      } catch { }
    }
    return picked;
  };

  ipcMain.handle('chat-file-begin', async (_event, transferId: unknown, fileName: unknown, size: unknown) => {
    if (typeof transferId !== 'string' || transferId.length === 0 || transferId.length > 128) {
      return { ok: false as const, error: 'bad-transfer-id' };
    }
    if (typeof size !== 'number' || !Number.isFinite(size) || size < 0 || size > MAX_FILE_BYTES) {
      return { ok: false as const, error: 'bad-size' };
    }
    if (writers.has(transferId)) return { ok: false as const, error: 'already-open' };
    if (writers.size >= MAX_OPEN_WRITERS) return { ok: false as const, error: 'too-many-transfers' };

    try {
      const root = await ensureRoot();
      const stored = `${randomBytes(12).toString('hex')}-${sanitizeName(fileName)}`;
      const path = join(root, stored);
      const stream = createWriteStream(path, { flags: 'wx' });
      const writer: ActiveWriter = {
        stream,
        path,
        written: 0,
        expected: size,
        lastTouched: Date.now(),
        failed: false
      };
      stream.on('error', () => { writer.failed = true; });
      writers.set(transferId, writer);
      scheduleSweep();
      return { ok: true as const, storedName: stored };
    } catch {
      return { ok: false as const, error: 'open-failed' };
    }
  });

  ipcMain.handle('chat-file-chunk', async (_event, transferId: unknown, chunk: unknown) => {
    if (typeof transferId !== 'string') return { ok: false as const, error: 'bad-transfer-id' };
    const writer = writers.get(transferId);
    if (!writer) return { ok: false as const, error: 'not-open' };
    if (writer.failed) return { ok: false as const, error: 'write-failed' };

    const buffer = chunk instanceof Uint8Array ? Buffer.from(chunk.buffer, chunk.byteOffset, chunk.byteLength) : null;
    if (!buffer) return { ok: false as const, error: 'bad-chunk' };
    if (writer.written + buffer.byteLength > writer.expected) {
      return { ok: false as const, error: 'size-exceeded' };
    }

    try {
      await writeChunk(writer, buffer);
      writer.written += buffer.byteLength;
      writer.lastTouched = Date.now();
      return { ok: true as const, written: writer.written };
    } catch {
      writer.failed = true;
      return { ok: false as const, error: 'write-failed' };
    }
  });

  ipcMain.handle('chat-file-commit', async (_event, transferId: unknown) => {
    if (typeof transferId !== 'string') return { ok: false as const, error: 'bad-transfer-id' };
    const writer = writers.get(transferId);
    if (!writer) return { ok: false as const, error: 'not-open' };
    writers.delete(transferId);

    await closeStream(writer);
    if (writer.failed || writer.written !== writer.expected) {
      await fsPromises.rm(writer.path, { force: true }).catch(() => { });
      return { ok: false as const, error: 'incomplete' };
    }
    return { ok: true as const, storedName: basename(writer.path), size: writer.written };
  });

  ipcMain.handle('chat-file-abort', async (_event, transferId: unknown) => {
    if (typeof transferId !== 'string') return true;
    const writer = writers.get(transferId);
    if (!writer) return true;
    writers.delete(transferId);
    writer.stream.destroy();
    await fsPromises.rm(writer.path, { force: true }).catch(() => { });
    return true;
  });

  ipcMain.handle('chat-file-delete', async (_event, storedName: unknown) => {
    const path = storedPath(storedName);
    if (!path) return false;
    try {
      await fsPromises.rm(path, { force: true });
      return true;
    } catch {
      return false;
    }
  });

  ipcMain.handle('chat-file-delete-many', async (_event, storedNames: unknown) => {
    if (!Array.isArray(storedNames)) return 0;
    let removed = 0;
    for (const entry of storedNames) {
      if (typeof entry !== 'string' || entry.length === 0) continue;
      const path = storedPath(entry);
      if (!path) continue;
      try {
        await fsPromises.rm(path, { force: true });
        removed += 1;
      } catch { }
    }
    return removed;
  });

  ipcMain.handle('chat-file-stat', async (_event, storedName: unknown) => {
    const path = storedPath(storedName);
    if (!path) return null;
    try {
      const stats = await fsPromises.stat(path);
      return { size: stats.size };
    } catch {
      return null;
    }
  });

  ipcMain.handle('chat-file-read-slice', async (_event, storedName: unknown, offset: unknown, length: unknown) => {
    const path = storedPath(storedName);
    if (!path) return null;
    if (typeof offset !== 'number' || offset < 0 || !Number.isFinite(offset)) return null;
    if (typeof length !== 'number' || length <= 0 || length > 4 * 1024 * 1024) return null;

    let handle: Awaited<ReturnType<typeof fsPromises.open>> | null = null;
    try {
      handle = await fsPromises.open(path, 'r');
      const buffer = Buffer.allocUnsafe(length);
      const { bytesRead } = await handle.read(buffer, 0, length, offset);
      return new Uint8Array(buffer.subarray(0, bytesRead));
    } catch {
      return null;
    } finally {
      await handle?.close().catch(() => { });
    }
  });

  ipcMain.handle('chat-file-hash', async (_event, storedName: unknown) => {
    const path = storedPath(storedName);
    if (!path) return null;
    try {
      return await hashFile(path);
    } catch {
      return null;
    }
  });

  ipcMain.handle('chat-file-pick', async () => {
    const window = BrowserWindow.getFocusedWindow() ?? BrowserWindow.getAllWindows()[0] ?? null;
    const result = window
      ? await dialog.showOpenDialog(window, { properties: ['openFile', 'multiSelections'] })
      : await dialog.showOpenDialog({ properties: ['openFile', 'multiSelections'] });
    if (result.canceled) return [];

    return importFiles(result.filePaths);
  });

  ipcMain.handle('chat-file-import', async (_event, filePaths: unknown) => importFiles(filePaths));

  ipcMain.handle('chat-file-save-as', async (_event, storedName: unknown, suggestedName: unknown) => {
    const source = storedPath(storedName);
    if (!source || !existsSync(source)) return false;

    const window = BrowserWindow.getFocusedWindow() ?? BrowserWindow.getAllWindows()[0] ?? null;
    const defaultPath = sanitizeName(typeof suggestedName === 'string' ? suggestedName : storedName);
    const result = window
      ? await dialog.showSaveDialog(window, { defaultPath })
      : await dialog.showSaveDialog({ defaultPath });
    if (result.canceled || !result.filePath) return false;

    try {
      await fsPromises.copyFile(source, result.filePath);
      return true;
    } catch {
      return false;
    }
  });

  ipcMain.handle('chat-file-reveal', async (_event, storedName: unknown) => {
    const path = storedPath(storedName);
    if (!path || !existsSync(path)) return false;
    shell.showItemInFolder(path);
    return true;
  });

  ipcMain.handle('chat-files-prune', async (_event, keepNames: unknown) => {
    if (!Array.isArray(keepNames)) return 0;
    const keep = new Set(keepNames.filter((entry): entry is string => typeof entry === 'string' && entry === basename(entry) && entry.length <= 255));
    const root = chatFilesRoot();
    if (!existsSync(root)) return 0;

    let removed = 0;
    try {
      const entries = await fsPromises.readdir(root);
      for (const entry of entries) {
        if (keep.has(entry)) continue;
        try {
          await fsPromises.rm(join(root, entry), { force: true });
          removed += 1;
        } catch { }
      }
    } catch { }
    return removed;
  });

  ipcMain.handle('chat-files-wipe', async () => {
    for (const [, writer] of writers) writer.stream.destroy();
    writers.clear();
    const root = chatFilesRoot();
    try {
      await fsPromises.rm(root, { recursive: true, force: true });
      return true;
    } catch {
      return false;
    }
  });
}

export function disposeChatFileWriters(): void {
  for (const [, writer] of writers) {
    writer.stream.destroy();
    void fsPromises.rm(writer.path, { force: true }).catch(() => { });
  }
  writers.clear();
  if (sweepTimer) {
    clearInterval(sweepTimer);
    sweepTimer = null;
  }
}
