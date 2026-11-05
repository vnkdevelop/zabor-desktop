import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
import { distributionChannel, parseVersion } from '../src/main/updatePolicy.ts';

export function validateReleaseState(channel, version, branch, clean) {
  if (channel !== 'beta' && channel !== 'stable') throw new Error('Unknown distribution channel');
  const parsed = parseVersion(version);
  if (!parsed || parsed.normalized !== version || distributionChannel(version) !== channel ||
    (channel === 'stable' && parsed.prerelease.length)) throw new Error('Package version does not match distribution channel');
  if (channel === 'beta' && branch !== 'beta/rust-migration') throw new Error('Beta builds require beta/rust-migration');
  if (channel === 'stable' && branch !== 'main') throw new Error('Stable builds require main');
  if (!clean) throw new Error('Commit reviewed changes before packaging; working tree is not clean');
}

export function releaseState(channel, allowBuiltHelpers = false) {
  const git = (...args) => execFileSync('git', args, { encoding: 'utf8', windowsHide: true }).trim();
  const pkg = JSON.parse(readFileSync('package.json', 'utf8'));
  const lock = JSON.parse(readFileSync('package-lock.json', 'utf8'));
  let branch = git('branch', '--show-current');
  if (!branch && process.env.GITHUB_ACTIONS === 'true') {
    const tag = process.env.GITHUB_REF?.replace(/^refs\/tags\//, '');
    if (tag !== `v${pkg.version}`) throw new Error('CI tag does not match package version');
    const base = channel === 'beta' ? 'origin/beta/rust-migration' : 'origin/main';
    git('merge-base', '--is-ancestor', 'HEAD', base);
    branch = channel === 'beta' ? 'beta/rust-migration' : 'main';
  }
  const changes = git('status', '--porcelain', '--untracked-files=all');
  const allowedHelpers = new Set(['build/ZaborUpdater.exe', 'build/ZaborInputSimulator.exe']);
  const clean = changes === '' || (allowBuiltHelpers && changes.split('\n').every(line =>
    /^ ?M /.test(line) && allowedHelpers.has(line.slice(line.startsWith(' ') ? 3 : 2))));
  validateReleaseState(channel, pkg.version, branch, clean);
  if (lock.version !== pkg.version || lock.packages?.['']?.version !== pkg.version) throw new Error('Lockfile version does not match package version');
  return { channel, version: pkg.version, branch, commit: git('rev-parse', 'HEAD') };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const state = releaseState(process.argv[2]);
    console.log(`${state.channel}: ${state.version} at ${state.commit}`);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
