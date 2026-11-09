const assert = require('node:assert/strict');
const { spawnSync, spawn } = require('node:child_process');
const { createHash } = require('node:crypto');
const { existsSync, readFileSync, writeFileSync, mkdirSync, mkdtempSync, copyFileSync, readdirSync, rmSync } = require('node:fs');
const { join, resolve, dirname } = require('node:path');
const { test, before, after } = require('node:test');
const vm = require('node:vm');

const root = resolve(__dirname, '../..');
const owned = ['native.bat', 'package.json', '.github/workflows/rust-migration.yml', '.github/workflows/build.yml',
  'scripts/native/developer-launcher.cs', 'scripts/native/developer-cache.cs',
  'scripts/native/developer-cache-regression.cs', 'scripts/native/developer-launcher.test.cjs', 'scripts/native/developer-transport.test.cjs'];
const immutable = ['.cargo/config.toml', 'rust/Cargo.toml', 'rust/Cargo.lock', 'rust/crates/zabor-native/Cargo.toml',
  'scripts/native/prepare-onnx.cjs', 'scripts/native/prepare-libclang.cjs', 'scripts/native/prepare-rnnoise.cjs',
  'scripts/native/packaging/policy.json'];
const digest = file => createHash('sha256').update(readFileSync(file)).digest('hex');
const frozen = Object.fromEntries(owned.map(path => [path, digest(join(root, path))]));
const pinned = Object.fromEntries(immutable.map(path => [path, digest(join(root, path))]));
const csc = join(process.env.WINDIR, 'Microsoft.NET/Framework64/v4.0.30319/csc.exe');
let temporary, fixture, launcher, cacheTester, admissionTester, mockTool, trace;
const processExits = [];
const quote = value => `"${value.replace(/(\\*)"/g, '$1$1\\"').replace(/(\\+)$/g, '$1$1')}"`;
function run(executable, args, env = {}) {
  const environment = { ...process.env, ...env, NATIVE_TEST_EXE: executable, NATIVE_TEST_ARGS: args.map(quote).join(' ') };
  delete environment.ZABOR_NATIVE_ARGV_JSON;
  delete environment.ZABOR_NATIVE_ARGV_FILE;
  const batch = fixture && executable.toLowerCase() === join(fixture, 'native.bat').toLowerCase();
  if (batch) environment.ZABOR_NATIVE_ARGV_JSON = JSON.stringify(args);
  for (const name of ['RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_BUILD_RUSTFLAGS', 'CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS'])
    if (!Object.hasOwn(env, name)) delete environment[name];
  const result = spawnSync(join(process.env.WINDIR, 'System32/cmd.exe'),
    batch ? ['/d', '/v:off', '/s', '/c', '"native.bat data"'] : ['/d', '/v:on', '/s', '/c', '""!NATIVE_TEST_EXE!" !NATIVE_TEST_ARGS!"'], {
      cwd: fixture || root, encoding: 'utf8', windowsVerbatimArguments: true, timeout: 30000,
      env: environment
    });
  assert.ifError(result.error);
  processExits.push({ image: executable, exit: result.status, signal: result.signal });
  return result;
}
function compile(output, sources, options = []) {
  const result = run(csc, ['/nologo', '/target:exe', '/r:System.Web.Extensions.dll', '/r:System.Management.dll', `/out:${output}`, ...options, ...sources]);
  assert.equal(result.status, 0, result.stdout + result.stderr);
}
function invoke(args, env = {}, executable = launcher) {
  rmSync(trace, { force: true });
  const result = run(executable, executable === launcher ? [fixture, ...args] : args,
    { CARGO_HOME: join(fixture, 'cargo home'), MOCK_TRACE: trace, ...env });
  return { ...result, calls: existsSync(trace) ? readFileSync(trace, 'utf8').trim().split('\n').map(line => JSON.parse(line)) : [] };
}

