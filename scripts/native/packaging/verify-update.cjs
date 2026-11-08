const { createHash, createPublicKey, verify } = require('node:crypto');
const { readFileSync, statSync } = require('node:fs');
const { resolve } = require('node:path');

const domain = Buffer.from('ZABOR-NATIVE-UPDATE-V1\0');
const maximum = 1024 * 1024 * 1024;
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const digest = value => typeof value === 'string' && /^[0-9a-f]{64}$/.test(value) && value !== '0'.repeat(64);
const fields = ['manifestVersion', 'version', 'channel', 'profile', 'platform', 'architecture', 'commit',
  'sequence', 'issuedAt', 'expiresAt', 'dataSchema', 'minimumDataSchema', 'maximumDataSchema', 'packageName',
  'packageSize', 'packageSha256', 'inventorySha256', 'publisherCertificateSha256', 'acceptanceSha256'];

function parseFlatManifest(raw) {
  const text = raw.toString('utf8');
  const manifest = JSON.parse(text);
  const pattern = /"(?:\\.|[^"\\])*"|[{}:,]|-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?|true|false|null|\s+/gy;
  const tokens = [];
  let offset = 0;
  while (offset < text.length) {
    pattern.lastIndex = offset;
    const match = pattern.exec(text);
    if (!match) throw new Error('non-flat-manifest-json');
    offset = pattern.lastIndex;
    if (!/^\s+$/.test(match[0])) tokens.push(match[0]);
  }
  if (tokens.shift() !== '{' || tokens.pop() !== '}') throw new Error('invalid-manifest-object');
  const seen = new Set();
  while (tokens.length) {
    const key = JSON.parse(tokens.shift());
    if (typeof key !== 'string' || seen.has(key) || tokens.shift() !== ':') throw new Error('duplicate-or-invalid-manifest-field');
    seen.add(key);
    const value = JSON.parse(tokens.shift());
    if (value !== null && !['string', 'number', 'boolean'].includes(typeof value)) throw new Error('non-flat-manifest-value');
    if (tokens.length && tokens.shift() !== ',') throw new Error('invalid-manifest-separator');
  }
  return manifest;
}

function verifyManifest(raw, signature, publicKey, now, minimumSequence) {
  if (!Number.isSafeInteger(now) || now < 0 || !Number.isSafeInteger(minimumSequence) || minimumSequence < 0) throw new Error('explicit-time-and-sequence-required');
  if (!raw.length || raw.length > 65536 || signature.length !== 64) throw new Error('invalid-manifest-length-or-signature');
  const key = createPublicKey({ key: publicKey, format: 'der', type: 'spki' });
  if (key.asymmetricKeyType !== 'ec' || key.asymmetricKeyDetails?.namedCurve !== 'prime256v1') throw new Error('trusted-P256-manifest-key-required');
  const order = BigInt('0xffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551');
  const s = BigInt(`0x${signature.subarray(32).toString('hex')}`);
  if (s === 0n || s > order / 2n) throw new Error('noncanonical-manifest-signature');
  if (!verify('sha256', Buffer.concat([domain, raw]), { key, dsaEncoding: 'ieee-p1363' }, signature)) throw new Error('untrusted-manifest-signature');
  const manifest = parseFlatManifest(raw);
  if (!manifest || typeof manifest !== 'object' || Array.isArray(manifest) || Object.keys(manifest).sort().join() !== fields.slice().sort().join()) throw new Error('invalid-manifest-fields');
  const version = typeof manifest.version === 'string' && /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)-rust(?:\.[a-zA-Z0-9-]+)*$/.test(manifest.version)
    && !manifest.version.split('-rust')[1].split('.').some(value => /^0\d+$/.test(value));
  if (manifest.manifestVersion !== 1 || !version || manifest.channel !== 'native-beta' || manifest.profile !== 'zabor-desktop-native-beta' ||
    manifest.platform !== 'win32' || manifest.architecture !== 'x64' || typeof manifest.commit !== 'string' || !/^[0-9a-f]{40}$/.test(manifest.commit) || manifest.commit === '0'.repeat(40) ||
    !Number.isSafeInteger(manifest.sequence) || manifest.sequence <= 0 || manifest.sequence < minimumSequence ||
    !Number.isSafeInteger(manifest.issuedAt) || manifest.issuedAt < 0 || manifest.issuedAt > now ||
    !Number.isSafeInteger(manifest.expiresAt) || manifest.expiresAt <= now || manifest.expiresAt <= manifest.issuedAt || manifest.expiresAt - manifest.issuedAt > 604800 ||
    !Number.isSafeInteger(manifest.dataSchema) || manifest.dataSchema < 1 || manifest.dataSchema > 1000 ||
    !Number.isSafeInteger(manifest.minimumDataSchema) || manifest.minimumDataSchema < 1 || manifest.minimumDataSchema > manifest.dataSchema ||
    !Number.isSafeInteger(manifest.maximumDataSchema) || manifest.maximumDataSchema < manifest.dataSchema || manifest.maximumDataSchema > 1000 ||
    manifest.packageName !== `ZABOR-Native-Beta-${manifest.version}-x64.zip` || !Number.isSafeInteger(manifest.packageSize) ||
    manifest.packageSize < 1 || manifest.packageSize > maximum || !digest(manifest.packageSha256) || !digest(manifest.inventorySha256) ||
    !digest(manifest.publisherCertificateSha256) || !digest(manifest.acceptanceSha256)) throw new Error('manifest-policy-rejected');
  return { manifest, manifestSha256: hash(raw) };
}

function verifyFiles(paths, now, minimumSequence) {
  const limited = (path, maximumSize) => {
    if (statSync(path).size > maximumSize) throw new Error('input-too-large');
    return readFileSync(path);
  };
  const result = verifyManifest(limited(paths.manifest, 65536), limited(paths.signature, 64), limited(paths.key, 4096), now, minimumSequence);
  const artifact = limited(paths.package, maximum);
  if (artifact.length !== result.manifest.packageSize || hash(artifact) !== result.manifest.packageSha256) throw new Error('package-length-or-digest-mismatch');
  return { ...result, packageIntegrity: 'passed', installationAllowed: false, nativeClientReady: false,
    remainingGates: ['native package composition and inventory digest', 'pinned Windows Authenticode', 'complete current-source native acceptance', 'app drain and durable rollback helper'] };
}

if (require.main === module) {
  try {
    const args = process.argv.slice(2);
    if (args.length !== 6) throw new Error('Usage: node verify-update.cjs MANIFEST SIGNATURE PUBLIC-KEY-SPKI PACKAGE NOW MINIMUM-SEQUENCE');
    const [manifest, signature, key, artifact, now, sequence] = args;
    if (!Number.isSafeInteger(Number(now)) || Number(now) < 0 || !Number.isSafeInteger(Number(sequence)) || Number(sequence) < 0) throw new Error('explicit-time-and-sequence-required');
    console.log(JSON.stringify(verifyFiles({ manifest: resolve(manifest), signature: resolve(signature), key: resolve(key), package: resolve(artifact) }, Number(now), Number(sequence)), null, 2));
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}

module.exports = { verifyManifest, verifyFiles };
