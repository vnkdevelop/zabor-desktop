const { spawnSync } = require('node:child_process');
const { createHash } = require('node:crypto');
const { existsSync, mkdirSync, readFileSync, writeFileSync } = require('node:fs');
const { join, resolve } = require('node:path');

const args = process.argv.slice(2);
if (args.length !== 2 || args[0] !== '--output') throw new Error('Usage: node scripts/native/verify-audio-ui.cjs --output DIRECTORY');
if (process.platform !== 'win32' || process.arch !== 'x64') throw new Error('Windows x64 is required');
const output = resolve(args[1]);
if (existsSync(output)) throw new Error('Verification output already exists');
const binary = resolve('rust/target/debug/zabor-native.exe');
const digest = file => createHash('sha256').update(readFileSync(file)).digest('hex');
const sources = Object.fromEntries([
  'scripts/native/verify-audio-ui.cjs', 'rust/crates/zabor-native/src/main.rs',
  'rust/crates/zabor-native/src/shell.rs', 'rust/crates/zabor-native/src/audio_settings.rs',
  'rust/crates/zabor-native/src/audio_preferences.rs', 'rust/crates/zabor-native/ui/app.slint',
  'rust/crates/zabor-native/ui/audio-settings.slint', 'rust/crates/zabor-native/ui/controls.slint',
  'src/renderer/src/locales/ru.json', 'src/renderer/src/locales/en.json',
].map(file => [file, digest(file)]));
const binarySha256 = digest(binary);
const captures = [];
const interactionRuns = [];
mkdirSync(output, { recursive: true });
const run = (name, parameters) => {
  const parts = [binary, ...parameters];
  if (parts.some(value => /["%\r\n]/.test(value))) throw new Error('Unsupported command argument');
  const command = `"${parts.map(value => `"${value}"`).join(' ')}"`;
  const result = spawnSync('cmd.exe', ['/d', '/v:off', '/s', '/c', command], {
    encoding: 'utf8', timeout: 30000, windowsVerbatimArguments: true, maxBuffer: 4 * 1024 * 1024,
  });
  const text = `${result.stdout || ''}\n${result.stderr || ''}`;
  writeFileSync(join(output, `${name}.log`), text, { flag: 'wx' });
  if (result.status !== 0) throw new Error(`${name} failed: ${result.error?.message || result.status}`);
  return text;
};
try {
  for (const scale of ['1', '1.25', '1.5', '2']) for (const locale of ['ru', 'en']) {
    const text = run(`interaction-${locale}-${scale}`, ['--app', '--interaction-check', '--locale', locale, '--scale', scale, '--reduced-motion']);
    if (!text.includes('"nativeAudioInteractionCheck":"passed"')) throw new Error('Audio interaction evidence is missing');
    interactionRuns.push({ scale, locale, status: 'passed' });
    let defaultHash;
    for (const state of ['audio', 'audio-manual', 'audio-raw', 'audio-ultra', 'audio-deepfilter', 'audio-general', 'audio-privacy', 'audio-volume']) {
      const file = `${state}-${locale}-${scale}.png`;
      run(`${state}-${locale}-${scale}`, ['--app', '--preview', state, '--locale', locale, '--scale', scale, '--reduced-motion', '--snapshot', join(output, file)]);
      const sha256 = digest(join(output, file));
      if (state === 'audio') defaultHash = sha256;
      if (['audio-manual', 'audio-raw', 'audio-ultra', 'audio-deepfilter'].includes(state) && sha256 === defaultHash) throw new Error(`${state} capture did not expose its distinct state`);
      captures.push({ state, locale, scale, file, sha256 });
    }
  }
  if (digest(binary) !== binarySha256) throw new Error('Binary changed during UI checks');
  for (const [file, hash] of Object.entries(sources)) if (digest(file) !== hash) throw new Error(`Source changed during UI checks: ${file}`);
  writeFileSync(join(output, 'verification.json'), JSON.stringify({
    automatedChecks: 'passed', nativeClientReady: false, binarySha256, sources, captures, interactionRuns,
    scope: 'native mouse/keyboard sliders, disabled heavy controls, close/volume lifetime and RU/EN/four-DPI captures; visual parity, popup focus and full UI acceptance remain separate',
  }, null, 2), { flag: 'wx' });
  console.log(JSON.stringify({ automatedChecks: 'passed', captures: captures.length, interactionRuns: interactionRuns.length, output }));
} catch (error) {
  writeFileSync(join(output, 'failure.json'), JSON.stringify({ automatedChecks: 'failed', nativeClientReady: false, message: error.message, binarySha256, sources, captures, interactionRuns }, null, 2));
  console.error(error.message);
  process.exitCode = 1;
}
