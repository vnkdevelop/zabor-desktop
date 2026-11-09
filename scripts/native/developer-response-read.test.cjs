const assert = require('node:assert/strict');
const { spawnSync } = require('node:child_process');
const { createHash } = require('node:crypto');
const { existsSync, readFileSync, writeFileSync, mkdirSync, mkdtempSync, copyFileSync, truncateSync, rmSync } = require('node:fs');
const { join, resolve } = require('node:path');
const { test, before, after } = require('node:test');

const root = resolve(__dirname, '../..');
const protectedPaths = ['native.bat', 'package.json', '.github/workflows/rust-migration.yml', '.github/workflows/build.yml',
  'scripts/native/developer-launcher.cs', 'scripts/native/developer-cache.cs', 'scripts/native/developer-cache-regression.cs',
  'scripts/native/developer-launcher.test.cjs', 'scripts/native/developer-transport.test.cjs', 'scripts/native/developer-response-read.test.cjs',
  'docs/migration/native-launch-completion.md', '.cargo/config.toml', 'rust/Cargo.toml', 'rust/Cargo.lock', 'rust/crates/zabor-native/Cargo.toml',
  'scripts/native/prepare-onnx.cjs', 'scripts/native/prepare-libclang.cjs', 'scripts/native/prepare-rnnoise.cjs', 'scripts/native/packaging/policy.json'];
const digest = path => createHash('sha256').update(readFileSync(path)).digest('hex');
const frozen = Object.fromEntries(protectedPaths.map(path => [path, digest(join(root, path))]));
const cmd = join(process.env.WINDIR, 'System32/cmd.exe');
const csc = join(process.env.WINDIR, 'Microsoft.NET/Framework64/v4.0.30319/csc.exe');
const quote = value => `"${value.replace(/(\\*)"/g, '$1$1\\"').replace(/(\\+)$/g, '$1$1')}"`;
const exits = [];
let temporary, real, probe, trace, metrics;

function response(file, options = {}) {
  rmSync(trace, { force: true });
  rmSync(metrics, { force: true });
  const env = { ...process.env, CARGO_HOME: join(temporary, 'cargo'), MOCK_TRACE: trace, ZABOR_NATIVE_ARGV_FILE: file };
  for (const key of ['RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_BUILD_RUSTFLAGS', 'CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS', 'ZABOR_NATIVE_ARGV_JSON', 'MOCK_READ_MODE', 'MOCK_READ_METRICS']) delete env[key];
  if (!options.real) env.MOCK_READ_METRICS = metrics;
  if (options.mode) env.MOCK_READ_MODE = options.mode;
  Object.assign(env, options.env || {});
  const result = spawnSync(cmd, ['/d', '/v:off', '/s', '/c', options.command || '"native.bat data"'],
    { cwd: options.real ? real : probe, env, encoding: 'utf8', windowsVerbatimArguments: true, timeout: 30000 });
  assert.ifError(result.error);
  exits.push({ operation: options.label || 'response request', exit: result.status, signal: result.signal });
  return { ...result, read: existsSync(metrics) ? JSON.parse(readFileSync(metrics, 'utf8')) : null,
    calls: existsSync(trace) ? readFileSync(trace, 'utf8').trim().split('\n').map(line => JSON.parse(line)) : [] };
}

function file(name, text, encoding = 'utf8') {
  const path = join(temporary, `${name}.json`);
  let bytes = Buffer.from(text, encoding === 'utf16be' ? 'utf16le' : encoding === 'utf8-bom' ? 'utf8' : encoding);
  if (encoding === 'utf8-bom') bytes = Buffer.concat([Buffer.from([0xef, 0xbb, 0xbf]), bytes]);
  if (encoding === 'utf16le') bytes = Buffer.concat([Buffer.from([0xff, 0xfe]), bytes]);
  if (encoding === 'utf16be') bytes = Buffer.concat([Buffer.from([0xfe, 0xff]), bytes.swap16()]);
  writeFileSync(path, bytes);
  return path;
}

