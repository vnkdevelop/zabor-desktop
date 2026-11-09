const assert = require('node:assert/strict');
const { spawnSync } = require('node:child_process');
const { createHash } = require('node:crypto');
const { existsSync, readFileSync, writeFileSync, mkdirSync, mkdtempSync, copyFileSync, rmSync } = require('node:fs');
const { join, resolve } = require('node:path');
const { test, before, after } = require('node:test');

const root = resolve(__dirname, '../..');
const paths = ['native.bat', 'package.json', '.github/workflows/rust-migration.yml', '.github/workflows/build.yml',
  'scripts/native/developer-launcher.cs', 'scripts/native/developer-launcher.test.cjs', 'scripts/native/developer-transport.test.cjs',
  'scripts/native/developer-cache.cs', 'scripts/native/developer-cache-regression.cs', 'docs/migration/native-launch-completion.md',
  '.cargo/config.toml', 'rust/Cargo.toml', 'rust/Cargo.lock', 'rust/crates/zabor-native/Cargo.toml',
  'scripts/native/prepare-onnx.cjs', 'scripts/native/prepare-libclang.cjs', 'scripts/native/prepare-rnnoise.cjs', 'scripts/native/packaging/policy.json'];
const hash = path => createHash('sha256').update(readFileSync(path)).digest('hex');
const frozen = Object.fromEntries(paths.map(path => [path, hash(join(root, path))]));
const cmd = join(process.env.WINDIR, 'System32/cmd.exe');
const csc = join(process.env.WINDIR, 'Microsoft.NET/Framework64/v4.0.30319/csc.exe');
const quote = value => `"${value.replace(/(\\*)"/g, '$1$1\\"').replace(/(\\+)$/g, '$1$1')}"`;
let temporary, fixture, trace;
const exits = [];

function raw(statement, switches = ['/d', '/v:off'], additions = {}) {
  rmSync(trace, { force: true });
  const env = { ...process.env, CARGO_HOME: join(fixture, 'cargo'), MOCK_TRACE: trace };
  for (const key of ['RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_BUILD_RUSTFLAGS', 'CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS',
    'ZABOR_NATIVE_ARGV_JSON', 'ZABOR_NATIVE_ARGV_FILE', 'LITERAL']) delete env[key];
  Object.assign(env, additions);
  const result = spawnSync(cmd, [...switches, '/c', statement], { cwd: fixture, env, encoding: 'utf8', windowsVerbatimArguments: true, timeout: 30000 });
  assert.ifError(result.error);
  exits.push({ statement, exit: result.status, signal: result.signal });
  return { ...result, calls: existsSync(trace) ? readFileSync(trace, 'utf8').trim().split('\n').map(line => JSON.parse(line)) : [] };
}

before(() => {
  temporary = mkdtempSync(join(process.env.RUNNER_TEMP || 'C:/Users/porad/AppData/Local/Temp/opencode', 'native-transport-'));
  fixture = join(temporary, 'fixture spaces');
  trace = join(temporary, 'calls.jsonl');
  mkdirSync(join(fixture, 'scripts/native'), { recursive: true });
  mkdirSync(join(fixture, 'cargo/bin'), { recursive: true });
  copyFileSync(join(root, 'native.bat'), join(fixture, 'native.bat'));
  copyFileSync(join(root, 'scripts/native/developer-launcher.cs'), join(fixture, 'scripts/native/developer-launcher.cs'));
  writeFileSync(join(fixture, 'scripts/native/developer-cache.cs'), 'internal static class NativeDeveloperCache { internal static void Verify(string root) {} }');
  const mock = join(temporary, 'mock.cs');
  writeFileSync(mock, `using System; using System.IO; using System.Web.Script.Serialization;
internal static class Mock {
 static int Main(string[] args) {
  File.AppendAllText(Environment.GetEnvironmentVariable("MOCK_TRACE"), new JavaScriptSerializer().Serialize(new { args = args }) + "\\n");
  return Int32.Parse(Environment.GetEnvironmentVariable("MOCK_EXIT") ?? "0");
 }
}`);
  const result = spawnSync(cmd, ['/d', '/v:on', '/s', '/c', '""!TRANSPORT_CSC!" !TRANSPORT_COMPILE_ARGS!"'], {
    cwd: fixture, encoding: 'utf8', windowsVerbatimArguments: true, timeout: 30000,
    env: { ...process.env, TRANSPORT_CSC: csc, TRANSPORT_COMPILE_ARGS: ['/nologo', '/r:System.Web.Extensions.dll', `/out:${join(fixture, 'cargo/bin/cargo.exe')}`, mock].map(quote).join(' ') }
  });
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  exits.push({ statement: 'mock compiler', exit: result.status, signal: result.signal });
});

