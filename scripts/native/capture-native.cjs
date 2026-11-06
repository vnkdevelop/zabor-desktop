const { execFileSync } = require('node:child_process');
const { existsSync, mkdirSync } = require('node:fs');
const { resolve, join } = require('node:path');

const args = process.argv.slice(2);
const value = (key, fallback) => args.includes(key) ? args[args.indexOf(key) + 1] : fallback;
const output = resolve(value('--output', 'migration-artifacts/native-ui/native'));
const binary = resolve(value('--binary', 'rust/target/debug/zabor-native.exe'));
const scale = value('--scale', '1');
const atMs = value('--at-ms', '550');
const reducedMotion = args.includes('--reduced-motion');
const suffix = reducedMotion ? '-reduced' : '';
if (!['1', '1.25', '1.5', '2'].includes(scale) || !/^\d+$/.test(atMs)) throw new Error('Invalid capture parameters');
if (!existsSync(binary)) throw new Error('Build the native preview first');
mkdirSync(output, { recursive: true });
for (const locale of ['ru', 'en']) for (const state of ['downloading', 'installing', 'error']) {
  const path = join(output, `native-updater-${locale}-${state}-${scale}-${atMs}${suffix}.png`);
  if (existsSync(path)) throw new Error(`Existing native capture preserved: ${path}`);
  execFileSync('cmd.exe', ['/d', '/s', '/c', `""${binary}" --preview updater --state ${state} --locale ${locale} --scale ${scale} --at-ms ${atMs} --snapshot "${path}"${reducedMotion ? ' --reduced-motion' : ''}"`], { stdio: 'inherit', timeout: 30000, windowsVerbatimArguments: true });
}
