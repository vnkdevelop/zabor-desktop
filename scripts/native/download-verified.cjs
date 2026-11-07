const { createHash } = require('node:crypto');
const { createReadStream, createWriteStream } = require('node:fs');
const { open, stat, truncate } = require('node:fs/promises');
const { Readable, Transform } = require('node:stream');
const { pipeline } = require('node:stream/promises');
const { setTimeout: backoff } = require('node:timers/promises');

async function downloadVerified({ url, path, sha256, length, chunkSize = 524288, retries = 8, requestTimeout = 45000, timeout = 600000 }) {
  if (!Number.isSafeInteger(length) || length <= 0 || !Number.isSafeInteger(chunkSize) || chunkSize <= 0 || !Number.isSafeInteger(retries) || retries < 0 || !/^[a-f0-9]{64}$/.test(sha256)) throw new Error('Invalid verified download parameters');
  const file = await open(path, 'wx');
  await file.close();
  const deadline = AbortSignal.timeout(timeout);
  let offset = 0;
  let failures = 0;
  let consecutiveFailures = 0;
  let etag;
  while (offset < length) {
    deadline.throwIfAborted();
    const end = Math.min(length - 1, offset + chunkSize - 1);
    const signal = AbortSignal.any([deadline, AbortSignal.timeout(requestTimeout)]);
    let response;
    try {
      const headers = { Range: `bytes=${offset}-${end}`, 'Accept-Encoding': 'identity', Connection: 'close' };
      if (etag) headers['If-Range'] = etag;
      response = await fetch(url, { headers, signal });
      const range = /^bytes (\d+)-(\d+)\/(\d+)$/.exec(response.headers.get('content-range') || '');
      const encoding = response.headers.get('content-encoding');
      const receivedEtag = response.headers.get('etag');
      const contentLength = response.headers.get('content-length');
      if (response.status !== 206 || !response.body || !range || Number(range[1]) !== offset || Number(range[2]) !== end || Number(range[3]) !== length || (encoding && encoding !== 'identity') || (contentLength !== null && Number(contentLength) !== end - offset + 1) || (etag && receivedEtag !== etag)) throw new Error('Server returned an inconsistent byte range');
      etag = receivedEtag || etag;
      let bytes = 0;
      const bounded = new Transform({ transform(chunk, _encoding, callback) {
        bytes += chunk.length;
        callback(bytes > end - offset + 1 ? new Error('Download byte range overflow') : null, chunk);
      } });
      await pipeline(Readable.fromWeb(response.body), bounded, createWriteStream(path, { flags: 'a' }), { signal });
      if (bytes !== end - offset + 1) throw new Error('Incomplete download byte range');
      offset = end + 1;
      consecutiveFailures = 0;
    } catch (error) {
      if (response?.body && !response.body.locked) await response.body.cancel().catch(() => {});
      const saved = (await stat(path)).size;
      if (saved < offset || saved > end + 1) {
        await truncate(path, offset);
      } else {
        offset = saved;
      }
      failures += 1;
      if (failures > retries || deadline.aborted) throw new Error(`Verified download retry limit reached at ${offset}/${length} bytes: ${error.message} (${error.cause?.code || error.name})`, { cause: error });
      consecutiveFailures += 1;
      await backoff(Math.min(30000, 250 * 2 ** Math.min(consecutiveFailures, 7)), undefined, { signal: deadline });
    }
  }
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  if ((await stat(path)).size !== length || hash.digest('hex') !== sha256) throw new Error('Downloaded archive SHA-256 mismatch');
}

module.exports = { downloadVerified };