before(() => {
  assert.equal(process.platform, 'win32');
  assert.ok(existsSync(csc), 'Windows .NET Framework compiler is required for these developer-only mocks');
  temporary = mkdtempSync(join(process.env.RUNNER_TEMP || 'C:/Users/porad/AppData/Local/Temp/opencode', 'native-launch-'));
  fixture = join(temporary, 'fixture space & percent% bang!');
  mkdirSync(join(fixture, 'scripts/native/packaging'), { recursive: true });
  trace = join(temporary, 'mock-calls.jsonl');
  launcher = join(temporary, 'mock launcher.exe');
  cacheTester = join(temporary, 'cache regression.exe');
  const cacheMock = join(temporary, 'cache-mock.cs');
  writeFileSync(cacheMock, `using System; using System.IO;
internal static class NativeDeveloperCache {
 internal static void Verify(string root) {
  if (Environment.GetEnvironmentVariable("MOCK_CACHE_FAIL") == "1") throw new InvalidDataException("Mock missing cache pin");
 }
}`);
  compile(launcher, [join(root, 'scripts/native/developer-launcher.cs'), cacheMock]);
  const admissionSource = join(temporary, 'admission-main.cs');
  writeFileSync(admissionSource, `using System; using System.Collections.Generic;
internal static class NativeAdmissionRegression {
 static int Main(string[] args) {
  try { return NativeDeveloperLauncher.Run(args[0], args[1], new[] { new string('x', Int32.Parse(args[2])) }, new Dictionary<string,string>()); }
  catch (Exception error) { Console.Error.WriteLine(error.Message); return 2; }
 }
}`);
  admissionTester = join(temporary, 'admission regression.exe');
  compile(admissionTester, [join(root, 'scripts/native/developer-launcher.cs'), cacheMock, admissionSource], ['/main:NativeAdmissionRegression']);
  compile(cacheTester, [join(root, 'scripts/native/developer-cache.cs'), join(root, 'scripts/native/developer-cache-regression.cs')]);
  const mockSource = join(temporary, 'tool-mock.cs');
  writeFileSync(mockSource, `using System; using System.IO; using System.Web.Script.Serialization;
internal static class ToolMock {
 static int Main(string[] args) {
  var data = new { args = args, target = Environment.GetEnvironmentVariable("CARGO_TARGET_DIR"),
   webrtc = Environment.GetEnvironmentVariable("LK_CUSTOM_WEBRTC"), ort = Environment.GetEnvironmentVariable("ORT_LIB_LOCATION"),
   tool = Environment.GetEnvironmentVariable("NATIVE_DEVELOPER_EXE") };
  File.AppendAllText(Environment.GetEnvironmentVariable("MOCK_TRACE"), new JavaScriptSerializer().Serialize(data) + "\\n");
  if (Environment.GetEnvironmentVariable("MOCK_HOLD") == "1" && args.Length > 0 && args[0] == "run") {
   Console.WriteLine("MOCK_READY"); Console.Out.Flush(); Console.ReadLine();
  }
  var fail = Environment.GetEnvironmentVariable("MOCK_FAIL_STEP");
  if (fail == null || Array.IndexOf(args, fail) >= 0) return Int32.Parse(Environment.GetEnvironmentVariable("MOCK_EXIT") ?? "0");
  return 0;
 }
}`);
  mockTool = join(fixture, 'cargo home/bin/cargo.exe');
  mkdirSync(dirname(mockTool), { recursive: true });
  compile(mockTool, [mockSource]);
  copyFileSync(mockTool, join(dirname(mockTool), 'node.exe'));
  copyFileSync(join(root, 'native.bat'), join(fixture, 'native.bat'));
  copyFileSync(join(root, 'scripts/native/developer-launcher.cs'), join(fixture, 'scripts/native/developer-launcher.cs'));
  copyFileSync(cacheMock, join(fixture, 'scripts/native/developer-cache.cs'));
});