after(() => {
  const unchanged = Object.fromEntries(paths.map(path => [path, hash(join(root, path))]));
  if (temporary) rmSync(temporary, { recursive: true, force: true });
  assert.deepEqual(unchanged, frozen);
  console.log(`TRANSPORT_FROZEN_SHA256 ${JSON.stringify(frozen)}`);
  console.log(`TRANSPORT_ORIGINAL_EXITS ${JSON.stringify(exits.map(item => ({ ...item, statement: item.statement.replace(temporary || '', '<owned-temp>') })))}`);
});

test('P1 actual raw CMD chain remains the caller responsibility, but is not admitted as child argv', () => {
  const statement = String.raw`native.bat run "literal\" & echo external-caller-command & rem \""`;
  const result = raw(statement);
  assert.match(result.stdout, /external-caller-command/);
  assert.equal(result.calls.length, 0, 'The child must not decode the caller chain into a Cargo runtime argument');
  assert.match(result.stderr, /CMD operator chain|raw CMD syntax/i);
});

for (const value of [undefined, 'expanded-wrongly']) test(`P2 raw V:OFF runtime text is explicitly unsupported, not reconstructed from env ${value}`, () => {
  const result = raw('native.bat run "!LITERAL!"', ['/d', '/v:off'], value === undefined ? {} : { LITERAL: value });
  assert.equal(result.status, 2);
  assert.equal(result.calls.length, 0);
  assert.match(result.stderr, /argv.*data|fixed.*selector/i);
});

for (const switches of [['/d', '/v:off', '/s'], ['/S', '/D', '/V:OFF']]) test(`P2 CMD S outer quote rules for native.bat help with ${switches.join(' ')}`, () => {
  const result = raw('"native.bat help"', switches);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.match(result.stdout, /Developer tools only/);
  assert.equal(result.calls.length, 0);
});

test('P2 CMD S inner quoted executable path with spaces retains the fixed selector', () => {
  const result = raw(`""${join(fixture, 'native.bat')}" help"`, ['/D', '/S', '/V:OFF']);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.equal(result.calls.length, 0);
});

for (const mode of ['off', 'on']) for (const value of [undefined, 'must-not-expand'])
  test(`argv-as-data JSON is never shell-interpolated in V:${mode}, env LITERAL=${value}`, () => {
    const args = ['run', '!LITERAL!', '%LITERAL%', 'literal" & echo not-a-command', '', 'C:\\path space\\', '^&|<>()', 'unbalanced"'];
    const env = { ZABOR_NATIVE_ARGV_JSON: JSON.stringify(args), ...(value === undefined ? {} : { LITERAL: value }) };
    const result = raw('"native.bat data"', ['/d', '/s', `/v:${mode}`], env);
    assert.equal(result.status, 0, result.stdout + result.stderr);
    assert.deepEqual(result.calls[0].args.slice(10), args.slice(1));
    assert.doesNotMatch(result.stdout, /not-a-command/);
  });

test('argv-as-data response file uses fixed trusted selector and preserves runtime bytes and original exit', () => {
  const args = ['run', 'file "quote" & literal', '', 'path with spaces'];
  const file = join(temporary, 'argv file.json');
  writeFileSync(file, JSON.stringify(args));
  const result = raw('native.bat data', ['/d', '/v:off'], { ZABOR_NATIVE_ARGV_FILE: file, MOCK_EXIT: '29' });
  assert.equal(result.status, 29, result.stdout + result.stderr);
  assert.deepEqual(result.calls[0].args.slice(10), args.slice(1));
});

