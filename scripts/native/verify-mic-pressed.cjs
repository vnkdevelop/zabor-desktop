const { spawnSync } = require('node:child_process');
const { createHash } = require('node:crypto');
const { existsSync, mkdirSync, readFileSync, writeFileSync } = require('node:fs');
const { join, resolve } = require('node:path');

const args = process.argv.slice(2);
const option = name => { const index = args.indexOf(name); if (index < 0) return undefined; const value = args[index + 1]; if (!value || value.startsWith('--')) throw new Error(`Missing ${name}`); args.splice(index, 2); return value; };
const requestedOutput = option('--output');
const requestedReference = option('--reference');
const requestedScale = option('--scale');
if (!requestedOutput || args.length || (requestedScale && !['1', '1.25', '1.5', '2'].includes(requestedScale))) throw new Error('Usage: node scripts/native/verify-mic-pressed.cjs --output DIRECTORY [--reference DIRECTORY] [--scale 1|1.25|1.5|2]');
const output = resolve(requestedOutput);
if (existsSync(output)) throw new Error('Pressed output already exists');
const referenceRoot = requestedReference ? resolve(requestedReference) : join(output, 'react');
const digest = file => createHash('sha256').update(readFileSync(file)).digest('hex');
const binary = resolve('rust/target/debug/zabor-native.exe');
const sources = Object.fromEntries(['scripts/native/verify-mic-pressed.cjs', 'scripts/native/capture-mic-pressed.cjs', 'scripts/native/capture-mic-test.cjs', 'rust/crates/zabor-native/ui/mic-test.slint', 'rust/crates/zabor-native/ui/app.slint', 'rust/crates/zabor-native/src/shell.rs', 'rust/crates/zabor-native/src/shell/mic_focus.rs', 'rust/crates/zabor-native/src/shell/mic_pressed.rs', 'src/renderer/src/components/Settings/MicTestPanel.tsx', 'src/renderer/src/index.css', 'src/renderer/src/lib/anime.ts', 'tailwind.config.js', 'src/renderer/src/locales/ru.json', 'src/renderer/src/locales/en.json'].map(file => [file, digest(file)]));
const binarySha256 = digest(binary);
const captures = [], differences = [], files = {};
mkdirSync(output, { recursive: true });
const freeze = (file, expected) => { const actual = digest(file); if (expected && actual !== expected) throw new Error(`Evidence mismatch: ${file}`); if (files[file] && files[file] !== actual) throw new Error(`Evidence changed: ${file}`); files[file] = actual; };
const run = (name, parts) => {
  if (parts.some(value => /["%\r\n]/.test(value))) throw new Error('Unsupported pressed argument');
  const result = spawnSync('cmd.exe', ['/d', '/v:off', '/s', '/c', `"${parts.map(value => `"${value}"`).join(' ')}"`], { windowsVerbatimArguments: true, encoding: 'utf8', timeout: 120000, maxBuffer: 8 * 1024 * 1024 });
  const text = `${result.stdout || ''}\n${result.stderr || ''}`;
  const log = join(output, `${name}.log`);
  writeFileSync(log, text, { flag: 'wx' });
  freeze(log);
  if (result.status !== 0) throw new Error(`${name} failed: ${result.error?.message || result.status}`);
  return text;
};
try {
  for (const scale of requestedScale ? [requestedScale] : ['1', '1.25', '1.5', '2']) {
    if (!requestedReference) run(`reference-${scale}`, [process.execPath, 'scripts/native/capture-mic-test.cjs', '--output', referenceRoot, '--scale', scale, '--pressed']);
    const referencePath = join(referenceRoot, `mic-pressed-reference-${scale}.json`);
    const reference = JSON.parse(readFileSync(referencePath));
    freeze(referencePath);
    for (const [file, hash] of Object.entries(reference.sources)) if (digest(file) !== hash) throw new Error('Reference sources do not match current sources');
    for (const locale of ['ru', 'en']) {
      const directory = join(output, `native-${locale}-${scale}`);
      const text = run(`native-${locale}-${scale}`, [binary, '--app', '--preview', 'mic-pressed', '--locale', locale, '--scale', scale, '--reduced-motion', '--snapshot', directory]);
      const native = text.split(/\r?\n/).filter(line => line.startsWith('{')).map(line => JSON.parse(line)).flatMap(item => item.nativeMicPressedCapture ? [item.nativeMicPressedCapture] : []);
      const expected = reference.captures.filter(item => item.locale === locale);
      if (native.length !== 148 || expected.length !== 148 || !text.includes('"nativeMicPressedCheck":"completed"')) throw new Error('Pressed evidence incomplete');
      for (const metrics of native) {
        const match = expected.find(item => item.state === metrics.state && item.scenario === metrics.scenario && item.action === metrics.action);
        if (!match || captures.some(item => item.locale === locale && item.scale === scale && item.state === metrics.state && item.scenario === metrics.scenario && item.action === metrics.action)) throw new Error('Missing or duplicate pressed action');
        if (metrics.scale !== Number(scale) || metrics.textScale !== Number(scale) || metrics.physicalDevices !== false || match.metrics.scale !== Number(scale)) throw new Error('Pressed DPI/local-only guard failed');
        const fields = [];
        const ref = match.metrics;
        const recorded = (metrics.scenario === 'space' && ['space-up', 'repeat-up'].includes(metrics.action)) || (metrics.scenario === 'disable' && ['disable', 'space-up'].includes(metrics.action)) || (metrics.scenario === 'rapid' && metrics.action === 'rapid-click') || (metrics.scenario === 'pointer-commit' && metrics.action === 'pointer-up');
        if (ref.recordCalls !== Number(recorded) || (ref.connected && Math.abs(ref.scaleX - (ref.target === 'primary' && ref.active && !ref.disabled ? 0.99 : 1)) > 0.00001)) throw new Error('Reference activation/scale contract failed');
        if (!metrics.panelActive) fields.push('panelActive');
        if (metrics.connected !== ref.connected) fields.push('connected');
        if (metrics.recordCalls !== ref.recordCalls) fields.push('recordCalls');
        if (metrics.primaryDisabled !== ref.primaryDisabled) fields.push('primaryDisabled');
        if ((metrics.focusedControl === metrics.target) !== ref.focused) fields.push('focused');
        if ((metrics.focusedControl === metrics.target && metrics.focusVisible) !== ref.focusVisible) fields.push('focusVisible');
        if (metrics.connected && ref.connected) {
          if (metrics.disabled !== ref.disabled) fields.push('disabled');
          if (Math.abs(metrics.scaleX - ref.scaleX) > 0.00001 || Math.abs(metrics.scaleY - ref.scaleY) > 0.00001) fields.push('scale');
          if (ref.focusVisible && Math.abs(metrics.outlineScale - ref.scaleX) > 0.00001) fields.push('outlineScale');
        }
        if (fields.length) differences.push({ locale, scale, state: metrics.state, scenario: metrics.scenario, action: metrics.action, fields, native: metrics, reference: ref });
        const file = join(directory, metrics.file);
        const referenceFile = join(referenceRoot, match.file);
        freeze(file);
        freeze(referenceFile, match.sha256);
        captures.push({ locale, scale, state: metrics.state, scenario: metrics.scenario, action: metrics.action, file, sha256: files[file], metrics, reference: { ...match, file: referenceFile } });
      }
    }
  }
  for (const [file, hash] of Object.entries({ ...sources, ...files })) if (digest(file) !== hash) throw new Error('Pressed source/evidence changed');
  if (digest(binary) !== binarySha256) throw new Error('Pressed binary changed');
  const passed = !differences.length;
  writeFileSync(join(output, 'verification.json'), JSON.stringify({ automatedChecks: passed ? 'passed' : 'failed', pressedStateAccepted: passed, binarySha256, sources, files, captures, differences, nativeClientReady: false, micTestUiReady: false, focusRasterAccepted: false, broaderModalFocusAccepted: false, scope: 'visible primary/retake settled scale/outline-scale/focus/activation after actual pointer/Space/repeat/Tab transfer, explicit blur/reset, disable and rapid event sequences; memory-only reduced-motion previews and frozen sources/binary/images; not physical window/full-modal/continuous/AA/exact raster acceptance' }, null, 2), { flag: 'wx' });
  console.log(JSON.stringify({ pressedStateChecks: passed ? 'passed' : 'failed', captures: captures.length, differences: differences.length, output }));
  if (!passed) process.exitCode = 1;
} catch (error) {
  writeFileSync(join(output, 'failure.json'), JSON.stringify({ message: error.message, sources, files, binarySha256, captures, differences, pressedStateAccepted: false, nativeClientReady: false }, null, 2), { flag: 'wx' });
  console.error(error.stack);
  process.exitCode = 1;
}
