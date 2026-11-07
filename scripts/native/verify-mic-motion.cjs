const { spawnSync } = require('node:child_process');
const { createHash } = require('node:crypto');
const { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } = require('node:fs');
const { basename, dirname, join, resolve } = require('node:path');
const { spring } = require('animejs');

const args = process.argv.slice(2);
if (args.length !== 4 || args[0] !== '--output' || args[2] !== '--audio-report') throw new Error('Usage: node scripts/native/verify-mic-motion.cjs --output DIRECTORY --audio-report DIRECTORY');
const output = resolve(args[1]);
const audioReport = resolve(args[3]);
if (existsSync(output)) throw new Error('Motion evidence directory already exists');
const binary = resolve('rust/target/debug/zabor-native.exe');
const digest = file => createHash('sha256').update(readFileSync(file)).digest('hex');
const audio = JSON.parse(readFileSync(join(audioReport, 'verification.json')));
const binarySha256 = digest(binary);
if (binarySha256 !== audio.binarySha256) throw new Error('Motion binary does not match the audio verification');
const sources = {
  ...audio.sources,
  ...Object.fromEntries([
    __filename, 'src/renderer/src/components/Settings/MicTestPanel.tsx', 'src/renderer/src/lib/anime.ts',
    require.resolve('animejs'), join(dirname(require.resolve('animejs')), 'easings/spring/index.cjs'),
    join(dirname(require.resolve('animejs')), 'animation/animation.cjs'),
  ].map(file => [file, digest(file)])),
};
for (const [file, hash] of Object.entries(sources)) if (digest(file) !== hash) throw new Error(`Audio source no longer matches: ${file}`);
const line = readFileSync(join(audioReport, 'test.log'), 'utf8').split(/\r?\n/).find(line => line.startsWith('MIC_THUMB_SPRING_REFERENCE='));
if (!line) throw new Error('Full test log is missing native spring evidence');
const samples = JSON.parse(line.slice('MIC_THUMB_SPRING_REFERENCE='.length));
if (samples.curves.length !== 2 || !['press', 'pop'].every(kind => samples.curves.filter(curve => curve.kind === kind).length === 1)) throw new Error('Native spring kinds are incomplete');
const curves = samples.curves.map(curve => {
  const reference = spring(curve.kind === 'pop' ? { stiffness: 120, damping: 9, mass: 1 } : { stiffness: 300, damping: 14, mass: 1 });
  if (reference.settlingDuration !== curve.settlingDuration || curve.samples.length !== curve.settlingDuration + 1) throw new Error('Native/reference spring duration or sample coverage differs');
  if (curve.samples.some((sample, index) => sample.elapsedMs !== index || !Number.isFinite(sample.value) || !Number.isFinite(sample.slintValue))) throw new Error('Native spring millisecond coverage is invalid');
  const maxError = Math.max(...curve.samples.map(sample => Math.abs(sample.value - reference.ease(sample.elapsedMs / curve.settlingDuration))));
  const maxSlintError = Math.max(...curve.samples.map(sample => Math.abs(sample.slintValue - reference.ease(sample.elapsedMs / curve.settlingDuration))));
  if (maxError > 1e-13 || maxSlintError > 1e-6) throw new Error('Native/reference pure spring curve differs');
  return { ...curve, maxError, maxSlintError };
});
mkdirSync(output, { recursive: true });
writeFileSync(join(output, 'curve-comparison.json'), JSON.stringify({ curves }, null, 2), { flag: 'wx' });
const runs = [];
try {
  for (const scale of ['1', '1.25', '1.5', '2']) {
    const referencePath = join(output, `react-${scale}`);
    const referenceParts = [process.execPath, 'scripts/native/capture-mic-test.cjs', '--output', referencePath, '--scale', scale, '--motion'];
    if (referenceParts.some(value => /["%\r\n]/.test(value))) throw new Error('Unsupported reference argument');
    const referenceRun = spawnSync('cmd.exe', ['/d', '/v:off', '/s', '/c', `"${referenceParts.map(value => `"${value}"`).join(' ')}"`], { encoding: 'utf8', windowsVerbatimArguments: true, timeout: 120000, maxBuffer: 8 * 1024 * 1024 });
    writeFileSync(join(output, `reference-${scale}.log`), `${referenceRun.stdout || ''}\n${referenceRun.stderr || ''}`, { flag: 'wx' });
    if (referenceRun.status !== 0) throw new Error(`Live React motion failed: ${scale}`);
    const reference = JSON.parse(readFileSync(join(referencePath, `mic-motion-reference-${scale}.json`)));
    if (reference.actualAudioServiceBundled !== false || reference.captures.length !== 4) throw new Error('Live reference isolation/coverage failed');
    if (reference.coldStartMotionAccepted !== false || reference.captures.some(capture => capture.warmupMs !== 2100 || capture.remountedAfterWarmup !== true)) throw new Error('Reference warm-up scope is missing');
    const referenceImages = readdirSync(referencePath).filter(file => file.endsWith('.png')).map(file => ({ file: join(`react-${scale}`, file), sha256: digest(join(referencePath, file)) }));
    if (referenceImages.length !== 10) throw new Error('Live reference PNG evidence missing');
    for (const locale of ['ru', 'en']) {
      const name = `${locale}-${scale}`;
      const framesPath = join(output, name);
      const parts = [binary, '--app', '--preview', 'mic-motion', '--locale', locale, '--scale', scale, '--snapshot', framesPath];
      if (parts.some(value => /["%\r\n]/.test(value))) throw new Error('Unsupported command argument');
      const result = spawnSync('cmd.exe', ['/d', '/v:off', '/s', '/c', `"${parts.map(value => `"${value}"`).join(' ')}"`], { encoding: 'utf8', windowsVerbatimArguments: true, timeout: 120000, maxBuffer: 8 * 1024 * 1024 });
      const text = `${result.stdout || ''}\n${result.stderr || ''}`;
      writeFileSync(join(output, `${name}.log`), text, { flag: 'wx' });
      if (result.status !== 0) throw new Error(`Native continuous motion failed: ${name}`);
      const events = text.split(/\r?\n/).filter(line => line.startsWith('{')).map(line => JSON.parse(line));
      const status = events.find(event => event.nativeMicMotionCheck)?.nativeMicMotionCheck;
      const frames = events.filter(event => event.nativeMicMotionFrame).map(event => event.nativeMicMotionFrame);
      if (status !== 'passed' || frames.length < 100 || frames.some(frame => frame.physicalDevices !== false || !Number.isFinite(frame.scale))) throw new Error('Native motion evidence missing or invalid');
      for (let phase = 0; phase < 13; phase++) if (!frames.some(frame => frame.phase === phase)) throw new Error('Native motion phase missing');
      const captureName = `${name}-capture`;
      const capturePath = join(output, captureName);
      const captureParts = [binary, '--app', '--preview', 'mic-motion-frames', '--locale', locale, '--scale', scale, '--snapshot', capturePath];
      if (captureParts.some(value => /["%\r\n]/.test(value))) throw new Error('Unsupported frame argument');
      const captureRun = spawnSync('cmd.exe', ['/d', '/v:off', '/s', '/c', `"${captureParts.map(value => `"${value}"`).join(' ')}"`], { encoding: 'utf8', windowsVerbatimArguments: true, timeout: 120000, maxBuffer: 8 * 1024 * 1024 });
      const captureText = `${captureRun.stdout || ''}\n${captureRun.stderr || ''}`;
      writeFileSync(join(output, `${captureName}.log`), captureText, { flag: 'wx' });
      if (captureRun.status !== 0 || !captureText.includes('"nativeMicMotionCheck":"captured"')) throw new Error('Native PNG capture pass failed');
      const images = readdirSync(capturePath).filter(file => file.endsWith('.png')).map(file => ({ file: join(captureName, file), sha256: digest(join(capturePath, file)) }));
      for (let phase = 0; phase < 13; phase++) if (!images.some(image => basename(image.file).startsWith(`phase-${phase}-frame-`))) throw new Error(`Rendered phase ${phase} image missing`);
      const referenceCapture = reference.captures.find(capture => capture.locale === locale && !capture.captureFrames);
      if (!referenceCapture || referenceCapture.phases.length !== 5) throw new Error('Live reference phase evidence missing');
      runs.push({ locale, scale, status, frames, images, reference: referenceCapture, referenceImages });
    }
  }
  if (digest(binary) !== binarySha256) throw new Error('Binary changed during motion verification');
  for (const [file, hash] of Object.entries(sources)) if (digest(file) !== hash) throw new Error(`Source changed during motion verification: ${file}`);
  writeFileSync(join(output, 'verification.json'), JSON.stringify({
    automatedChecks: 'passed', nativeClientReady: false, audioIntegrationAccepted: false, micTestUiReady: false, coldStartMotionAccepted: false,
    binarySha256, sources, curves: curves.map(({ kind, settlingDuration, maxError, maxSlintError }) => ({ kind, settlingDuration, maxError, maxSlintError })), runs,
    scope: 'all millisecond solver samples against installed Anime.js; snapshot-free real native 13-phase timers and unchanged React RAF/DOMMatrix/CDP with explicit 2100ms warm-up/fresh remount at four DPI/RUEN, separately captured PNG replays; cold-start motion, cross-renderer pixel/baseline/focus/card-reveal/full acceptance remain separate',
  }, null, 2), { flag: 'wx' });
  console.log(JSON.stringify({ automatedChecks: 'passed', runs: runs.length, frames: runs.reduce((sum, run) => sum + run.frames.length, 0), nativeClientReady: false, micTestUiReady: false }));
} catch (error) {
  writeFileSync(join(output, 'failure.json'), JSON.stringify({ message: error.message, binarySha256, sources, runs, nativeClientReady: false, micTestUiReady: false }, null, 2), { flag: 'wx' });
  console.error(error.message);
  process.exitCode = 1;
}
