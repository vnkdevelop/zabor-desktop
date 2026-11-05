import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { releaseState } from './release-guard.mjs';
import { writeBetaManifest } from './beta-manifest.mjs';

let temporaryDirectory;
try {
  const state = releaseState('beta');
  const tag = `v${state.version}`;
  execFileSync('git', ['fetch', 'origin'], { stdio: 'inherit', windowsHide: true });
  execFileSync('git', ['merge-base', '--is-ancestor', 'origin/main', 'HEAD'], { stdio: 'inherit', windowsHide: true });
  const tagExists = spawnSync('git', ['rev-parse', '--verify', `refs/tags/${tag}`], { stdio: 'ignore', windowsHide: true });
  if (tagExists.status === 0) throw new Error('Beta tag already exists; choose a new package version');
  execFileSync('gh', ['auth', 'status'], { stdio: 'ignore', windowsHide: true });
  const result = spawnSync('cmd.exe', ['/d', '/c', 'npm run typecheck && npm test && npm run test:migration && npm run test:rust && npm run dist:win:beta'], { stdio: 'inherit', windowsHide: true });
  if (result.status !== 0) throw new Error('Beta verification or packaging failed');
  const manifest = await writeBetaManifest();
  if (manifest.commit !== state.commit || manifest.version !== state.version) throw new Error('Release state changed during packaging');
  temporaryDirectory = mkdtempSync(join(tmpdir(), 'zabor-beta-release-'));
  const notes = join(temporaryDirectory, 'notes.md');
  writeFileSync(notes, `beta / prerelease\n\ncommit: ${state.commit}\ndata schema: ${manifest.dataSchema}\nenabled Rust modules: none\n\nP0 is in progress; profile import and data-preserving reverse migration are not yet enabled.\n`);
  execFileSync('git', ['push', 'origin', 'HEAD:refs/heads/beta/rust-migration'], { stdio: 'inherit', windowsHide: true });
  execFileSync('git', ['tag', '-a', tag, state.commit, '-m', `Beta ${state.version} at ${state.commit}`], { stdio: 'inherit', windowsHide: true });
  execFileSync('git', ['push', 'origin', `refs/tags/${tag}`], { stdio: 'inherit', windowsHide: true });
  execFileSync('gh', ['release', 'create', tag, join('release/beta', manifest.artifact.name), 'release/beta/zabor-beta-manifest.json',
    '--repo', 'vnkdevelop/zabor-desktop', '--verify-tag', '--prerelease', '--latest=false', '--title', `ZABOR beta ${state.version}`, '--notes-file', notes],
  { stdio: 'inherit', windowsHide: true });
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
} finally {
  if (temporaryDirectory) rmSync(temporaryDirectory, { recursive: true, force: true });
}