after(() => {
  const unchanged = Object.fromEntries(owned.map(path => [path, digest(join(root, path))]));
  if (temporary) rmSync(temporary, { recursive: true, force: true });
  assert.deepEqual(unchanged, frozen, 'Scoped sources changed during their original checks');
  assert.deepEqual(Object.fromEntries(immutable.map(path => [path, digest(join(root, path))])), pinned);
  console.log(`FROZEN_SOURCE_SHA256 ${JSON.stringify(frozen)}`);
  console.log(`UNCHANGED_PIN_INPUT_SHA256 ${JSON.stringify(pinned)}`);
  console.log(`ORIGINAL_MOCK_PROCESS_EXITS ${JSON.stringify(processExits.map(item => ({ image: item.image.replace(temporary || '', '<owned-temp>'), exit: item.exit, signal: item.signal })))}`);
});

test('default developer launch explicitly selects locked offline native MSVC package and own target', () => {
  const result = invoke([]);
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(result.calls[0].args, ['run', '--manifest-path', 'rust/Cargo.toml', '-p', 'zabor-native', '--locked', '--offline', '--target', 'x86_64-pc-windows-msvc', '--']);
  assert.equal(result.calls[0].target, join(fixture, 'rust/target/native-developer'));
  assert.equal(result.calls[0].webrtc, join(fixture, 'rust/target/webrtc-win-x64-a545de4'));
});

for (const args of [
  ['--preview', 'updater', '--output', 'space path'],
  ['run', '', 'say "hello"', 'C:\\path with space\\'],
  ['run', 'a&b|c<d>e^f%PATH%!x!', '(parentheses)', 'say "hello" & literal'],
  ['--', '--literal', '--release']
]) test(`runtime argument forwarding ${JSON.stringify(args)}`, () => {
  const result = invoke(args);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.deepEqual(result.calls[0].args.slice(10), args[0] === 'run' || args[0] === '--' ? args.slice(1) : args);
});

test('native.bat forwards normal quoted arguments through a repository path containing spaces and metacharacters', () => {
  const args = ['run', '--preview', 'space path', 'a&b', 'percent% bang!', 'say "hello"', 'say "hello" & literal'];
  const result = invoke(args, {}, join(fixture, 'native.bat'));
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.deepEqual(result.calls[0].args.slice(10), args.slice(1));
});

test('native.bat fails closed when the built-in developer compiler is unavailable', () => {
  const result = invoke(['run'], { WINDIR: join(temporary, 'missing windows mock') }, join(fixture, 'native.bat'));
  assert.equal(result.status, 2, result.stdout + result.stderr);
  assert.equal(result.calls.length, 0);
});

for (const code of [1, 23, 79]) test(`original Cargo process exit ${code} is forwarded without retry`, () => {
  const result = invoke(['build'], { MOCK_EXIT: String(code) });
  assert.equal(result.status, code);
  assert.equal(result.calls.length, 1);
});

test('native check runs fmt Clippy native lib tests and build, never parity', () => {
  const result = invoke(['check']);
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(result.calls.map(call => call.args[0]), ['fmt', 'clippy', 'test', 'build']);
  for (const call of result.calls) assert.equal(call.args[4], 'zabor-native');
  assert.ok(result.calls[2].args.includes('--lib'));
});

for (const step of ['fmt', 'clippy', 'test', 'build']) test(`check stops at original failing ${step} process`, () => {
  const result = invoke(['check'], { MOCK_EXIT: '67', MOCK_FAIL_STEP: step });
  assert.equal(result.status, 67);
  assert.equal(result.calls.at(-1).args[0], step);
  assert.equal(result.calls.length, ['fmt', 'clippy', 'test', 'build'].indexOf(step) + 1);
});

test('missing cache fails before any Cargo or preparation subprocess', () => {
  const result = invoke(['run'], { MOCK_CACHE_FAIL: '1' });
  assert.equal(result.status, 2);
  assert.equal(result.calls.length, 0);
  assert.match(result.stderr, /native\.bat prepare/);
});

test('cache-check does not admit any subprocess', () => {
  const result = invoke(['cache-check']);
  assert.equal(result.status, 0);
  assert.equal(result.calls.length, 0);
});