function sized(length, unit = ' ') {
  if (unit === ' ') return '["help"]' + ' '.repeat(length - '["help"]'.length);
  const overhead = JSON.stringify(['help', '']).length;
  return JSON.stringify(['help', unit.repeat(length - overhead)]);
}

function bounded(result, expected) {
  assert.ok(result.read, result.stdout + result.stderr);
  assert.equal(result.read.FullReadCalls, 0, 'The predecessor full-read API must not be requested before checking the decoded limit');
  assert.equal(result.read.Characters, expected);
  assert.ok(result.read.LargestRequest <= 8192);
  assert.ok(result.read.ReadCalls > 0);
}

before(() => {
  temporary = mkdtempSync(join(process.env.RUNNER_TEMP || 'C:/Users/porad/AppData/Local/Temp/opencode', 'native-response-read-'));
  real = join(temporary, 'real fixture');
  probe = join(temporary, 'probe fixture');
  trace = join(temporary, 'cargo-calls.jsonl');
  metrics = join(temporary, 'reader-metrics.json');
  mkdirSync(join(temporary, 'cargo/bin'), { recursive: true });
  const source = readFileSync(join(root, 'scripts/native/developer-launcher.cs'), 'utf8');
  const cache = 'internal static class NativeDeveloperCache { internal static void Verify(string root) {} }';
  const instrumentation = `using System; using System.IO; using System.Text; using System.Web.Script.Serialization;
internal static class RequestProbeFile {
 public static bool Exists(string path) { return System.IO.File.Exists(path); }
 public static string ReadAllText(string path) {
  using (var reader = new RequestProbeReader(path, Encoding.UTF8, true)) return reader.ReadToEnd();
 }
}
internal sealed class RequestProbeReader : TextReader {
 readonly System.IO.StreamReader inner;
 readonly string path;
 readonly long initialLength;
 int fullReadCalls, largestRequest, characters, readCalls;
 bool grew;
 public RequestProbeReader(string path, Encoding encoding, bool detectBom) {
  this.path = path;
  var stream = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite);
  initialLength = stream.Length;
  inner = new System.IO.StreamReader(stream, encoding, detectBom, 128);
 }
 public override int Read(char[] buffer, int index, int count) {
  readCalls++; largestRequest = Math.Max(largestRequest, count);
  if (Environment.GetEnvironmentVariable("MOCK_READ_MODE") == "error") throw new IOException("Owned injected reader error");
  var chunk = Environment.GetEnvironmentVariable("MOCK_READ_MODE") == "short" ? 7 : 257;
  int read = inner.Read(buffer, index, Math.Min(count, chunk)); characters += read;
  if (!grew && read > 0 && Environment.GetEnvironmentVariable("MOCK_READ_MODE") == "grow") {
   grew = true;
   using (var append = new FileStream(path, FileMode.Append, FileAccess.Write, FileShare.ReadWrite)) {
    var padding = new byte[4096]; for (int i = 0; i < padding.Length; i++) padding[i] = 32;
    append.Write(padding, 0, padding.Length);
   }
  }
  return read;
 }
 public override string ReadToEnd() {
  fullReadCalls++;
  var safe = new char[8192]; int length = 0;
  while (length < safe.Length) {
   int read = Read(safe, length, safe.Length - length);
   if (read == 0) return new string(safe, 0, length);
   length += read;
  }
  throw new ArgumentException("Safety probe intercepted the full-read request beyond 8191 characters");
 }
 protected override void Dispose(bool disposing) {
  if (disposing) {
   var position = inner.BaseStream.Position; inner.Dispose();
   var destination = Environment.GetEnvironmentVariable("MOCK_READ_METRICS");
   if (destination != null) System.IO.File.WriteAllText(destination, new JavaScriptSerializer().Serialize(new {
    FullReadCalls = fullReadCalls, LargestRequest = largestRequest, Characters = characters, ReadCalls = readCalls,
    BytesRead = position, InitialLength = initialLength, FinalLength = new FileInfo(path).Length, Grew = grew
   }));
  }
  base.Dispose(disposing);
 }
}
`;
  for (const directory of [real, probe]) {
    mkdirSync(join(directory, 'scripts/native'), { recursive: true });
    copyFileSync(join(root, 'native.bat'), join(directory, 'native.bat'));
    writeFileSync(join(directory, 'scripts/native/developer-launcher.cs'), directory === probe
      ? 'using File = RequestProbeFile;\nusing StreamReader = RequestProbeReader;\n' + source : source);
    writeFileSync(join(directory, 'scripts/native/developer-cache.cs'), directory === probe ? instrumentation + cache : cache);
  }
  const mock = join(temporary, 'cargo-mock.cs');
  writeFileSync(mock, `using System; using System.IO; using System.Web.Script.Serialization;
internal static class CargoMock { static int Main(string[] args) {
 File.AppendAllText(Environment.GetEnvironmentVariable("MOCK_TRACE"), new JavaScriptSerializer().Serialize(new { args = args }) + "\\n");
 return Int32.Parse(Environment.GetEnvironmentVariable("MOCK_EXIT") ?? "0");
} }`);
  const result = spawnSync(cmd, ['/d', '/v:on', '/s', '/c', '""!RESPONSE_CSC!" !RESPONSE_COMPILE_ARGS!"'], {
    cwd: temporary, encoding: 'utf8', windowsVerbatimArguments: true, timeout: 30000,
    env: { ...process.env, RESPONSE_CSC: csc, RESPONSE_COMPILE_ARGS: ['/nologo', '/r:System.Web.Extensions.dll', `/out:${join(temporary, 'cargo/bin/cargo.exe')}`, mock].map(quote).join(' ') }
  });
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  exits.push({ operation: 'owned mock compiler', exit: result.status, signal: result.signal });
});

