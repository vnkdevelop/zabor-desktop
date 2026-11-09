const { test, before, after } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { createHash } = require('node:crypto');
const { gzipSync } = require('node:zlib');
const { execFileSync } = require('node:child_process');
const repository = path.resolve(__dirname, '../..');
const predecessor = process.env.ZABOR_REFERENCE_CLEANUP_PREDECESSOR === '1';
const workspace = process.env.ZABOR_REFERENCE_TEST_WORKSPACE;
const owner = process.env.ZABOR_REFERENCE_TEST_OWNER;
const identity = file => { const stat = fs.lstatSync(file); return [stat.dev, stat.ino]; };
before(() => {
  assert.ok(workspace && owner, 'Use the marked external cleanup-check runner');
  assert.equal(fs.readFileSync(path.join(workspace, '.fixture-owner'), 'utf8'), owner);
  assert.equal(fs.realpathSync.native(workspace), workspace);
});
after(() => assert.equal(fs.readFileSync(path.join(workspace, '.fixture-owner'), 'utf8'), owner));
const source = () => predecessor ? execFileSync('cmd.exe', ['/c', 'git show b674b9b1e11fe5113318574f5f5536f9f28a190b:scripts/native/reference-contract.cjs'], { cwd: repository }).toString('utf8') : fs.readFileSync(path.join(repository, 'scripts/native/reference-contract.cjs'), 'utf8');
const scenario = (mode, success = false, race = true, redirect = false) => {
  const directory = fs.mkdtempSync(path.join(workspace, `${mode}-${success ? 'success' : 'failure'}-`));
  const target = path.join(directory, 'reference');
  const fixtureMarker = path.join(directory, 'user-marker.txt');
  fs.writeFileSync(fixtureMarker, 'ORIGINAL_AGENT_FIXTURE_MARKER', { flag: 'wx' });
  const fixtureIdentity = identity(fixtureMarker);
  const bytes = Buffer.from('UNCHANGED_AGENT_OWNED_REFERENCE_BYTES');
  const files = ['data/owned.bin'];
  const bundle = gzipSync(Buffer.from(JSON.stringify({ [files[0]]: bytes.toString('base64') })));
  const manifest = { files: files.map(file => ({ path: file })), bundle: { path: 'tests/native-reference/sources.json.gz', sha256: createHash('sha256').update(bundle).digest('hex') } };
  const state = { deletes: [], injection: null, rootBefore: null, failedWrite: false, markerBefore: null };
  const fixtureReference = ({ root: selected }) => ({ manifest, validate(file, actual) { assert.equal(file, files[0]); assert.ok(actual.equals(bytes)); return actual; }, read(file) { const actual = fs.readFileSync(path.join(selected, file)); assert.ok(actual.equals(bytes)); return actual; } });
  const markerPath = () => fs.readdirSync(directory).find(file => file.startsWith('.native-reference-owner-'));
  const inject = destination => {
    const rootBefore = identity(target);
    const marker = markerPath();
    const markerBefore = marker ? identity(path.join(directory, marker)) : fixtureIdentity;
    fs.renameSync(destination, `${destination}.agent-held`);
    if (mode === 'directory' || mode === 'root') fs.mkdirSync(destination);
    else fs.writeFileSync(destination, 'USER_REPLACEMENT_MUST_SURVIVE', { flag: 'wx' });
    state.injection = { destination, replacementIdentity: identity(destination), timing: state.deletes.length ? 'after-final-ownership-check-at-deletion-entry' : 'after-preserved-return-with-no-deletion-entry' };
    if (mode !== 'root') assert.deepEqual(identity(target), rootBefore);
    if (mode !== 'marker' && marker) assert.deepEqual(identity(path.join(directory, marker)), markerBefore);
    assert.deepEqual(identity(fixtureMarker), fixtureIdentity);
  };
  const filesystem = new Proxy(fs, { get(original, key) {
    if (key === 'readFileSync') return (file, ...args) => file === path.join(repository, manifest.bundle.path) ? bundle : fs.readFileSync(file, ...args);
    if (key === 'writeFileSync') return (file, value, ...args) => {
      if (typeof file === 'number' && !success) {
        state.rootBefore = identity(target);
        const marker = markerPath();
        if (marker) state.markerBefore = identity(path.join(directory, marker));
        state.failedWrite = true;
        fs.writeFileSync(file, value.subarray(0, 7), ...args);
        if (mode === 'marker') fs.writeFileSync(path.join(target, 'synthetic-user-arrival.txt'), 'USER_ARRIVAL_MUST_SURVIVE', { flag: 'wx' });
        throw new Error('Injected partial reference write');
      }
      return fs.writeFileSync(file, value, ...args);
    };
    if (key === 'closeSync') return handle => {
      fs.closeSync(handle);
      if (redirect && state.failedWrite && !state.redirected) {
        const alias = path.join(directory, 'synthetic-user-root');
        fs.mkdirSync(alias);
        fs.writeFileSync(path.join(alias, 'user-marker.txt'), 'ALIAS_USER_BYTES_MUST_SURVIVE', { flag: 'wx' });
        fs.renameSync(target, `${target}.agent-held`);
        fs.symlinkSync(alias, target, 'junction');
        state.redirected = true;
      }
    };
    if (key === 'unlinkSync' || key === 'rmdirSync' || key === 'rmSync') return (file, ...args) => {
      state.deletes.push([key, file]);
      const selected = mode === 'data' ? path.join(target, files[0]) : mode === 'directory' ? path.join(target, 'data') : mode === 'root' ? target : path.join(directory, markerPath() || '.native-reference-owner-fixture');
      if (race && file === selected && !state.injection) inject(file);
      return original[key](file, ...args);
    };
    return original[key];
  } });
  const module = { exports: {} };
  let code = source();
  const start = code.indexOf('const openReference = (options = {}) => {');
  const end = code.indexOf('\nconst freezeReference = destination => {', start);
  assert.ok(start >= 0 && end > start);
  code = code.slice(0, start) + 'const openReference = fixtureReference;\n' + code.slice(end);
  vm.runInNewContext(code, { module, __dirname: path.join(repository, 'scripts/native'), process: { platform: 'win32', env: {} }, Buffer, fixtureReference, require(name) { return name === 'node:fs' ? filesystem : require(name); } });
  let result;
  let failure;
  try { result = module.exports.freezeReference(target); } catch (error) { failure = error; }
  if (race && !state.injection) {
    const destination = mode === 'data' ? path.join(target, files[0]) : mode === 'directory' ? path.join(target, 'data') : mode === 'root' ? target : path.join(directory, '.native-reference-owner-fixture');
    if (mode === 'marker') fs.writeFileSync(destination, 'ORIGINAL_AGENT_FIXTURE_MARKER', { flag: 'wx' });
    inject(destination);
  }
  return { target, fixtureMarker, fixtureIdentity, state, result, failure };
};
for (const mode of ['data', 'directory', 'root', 'marker']) {
  test(`${mode} replacement at final check/delete boundary remains preserved`, () => {
    const check = scenario(mode);
    assert.equal(fs.existsSync(check.state.injection.destination), true, `Replacement deleted at ${check.state.injection.timing}`);
    assert.deepEqual(identity(check.state.injection.destination), check.state.injection.replacementIdentity);
    if (mode === 'data' || mode === 'marker') assert.equal(fs.readFileSync(check.state.injection.destination, 'utf8'), 'USER_REPLACEMENT_MUST_SURVIVE');
    assert.equal(check.state.deletes.length, 0);
    assert.equal(check.failure.freezeFailure.status, 'failed');
    assert.equal(check.failure.freezeFailure.cleanupPerformed, false);
    assert.equal(check.failure.freezeFailure.referenceValidated, false);
  });
}
test('successful freeze marker replacement at the deletion boundary keeps admitted own bytes', () => {
  const check = scenario('marker', true);
  assert.equal(fs.existsSync(check.state.injection.destination), true, 'Replacement marker was deleted by success-finally cleanup');
  assert.equal(fs.readFileSync(check.state.injection.destination, 'utf8'), 'USER_REPLACEMENT_MUST_SURVIVE');
  assert.equal(check.state.deletes.length, 0);
  assert.equal(check.result.cleanupPolicy, 'preserve-on-failure');
  assert.equal(fs.readFileSync(path.join(check.target, 'data/owned.bin'), 'utf8'), 'UNCHANGED_AGENT_OWNED_REFERENCE_BYTES');
});
test('failed unchanged owned file remains preserved with truthful failure state', () => {
  const check = scenario('data', false, false);
  assert.equal(check.state.deletes.length, 0);
  assert.equal(fs.readFileSync(path.join(check.target, 'data/owned.bin'), 'utf8'), 'UNCHANG');
  assert.equal(check.failure.code, 'NATIVE_REFERENCE_FREEZE_PRESERVED');
  assert.equal(check.failure.freezeFailure.createdDestination, true);
  assert.equal(check.failure.freezeFailure.cleanupPerformed, false);
  assert.equal(check.failure.freezeFailure.referenceValidated, false);
  assert.deepEqual(identity(check.fixtureMarker), check.fixtureIdentity);
});
test('failed root junction and its synthetic user bytes remain untouched', () => {
  const check = scenario('data', false, false, true);
  assert.equal(check.state.deletes.length, 0);
  assert.equal(fs.lstatSync(check.target).isSymbolicLink(), true);
  assert.equal(fs.readFileSync(path.join(check.target, 'user-marker.txt'), 'utf8'), 'ALIAS_USER_BYTES_MUST_SURVIVE');
  assert.equal(check.failure.freezeFailure.cleanupPerformed, false);
});
test('CLI failure identifies the persistent destination as preserved and unvalidated', () => {
  const directory = fs.mkdtempSync(path.join(workspace, 'cli-failure-'));
  const target = path.join(directory, 'reference');
  const preload = path.join(directory, 'fail-write.cjs');
  fs.writeFileSync(preload, "const fs = require('node:fs'); const original = fs.writeFileSync; fs.writeFileSync = (file, bytes, ...args) => { if (typeof file === 'number') { original(file, bytes.subarray(0, 7), ...args); throw new Error('CLI injected partial write'); } return original(file, bytes, ...args); };", { flag: 'wx' });
  const parts = [process.execPath, '-r', preload, path.join(repository, 'scripts/native/reference-contract.cjs'), '--freeze', target];
  let outcome;
  try { execFileSync('cmd.exe', ['/d', '/v:off', '/s', '/c', `"${parts.map(value => `"${value}"`).join(' ')}"`], { windowsVerbatimArguments: true, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], timeout: 120000 }); }
  catch (error) { outcome = error; }
  assert.equal(outcome.status, 1);
  const failure = JSON.parse(outcome.stderr.trim());
  assert.equal(failure.code, 'NATIVE_REFERENCE_FREEZE_PRESERVED');
  assert.equal(failure.status, 'failed');
  assert.equal(failure.destination, target);
  assert.equal(failure.cleanupPerformed, false);
  assert.equal(failure.referenceValidated, false);
  assert.equal(fs.readFileSync(path.join(target, 'src/renderer/src/locales/ru.json')).length, 7);
});
test('freezer contains no automatic pathname deletion fallback for any object class', () => {
  const code = source();
  const body = code.slice(code.indexOf('const freezeReference = destination => {'), code.indexOf('\nmodule.exports'));
  assert.ok(!/\b(?:unlinkSync|rmdirSync|rmSync|renameSync)\s*\(/.test(body));
  assert.ok(!body.includes('ownedMarker'));
});