test('explicit preparation stops on first Node failure and preserves exit', () => {
  const result = invoke(['prepare'], { PATH: dirname(mockTool), MOCK_EXIT: '51' });
  assert.equal(result.status, 51);
  assert.deepEqual(result.calls.map(call => call.args), [['scripts/native/prepare-onnx.cjs']]);
});

test('explicit preparation invokes only existing three pinned preparers', () => {
  const result = invoke(['prepare'], { PATH: dirname(mockTool) });
  assert.equal(result.status, 0);
  assert.deepEqual(result.calls.map(call => call.args[0]), ['scripts/native/prepare-onnx.cjs', 'scripts/native/prepare-libclang.cjs', 'scripts/native/prepare-rnnoise.cjs']);
});

test('parity route is explicit and forwards the immutable reference harness arguments and exit', () => {
  const result = invoke(['reference-check', '--output', 'space path'], { PATH: dirname(mockTool), MOCK_EXIT: '39' });
  assert.equal(result.status, 39);
  assert.deepEqual(result.calls[0].args, ['scripts/native/verify.cjs', '--output', 'space path']);
});

for (const command of ['dist', 'dist:win', 'publish', 'release', 'release:beta']) test(`${command} is fail-closed before subprocess/cache admission`, () => {
  const result = invoke([command], { MOCK_CACHE_FAIL: '1' });
  assert.equal(result.status, 2);
  assert.equal(result.calls.length, 0);
  assert.match(result.stderr, /Packaging\/publishing is blocked/);
});

for (const variable of ['RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_BUILD_RUSTFLAGS', 'CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS'])
  test(`${variable} cannot silently override pinned CRT configuration`, () => {
    const result = invoke(['build'], { [variable]: 'override' });
    assert.equal(result.status, 2);
    assert.equal(result.calls.length, 0);
  });

test('Cargo selector and target overrides are not admitted as developer build options', () => {
  const result = invoke(['build', '-p', 'other-package']);
  assert.equal(result.status, 2);
  assert.equal(result.calls.length, 0);
});

for (const scenario of ['valid', 'missing', 'changed', 'zero-pin', 'bad-pin', 'escape']) test(`real read-only SHA256 cache verifier: ${scenario}`, () => {
  const directory = join(temporary, `hash-${scenario}`);
  mkdirSync(directory);
  const payload = join(directory, 'model cache.bin');
  writeFileSync(payload, 'mock nonproduction cache bytes');
  const expected = digest(payload);
  const pins = join(temporary, `pins-${scenario}.json`);
  const name = scenario === 'escape' ? '../outside.bin' : 'model cache.bin';
  writeFileSync(pins, JSON.stringify({ [name]: scenario === 'zero-pin' ? '0'.repeat(64) : scenario === 'bad-pin' ? 'missing' : expected }));
  if (scenario === 'missing') rmSync(payload);
  if (scenario === 'changed') writeFileSync(payload, 'corrupt mock');
  const before = existsSync(payload) ? digest(payload) : null;
  const result = run(cacheTester, ['files', directory, pins]);
  assert.equal(result.status, scenario === 'valid' ? 0 : 2, result.stdout + result.stderr);
  assert.equal(existsSync(payload) ? digest(payload) : null, before);
});

test('full production cache verifier fails on missing preparation pins without downloading', () => {
  const result = run(cacheTester, ['verify', fixture]);
  assert.equal(result.status, 2);
  assert.equal(existsSync(join(fixture, 'rust/target/onnxruntime-win-x64-1.31.1')), false);
});