after(() => {
  const unchanged = Object.fromEntries(protectedPaths.map(path => [path, digest(join(root, path))]));
  if (temporary) rmSync(temporary, { recursive: true, force: true });
  assert.deepEqual(unchanged, frozen);
  console.log(`RESPONSE_READ_FROZEN_SHA256 ${JSON.stringify(frozen)}`);
  console.log(`RESPONSE_READ_ORIGINAL_EXITS ${JSON.stringify(exits)}`);
});

test('owned oversized response stops at 8192 decoded characters without requesting a full-file read', () => {
  const request = file('owned-one-megabyte', '["help"]');
  truncateSync(request, 1024 * 1024);
  const result = response(request, { label: '1 MiB fixture' });
  assert.equal(result.status, 2);
  assert.equal(result.calls.length, 0);
  bounded(result, 8192);
  assert.ok(result.read.BytesRead <= 16384);
  assert.equal(result.read.InitialLength, 1024 * 1024);
});

for (const [length, encoding, unit] of [[8191, 'utf8', ' '], [8192, 'utf8', ' '], [8191, 'utf8', 'é'],
  [8191, 'utf8-bom', 'é'], [8192, 'utf8-bom', 'é'], [8191, 'utf16le', 'é'], [8191, 'utf16be', 'é']])
  test(`decoded character boundary ${length}, ${encoding}, ${unit === ' ' ? 'ASCII' : 'multibyte'}`, () => {
    const result = response(file(`boundary-${length}-${encoding}-${unit === ' ' ? 'ascii' : 'multi'}`, sized(length, unit), encoding), { label: `${encoding} ${length}` });
    assert.equal(result.status, length === 8191 ? 0 : 2, result.stdout + result.stderr);
    assert.equal(result.calls.length, 0);
    bounded(result, length);
  });

test('escaped JSON source counts decoded file characters before JSON escapes are interpreted', () => {
  let text = '["help","' + '\\u00e9'.repeat(1300) + '"]';
  text += ' '.repeat(8191 - text.length);
  const result = response(file('escaped-boundary', text, 'utf8-bom'), { label: 'escaped JSON 8191' });
  assert.equal(result.status, 0, result.stdout + result.stderr);
  bounded(result, 8191);
});

