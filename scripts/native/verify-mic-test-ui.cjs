const { spawnSync } = require('node:child_process');
const { createHash } = require('node:crypto');
const { existsSync, mkdirSync, readFileSync, writeFileSync } = require('node:fs');
const { join, resolve } = require('node:path');

const args = process.argv.slice(2);
if (args.length !== 2 || args[0] !== '--output') throw new Error('Usage: node scripts/native/verify-mic-test-ui.cjs --output DIRECTORY');
const output = resolve(args[1]);
if (existsSync(output)) throw new Error('Verification output already exists');
const binary = resolve('rust/target/debug/zabor-native.exe');
const digest = file => createHash('sha256').update(readFileSync(file)).digest('hex');
const sources = Object.fromEntries([
  'scripts/native/verify-mic-test-ui.cjs', 'scripts/native/capture-mic-test.cjs',
  'rust/crates/zabor-native/src/main.rs', 'rust/crates/zabor-native/src/shell.rs',
  'rust/crates/zabor-native/src/mic_test.rs', 'rust/crates/zabor-native/src/mic_test_ui.rs',
  'rust/crates/zabor-native/src/audio_runtime.rs', 'rust/crates/zabor-native/src/native_audio.rs',
  'rust/crates/zabor-native/ui/app.slint', 'rust/crates/zabor-native/ui/mic-test.slint',
  'rust/crates/zabor-native/ui/audio-settings.slint', 'rust/crates/zabor-native/assets/icons/test-play.svg',
  'rust/crates/zabor-native/assets/icons/test-pause.svg', 'src/renderer/src/components/Settings/MicTestPanel.tsx',
  'src/renderer/src/lib/anime.ts', 'src/renderer/src/index.css', 'tailwind.config.js',
  'src/renderer/src/locales/ru.json', 'src/renderer/src/locales/en.json',
].map(file => [file, digest(file)]));
const binarySha256 = digest(binary);
const captures = [];
const interactionRuns = [];
mkdirSync(output, { recursive: true });
const run = (name, executable, parameters) => {
  const parts = [executable, ...parameters];
  if (parts.some(value => /["%\r\n]/.test(value))) throw new Error('Unsupported command argument');
  const command = `"${parts.map(value => `"${value}"`).join(' ')}"`;
  const result = spawnSync('cmd.exe', ['/d', '/v:off', '/s', '/c', command], { encoding: 'utf8', timeout: 120000, windowsVerbatimArguments: true, maxBuffer: 8 * 1024 * 1024 });
  const text = `${result.stdout || ''}\n${result.stderr || ''}`;
  writeFileSync(join(output, `${name}.log`), text, { flag: 'wx' });
  if (result.status !== 0) throw new Error(`${name} failed: ${result.error?.message || result.status}`);
  return text;
};
try {
  for (const scale of ['1', '1.25', '1.5', '2']) {
    run(`reference-${scale}`, process.execPath, ['scripts/native/capture-mic-test.cjs', '--output', join(output, 'react'), '--scale', scale, '--layout']);
    const reference = JSON.parse(readFileSync(join(output, 'react', `mic-reference-${scale}.json`)));
    for (const locale of ['ru', 'en']) {
      const text = run(`interaction-${locale}-${scale}`, binary, ['--app', '--interaction-check', '--locale', locale, '--scale', scale, '--reduced-motion']);
      if (!text.includes('"nativeMicTestInteractionCheck":"passed"')) throw new Error('Microphone interaction evidence missing');
      interactionRuns.push({ scale, locale, status: 'passed' });
      const hashes = new Set();
      for (const state of ['idle', 'arming', 'recording', 'playing', 'paused', 'silent', 'failed']) {
        const file = `native-mic-${state}-${locale}-${scale}.png`;
        const text = run(`capture-${state}-${locale}-${scale}`, binary, ['--app', '--preview', `mic-${state}`, '--locale', locale, '--scale', scale, '--reduced-motion', '--snapshot', join(output, file)]);
        const metrics = text.split('\n').filter(line => line.startsWith('{')).map(line => JSON.parse(line)).find(item => item.nativeMicTestCapture)?.nativeMicTestCapture;
        const textLayout = text.split('\n').filter(line => line.startsWith('{')).map(line => JSON.parse(line)).find(item => item.nativeMicTextLayout)?.nativeMicTextLayout;
        if (textLayout?.scale !== Number(scale) || textLayout?.windowScale !== Number(scale)) throw new Error('Native text placement DPI differs from the window');
        const expected = ['playing', 'paused', 'silent'].includes(state) ? 'ready' : state === 'failed' ? 'idle' : state;
        const referenceCapture = reference.captures.find(item => item.locale === locale && item.state === state);
        if (!referenceCapture || !Array.isArray(referenceCapture.metrics.layout) || !referenceCapture.metrics.layout.length) throw new Error('Reference baseline evidence missing');
        for (const label of referenceCapture.metrics.layout) {
          const offset = label.kind === 'retake' ? 14 : label.kind === 'issue' ? 10 : 23;
          if (Math.abs(label.baseline - label.box.y - offset) > 1 / 64) throw new Error(`Reference text baseline changed: ${label.kind} ${locale} ${scale}`);
        }
        if (!metrics || metrics.stage !== expected || metrics.physicalDevices !== false || metrics.panelWidth !== 156) throw new Error('Native panel state/geometry guard failed');
        const heightDifference = metrics.panelHeight - referenceCapture.metrics.height;
        if (Math.abs(heightDifference) > 1) throw new Error(`Native/reference panel height differs: ${state} ${locale} ${heightDifference}`);
        const sha256 = digest(join(output, file));
        if (hashes.has(sha256)) throw new Error('Distinct microphone states produced the same image');
        hashes.add(sha256);
        captures.push({ state, locale, scale, file, sha256, metrics, reference: { file: referenceCapture.file, sha256: referenceCapture.sha256, metrics: referenceCapture.metrics }, heightDifference });
      }
    }
  }
  if (digest(binary) !== binarySha256) throw new Error('Binary changed during panel checks');
  for (const [file, hash] of Object.entries(sources)) if (digest(file) !== hash) throw new Error(`Source changed during panel checks: ${file}`);
  writeFileSync(join(output, 'verification.json'), JSON.stringify({ automatedChecks: 'passed', nativeClientReady: false, audioIntegrationAccepted: false, micTestUiReady: false, binarySha256, sources, captures, interactionRuns, scope: 'native/reference RU/EN seven reduced-motion states at four DPI; native pointer/keyboard record/play/pause/scrub/retake and visibility disposal; synthetic audio only; pixel/complete motion/physical/full audio acceptance remains separate' }, null, 2), { flag: 'wx' });
  console.log(JSON.stringify({ automatedChecks: 'passed', captures: captures.length, interactionRuns: interactionRuns.length, output }));
} catch (error) {
  writeFileSync(join(output, 'failure.json'), JSON.stringify({ automatedChecks: 'failed', nativeClientReady: false, message: error.message, binarySha256, sources, captures, interactionRuns }, null, 2), { flag: 'wx' });
  console.error(error.stack);
  process.exitCode = 1;
}