for (const args of [['build', '--release'], ['check'], ['help']]) test(`JSON argv-as-data preserves supported action ${JSON.stringify(args)}`, () => {
  const result = raw('native.bat data', ['/d', '/v:off'], { ZABOR_NATIVE_ARGV_JSON: JSON.stringify(args) });
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.deepEqual(result.calls.map(call => call.args[0]), args[0] === 'check' ? ['fmt', 'clippy', 'test', 'build'] : args[0] === 'help' ? [] : ['build']);
  if (args[0] === 'build') assert.ok(result.calls[0].args.includes('--release'));
});

for (const selector of ['run', 'build', 'check']) test(`fixed trusted ${selector} call keeps ordinary developer routing`, () => {
  const result = raw(`"native.bat ${selector}"`, ['/s', '/d', '/v:off']);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.deepEqual(result.calls.map(call => call.args[0]), selector === 'check' ? ['fmt', 'clippy', 'test', 'build'] : [selector]);
});

for (const data of ['null', '[1]', '["run",null]']) test(`invalid JSON argv ${data} fails before Cargo admission`, () => {
  const result = raw('native.bat data', ['/d', '/v:off'], { ZABOR_NATIVE_ARGV_JSON: data });
  assert.equal(result.status, 2);
  assert.equal(result.calls.length, 0);
});

test('fixed data selector never defaults to run when the request is missing', () => {
  const result = raw('native.bat data');
  assert.equal(result.status, 2);
  assert.equal(result.calls.length, 0);
});

test('mixed positional/JSON routes fail rather than silently ignoring the request', () => {
  const result = raw('native.bat help', ['/d', '/v:off'], { ZABOR_NATIVE_ARGV_JSON: '["run"]' });
  assert.equal(result.status, 2);
  assert.equal(result.calls.length, 0);
});

for (const statement of [String.raw`native.bat run plain^&operator`, 'native.bat run "unterminated'])
  test(`unsupported raw quote/escape transport is rejected: ${statement}`, () => {
    const result = raw(statement);
    assert.notEqual(result.status, 0);
    assert.equal(result.calls.length, 0);
  });

test('five previously resolved fixes are byte-inherited without rerunning their old cases', () => {
  function prior(path) {
    const result = spawnSync(cmd, ['/d', '/c', 'git', 'show', `173232b394589a2aad64ec98776ddcc2cec15934:${path}`],
      { cwd: root, encoding: 'utf8', windowsVerbatimArguments: true, timeout: 30000 });
    assert.ifError(result.error);
    assert.equal(result.status, 0, result.stderr);
    exits.push({ statement: `read-only prior ${path}`, exit: result.status, signal: result.signal });
    return result.stdout.replace(/\r\n/g, '\n');
  }
  const current = path => readFileSync(join(root, path), 'utf8').replace(/\r\n/g, '\n');
  const old = prior('scripts/native/developer-launcher.cs');
  const now = current('scripts/native/developer-launcher.cs');
  const fragment = (text, begin, end) => {
    const start = text.indexOf(begin);
    const finish = text.indexOf(end, start);
    assert.ok(start >= 0 && finish > start);
    return text.slice(start, finish);
  };
  assert.equal(current('native.bat'), prior('native.bat'));
  assert.equal(fragment(now, 'foreach (var variable in new[] { "RUSTFLAGS"', '            var cache ='), fragment(old, 'foreach (var variable in new[] { "RUSTFLAGS"', '            var cache ='));
  assert.equal(fragment(now, 'private static IEnumerable<string> Cargo', '    private static int Main'), fragment(old, 'private static IEnumerable<string> Cargo', '    private static int Main'));
  assert.equal(current('.github/workflows/build.yml'), prior('.github/workflows/build.yml'));
  assert.deepEqual(current('.github/workflows/rust-migration.yml').match(/^    if: .+$/gm), prior('.github/workflows/rust-migration.yml').match(/^    if: .+$/gm));
  assert.equal(fragment(now, 'internal static int Run', '    private static string Tool'), fragment(old, 'internal static int Run', '    private static string Tool'));
  console.log('PRIOR_FIX_INHERITANCE 5 unchanged: lifetime, empty flags, features, PR gates, CMD length');
});
