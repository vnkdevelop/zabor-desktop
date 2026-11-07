const { spawnSync } = require('node:child_process');
const { createHash } = require('node:crypto');
const { existsSync, mkdirSync, readFileSync, writeFileSync } = require('node:fs');
const { join, resolve } = require('node:path');
const { homedir } = require('node:os');

const args = process.argv.slice(2);
if (args.length !== 2 || args[0] !== '--output') throw new Error('Usage: node scripts/native/verify-mic-icons.cjs --output DIRECTORY');
const output = resolve(args[1]);
if (existsSync(output)) throw new Error('Icon regression output already exists');
const read = path => JSON.parse(readFileSync(path));
const digest = path => createHash('sha256').update(readFileSync(path)).digest('hex');
const previous = {
  audio: 'migration-artifacts/native-audio/mic-focus-final-2026-11-07-verified',
  panel: 'migration-artifacts/native-ui/mic-focus-final-2026-11-07',
  settings: 'migration-artifacts/native-ui/mic-focus-final-2026-11-07-settings',
  auth: 'migration-artifacts/native-auth/mic-focus-final-2026-11-07',
};
const priorAudio = read(join(previous.audio, 'verification.json'));
const sourceFiles = [...Object.keys(priorAudio.sources), 'scripts/native/measure-mic-icons.py', 'scripts/native/verify-mic-icons.cjs'];
if (new Set(sourceFiles).size !== sourceFiles.length) throw new Error('Duplicate source inventory');
const sources = Object.fromEntries(sourceFiles.map(path => [path, digest(path)]));
const priorPanel = read(join(previous.panel, 'verification.json'));
const referenceSources = Object.fromEntries(Object.keys(priorPanel.sources).filter(path => !sources[path]).map(path => [path, digest(path)]));
const paths = Object.fromEntries(['audio-default', 'audio-no-neural', 'panel', 'baseline', 'settings', 'auth', 'motion', 'focus', 'focus-review', 'icon-review'].map(name => [name, join(output, name)]));
const cargo = join(process.env.CARGO_HOME || join(homedir(), '.cargo'), 'bin/cargo.exe');
const binary = resolve('rust/target/debug/zabor-native.exe');
const checks = [];
const files = {};
mkdirSync(output, { recursive: true });
const checkSources = () => {
  for (const [path, value] of Object.entries({ ...sources, ...referenceSources })) if (digest(path) !== value) throw new Error(`Source changed during icon regression: ${path}`);
};
const run = (name, parts) => {
  checkSources();
  if (parts.some(value => /["%\r\n]/.test(value))) throw new Error('Unsupported regression command argument');
  console.log(`Running ${name}`);
  const result = spawnSync('cmd.exe', ['/d', '/v:off', '/s', '/c', `"${parts.map(value => `"${value}"`).join(' ')}"`], { windowsVerbatimArguments: true, encoding: 'utf8', timeout: 1800000, maxBuffer: 16 * 1024 * 1024 });
  const path = join(output, `${name}.log`);
  const text = `${result.stdout || ''}\n${result.stderr || ''}`;
  writeFileSync(path, text, { flag: 'wx' });
  files[path] = digest(path);
  checks.push({ name, status: result.status === 0 ? 'passed' : 'failed', exitCode: result.status, log: `${name}.log` });
  if (result.status !== 0) throw new Error(`${name} failed: ${result.error?.message || result.status}`);
  checkSources();
  return text;
};
const freezeFile = (path, expected) => {
  const actual = digest(path);
  if (expected && actual !== expected) throw new Error(`Evidence hash mismatch: ${path}`);
  if (files[path] && files[path] !== actual) throw new Error(`Evidence changed: ${path}`);
  files[path] = actual;
};
try {
  for (const name of ['capture-mic-test.cjs', 'verify-mic-icons.cjs']) run(`${name}-syntax`, [process.execPath, '--check', `scripts/native/${name}`]);
  run('icon-measure-syntax', ['python', '-B', '-c', "import ast;from pathlib import Path;ast.parse(Path('scripts/native/measure-mic-icons.py').read_text())"]);
  run('no-neural-clippy', [cargo, 'clippy', '--manifest-path', 'rust/Cargo.toml', '-p', 'zabor-native', '--locked', '--no-default-features', '--features', 'native-media', '--all-targets', '-j', '4', '--', '-D', 'warnings']);
  const tests = run('no-neural-test', [cargo, 'test', '--manifest-path', 'rust/Cargo.toml', '-p', 'zabor-native', '--locked', '--no-default-features', '--features', 'native-media', '-j', '4', '--', '--nocapture']);
  const nativeTestsPassed = [...tests.matchAll(/test result: ok\. (\d+) passed/g)].reduce((sum, match) => sum + Number(match[1]), 0);
  if (nativeTestsPassed !== 152) throw new Error('Neural-disabled test count changed');
  mkdirSync(paths['audio-no-neural']);
  writeFileSync(join(paths['audio-no-neural'], 'verification.json'), JSON.stringify({ automatedChecks: 'passed', nativeTestsPassed, sources, nativeClientReady: false, audioIntegrationAccepted: false, scope: 'local-only native-media neural-disabled tests and Clippy; no physical devices or production services' }, null, 2), { flag: 'wx' });
  run('audio-default', [process.execPath, 'scripts/native/verify-audio.cjs', '--output', paths['audio-default']]);
  const audio = read(join(paths['audio-default'], 'verification.json'));
  if (audio.nativeTestsPassed !== 162 || Object.keys(audio.sources).length !== sourceFiles.length || sourceFiles.some(path => audio.sources[path] !== sources[path])) throw new Error('Default test/source inventory mismatch');
  const binarySha256 = audio.binarySha256;
  run('panel', [process.execPath, 'scripts/native/verify-mic-test-ui.cjs', '--output', paths.panel]);
  run('icon-review', ['python', '-B', 'scripts/native/measure-mic-icons.py', '--captures', paths.panel, '--previous', previous.panel, '--output', paths['icon-review']]);
  run('baseline', ['python', '-B', 'scripts/native/measure-mic-baseline.py', '--captures', paths.panel, '--previous', previous.panel, '--output', paths.baseline]);
  run('settings', [process.execPath, 'scripts/native/verify-audio-ui.cjs', '--output', paths.settings]);
  run('auth', [process.execPath, 'scripts/native/verify-auth.cjs', '--output', paths.auth]);
  run('motion', [process.execPath, 'scripts/native/verify-mic-motion.cjs', '--output', paths.motion, '--audio-report', paths['audio-default']]);
  run('focus', [process.execPath, 'scripts/native/verify-mic-focus.cjs', '--output', paths.focus]);
  run('focus-review', ['python', '-B', 'scripts/native/measure-mic-focus.py', '--captures', paths.focus, '--output', paths['focus-review']]);
  const reportPaths = Object.fromEntries(Object.entries(paths).map(([name, path]) => [name, join(path, ['baseline', 'focus-review', 'icon-review'].includes(name) ? 'comparison.json' : 'verification.json')]));
  const reports = Object.fromEntries(Object.entries(reportPaths).map(([name, path]) => { freezeFile(path); return [name, read(path)]; }));
  for (const [name, report] of Object.entries(reports)) {
    if (name !== 'audio-no-neural' && (report.binarySha256 || report.nativeBinarySha256) !== binarySha256) throw new Error(`Binary mismatch: ${name}`);
    for (const [path, value] of Object.entries(report.sources || {})) if (digest(path) !== value) throw new Error(`Report source mismatch: ${name}/${path}`);
    for (const [path, value] of Object.entries(report.files || {})) freezeFile(path, value);
  }
  for (const name of ['panel', 'settings', 'auth']) for (const capture of reports[name].captures) {
    freezeFile(join(paths[name], capture.file), capture.sha256);
    if (capture.reference) freezeFile(capture.reference.file, capture.reference.sha256);
  }
  for (const capture of reports.focus.captures) {
    freezeFile(join(paths.focus, capture.file), capture.sha256);
    freezeFile(join(paths.focus, 'react', capture.reference.file), capture.reference.sha256);
  }
  for (const run of reports.motion.runs) for (const image of [...run.images, ...run.referenceImages]) freezeFile(join(paths.motion, image.file), image.sha256);
  const changes = (current, prior) => current.captures.filter(capture => prior.captures.find(old => old.file === capture.file)?.sha256 !== capture.sha256).map(capture => capture.file);
  const settingsChanges = changes(reports.settings, read(join(previous.settings, 'verification.json')));
  const authChanges = changes(reports.auth, read(join(previous.auth, 'verification.json')));
  const neuralChanges = Object.keys(priorAudio.neuralReference).filter(key => audio.neuralReference[key] !== priorAudio.neuralReference[key]);
  const icons = reports['icon-review'];
  const outlines = reports['focus-review'];
  if (settingsChanges.length || authChanges.length || neuralChanges.length || icons.iconImprovementChecks !== 'passed' || icons.pauseContourAccepted !== true || icons.playContourAccepted !== false || icons.measurements.length !== 24 || icons.measurements.filter(item => item.check === 'pause-improvement').length !== 16 || icons.measurements.filter(item => item.check === 'play-unchanged').length !== 8 || icons.unexpectedChanges.length || icons.referenceChanges.length || reports.baseline.textRowChecks !== 'passed' || reports.focus.automatedChecks !== 'passed' || reports.focus.captures.length !== 800 || reports.focus.differences.length || outlines.outlineBoundsChecks !== 'passed' || outlines.measurements.length !== 448 || reports.motion.coldStartMotionAccepted !== false) throw new Error('Icon/isolation/regression acceptance guard failed');
  for (const path of sourceFiles) if (reports.motion.sources[path] !== sources[path]) throw new Error('Motion source inventory differs');
  checkSources();
  for (const [path, value] of Object.entries(files)) if (digest(path) !== value) throw new Error(`Final evidence hash mismatch: ${path}`);
  if (digest(binary) !== binarySha256) throw new Error('Final executable changed');
  const summary = { automatedChecks: 'passed', binarySha256, sources, referenceSources, files, checks, defaultTests: audio.nativeTestsPassed, noNeuralTests: nativeTestsPassed, iconMeasurements: icons.measurements.length, pauseContourAccepted: true, playContourAccepted: false, panelCaptures: reports.panel.captures.length, settingsCaptures: reports.settings.captures.length, authCaptures: reports.auth.captures.length, panelChanges: changes(reports.panel, priorPanel), settingsChanges, authChanges, neuralChanges, focusPairs: reports.focus.captures.length, outlineMeasurements: outlines.measurements.length, nativeMotionFrames: reports.motion.runs.reduce((sum, run) => sum + run.frames.length, 0), nativeClientReady: false, audioIntegrationAccepted: false, micTestUiReady: false, focusRasterAccepted: false, broaderModalFocusAccepted: false, coldStartMotionAccepted: false, scope: 'exact React DOM/native SVG pause contour and strictly improved16pause pixel measurements;8play images and asset unchanged, play contour mismatch remains unaccepted; isolated unchanged non-icon panel/settings/auth/reference/neural evidence and existing local-only tests/caption/focus/annulus/warmed motion regressions; not exact raster/pressed keyboard/full-modal/cold/physical/full-client acceptance' };
  writeFileSync(join(output, 'verification.json'), JSON.stringify(summary, null, 2), { flag: 'wx' });
  console.log(JSON.stringify({ automatedChecks: 'passed', defaultTests: summary.defaultTests, noNeuralTests: summary.noNeuralTests, sources: sourceFiles.length, iconMeasurements: summary.iconMeasurements, focusPairs: summary.focusPairs, outlineMeasurements: summary.outlineMeasurements, binarySha256, output }));
} catch (error) {
  writeFileSync(join(output, 'failure.json'), JSON.stringify({ automatedChecks: 'failed', message: error.message, sources, referenceSources, checks, nativeClientReady: false, micTestUiReady: false, focusRasterAccepted: false }, null, 2), { flag: 'wx' });
  console.error(error.stack);
  process.exitCode = 1;
}
