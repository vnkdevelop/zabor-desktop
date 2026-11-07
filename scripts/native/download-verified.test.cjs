const assert = require('node:assert/strict');
const { createHash } = require('node:crypto');
const { mkdtempSync, readFileSync, rmSync, writeFileSync } = require('node:fs');
const { createServer } = require('node:http');
const { tmpdir } = require('node:os');
const { join } = require('node:path');
const { test } = require('node:test');
const { downloadVerified } = require('./download-verified.cjs');

async function fixture(t, handler, headers = {}) {
  const directory = mkdtempSync(join(tmpdir(), 'zabor-download-test-'));
  const data = Buffer.from(Array.from({ length: 16384 }, (_, i) => i % 251));
  const ranges = [];
  const server = createServer((request, response) => {
    const [, first, last] = /^bytes=(\d+)-(\d+)$/.exec(request.headers.range);
    const start = Number(first), end = Number(last);
    ranges.push(start);
    const overrides = typeof headers === 'function' ? headers({ start, end, ranges }) : headers;
    response.writeHead(206, { 'Content-Range': `bytes ${start}-${end}/${data.length}`, 'Content-Length': end - start + 1, ETag: '"fixture-v1"', ...overrides });
    handler({ request, response, data, start, end, ranges });
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(async () => {
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
    rmSync(directory, { recursive: true, force: true });
  });
  return { data, ranges, options: { url: `http://127.0.0.1:${server.address().port}`, path: join(directory, 'partial'), sha256: createHash('sha256').update(data).digest('hex'), length: data.length, chunkSize: 4096, requestTimeout: 1000, timeout: 10000 } };
}

test('range downloads preserve all bytes and verify the complete archive', async t => {
  const f = await fixture(t, ({ response, data, start, end }) => response.end(data.subarray(start, end + 1)));
  await downloadVerified(f.options);
  assert.deepEqual(readFileSync(f.options.path), f.data);
  assert.deepEqual(f.ranges, [0, 4096, 8192, 12288]);
});

test('interrupted responses resume from bytes actually written', async t => {
  const f = await fixture(t, ({ request, response, data, start, end, ranges }) => {
    if (ranges.length === 1) {
      response.write(data.subarray(start, start + 1024), () => setTimeout(() => response.destroy(), 25));
    } else {
      assert.equal(request.headers['if-range'], '"fixture-v1"');
      response.end(data.subarray(start, end + 1));
    }
  });
  await downloadVerified(f.options);
  assert.deepEqual(readFileSync(f.options.path), f.data);
  assert.equal(f.ranges[1], 1024);
});

test('inconsistent ranges cannot append untrusted data and retries are bounded', async t => {
  const f = await fixture(t, ({ response, data }) => response.end(data.subarray(0, 4096)), { 'Content-Range': 'bytes 1-4096/16384' });
  await assert.rejects(downloadVerified({ ...f.options, retries: 2 }), /retry limit/);
  assert.equal(f.ranges.length, 3);
  assert.equal(readFileSync(f.options.path).length, 0);
});

test('corrupted complete archive is rejected', async t => {
  const f = await fixture(t, ({ response, data, start, end }) => response.end(data.subarray(start, end + 1)));
  await assert.rejects(downloadVerified({ ...f.options, sha256: '0'.repeat(64) }), /SHA-256 mismatch/);
});

test('existing paths are never overwritten', async t => {
  const f = await fixture(t, ({ response }) => response.end());
  writeFileSync(f.options.path, 'preserved');
  await assert.rejects(downloadVerified(f.options), { code: 'EEXIST' });
  assert.equal(readFileSync(f.options.path, 'utf8'), 'preserved');
  assert.equal(f.ranges.length, 0);
});

test('changed ETags cannot mix ranges from two archive versions', async t => {
  const f = await fixture(t, ({ request, response, data, start, end, ranges }) => {
    if (ranges.length > 1) assert.equal(request.headers['if-range'], '"fixture-v1"');
    response.end(data.subarray(start, end + 1));
  }, ({ ranges }) => ({ ETag: ranges.length === 1 ? '"fixture-v1"' : '"fixture-v2"' }));
  await assert.rejects(downloadVerified({ ...f.options, retries: 1 }), /retry limit/);
  assert.deepEqual(readFileSync(f.options.path), f.data.subarray(0, 4096));
  assert.deepEqual(f.ranges, [0, 4096, 4096]);
});

test('incorrect total lengths are rejected before bytes are appended', async t => {
  const f = await fixture(t, ({ response, data }) => response.end(data.subarray(0, 4096)), { 'Content-Range': 'bytes 0-4095/99999' });
  await assert.rejects(downloadVerified({ ...f.options, retries: 0 }), /retry limit/);
  assert.equal(readFileSync(f.options.path).length, 0);
});

test('encoded byte ranges cannot bypass identity-byte verification', async t => {
  const f = await fixture(t, ({ response, data }) => response.end(data.subarray(0, 4096)), { 'Content-Encoding': 'gzip' });
  await assert.rejects(downloadVerified({ ...f.options, retries: 0 }), /retry limit/);
  assert.equal(readFileSync(f.options.path).length, 0);
});

test('stalled range bodies terminate within the configured deadline', async t => {
  const f = await fixture(t, () => {});
  await assert.rejects(downloadVerified({ ...f.options, retries: 0, requestTimeout: 50, timeout: 1000 }), /retry limit/);
  assert.equal(readFileSync(f.options.path).length, 0);
});