for (const scenario of ['verified-onnx', 'missing-manifest', 'wrong-version', 'wrong-package-pin',
  'missing-file-pin', 'extra-file-pin', 'missing-source-pin', 'changed-file', 'unhashed-dll'])
  test(`real preparation/manifest admission: ${scenario}`, () => {
    const local = join(temporary, `manifest-${scenario}`);
    const scripts = join(local, 'scripts/native');
    const cache = join(local, 'rust/target/onnxruntime-win-x64-1.31.1');
    mkdirSync(join(scripts, 'packaging'), { recursive: true });
    mkdirSync(join(cache, 'native'), { recursive: true });
    mkdirSync(join(cache, 'licenses'));
    let source = readFileSync(join(root, 'scripts/native/prepare-onnx.cjs'), 'utf8');
    const policy = JSON.parse(readFileSync(join(root, 'scripts/native/packaging/policy.json'), 'utf8'));
    const files = {};
    for (const [name, hash] of Object.entries(policy.onnxFiles)) {
      writeFileSync(join(cache, name), `nonproduction fixture ${name}`);
      files[name] = digest(join(cache, name));
      source = source.replace(hash, files[name]);
    }
    policy.onnxFiles = { ...files };
    const packageHash = source.match(/const packageHash = '([^']+)';/)[1];
    const manifest = { version: '1.31.1', packageHash, files: { ...files } };
    if (scenario === 'wrong-version') manifest.version = 'unapproved';
    if (scenario === 'wrong-package-pin') manifest.packageHash = 'unapproved';
    if (scenario === 'missing-file-pin') delete manifest.files['native/onnxruntime.lib'];
    if (scenario === 'extra-file-pin') manifest.files['native/unapproved.dll'] = 'a'.repeat(64);
    if (scenario === 'missing-source-pin') source = source.replace(/const packageHash = '[^']+';/, '');
    if (scenario === 'changed-file') writeFileSync(join(cache, 'native/onnxruntime.dll'), 'changed mock bytes');
    if (scenario === 'unhashed-dll') writeFileSync(join(cache, 'native/unapproved.dll'), 'unverified mock');
    writeFileSync(join(scripts, 'prepare-onnx.cjs'), source);
    copyFileSync(join(root, 'scripts/native/prepare-libclang.cjs'), join(scripts, 'prepare-libclang.cjs'));
    writeFileSync(join(scripts, 'packaging/policy.json'), JSON.stringify(policy));
    if (scenario !== 'missing-manifest') writeFileSync(join(cache, 'manifest.json'), JSON.stringify(manifest));
    const result = run(cacheTester, ['verify', local]);
    assert.equal(result.status, 2);
    const patterns = {
      'verified-onnx': /libclang-win-x64-18\.1\.1/,
      'missing-manifest': /manifest\.json/,
      'wrong-version': /manifest identity differs/,
      'wrong-package-pin': /manifest identity differs/,
      'missing-file-pin': /pin count differs/,
      'extra-file-pin': /pin count differs/,
      'missing-source-pin': /Missing or ambiguous preparation pin/,
      'changed-file': /changed pinned cache file/,
      'unhashed-dll': /Unexpected unverified cache files/
    };
    assert.match(result.stderr, patterns[scenario]);
  });

test('package entrypoints separate native developer routes from preserved legacy reference tools', () => {
  const { scripts } = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));
  for (const key of ['start', 'dev', 'build', 'test:native']) assert.match(scripts[key], /^cmd\.exe .*native\.bat/);
  for (const key of ['dist:win', 'dist:win:beta', 'release', 'release:beta', 'publish', 'prepublishOnly'])
    assert.match(scripts[key], /^cmd\.exe .*native\.bat (dist|release|publish)/);
  assert.doesNotMatch(scripts.postinstall, /node |electron|rebuild|fetch/);
  assert.match(scripts['reference:build'], /electron-vite build/);
  assert.equal(scripts['test:native:reference'], 'node scripts/native/verify.cjs');
});

