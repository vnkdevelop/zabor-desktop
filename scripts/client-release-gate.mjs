import { createHash } from 'node:crypto';
import { readdirSync, readFileSync } from 'node:fs';
import { join, relative } from 'node:path';

export function clientSourceHash(root = '.') {
  const files = [];
  const visit = directory => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) visit(path);
      else files.push(path);
    }
  };
  for (const directory of ['src/main', 'src/preload', 'src/shared', 'src/renderer/src', 'src/renderer/public/rust-voicing', 'rust/crates/zabor-dsp', 'rust/crates/zabor-dsp-wasm']) visit(join(root, directory));
  for (const file of ['build/ZaborInputSimulator.cs', 'build/ZaborUpdater.cs']) files.push(join(root, file));
  const hash = createHash('sha256');
  const pkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));
  hash.update(JSON.stringify(pkg.zaborRuntime || {}));
  for (const file of files.sort()) hash.update(relative(root, file).replaceAll('\\', '/')).update(readFileSync(file));
  return hash.digest('hex');
}

export function assertClientApproved() {
  const pkg = JSON.parse(readFileSync('package.json', 'utf8'));
  if (pkg.zaborRuntime?.rustVoicing !== true) return;
  const approval = JSON.parse(readFileSync('client-release-approval.json', 'utf8'));
  if (approval.version !== 1 || approval.ownerTwoDeviceApproved !== true || approval.dataCompatible !== true ||
    approval.clientSourceHash !== clientSourceHash()) throw new Error('Rust client stable release requires current two-device and data-compatibility approval; publish beta for testing first');
}
