const { createHash } = require('node:crypto');
const { execFileSync } = require('node:child_process');
const { createWriteStream, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, copyFileSync, renameSync, rmSync, writeFileSync } = require('node:fs');
const { resolve, join } = require('node:path');
const { Readable, Transform } = require('node:stream');
const { pipeline } = require('node:stream/promises');

const version = '1.31.1';
const packageHash = 'QvE8+JyhEccfOL7Oz1m+cSXZGYd4QK7/BtsdoCan0eaF2qqgOSVDV/H7hN5D5JJGesJ5vINCDLDVdHLnO0xjJw==';
const pinnedFiles = {
  'native/onnxruntime.dll': 'cb2fa518b18356617f6f9f460c012501da2819ec7e5c1d80bd8f5de20492db0c',
  'native/onnxruntime.lib': '8022d994b1abb84d4b8b25e37937dfa921ef1e2a2ebba66f5a9af1f8433fa05c',
  'native/onnxruntime_providers_shared.dll': '8cea20be64259950cda8643af0ed42c2a424400fc3bb9bb29f9a35cee6b3419f',
  'native/onnxruntime_providers_shared.lib': '04e1ee653cbefa7ed5dd342747484d20422463710577bdc597d2b323d332bcee',
  'licenses/LICENSE': 'c250d6278f0b47a6439fb7592b08b58a55eb9f535aa49a1db63211c3f982b674',
  'licenses/ThirdPartyNotices.txt': 'c53a76501ef60db6f865f20599f220761201ac4057683acefdab37d861b86622'
};
const base = resolve(__dirname, '../../rust/target');
const output = join(base, `onnxruntime-win-x64-${version}`);
const digest = path => createHash('sha256').update(readFileSync(path)).digest('hex');

async function main() {
  if (process.platform !== 'win32' || process.arch !== 'x64') throw new Error('Native runtime preparation requires Windows x64');
  if (existsSync(output)) {
    const manifest = JSON.parse(readFileSync(join(output, 'manifest.json'), 'utf8'));
    if (manifest.version !== version || manifest.packageHash !== packageHash || !manifest.files || Object.keys(manifest.files).length !== Object.keys(pinnedFiles).length) throw new Error('Native ONNX Runtime manifest does not match the pinned package');
    for (const [name, hash] of Object.entries(pinnedFiles)) {
      if (manifest.files[name] !== hash || digest(join(output, name)) !== hash) throw new Error('Native ONNX Runtime cache verification failed');
    }
    return;
  }
  mkdirSync(base, { recursive: true });
  const temporary = mkdtempSync(join(base, '.onnx-prepare-'));
  try {
    const archive = join(temporary, 'runtime.nupkg');
    const extracted = join(temporary, 'package');
    const ready = join(temporary, 'ready');
    mkdirSync(extracted); mkdirSync(ready); mkdirSync(join(ready, 'native')); mkdirSync(join(ready, 'licenses'));
    const response = await fetch(`https://api.nuget.org/v3-flatcontainer/microsoft.ml.onnxruntime/${version}/microsoft.ml.onnxruntime.${version}.nupkg`, { signal: AbortSignal.timeout(240000) });
    if (!response.ok || !response.body) throw new Error(`ONNX Runtime download failed: ${response.status}`);
    const hash = createHash('sha512');
    const hashed = new Transform({ transform(chunk, _encoding, callback) { hash.update(chunk); callback(null, chunk); } });
    await pipeline(Readable.fromWeb(response.body), hashed, createWriteStream(archive, { flags: 'wx' }));
    if (hash.digest('base64') !== packageHash) throw new Error('ONNX Runtime package SHA-512 does not match the NuGet catalog');
    const args = ['tar.exe', '-xf', archive, '-C', extracted];
    if (args.some(value => /["%\r\n]/.test(value))) throw new Error('Unsupported runtime extraction path');
    execFileSync('cmd.exe', ['/d', '/v:off', '/s', '/c', `"${args.map(value => `"${value}"`).join(' ')}"`], { windowsVerbatimArguments: true, stdio: 'inherit', timeout: 120000 });
    const native = join(extracted, 'runtimes/win-x64/native');
    for (const name of readdirSync(native)) if (/\.(dll|lib)$/i.test(name)) copyFileSync(join(native, name), join(ready, 'native', name));
    for (const name of readdirSync(extracted)) if (/^(license|thirdpartynotices)(\.[a-z]+)?$/i.test(name)) copyFileSync(join(extracted, name), join(ready, 'licenses', name));
    if (!existsSync(join(ready, 'native/onnxruntime.dll')) || !existsSync(join(ready, 'native/onnxruntime.lib')) || readdirSync(join(ready, 'licenses')).length === 0) throw new Error('ONNX Runtime package is missing required files');
    const files = {};
    for (const folder of ['native', 'licenses']) for (const name of readdirSync(join(ready, folder))) files[`${folder}/${name}`] = digest(join(ready, folder, name));
    if (Object.keys(files).length !== Object.keys(pinnedFiles).length || Object.entries(pinnedFiles).some(([name, hash]) => files[name] !== hash)) throw new Error('ONNX Runtime extracted files differ from the pinned package');
    writeFileSync(join(ready, 'manifest.json'), JSON.stringify({ version, packageHash, files }, null, 2));
    renameSync(ready, output);
    console.log(`Prepared Microsoft ONNX Runtime ${version} for native Windows x64`);
  } finally {
    rmSync(temporary, { recursive: true, force: true });
  }
}

main().catch(error => { console.error(error.message); process.exitCode = 1; });