const workflow = readFileSync(join(root, '.github/workflows/rust-migration.yml'), 'utf8');
const legacy = readFileSync(join(root, '.github/workflows/build.yml'), 'utf8');
function condition(file, job) {
  const value = file.match(new RegExp(`^  ${job}:\\r?\\n(?:    [^\\r\\n]*\\r?\\n)*?    if: ([^\\r\\n]+)`, 'm'));
  assert.ok(value, `Missing ${job} workflow gate`);
  return value[1].replace(/^"|"$/g, '');
}
function evaluate(expression, event, ref, tag = '', channel = 'native', lane = 'native', head = '', base = '') {
  return vm.runInNewContext(expression, {
    github: { ref, head_ref: head, base_ref: base, event_name: event, event: { action: 'published', release: { tag_name: tag, prerelease: false } } },
    inputs: { channel, lane }, contains: (value, text) => value.includes(text), startsWith: (value, text) => value.startsWith(text)
  }, { timeout: 1000 });
}

test('native CI has explicit MSVC gates, pinned cache provisioning, native artifacts, no npm or Electron in the active lane', () => {
  const native = workflow.split(/^  reference:/m)[0];
  assert.match(native, /shell: cmd/);
  assert.match(native, /v\*-rust\*/);
  assert.match(native, /fail-on-cache-miss: true/);
  for (const step of ['fmt', 'clippy', 'test', 'build --release', 'cache-check', 'prepare']) assert.ok(native.includes(`native.bat ${step}`));
  assert.match(native, /cargo fetch --manifest-path rust\/Cargo\.toml --locked --target x86_64-pc-windows-msvc/);
  assert.match(native, /zabor-native\.exe/);
  assert.match(native, /if-no-files-found: error/);
  assert.doesNotMatch(native, /npm ci|npm run|electron-vite|electron-builder/);
  assert.ok(evaluate(condition(workflow, 'native'), 'push', 'refs/tags/v3.8.0-rust'));
  assert.ok(!evaluate(condition(workflow, 'reference'), 'push', 'refs/tags/v3.8.0-rust'));
});

for (const [event, ref, tag] of [
  ['push', 'refs/tags/v3.8.0-rust', ''],
  ['release', 'refs/heads/main', 'v3.8.0-rust'],
  ['workflow_dispatch', 'refs/heads/rust/native-client', ''],
  ['push', 'refs/heads/beta/rust-migration', ''],
  ['workflow_dispatch', 'refs/heads/main', '']
]) test(`legacy workflow gates deny native event ${event} ${ref} ${tag}`, () => {
  for (const job of ['verify', 'release']) assert.ok(!evaluate(condition(legacy, job), event, ref, tag));
});

test('legacy stable gate remains distinct but native default dispatch cannot package', () => {
  assert.ok(evaluate(condition(legacy, 'release'), 'push', 'refs/tags/v3.7.2'));
  assert.ok(!evaluate(condition(legacy, 'release'), 'push', 'refs/tags/v3.7.2-beta'));
  assert.ok(evaluate(condition(workflow, 'reference'), 'workflow_dispatch', 'refs/heads/main', '', 'native', 'reference'));
});

test('followup P1 adversarial unmatched CRT quote cannot execute CMD syntax before check admission', () => {
  const result = invoke(['check', 'literal" & echo injected & rem "'], {}, join(fixture, 'native.bat'));
  assert.equal(result.status, 2);
  assert.doesNotMatch(result.stdout, /injected/);
  assert.equal(result.calls.length, 0);
});

test('followup P1 supported batch runtime arguments preserve odd quotes and literal metacharacters', () => {
  const args = ['run', 'literal" & echo not-a-command', '"single', '', 'C:\\path space\\', '%PATH%!name!'];
  const result = invoke(args, {}, join(fixture, 'native.bat'));
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.deepEqual(result.calls[0].args.slice(10), args.slice(1));
});

test('followup P1 batch never interpolates positional data and ignores a hostile inherited RANDOM override', () => {
  const source = readFileSync(join(root, 'native.bat'), 'utf8');
  assert.doesNotMatch(source, /%\*|%~?[1-9]/);
  const result = invoke(['help'], { RANDOM: 'literal" & echo injected-random & rem "' }, join(fixture, 'native.bat'));
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.doesNotMatch(result.stdout, /injected-random/);
  assert.equal(result.calls.length, 0);
});

