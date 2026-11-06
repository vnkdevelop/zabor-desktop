const { execFileSync } = require('node:child_process');
const { createHash } = require('node:crypto');
const { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } = require('node:fs');
const { homedir } = require('node:os');
const { join, resolve, relative } = require('node:path');

const root = resolve(__dirname, '../..');
process.chdir(root);
if (process.platform !== 'win32') throw new Error('The native parity harness currently requires Windows');
const args = process.argv.slice(2);
if (args.length && (args.length !== 2 || args[0] !== '--output')) throw new Error('Usage: node scripts/native/verify.cjs [--output DIRECTORY]');
const output = resolve(args[1] || `migration-artifacts/native-ui/verified-${new Date().toISOString().replace(/[:.]/g, '-')}`);
if (existsSync(output)) throw new Error('Verification output already exists');
const cargo = join(process.env.CARGO_HOME || join(homedir(), '.cargo'), 'bin/cargo.exe');
const binary = resolve('rust/target/debug/zabor-native.exe');
const run = (command, parameters, timeout = 120000) => {
  const parts = [command, ...parameters];
  if (parts.some(value => /["%\r\n]/.test(value))) throw new Error('Unsupported command argument');
  return execFileSync('cmd.exe', ['/d', '/v:off', '/s', '/c', `"${parts.map(value => `"${value}"`).join(' ')}"`], { stdio: 'inherit', timeout, windowsVerbatimArguments: true });
};
const filesIn = directory => readdirSync(directory, { withFileTypes: true }).flatMap(entry => {
  const path = join(directory, entry.name);
  return entry.isDirectory() ? filesIn(path) : [path];
});
const sourceFiles = [...filesIn('rust/crates/zabor-native'), ...filesIn('scripts/native'), 'rust/Cargo.toml', 'rust/Cargo.lock', 'src/renderer/src/components/UpdaterWindow.tsx', 'src/renderer/src/index.css', 'tailwind.config.js', 'src/renderer/src/locales/ru.json', 'src/renderer/src/locales/en.json'];
const digest = path => createHash('sha256').update(readFileSync(path)).digest('hex');
const sources = Object.fromEntries(sourceFiles.map(path => [path.replace(/\\/g, '/'), digest(path)]));
for (const command of [
  ['fmt', '--manifest-path', 'rust/Cargo.toml', '-p', 'zabor-native', '--', '--check'],
  ['clippy', '--manifest-path', 'rust/Cargo.toml', '-p', 'zabor-native', '--all-targets', '--locked', '--', '-D', 'warnings'],
  ['test', '--manifest-path', 'rust/Cargo.toml', '-p', 'zabor-native', '--locked'],
  ['build', '--manifest-path', 'rust/Cargo.toml', '-p', 'zabor-native', '--locked'],
]) run(existsSync(cargo) ? cargo : 'cargo', command, 600000);
mkdirSync(output, { recursive: true });
const reference = join(output, 'react');
const native = join(output, 'native');
const cases = [
  ...['1', '1.25', '1.5', '2'].map(scale => ({ scale, atMs: '550', reducedMotion: false })),
  ...['0', '275', '1100', '1399'].map(atMs => ({ scale: '1', atMs, reducedMotion: false })),
  { scale: '1', atMs: '550', reducedMotion: true },
];
for (const item of cases) {
  const flags = ['--scale', item.scale, '--at-ms', item.atMs, ...(item.reducedMotion ? ['--reduced-motion'] : [])];
  run(process.execPath, ['scripts/native/capture-react.cjs', '--output', reference, ...flags]);
  run(process.execPath, ['scripts/native/capture-native.cjs', '--output', native, '--binary', binary, ...flags], 240000);
}
for (const scale of ['1', '1.25', '1.5', '2']) for (const locale of ['ru', 'en']) {
  run(binary, ['--preview', 'updater', '--interaction-check', '--scale', scale, '--locale', locale], 30000);
}
run('python', ['scripts/native/compare-ui.py', '--reference', reference, '--native', native, '--output', join(output, 'comparison')]);
for (const [path, hash] of Object.entries(sources)) if (digest(path) !== hash) throw new Error(`Source changed during verification: ${path}`);
const images = Object.fromEntries([...filesIn(reference), ...filesIn(native)].map(path => [relative(output, path).replace(/\\/g, '/'), digest(path)]));
const summary = { automatedChecks: 'passed', visualAcceptance: 'requires review of comparison images; not a full-client parity claim', nativeClientReady: false, pairs: cases.length * 6, interactionRuns: 8, cases, sources, binarySha256: digest(binary), artifacts: images };
writeFileSync(join(output, 'verification.json'), JSON.stringify(summary, null, 2));
console.log(JSON.stringify({ automatedChecks: summary.automatedChecks, pairs: summary.pairs, interactionRuns: summary.interactionRuns, report: join(output, 'verification.json') }));
