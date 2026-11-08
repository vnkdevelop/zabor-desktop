const assert = require('node:assert/strict');
const { createHash, generateKeyPairSync, sign } = require('node:crypto');
const { mkdtempSync, rmSync, writeFileSync } = require('node:fs');
const { resolve, join } = require('node:path');
const { test } = require('node:test');
const { verifyManifest, verifyFiles } = require('./verify-update.cjs');

const order = BigInt('0xffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551');
const hash = value => createHash('sha256').update(value).digest('hex');
const fixturePackage = Buffer.from('LOCAL TEST FIXTURE ONLY');
const fixture = () => ({ manifestVersion: 1, version: '3.8.0-rust', channel: 'native-beta', profile: 'zabor-desktop-native-beta',
  platform: 'win32', architecture: 'x64', commit: '1234567890abcdef1234567890abcdef12345678', sequence: 2,
  issuedAt: 100, expiresAt: 200, dataSchema: 1, minimumDataSchema: 1, maximumDataSchema: 1,
  packageName: 'ZABOR-Native-Beta-3.8.0-rust-x64.zip', packageSize: fixturePackage.length,
  packageSha256: hash(fixturePackage), inventorySha256: hash('LOCAL INVENTORY FIXTURE'),
  publisherCertificateSha256: hash('LOCAL CERTIFICATE PIN FIXTURE'), acceptanceSha256: hash('LOCAL CONTRACT FIXTURE; NOT APPROVAL') });

function signed(raw = Buffer.from(JSON.stringify(fixture()))) {
  const { publicKey, privateKey } = generateKeyPairSync('ec', { namedCurve: 'prime256v1' });
  const signature = sign('sha256', Buffer.concat([Buffer.from('ZABOR-NATIVE-UPDATE-V1\0'), raw]), { key: privateKey, dsaEncoding: 'ieee-p1363' });
  const s = BigInt(`0x${signature.subarray(32).toString('hex')}`);
  if (s > order / 2n) Buffer.from((order - s).toString(16).padStart(64, '0'), 'hex').copy(signature, 32);
  return { raw, signature, key: publicKey.export({ format: 'der', type: 'spki' }) };
}

test('ephemeral fixture signature verifies exact bytes and pinned P256 key', () => {
  const value = signed();
  const result = verifyManifest(value.raw, value.signature, value.key, 150, 1);
  assert.equal(result.manifest.version, '3.8.0-rust');
  assert.equal(result.manifestSha256, hash(value.raw));
  assert.throws(() => verifyManifest(Buffer.concat([value.raw, Buffer.from(' ')]), value.signature, value.key, 150, 1));
  assert.throws(() => verifyManifest(value.raw, value.signature, signed().key, 150, 1));
});

test('missing key/signature and high-S signature fail closed', () => {
  const value = signed();
  assert.throws(() => verifyManifest(value.raw, value.signature, Buffer.alloc(0), 150, 1));
  assert.throws(() => verifyManifest(value.raw, Buffer.alloc(0), value.key, 150, 1));
  const s = BigInt(`0x${value.signature.subarray(32).toString('hex')}`);
  Buffer.from((order - s).toString(16).padStart(64, '0'), 'hex').copy(value.signature, 32);
  assert.throws(() => verifyManifest(value.raw, value.signature, value.key, 150, 1), /noncanonical/);
});

test('signed manifests cannot cross Electron profile/channel or include schema/path overrides', () => {
  for (const field of ['version', 'channel', 'profile', 'platform', 'architecture', 'packageName']) {
    const manifest = fixture();
    manifest[field] = '../zabor-desktop-beta';
    const value = signed(Buffer.from(JSON.stringify(manifest)));
    assert.throws(() => verifyManifest(value.raw, value.signature, value.key, 150, 1), /policy/);
  }
});

test('expiry, future issuance and persisted sequence fence replay', () => {
  const value = signed();
  for (const [now, sequence] of [[99, 1], [200, 1], [150, 3], [NaN, 1], [150, NaN]]) {
    assert.throws(() => verifyManifest(value.raw, value.signature, value.key, now, sequence));
  }
});

test('unknown and duplicate signed fields are not silently accepted', () => {
  for (const suffix of [',"channel":"native-beta"}', ',"unknown":true}']) {
    const raw = Buffer.from(JSON.stringify(fixture()).slice(0, -1) + suffix);
    const value = signed(raw);
    assert.throws(() => verifyManifest(value.raw, value.signature, value.key, 150, 1));
  }
});

test('raw manifest resource bounds and impossible zero digests are enforced', () => {
  const value = signed();
  assert.throws(() => verifyManifest(Buffer.alloc(65537), value.signature, value.key, 150, 1));
  for (const field of ['packageSha256', 'inventorySha256', 'publisherCertificateSha256', 'acceptanceSha256']) {
    const manifest = fixture();
    manifest[field] = '0'.repeat(64);
    const value = signed(Buffer.from(JSON.stringify(manifest)));
    assert.throws(() => verifyManifest(value.raw, value.signature, value.key, 150, 1));
  }
});

test('local fixture file verification never grants acceptance or installation', () => {
  const directory = mkdtempSync(resolve(__dirname, '../../../.native-updater-fixture-'));
  try {
    const value = signed();
    const paths = { manifest: join(directory, 'manifest.json'), signature: join(directory, 'manifest.sig'),
      key: join(directory, 'fixture-public.der'), package: join(directory, 'fixture.zip') };
    writeFileSync(paths.manifest, value.raw);
    writeFileSync(paths.signature, value.signature);
    writeFileSync(paths.key, value.key);
    writeFileSync(paths.package, fixturePackage);
    const result = verifyFiles(paths, 150, 1);
    assert.equal(result.packageIntegrity, 'passed');
    assert.equal(result.nativeClientReady, false);
    assert.equal(result.installationAllowed, false);
    writeFileSync(paths.package, Buffer.from('tampered fixture'));
    assert.throws(() => verifyFiles(paths, 150, 1), /digest/);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
