const { execFileSync } = require('node:child_process');
const { createHash } = require('node:crypto');
const { existsSync, mkdirSync, readFileSync, writeFileSync } = require('node:fs');
const { resolve, join } = require('node:path');
const args = process.argv.slice(2);
if (args.length !== 2 || args[0] !== '--output') throw new Error('Usage: node scripts/native/verify-auth.cjs --output DIRECTORY');
const output = resolve(args[1]);
if (existsSync(output)) throw new Error('Output already exists');
const binary = resolve('rust/target/debug/zabor-native.exe');
if (!existsSync(binary)) throw new Error('Build the native client first');
const run = (command, parameters) => {
  const parts = [command, ...parameters];
  if (parts.some(value => /["%\r\n]/.test(value))) throw new Error('Unsupported command argument');
  execFileSync('cmd.exe', ['/d', '/v:off', '/s', '/c', `"${parts.map(value => `"${value}"`).join(' ')}"`], { stdio: 'inherit', timeout: 120000, windowsVerbatimArguments: true });
};
mkdirSync(output, { recursive: true });
const captures = [];
for (const scale of ['1', '1.25', '1.5', '2']) {
  run(process.execPath, ['scripts/native/capture-auth.cjs', '--output', join(output, 'react'), '--scale', scale]);
  for (const locale of ['ru', 'en']) {
    run(binary, ['--app', '--interaction-check', '--scale', scale, '--locale', locale]);
    for (const state of ['login', 'confirm', 'setup']) {
      const file = `native-auth-${locale}-${state}-${scale}.png`;
      run(binary, ['--app', '--preview', state, '--scale', scale, '--locale', locale, '--snapshot', join(output, file)]);
      captures.push({ file, sha256: createHash('sha256').update(readFileSync(join(output, file))).digest('hex') });
    }
  }
}
writeFileSync(join(output, 'verification.json'), JSON.stringify({ nativeBinarySha256: createHash('sha256').update(readFileSync(binary)).digest('hex'), captures, interactionRuns: 8, status: 'capture and interaction checks passed; visual acceptance is separate' }, null, 2));
console.log(JSON.stringify({ captures: captures.length, interactionRuns: 8, output }));
