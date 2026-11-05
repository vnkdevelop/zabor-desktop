import { createHash } from 'node:crypto';
import { createReadStream, statSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { BETA_MANIFEST_NAME, LOCAL_DATA_SCHEMA } from '../src/main/updatePolicy.ts';
import { releaseState } from './release-guard.mjs';

const require = createRequire(import.meta.url);

export async function writeBetaManifest() {
  const state = releaseState('beta', true);
  const directory = 'release/beta';
  const name = `ZABOR-Beta-Setup-${state.version}-x64.exe`;
  const installer = join(directory, name);
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(installer)) hash.update(chunk);
  const { extractFile } = require('@electron/asar');
  const bundled = JSON.parse(extractFile(join(directory, 'win-unpacked/resources/app.asar'), 'package.json').toString('utf8'));
  if (bundled.version !== state.version || bundled.zaborBuild?.commit !== state.commit || bundled.zaborBuild?.channel !== 'beta' ||
    bundled.zaborBuild?.runtime?.rustVoicing !== bundled.zaborRuntime?.rustVoicing) {
    throw new Error('Packaged application does not match beta commit/version/channel');
  }
  const manifest = {
    manifestVersion: 1, channel: 'beta', version: state.version, commit: state.commit,
    platform: 'win32', architecture: 'x64', dataSchema: LOCAL_DATA_SCHEMA,
    minimumDataSchema: LOCAL_DATA_SCHEMA, maximumDataSchema: LOCAL_DATA_SCHEMA,
    enabledModules: bundled.zaborRuntime?.rustVoicing === true ? ['rust_voicing'] : [], artifact: { name, size: statSync(installer).size, sha256: hash.digest('hex') }
  };
  writeFileSync(join(directory, BETA_MANIFEST_NAME), `${JSON.stringify(manifest, null, 2)}\n`);
  return manifest;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const manifest = await writeBetaManifest();
    console.log(`Beta manifest: ${manifest.version} at ${manifest.commit}`);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
