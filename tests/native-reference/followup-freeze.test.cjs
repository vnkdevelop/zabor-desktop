const { test, before, after } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { execFileSync } = require('node:child_process');
const { freezeReference } = require('../../scripts/native/reference-contract.cjs');
const repository = path.resolve(__dirname, '../..');
const predecessor = process.env.ZABOR_REFERENCE_PREDECESSOR === '1';
const owner = process.env.ZABOR_REFERENCE_TEST_OWNER;
let temporary;
before(() => {
  temporary = process.env.ZABOR_REFERENCE_TEST_WORKSPACE;
  assert.ok(temporary && owner, 'Use the marked external cleanup-check runner');
  assert.equal(fs.readFileSync(path.join(temporary, '.fixture-owner'), 'utf8'), owner);
});
after(() => {
  assert.equal(fs.readFileSync(path.join(temporary, '.fixture-owner'), 'utf8'), owner);
  assert.equal(fs.realpathSync.native(temporary), temporary);
});
const source = () => predecessor ? execFileSync('cmd.exe', ['/c', 'git show 2f4c19ab7221c1a70eac93aef9ccd14f22b6b371:scripts/native/reference-contract.cjs'], { cwd: repository }).toString('utf8') : fs.readFileSync(path.join(repository, 'scripts/native/reference-contract.cjs'), 'utf8');
const sandbox = overrides => {
  const mutations = [];
  const virtual = new Map();
  const filesystem = new Proxy(fs, { get(target, key) {
    if (overrides?.[key]) return overrides[key];
    if (key === 'mkdirSync') return value => { mutations.push(['mkdir', value]); virtual.set(value, null); };
    if (key === 'writeFileSync') return (value, bytes) => { mutations.push(['write', value]); virtual.set(value, Buffer.from(bytes)); };
    if (key === 'readdirSync') return value => value === '\\' ? [...virtual.keys()].filter(file => path.dirname(file) === value).map(file => path.basename(file)) : fs.readdirSync(value);
    if (key === 'lstatSync') return (value, options) => virtual.has(value) ? { isSymbolicLink: () => false, isDirectory: () => virtual.get(value) === null, isFile: () => virtual.get(value) !== null, nlink: 1 } : fs.lstatSync(value, options);
    if (key === 'readFileSync') return (value, ...args) => virtual.has(value) ? virtual.get(value) : fs.readFileSync(value, ...args);
    if (key === 'realpathSync') return { native(value) { return value.startsWith('\\') ? path.resolve(value) : fs.realpathSync.native(value); } };
    if (['openSync', 'unlinkSync', 'rmdirSync', 'rmSync', 'renameSync'].includes(key)) return (...args) => { mutations.push([key, ...args]); throw new Error('Unexpected mocked mutation'); };
    return target[key];
  } });
  const module = { exports: {} };
  vm.runInNewContext(source(), { module, __dirname: path.join(repository, 'scripts/native'), process: { platform: 'win32', env: {} }, Buffer, require(name) {
    if (name === 'node:fs') return filesystem;
    if (name === 'node:child_process') return { execFileSync(command, args, options) {
      if (options?.input && JSON.parse(options.input).includes('\\')) return Buffer.alloc(0);
      return execFileSync(command, args, options);
    } };
    return require(name);
  } });
  return { freeze: module.exports.freezeReference, mutations };
};