for (const exit of [17, 73]) test(`followup P2 batch preserves original exit ${exit} and removes only its invocation tool`, () => {
  const result = invoke(['build'], { MOCK_EXIT: String(exit) }, join(fixture, 'native.bat'));
  assert.equal(result.status, exit, result.stdout + result.stderr);
  assert.equal(result.calls.length, 1);
  assert.equal(existsSync(result.calls[0].tool), false);
});

test('followup P2 compiler failure is observed before reserved tool cleanup', () => {
  const file = join(fixture, 'scripts/native/developer-cache.cs');
  const original = readFileSync(file);
  let result;
  try {
    writeFileSync(file, 'this is a deliberately invalid mock compiler input');
    result = invoke(['build'], {}, join(fixture, 'native.bat'));
  } finally { writeFileSync(file, original); }
  assert.equal(result.status, 1, result.stdout + result.stderr);
  assert.equal(result.calls.length, 0);
  assert.deepEqual(readdirSync(join(fixture, 'rust/target/native-developer-tools')).filter(name => name.startsWith('invocation-')), []);
});

test('followup P1 unsupported batch-chain caller fails closed rather than dropping arguments', () => {
  const chained = join(fixture, 'unsupported-chain.cmd');
  writeFileSync(chained, '@echo off\r\nsetlocal DisableDelayedExpansion\r\n"%~dp0native.bat" run\r\nexit /b %errorlevel%\r\n');
  const result = invoke([], {}, chained);
  assert.equal(result.status, 2, result.stdout + result.stderr);
  assert.equal(result.calls.length, 0);
});

for (const name of ['RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_BUILD_RUSTFLAGS', 'CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS'])
  test(`followup P2 present empty ${name} cannot suppress configured CRT flags`, () => {
    const result = invoke(['build'], { [name]: '' });
    assert.equal(result.status, 2);
    assert.equal(result.calls.length, 0);
  });

test('followup P2 absent Rust flag overrides admit the pinned configuration', () => {
  assert.equal(invoke(['build']).status, 0);
});

for (const operation of ['test', 'clippy']) test(`followup P2 ${operation} explicitly enables synthetic native fixtures`, () => {
  const result = invoke([operation]);
  assert.equal(result.status, 0);
  assert.ok(result.calls[0].args.includes('--features'));
  assert.ok(result.calls[0].args.includes('camera-test-source,stream-audio-test-source'));
  if (operation === 'test') {
    assert.ok(result.calls[0].args.includes('--lib'));
    assert.ok(result.calls[0].args.includes('--bins'));
    assert.ok(!result.calls[0].args.includes('--tests'));
  }
});

for (const args of [[], ['build'], ['build', '--release']]) test(`followup P2 production route ${JSON.stringify(args)} excludes fixture features`, () => {
  const result = invoke(args);
  assert.equal(result.status, 0);
  assert.ok(!result.calls[0].args.includes('--features'));
});

test('followup P2 developer cannot override synthetic feature selection', () => {
  const result = invoke(['test', '--features', 'arbitrary']);
  assert.equal(result.status, 2);
  assert.equal(result.calls.length, 0);
});

test('followup P3 command beyond the expanded CMD limit fails before a child shell is admitted', () => {
  rmSync(trace, { force: true });
  const result = run(admissionTester, [fixture, mockTool, '8300'], { MOCK_TRACE: trace });
  assert.equal(result.status, 2, result.stdout + result.stderr);
  assert.match(result.stderr, /8191/);
  assert.equal(existsSync(trace), false);
});

test('followup P3 supported command transports every argument byte', () => {
  rmSync(trace, { force: true });
  const result = run(admissionTester, [fixture, mockTool, '6500'], { MOCK_TRACE: trace });
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.equal(JSON.parse(readFileSync(trace, 'utf8').trim()).args[0], 'x'.repeat(6500));
});

