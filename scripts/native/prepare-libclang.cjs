const { createHash } = require('node:crypto');
const { execFileSync } = require('node:child_process');
const { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, renameSync, rmSync, writeFileSync } = require('node:fs');
const { resolve, join, relative } = require('node:path');
const { downloadVerified } = require('./download-verified.cjs');

const version = '18.1.1';
const archiveHash = '4dd2d3b82fab35e2bf9ca717d7b63ac990a3519c7e312f19fa8e86dcc712f7fb';
const pinnedFiles = {
  'licenses/LICENSE-0.txt': '3b6226c32e168c83b891d8d6f0d3c29c2116dc3ef93dc93c307b54f279ecf383',
  'native/libclang.dll': 'b13ec96d9a036d6c6c36dd09e00415ead7acc7fcfd40e721bf33ea86b3490f9d'
};
const url = 'https://files.pythonhosted.org/packages/0b/2d/3f480b1e1d31eb3d6de5e3ef641954e5c67430d5ac93b7fa7e07589576c7/libclang-18.1.1-py2.py3-none-win_amd64.whl';
const base = resolve(__dirname, '../../rust/target');
const output = join(base, `libclang-win-x64-${version}`);
const digest = path => createHash('sha256').update(readFileSync(path)).digest('hex');
const walk = directory => readdirSync(directory, { withFileTypes: true }).flatMap(entry => entry.isDirectory() ? walk(join(directory, entry.name)) : [join(directory, entry.name)]);

async function main() {
  if (process.platform !== 'win32' || process.arch !== 'x64') throw new Error('libclang preparation requires Windows x64');
  if (existsSync(output)) {
    const manifest = JSON.parse(readFileSync(join(output, 'manifest.json'), 'utf8'));
    if (manifest.version !== version || manifest.archiveHash !== archiveHash || !manifest.files || Object.keys(manifest.files).length !== Object.keys(pinnedFiles).length) throw new Error('libclang cache does not match the pinned wheel');
    for (const [name, hash] of Object.entries(pinnedFiles)) if (manifest.files[name] !== hash || digest(join(output, name)) !== hash) throw new Error('libclang cache verification failed');
    return;
  }
  mkdirSync(base, { recursive: true });
  const temporary = mkdtempSync(join(base, '.libclang-prepare-'));
  try {
    const archive = join(temporary, 'clang.whl');
    const packagePath = join(temporary, 'package');
    const ready = join(temporary, 'ready');
    mkdirSync(packagePath); mkdirSync(ready); mkdirSync(join(ready, 'native')); mkdirSync(join(ready, 'licenses'));
    await downloadVerified({ url, path: archive, sha256: archiveHash, length: 26415083, timeout: 300000 });
    const args = ['tar.exe', '-xf', archive, '-C', packagePath];
    if (args.some(value => /["%\r\n]/.test(value))) throw new Error('Unsupported libclang extraction path');
    execFileSync('cmd.exe', ['/d', '/v:off', '/s', '/c', `"${args.map(value => `"${value}"`).join(' ')}"`], { windowsVerbatimArguments: true, stdio: 'inherit', timeout: 60000 });
    const files = walk(packagePath);
    const libraries = files.filter(path => /[\\/]libclang\.dll$/i.test(path));
    const licenses = files.filter(path => /[\\/]LICENSE(\.[a-z]+)?$/i.test(path));
    if (libraries.length !== 1 || licenses.length === 0) throw new Error('libclang wheel is missing the library or licence');
    copyFileSync(libraries[0], join(ready, 'native/libclang.dll'));
    for (const [index, path] of licenses.entries()) copyFileSync(path, join(ready, 'licenses', `LICENSE-${index}.txt`));
    const hashes = Object.fromEntries(walk(ready).map(path => [relative(ready, path).replace(/\\/g, '/'), digest(path)]));
    if (Object.keys(hashes).length !== Object.keys(pinnedFiles).length || Object.entries(pinnedFiles).some(([name, hash]) => hashes[name] !== hash)) throw new Error('Extracted libclang files differ from the pinned wheel');
    writeFileSync(join(ready, 'manifest.json'), JSON.stringify({ version, archiveHash, files: hashes }, null, 2));
    renameSync(ready, output);
    console.log(`Prepared local libclang ${version} for native bindings`);
  } finally { rmSync(temporary, { recursive: true, force: true }); }
}

main().catch(error => { console.error(error.message); process.exitCode = 1; });
