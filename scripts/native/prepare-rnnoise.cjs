const { createHash } = require('node:crypto');
const { setDefaultResultOrder } = require('node:dns');
const { existsSync, mkdirSync, mkdtempSync, readFileSync, linkSync, rmSync } = require('node:fs');
const { resolve, join } = require('node:path');
const { downloadVerified } = require('./download-verified.cjs');

setDefaultResultOrder('ipv4first');

const expected = '4ac81c5c0884ec4bd5907026aaae16209b7b76cd9d7f71af582094a2f98f4b43';
const directory = resolve(__dirname, '../../rust/target/rnnoise-4ac81c5c');
const target = join(directory, 'rnnoise-modeltar.gz');

async function main() {
  if (existsSync(target)) {
    if (createHash('sha256').update(readFileSync(target)).digest('hex') !== expected) throw new Error('RNNoise model cache hash mismatch');
    return;
  }
  mkdirSync(directory, { recursive: true });
  const temporary = mkdtempSync(join(directory, '.download-'));
  try {
    const sources = [
      `https://tarballs.nixos.org/sha256/${expected}`,
      `https://media.xiph.org/rnnoise/models/rnnoise_data-${expected}.tar.gz`,
    ];
    let archive;
    const failures = [];
    for (const [index, url] of sources.entries()) {
      const candidate = join(temporary, `model-${index}.tar.gz`);
      try {
        await downloadVerified({ url, path: candidate, sha256: expected, length: 22270507, chunkSize: 524288, retries: 4, requestTimeout: 20000, timeout: 240000 });
        archive = candidate;
        break;
      } catch (error) { failures.push(error.message); }
    }
    if (!archive) throw new Error(`RNNoise sources failed verification: ${failures.join('; ')}`);
    try { linkSync(archive, target); }
    catch (error) {
      if (error.code !== 'EEXIST' || createHash('sha256').update(readFileSync(target)).digest('hex') !== expected) throw error;
    }
    console.log('Prepared the pinned RNNoise model for native compilation');
  } finally { rmSync(temporary, { recursive: true, force: true }); }
}

main().catch(error => { console.error(error.message); process.exitCode = 1; });
