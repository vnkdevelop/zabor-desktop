const { spawnSync } = require('node:child_process');
const { createHash } = require('node:crypto');
const { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } = require('node:fs');
const { homedir } = require('node:os');
const { join, resolve } = require('node:path');

const root = resolve(__dirname, '../..');
const args = process.argv.slice(2);
if (args.length !== 2 || args[0] !== '--output') throw new Error('Usage: node scripts/native/verify-audio.cjs --output DIRECTORY');
const output = resolve(root, args[1]);
if (process.platform !== 'win32' || process.arch !== 'x64') throw new Error('Native audio verification requires Windows x64');
if (existsSync(output)) throw new Error('Verification output already exists');
const cargo = join(process.env.CARGO_HOME || join(homedir(), '.cargo'), 'bin/cargo.exe');
const filesIn = directory => readdirSync(join(root, directory), { withFileTypes: true }).flatMap(entry => {
  const path = `${directory}/${entry.name}`;
  return entry.isDirectory() ? filesIn(path) : [path];
});
const sourceFiles = [
  ...filesIn('rust/crates/zabor-native'), ...filesIn('rust/crates/zabor-dsp'),
  ...filesIn('rust/vendor'), ...filesIn('scripts/native'),
  '.cargo/config.toml', 'rust/Cargo.toml', 'rust/Cargo.lock', 'package.json', 'package-lock.json',
  'src/renderer/src/services/mic-pipeline-core.ts', 'src/renderer/src/services/manual-gate-processor.ts',
  'src/renderer/src/services/voice-shaping.ts', 'src/renderer/src/services/webrtc.ts',
  'src/renderer/src/services/silero-vad.worker.ts', 'src/renderer/src/services/webrtc-vad.ts',
  'src/renderer/src/services/fvad-binary.ts', 'src/renderer/public/silero_vad.onnx',
  'src/renderer/public/deepfilternet3/models/DeepFilterNet3_onnx.tar.gz',
  'src/renderer/public/deepfilternet3/pkg/df_bg.wasm',
  'node_modules/@timephy/rnnoise-wasm/dist/RnnoiseProcessor.js',
  'node_modules/@timephy/rnnoise-wasm/dist/generated/rnnoise-sync.js',
];
const digest = path => createHash('sha256').update(readFileSync(resolve(root, path))).digest('hex');
const sources = Object.fromEntries(sourceFiles.map(path => [path, digest(path)]));
mkdirSync(output, { recursive: true });
const checks = [];
let metrics = {};
let nativeTestsPassed = 0;
const run = (name, command, parameters, timeout = 1200000) => {
  const parts = [command, ...parameters];
  if (parts.some(value => /["%\r\n]/.test(value))) throw new Error('Unsupported command argument');
  const log = join(output, `${name}.log`);
  console.log(`Running ${name}`);
  const result = spawnSync('cmd.exe', ['/d', '/v:off', '/s', '/c', `"${parts.map(value => `"${value}"`).join(' ')}"`], {
    cwd: root, encoding: 'utf8', timeout, windowsVerbatimArguments: true,
    stdio: ['ignore', 'pipe', 'pipe'], maxBuffer: 16 * 1024 * 1024,
  });
  const text = `${result.stdout || ''}\n${result.stderr || ''}`;
  writeFileSync(log, text);
  if (result.status !== 0) {
    checks.push({ name, status: 'failed', exitCode: result.status, log: `${name}.log` });
    writeFileSync(join(output, 'failure.json'), JSON.stringify({ automatedChecks: 'failed', nativeClientReady: false, checks, sources }, null, 2));
    console.error(String(result.stderr || result.error?.message || 'Command failed').slice(-12000));
    throw new Error(`${name} failed; see ${log}`);
  }
  const warnings = [...text.matchAll(/^warning:/gm)].length;
  checks.push({ name, status: 'passed', warnings, log: `${name}.log` });
  if (warnings) console.log(`${name}: ${warnings} warning diagnostics retained in the log`);
  if (name === 'test') {
    nativeTestsPassed = [...text.matchAll(/test result: ok\. (\d+) passed/g)].reduce((sum, match) => sum + Number(match[1]), 0);
    const rnnoise = /rnnoise waveform difference mean=([\d.e+-]+) max=([\d.e+-]+)/.exec(text);
    const deepfilter = /deepfilter waveform difference mean=([\d.e+-]+) max=([\d.e+-]+)/.exec(text);
    const silero = /silero probability difference max=([\d.e+-]+)/.exec(text);
    if (!rnnoise || !deepfilter || !silero || nativeTestsPassed === 0) throw new Error('Native test output is missing neural verification evidence');
    metrics = { rnnoiseMean: Number(rnnoise[1]), rnnoiseMax: Number(rnnoise[2]), deepfilterMean: Number(deepfilter[1]), deepfilterMax: Number(deepfilter[2]), sileroMax: Number(silero[1]) };
    console.log(text.split('\n').filter(line => line.startsWith('test result:') || /^(rnnoise|deepfilter|silero) /.test(line)).join('\n'));
  }
};

try {
  run('prepare-onnx', process.execPath, ['scripts/native/prepare-onnx.cjs']);
  run('prepare-libclang', process.execPath, ['scripts/native/prepare-libclang.cjs']);
  run('prepare-rnnoise', process.execPath, ['scripts/native/prepare-rnnoise.cjs']);
  run('download-tests', process.execPath, ['--test', 'scripts/native/download-verified.test.cjs']);
  run('fmt', cargo, ['fmt', '--manifest-path', 'rust/Cargo.toml', '-p', 'zabor-native', '--', '--check']);
  run('clippy', cargo, ['clippy', '--manifest-path', 'rust/Cargo.toml', '-p', 'zabor-native', '--all-targets', '--locked', '-j', '4', '--', '-D', 'warnings']);
  run('test', cargo, ['test', '--manifest-path', 'rust/Cargo.toml', '-p', 'zabor-native', '--locked', '-j', '4', '--', '--nocapture']);
  run('build', cargo, ['build', '--manifest-path', 'rust/Cargo.toml', '-p', 'zabor-native', '--locked', '-j', '4']);
  for (const [path, hash] of Object.entries(sources)) if (digest(path) !== hash) throw new Error(`Source changed during verification: ${path}`);
  const summary = {
    automatedChecks: 'passed', nativeClientReady: false, audioIntegrationAccepted: false,
    scope: 'default-feature native tests, neural reference fixtures, formatting, Clippy and fresh build; no physical devices or production services',
    nativeTestsPassed, neuralReference: metrics, checks, sources, binarySha256: digest('rust/target/debug/zabor-native.exe'),
  };
  writeFileSync(join(output, 'verification.json'), JSON.stringify(summary, null, 2));
  console.log(JSON.stringify({ automatedChecks: 'passed', nativeTestsPassed, nativeClientReady: false, report: join(output, 'verification.json') }));
} catch (error) { console.error(error.message); process.exitCode = 1; }
