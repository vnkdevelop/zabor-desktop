const { spawnSync } = require('node:child_process');
const { createHash } = require('node:crypto');
const { existsSync, mkdirSync, readFileSync, writeFileSync } = require('node:fs');
const { join, resolve } = require('node:path');

const args = process.argv.slice(2);
if (args.length !== 2 || args[0] !== '--output') throw new Error('Usage: node scripts/native/verify-mic-focus.cjs --output DIRECTORY');
const output = resolve(args[1]);
if (existsSync(output)) throw new Error('Focus verification directory already exists');
const binary = resolve('rust/target/debug/zabor-native.exe');
const digest = file => createHash('sha256').update(readFileSync(file)).digest('hex');
const sources = Object.fromEntries([
  'scripts/native/verify-mic-focus.cjs', 'scripts/native/capture-mic-focus.cjs', 'scripts/native/capture-mic-test.cjs',
  'rust/crates/zabor-native/src/shell.rs', 'rust/crates/zabor-native/src/shell/mic_focus.rs',
  'rust/crates/zabor-native/ui/app.slint', 'rust/crates/zabor-native/ui/mic-test.slint',
  'src/renderer/src/components/Settings/MicTestPanel.tsx', 'src/renderer/src/index.css', 'src/renderer/src/lib/anime.ts',
  'tailwind.config.js', 'src/renderer/src/locales/ru.json', 'src/renderer/src/locales/en.json',
].map(file => [file, digest(file)]));
const binarySha256 = digest(binary);
const captures = [];
const differences = [];
mkdirSync(output, { recursive: true });
const run = (name, parts) => {
  if (parts.some(value => /["%\r\n]/.test(value))) throw new Error('Unsupported focus verification argument');
  const result = spawnSync('cmd.exe', ['/d', '/v:off', '/s', '/c', `"${parts.map(value => `"${value}"`).join(' ')}"`], { windowsVerbatimArguments: true, encoding: 'utf8', timeout: 120000, maxBuffer: 8 * 1024 * 1024 });
  const text = `${result.stdout || ''}\n${result.stderr || ''}`;
  writeFileSync(join(output, `${name}.log`), text, { flag: 'wx' });
  if (result.status !== 0) throw new Error(`${name} failed: ${result.error?.message || result.status}`);
  return text;
};
try {
  for (const scale of ['1', '1.25', '1.5', '2']) {
    run(`reference-${scale}`, [process.execPath, 'scripts/native/capture-mic-test.cjs', '--output', join(output, 'react'), '--scale', scale, '--focus']);
    const reference = JSON.parse(readFileSync(join(output, 'react', `mic-focus-reference-${scale}.json`)));
    for (const locale of ['ru', 'en']) {
      const directory = `native-${locale}-${scale}`;
      const text = run(`native-${locale}-${scale}`, [binary, '--app', '--preview', 'mic-focus', '--locale', locale, '--scale', scale, '--reduced-motion', '--snapshot', join(output, directory)]);
      const native = text.split(/\r?\n/).filter(line => line.startsWith('{')).map(line => JSON.parse(line)).flatMap(item => item.nativeMicFocusCapture ? [item.nativeMicFocusCapture] : []);
      const expected = reference.captures.filter(capture => capture.locale === locale);
      if (native.length !== expected.length || !text.includes('"nativeMicFocusCheck":"completed"')) throw new Error('Native focus evidence is incomplete');
      for (const capture of native) {
        const match = expected.find(item => item.state === capture.state && item.action === capture.action);
        if (!match || captures.some(item => item.locale === locale && item.scale === scale && item.state === capture.state && item.action === capture.action)) throw new Error('Missing or duplicate focus action');
        if (capture.scale !== Number(scale) || capture.textScale !== Number(scale) || capture.physicalDevices !== false) throw new Error('Native focus DPI/local-only guard failed');
        if (digest(join(output, 'react', match.file)) !== match.sha256) throw new Error('Reference focus image hash mismatch');
        const visible = !capture.hidden && capture.focusVisible;
        const referenceVisible = !match.metrics.hidden && match.metrics.focusVisible;
        const fields = [];
        if (!capture.hidden && capture.focusedControl !== match.metrics.focusedControl) fields.push('focusedControl');
        if (visible !== referenceVisible) fields.push('focusVisible');
        if (capture.hidden !== match.metrics.hidden) fields.push('hidden');
        if (capture.playing !== match.metrics.playing) fields.push('playing');
        if (Math.abs(capture.position - match.metrics.position) > 0.001) fields.push('position');
        if (fields.length) differences.push({ locale, scale, state: capture.state, action: capture.action, fields, native: capture, reference: match.metrics });
        captures.push({ locale, scale, state: capture.state, action: capture.action, file: `${directory}/${capture.file}`, sha256: digest(join(output, directory, capture.file)), metrics: capture, reference: match });
      }
    }
  }
  if (digest(binary) !== binarySha256) throw new Error('Binary changed during focus verification');
  for (const [file, hash] of Object.entries(sources)) if (digest(file) !== hash) throw new Error('Source changed during focus verification');
  writeFileSync(join(output, 'verification.json'), JSON.stringify({ automatedChecks: differences.length ? 'failed' : 'passed', nativeClientReady: false, micTestUiReady: false, focusRasterAccepted: false, broaderModalFocusAccepted: false, binarySha256, sources, captures, differences, scope: 'visible-panel focus identity/outline modality, actual forward/reverse Tab, pointer down/cancel and Space activation, disabled/hidden exclusions at four DPI/RUEN; synthetic local-only audio, cropped static images; hidden DOM focus retention, OS accessibility and exact raster/full-modal focus remain separate' }, null, 2), { flag: 'wx' });
  console.log(JSON.stringify({ focusSemantics: differences.length ? 'failed' : 'passed', captures: captures.length, differences: differences.length, output }));
  if (differences.length) process.exitCode = 1;
} catch (error) {
  writeFileSync(join(output, 'failure.json'), JSON.stringify({ message: error.message, binarySha256, sources, captures, differences, nativeClientReady: false, focusRasterAccepted: false }, null, 2), { flag: 'wx' });
  console.error(error.stack);
  process.exitCode = 1;
}