for (const operation of ['build', 'check']) test(`followup P2 concurrent dev and ${operation} use disjoint immutable tools and clean only after original exits`, async () => {
  const ownTrace = join(temporary, `concurrent-original-${operation}.jsonl`);
  const preserved = join(fixture, 'rust/target/native-developer-tools/user-preserved.txt');
  mkdirSync(dirname(preserved), { recursive: true });
  writeFileSync(preserved, 'unexpected user artifact');
  const environment = { ...process.env, CARGO_HOME: join(fixture, 'cargo home'), MOCK_TRACE: ownTrace, MOCK_HOLD: '1',
    ZABOR_NATIVE_ARGV_JSON: JSON.stringify(['run']) };
  delete environment.ZABOR_NATIVE_ARGV_FILE;
  for (const name of ['RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_BUILD_RUSTFLAGS', 'CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS']) delete environment[name];
  const child = spawn(join(process.env.WINDIR, 'System32/cmd.exe'), ['/d', '/v:off', '/s', '/c', '"native.bat data"'],
    { cwd: fixture, env: environment, windowsVerbatimArguments: true, stdio: ['pipe', 'pipe', 'pipe'] });
  let output = '';
  const done = new Promise((resolve, reject) => { child.once('error', reject); child.once('close', (code, signal) => resolve({ code, signal })); });
  const ready = new Promise(resolve => {
    child.stdout.on('data', chunk => { output += chunk; if (output.includes('MOCK_READY')) resolve(true); });
    child.stderr.on('data', chunk => { output += chunk; });
    child.once('close', () => resolve(false));
  });
  let firstTool, secondTool, result;
  try {
    assert.equal(await ready, true, output);
    firstTool = JSON.parse(readFileSync(ownTrace, 'utf8').trim()).tool;
    assert.ok(firstTool && existsSync(firstTool));
    result = invoke([operation], {}, join(fixture, 'native.bat'));
    secondTool = result.calls[0]?.tool;
  } finally {
    child.stdin.end('exit\n');
    const original = await done;
    processExits.push({ image: `concurrent original native.bat with ${operation}`, exit: original.code, signal: original.signal });
    assert.equal(original.code, 0, output);
  }
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.notEqual(firstTool, secondTool);
  assert.equal(existsSync(firstTool), false);
  assert.equal(existsSync(secondTool), false);
  assert.equal(readFileSync(preserved, 'utf8'), 'unexpected user artifact');
  assert.deepEqual(readdirSync(dirname(preserved)).filter(name => name.startsWith('invocation-')), []);
});

for (const [head, base, expected] of [
  ['rust/native-client', 'main', true], ['beta/rust-migration', 'main', true],
  ['feature/example', 'rust/native-client', true], ['feature/example', 'beta/rust-migration', true],
  ['feature/example', 'main', false]
]) test(`followup P2 actual PR head=${head} base=${base} routes explicitly`, () => {
  assert.equal(evaluate(condition(workflow, 'native'), 'pull_request', 'refs/pull/27/merge', '', 'native', 'native', head, base), expected);
  assert.equal(evaluate(condition(legacy, 'verify'), 'pull_request', 'refs/pull/27/merge', '', 'native', 'native', head, base), !expected);
  assert.equal(evaluate(condition(legacy, 'release'), 'pull_request', 'refs/pull/27/merge', '', 'native', 'native', head, base), false);
  if (base === 'main') assert.match(workflow, /pull_request:[\s\S]*?branches: \[[^\]]*main/);
});

for (const [event, ref, tag, expected] of [
  ['push', 'refs/tags/v3.8.0-rust', '', true], ['push', 'refs/tags/v3.7.2', '', false],
  ['release', 'refs/heads/main', 'v3.8.0-rust', true], ['release', 'refs/heads/main', 'v3.7.2', false]
]) test(`followup P2 tag/release event ${event} ${ref} ${tag} is native only when explicitly named`, () => {
  assert.equal(evaluate(condition(workflow, 'native'), event, ref, tag), expected);
  if (expected) for (const job of ['verify', 'release']) assert.equal(evaluate(condition(legacy, job), event, ref, tag), false);
});
