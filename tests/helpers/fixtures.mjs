import { Readable } from 'node:stream';
import { createHash } from 'node:crypto';

export const CURRENT_APP_VERSION = '3.6.4';
export const NEW_APP_VERSION = '3.6.5';
export const GITHUB_REPO = 'vnkdevelop/zabor-desktop';
export const ALLOWED_DOWNLOAD_PREFIX = `https://github.com/${GITHUB_REPO}/releases/download/`;

export function calculateSha256(bufferOrString) {
  return createHash('sha256').update(bufferOrString).digest('hex').toLowerCase();
}

export function createMockReleasePayload({
  tagName = `v${NEW_APP_VERSION}`,
  name = `zabor ${NEW_APP_VERSION}`,
  body = 'Release notes for testing',
  publishedAt = new Date().toISOString(),
  exeName = `ZABOR-Setup-${NEW_APP_VERSION}.exe`,
  fileSize = 1048576, // 1 MB
  sha256 = 'a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2',
  assets = null
} = {}) {
  const downloadUrl = `${ALLOWED_DOWNLOAD_PREFIX}${tagName}/${exeName}`;
  const defaultAssets = [
    {
      name: exeName,
      browser_download_url: downloadUrl,
      size: fileSize,
      digest: sha256 ? `sha256:${sha256}` : ''
    }
  ];

  return {
    tag_name: tagName,
    name,
    body,
    published_at: publishedAt,
    html_url: `https://github.com/${GITHUB_REPO}/releases/tag/${tagName}`,
    assets: assets !== null ? assets : defaultAssets
  };
}

export function createMockChunkStream({
  totalBytes = 1024 * 1024,
  chunkCount = 4,
  failAtChunk = -1,
  failError = new Error('Network socket disconnected abruptly')
} = {}) {
  const chunkSize = Math.floor(totalBytes / chunkCount);
  const chunks = [];
  let remaining = totalBytes;

  for (let i = 0; i < chunkCount; i++) {
    const size = i === chunkCount - 1 ? remaining : chunkSize;
    remaining -= size;
    const buf = Buffer.alloc(size, (i + 1) % 256);
    chunks.push(buf);
  }

  let currentChunk = 0;
  let aborted = false;

  const webStream = new ReadableStream({
    async pull(controller) {
      if (aborted) return;
      await new Promise(r => setTimeout(r, 20));
      if (currentChunk === failAtChunk) {
        controller.error(failError);
        return;
      }
      if (currentChunk < chunks.length) {
        controller.enqueue(new Uint8Array(chunks[currentChunk]));
        currentChunk++;
      } else {
        controller.close();
      }
    },
    cancel() {
      aborted = true;
    }
  });

  return {
    stream: webStream,
    chunks,
    fullBuffer: Buffer.concat(chunks),
    sha256: calculateSha256(Buffer.concat(chunks))
  };
}

export function createMockFetchResponse({
  status = 200,
  statusText = 'OK',
  bodyStream = null,
  jsonBody = null,
  contentLength = null
} = {}) {
  const headers = new Headers();
  if (contentLength !== null) {
    headers.set('content-length', String(contentLength));
  }
  if (jsonBody !== null) {
    headers.set('content-type', 'application/json');
  }

  return {
    ok: status >= 200 && status < 300,
    status,
    statusText,
    headers,
    body: bodyStream,
    async json() {
      if (jsonBody !== null) return jsonBody;
      throw new Error('Unexpected json call');
    },
    async text() {
      if (jsonBody !== null) return JSON.stringify(jsonBody);
      return '';
    }
  };
}