test('drive-rooted destination is rejected before any mkdir/write (predecessor regression)', () => {
  const mock = sandbox();
  assert.throws(() => mock.freeze('\\new-reference'));
  assert.equal(mock.mutations.length, 0, `Mock-only predecessor mutations: ${JSON.stringify(mock.mutations.map(row => row[0]))}`);
});
test('drive-relative, UNC, device, volume, case and path aliases fail before mutation', () => {
  if (predecessor) return;
  for (const value of ['C:new-reference', '/new-reference', '\\new-reference', '\\\\host\\share\\new-reference', '\\\\?\\C:\\new-reference', '\\\\?\\Volume{fixture}\\new-reference', '\\\\.\\C:\\new-reference', 'c:\\new-reference', `${temporary}\\..\\new-reference`, `${temporary}\\\\new-reference`, `${temporary}\\new-reference.`, `${temporary}\\CON`]) {
    const mock = sandbox();
    assert.throws(() => mock.freeze(value));
    assert.equal(mock.mutations.length, 0, value);
  }
});
test('aliased volume realpath fails before any mutation', () => {
  if (predecessor) return;
  const mock = sandbox({ realpathSync: { native: value => value === path.parse(temporary).root ? 'Z:\\' : fs.realpathSync.native(value) } });
  assert.throws(() => mock.freeze(path.join(temporary, 'volume-alias')), /volume/);
  assert.equal(mock.mutations.length, 0);
});
test('reparse parent fails without creating a destination or ownership marker', () => {
  if (predecessor) return;
  const alias = path.join(temporary, 'parent-alias');
  fs.symlinkSync(temporary, alias, 'junction');
  const before = fs.readdirSync(temporary).sort();
  try {
    assert.throws(() => freezeReference(path.join(alias, 'never-created')), /redirected\/reparse/);
    assert.deepEqual(fs.readdirSync(temporary).sort(), before);
  } finally { fs.unlinkSync(alias); }
});
test('existing destination and user marker remain byte-identical', () => {
  if (predecessor) return;
  const before = fs.readFileSync(path.join(temporary, 'user-marker.txt'));
  assert.throws(() => freezeReference(temporary), /existing freeze destination preserved/);
  assert.deepEqual(fs.readFileSync(path.join(temporary, 'user-marker.txt')), before);
});
test('failed partial write reports and preserves the failed reference tree', () => {
  if (predecessor) return;
  const target = path.join(temporary, 'failed-write');
  const original = fs.writeFileSync;
  const sentinel = new Error('Injected partial write failure');
  fs.writeFileSync = (file, bytes, ...args) => {
    if (typeof file === 'number') { original(file, bytes.subarray(0, 13), ...args); throw sentinel; }
    return original(file, bytes, ...args);
  };
  try { assert.throws(() => freezeReference(target), error => error.cause === sentinel && error.freezeFailure.status === 'failed' && error.freezeFailure.cleanupPerformed === false && error.freezeFailure.referenceValidated === false); }
  finally { fs.writeFileSync = original; }
  assert.equal(fs.existsSync(target), true);
  assert.equal(fs.readFileSync(path.join(target, 'src/renderer/src/locales/ru.json')).length, 13);
});
test('failed directory creation preserves its owned prefix without any deletion', () => {
  if (predecessor) return;
  const target = path.join(temporary, 'failed-mkdir');
  const original = fs.mkdirSync;
  const sentinel = new Error('Injected directory failure');
  fs.mkdirSync = (file, ...args) => { if (file === path.join(target, 'src/renderer')) throw sentinel; return original(file, ...args); };
  try { assert.throws(() => freezeReference(target), error => error.cause === sentinel && error.freezeFailure.cleanupPerformed === false); }
  finally { fs.mkdirSync = original; }
  assert.equal(fs.existsSync(path.join(target, 'src')), true);
});
test('unexpected user file inside a failed destination is preserved', () => {
  if (predecessor) return;
  const target = path.join(temporary, 'nonempty-failure');
  const original = fs.writeFileSync;
  fs.writeFileSync = (file, bytes, ...args) => {
    if (typeof file === 'number') {
      original(path.join(target, 'user-file.txt'), 'preserved user bytes', { flag: 'wx' });
      throw new Error('Injected write failure after user arrival');
    }
    return original(file, bytes, ...args);
  };
  try { assert.throws(() => freezeReference(target), error => error.freezeFailure.cleanupPerformed === false && error.freezeFailure.destination === target); }
  finally { fs.writeFileSync = original; }
  assert.equal(fs.readFileSync(path.join(target, 'user-file.txt'), 'utf8'), 'preserved user bytes');
  assert.ok(!fs.readdirSync(temporary).some(file => file.startsWith('.native-reference-owner-')));
});