test('short reader chunks still enforce exactly LIMIT plus one', () => {
  const result = response(file('short-reads', sized(8192)), { mode: 'short', label: 'short chunks' });
  assert.equal(result.status, 2);
  assert.equal(result.calls.length, 0);
  bounded(result, 8192);
  assert.ok(result.read.ReadCalls > 1000);
});

test('deterministic growth after the first actual read cannot evade the decoder budget', () => {
  const result = response(file('growing-request', sized(8191)), { mode: 'grow', label: 'growth during read' });
  assert.equal(result.status, 2);
  assert.equal(result.calls.length, 0);
  bounded(result, 8192);
  assert.equal(result.read.InitialLength, 8191);
  assert.ok(result.read.FinalLength > 8191);
  assert.equal(result.read.Grew, true);
});

test('reader errors close the owned handle and fail before JSON or Cargo admission', () => {
  const result = response(file('read-error', '["help"]'), { mode: 'error', label: 'read error' });
  assert.equal(result.status, 2);
  assert.equal(result.calls.length, 0);
  bounded(result, 0);
  assert.match(result.stderr, /Owned injected reader error/);
});

test('real UTF8 BOM request preserves quotes operators exclamation and original child exit', () => {
  const args = ['run', 'odd" & operator', '!LITERAL!', '%LITERAL%', '', 'C:\\path space\\'];
  const result = response(file('real-argv', JSON.stringify(args), 'utf8-bom'), { real: true, env: { MOCK_EXIT: '29' }, label: 'real argv data' });
  assert.equal(result.status, 29, result.stdout + result.stderr);
  assert.deepEqual(result.calls[0].args.slice(10), args.slice(1));
});

test('real empty argv remains the default developer run', () => {
  const result = response(file('empty-argv', '[]'), { real: true, label: 'empty argv' });
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.equal(result.calls[0].args[0], 'run');
  assert.deepEqual(result.calls[0].args.slice(10), []);
});

for (const [name, text] of [['empty-file', ''], ['invalid-json', '[not-json']]) test(`real ${name} fails closed`, () => {
  const result = response(file(name, text), { real: true, label: name });
  assert.equal(result.status, 2);
  assert.equal(result.calls.length, 0);
});

test('missing owned response filename remains a read error', () => {
  const result = response(join(temporary, 'missing.json'), { real: true, label: 'missing filename' });
  assert.equal(result.status, 2);
  assert.equal(result.calls.length, 0);
});

test('exclusive request and fixed-selector checks happen before opening the response', () => {
  const request = file('exclusive-request', '["help"]');
  for (const options of [{ env: { ZABOR_NATIVE_ARGV_JSON: '["help"]' }, label: 'exclusive fields' }, { command: '"native.bat help"', label: 'wrong selector' }]) {
    const result = response(request, options);
    assert.equal(result.status, 2);
    assert.equal(result.calls.length, 0);
    assert.equal(result.read, null);
  }
});

test('prior 25-case and 31/54-case test sources remain byte-inherited, not freshly executed', () => {
  for (const path of ['scripts/native/developer-transport.test.cjs', 'scripts/native/developer-launcher.test.cjs']) {
    const result = spawnSync(cmd, ['/d', '/c', 'git', 'show', `7d47a8ecb0f53d364e7550b45a00121bba517884:${path}`], { cwd: root, encoding: 'utf8', windowsVerbatimArguments: true, timeout: 30000 });
    assert.ifError(result.error);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(readFileSync(join(root, path), 'utf8').replace(/\r\n/g, '\n'), result.stdout.replace(/\r\n/g, '\n'));
    exits.push({ operation: `prior test-source hash ${path}`, exit: result.status, signal: result.signal });
  }
  console.log('RESPONSE_READ_INHERITANCE unchanged prior 25 and 31/54 test sources');
});
