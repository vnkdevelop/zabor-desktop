const { test, before, after } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { createHash } = require('node:crypto');
const { gunzipSync } = require('node:zlib');
const { openReference, freezeReference, validateManifest, logicalPath, checkProvenance } = require('../../scripts/native/reference-contract.cjs');
const repository = path.resolve(__dirname, '../..');
const manifest = JSON.parse(fs.readFileSync(path.join(__dirname, 'manifest.json')));
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
let temporary;
let root;
let reference;
const catalog = 'src/renderer/src/locales/ru.json';

before(() => {
  const base = path.join(process.env.LOCALAPPDATA, 'Temp/opencode');
  temporary = fs.mkdtempSync(path.join(base, 'native-reference-contract-'));
  root = path.join(temporary, 'frozen');
  const receipt = freezeReference(root);
  assert.equal(receipt.files, 15);
  assert.equal(receipt.packagesAdmitted, false);
  reference = openReference({ root });
});
after(() => fs.rmSync(temporary, { recursive: true, force: true }));

test('all fifteen frozen sources retain exact bytes and historical Git provenance', () => {
  for (const row of manifest.files) assert.equal(hash(reference.read(row.path)), row.sha256);
});
test('current CRLF catalogs retain native hashes rather than old or normalized catalogs', () => {
  const resources = JSON.parse(fs.readFileSync(path.join(repository, 'rust/crates/zabor-native/assets/resources.json')));
  for (const row of resources.resources) assert.equal(hash(reference.read(row.sourcePath)), row.sha256);
  const bytes = reference.read(catalog);
  assert.ok(bytes.includes(Buffer.from('\r\n')));
  assert.notEqual(hash(Buffer.from(bytes.toString('utf8').replace(/\r\n/g, '\n'))), hash(bytes));
});
test('freeze preserves an existing destination without overwrite', () => assert.throws(() => freezeReference(root), /existing freeze destination preserved/));
test('independent root contains only the explicit retained fifteen-file closure', () => {
  const walk = directory => fs.readdirSync(directory, { withFileTypes: true }).flatMap(entry => entry.isDirectory() ? walk(path.join(directory, entry.name)) : [path.relative(root, path.join(directory, entry.name)).replace(/\\/g, '/')]);
  assert.deepEqual(walk(root).sort(), manifest.files.map(row => row.path).sort());
});
test('missing file fails closed', () => {
  const file = path.join(root, catalog);
  fs.renameSync(file, `${file}.held`);
  try { assert.throws(() => reference.read(catalog), /missing/); }
  finally { fs.renameSync(`${file}.held`, file); }
});
test('same-size corruption fails closed', () => {
  const file = path.join(root, catalog);
  const original = fs.readFileSync(file);
  const changed = Buffer.from(original);
  changed[10] ^= 1;
  fs.writeFileSync(file, changed);
  try { assert.throws(() => reference.read(catalog), /hash mismatch/); }
  finally { fs.writeFileSync(file, original); }
});
test('LF substitution fails closed', () => {
  const file = path.join(root, catalog);
  const original = fs.readFileSync(file);
  fs.writeFileSync(file, original.toString('utf8').replace(/\r\n/g, '\n'));
  try { assert.throws(() => reference.read(catalog), /size\/hash mismatch/); }
  finally { fs.writeFileSync(file, original); }
});
test('outside-closure existing files are not admitted', () => {
  fs.writeFileSync(path.join(root, 'unlisted.ts'), 'fixture');
  try { assert.throws(() => reference.read('unlisted.ts'), /outside retained closure/); }
  finally { fs.unlinkSync(path.join(root, 'unlisted.ts')); }
});
test('relative path traversal, drive, separators, ADS and Windows aliases fail', () => {
  for (const value of ['../a', './a', 'a/../b', 'a//b', '/a', 'C:/a', 'a\\b', 'a:stream', 'a.', 'a ', 'CON', 'a/nul.txt', 'a/%b', 'a/é']) assert.throws(() => logicalPath(value), /invalid/);
});
test('nonabsolute, traversal and UNC roots fail before reads', () => {
  for (const value of ['relative', `${root}/../frozen`, `\\\\localhost\\${root}`, `${root}/.`, root.replace('frozen', '/frozen')]) assert.throws(() => openReference({ root: value }), /absolute, non-aliased/);
});
test('case-aliased files fail', () => assert.throws(() => reference.read('src/renderer/src/locales/RU.json'), /outside retained closure/));
test('hard-linked files fail even with correct hashes', () => {
  const link = path.join(temporary, 'catalog-link.json');
  fs.linkSync(path.join(root, catalog), link);
  try { assert.throws(() => reference.read(catalog), /unaliased regular file/); }
  finally { fs.unlinkSync(link); }
});
test('redirected parent junction fails rather than using another root', () => {
  const parent = path.join(root, 'src/renderer/src/locales');
  const held = path.join(temporary, 'held-locales');
  fs.renameSync(parent, held);
  fs.symlinkSync(held, parent, 'junction');
  try { assert.throws(() => reference.read(catalog), /redirected\/reparse/); }
  finally { fs.unlinkSync(parent); fs.renameSync(held, parent); }
});
test('redirected root junction fails', () => {
  const link = path.join(temporary, 'redirected');
  fs.symlinkSync(root, link, 'junction');
  try { assert.throws(() => openReference({ root: link }), /redirected\/reparse/); }
  finally { fs.unlinkSync(link); }
});
test('environment selects only the explicit frozen root', () => {
  const previous = process.env.ZABOR_NATIVE_REFERENCE_ROOT;
  process.env.ZABOR_NATIVE_REFERENCE_ROOT = root;
  try { assert.equal(openReference().text(catalog), reference.text(catalog)); }
  finally { if (previous === undefined) delete process.env.ZABOR_NATIVE_REFERENCE_ROOT; else process.env.ZABOR_NATIVE_REFERENCE_ROOT = previous; }
});
test('each explicit catalog snapshot re-admits current root and bytes without a global trust cache', () => {
  const previous = process.env.ZABOR_NATIVE_REFERENCE_ROOT;
  const file = path.join(root, catalog);
  const original = fs.readFileSync(file);
  process.env.ZABOR_NATIVE_REFERENCE_ROOT = root;
  try {
    const first = openReference().snapshot('catalogs');
    assert.equal(first.root, root);
    assert.equal(first.files[catalog], original.toString('utf8'));
    process.env.ZABOR_NATIVE_REFERENCE_ROOT = path.join(root, 'missing-root');
    assert.throws(() => openReference().snapshot('catalogs'), /missing/);
    process.env.ZABOR_NATIVE_REFERENCE_ROOT = root;
    const changed = Buffer.from(original);
    changed[10] ^= 1;
    fs.writeFileSync(file, changed);
    assert.throws(() => openReference().snapshot('catalogs'), /hash mismatch/);
    assert.equal(first.files[catalog], original.toString('utf8'));
  } finally {
    fs.writeFileSync(file, original);
    if (previous === undefined) delete process.env.ZABOR_NATIVE_REFERENCE_ROOT;
    else process.env.ZABOR_NATIVE_REFERENCE_ROOT = previous;
  }
});
test('invalid digests, sizes, duplicate paths and unknown group entries fail', () => {
  for (const change of [value => { value.files[0].sha256 = '0'; }, value => { value.files[0].size = -1; }, value => { value.files.push({ ...value.files[0], path: value.files[0].path.toUpperCase() }); }, value => { value.groups.catalogs.push('outside.ts'); }, value => { value.sourceCommit = 'HEAD'; }, value => { value.files[0].checkout = 'normalize-anything'; }]) {
    const changed = structuredClone(manifest);
    change(changed);
    assert.throws(() => validateManifest(changed), /invalid/);
  }
});
test('self-asserted package admission cannot bypass the reviewed gate', () => {
  const changed = structuredClone(manifest);
  changed.packages.typescript.admitted = true;
  assert.throws(() => validateManifest(changed), /unreviewed/);
  for (const name of Object.keys(manifest.packages)) assert.throws(() => reference.requirePackage(name), /package closure not frozen/);
  assert.throws(() => reference.packagePath('@timephy/rnnoise-wasm', '../outside.js'), /invalid/);
  assert.throws(() => reference.requirePackage('unlisted'), /unlisted package/);
});
test('original full-renderer Tailwind/capture closure is an explicit pre-Electron gate', () => assert.throws(() => reference.capture(), /Tailwind candidate closure/));
test('Git provenance rejects wrong paths, blobs, modes and content', () => {
  const row = manifest.files[0];
  for (const listing of [`100644 blob ${'0'.repeat(40)}\t${row.path}`, `120000 blob ${row.gitBlob}\t${row.path}`, `100644 blob ${row.gitBlob}\tother.ts`]) assert.throws(() => checkProvenance(row, manifest.sourceCommit, () => Buffer.from(listing)), /Git path\/blob mismatch/);
  assert.throws(() => checkProvenance(row, manifest.sourceCommit, args => Buffer.from(args[0] === 'ls-tree' ? `100644 blob ${row.gitBlob}\t${row.path}` : 'wrong bytes')), /Git hash mismatch/);
});
test('package versions and integrity pins come from the exact historical Git lock blob', () => {
  const lock = JSON.parse(checkProvenance(manifest.packageLock, manifest.sourceCommit));
  for (const [name, pin] of Object.entries(manifest.packages)) {
    assert.equal(pin.version, lock.packages[`node_modules/${name}`].version);
    assert.equal(pin.integrity, lock.packages[`node_modules/${name}`].integrity);
  }
});
test('frozen bundle has a hash-checked bounded inventory, not arbitrary source copies', () => {
  const bytes = fs.readFileSync(path.join(repository, manifest.bundle.path));
  assert.equal(hash(bytes), manifest.bundle.sha256);
  const bundle = JSON.parse(gunzipSync(bytes));
  assert.deepEqual(Object.keys(bundle).sort(), manifest.files.filter(row => !row.seed).map(row => row.path).sort());
  for (const row of manifest.files.filter(row => !row.seed)) assert.equal(hash(Buffer.from(bundle[row.path], 'base64')), row.sha256);
});
for (const file of ['scripts/native/verify.cjs', 'scripts/native/capture-react.cjs', 'rust/crates/zabor-native/tests/voice-reference.cjs', 'rust/crates/zabor-native/tests/vad-reference.cjs', 'rust/crates/zabor-native/tests/silero-reference.cjs']) {
  test(`${file} admits references before loading packages or subprocesses`, async () => {
    const admissions = [];
    const sentinel = new Error('mocked immutable admission gate');
    const context = { require(name) {
      if (name.includes('reference-contract.cjs')) return { openReference() { admissions.push('root'); throw sentinel; } };
      if (name.startsWith('node:')) return require(name);
      throw new Error(`Unadmitted package load: ${name}`);
    }, __dirname: path.dirname(path.join(repository, file)), process: { argv: [], env: {}, platform: 'win32' }, console: { error(error) { if (error !== sentinel) throw error; } } };
    if (file.endsWith('vad-reference.cjs')) {
      vm.runInNewContext(fs.readFileSync(path.join(repository, file), 'utf8'), context);
      await Promise.resolve();
      assert.equal(context.process.exitCode, 1);
    } else assert.throws(() => vm.runInNewContext(`(function(){${fs.readFileSync(path.join(repository, file), 'utf8')}\n})()`, context), error => error === sentinel);
    assert.deepEqual(admissions, ['root']);
  });
}
test('ESM neural imports only the contract and standard Node modules before admission', () => {
  const source = fs.readFileSync(path.join(repository, 'rust/crates/zabor-native/tests/neural-reference.mjs'), 'utf8');
  assert.ok(source.indexOf('contract.openReference()') < source.indexOf('JSON.parse'));
  assert.ok(source.includes("reference.group('neural')"));
  assert.ok(!source.includes('node_modules/') && !source.includes('new URL('));
});
