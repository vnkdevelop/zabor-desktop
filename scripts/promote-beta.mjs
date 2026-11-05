import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { clientSourceHash } from './client-release-gate.mjs';
import { parseVersion } from '../src/main/updatePolicy.ts';

const git = (...args) => execFileSync('git', args, { encoding: 'utf8', windowsHide: true }).trim();
try {
  const branch = git('branch', '--show-current');
  if (!['main', 'beta/rust-migration'].includes(branch) || git('status', '--porcelain', '--untracked-files=all')) throw new Error('Promotion requires a clean main or beta/rust-migration working tree; no stash or forced checkout is performed');
  git('fetch', 'origin');
  const beta = 'origin/beta/rust-migration';
  git('merge-base', '--is-ancestor', 'main', beta);
  git('merge-base', '--is-ancestor', 'origin/main', beta);
  const testedCommit = git('rev-parse', beta);
  const metadata = JSON.parse(git('show', `${beta}:package.json`));
  const version = parseVersion(metadata.version);
  if (!version || version.prerelease[0] !== 'beta') throw new Error('Selected beta is not a prerelease');
  if (!process.argv.includes('--apply')) {
    console.log(JSON.stringify({ action: 'fast-forward main to tested beta, prepare stable version and explicit acceptance', betaCommit: testedCommit, stableVersion: version.core.join('.') }));
    console.log('No files changed. Run --apply --owner-approved --data-gates-approved --app-closed only after two-device tests and data/rollback review.');
  } else {
    if (!process.argv.includes('--owner-approved') || !process.argv.includes('--data-gates-approved') || !process.argv.includes('--app-closed')) throw new Error('Owner two-device/data approval and app-closed confirmation are mandatory');
    if (branch !== 'main') git('switch', 'main');
    git('merge', '--ff-only', beta);
    const stable = version.core.join('.');
    const pkg = readFileSync('package.json', 'utf8').replace(/("version"\s*:\s*")[^"]+("\s*,)/, `$1${stable}$2`);
    let count = 0;
    const lock = readFileSync('package-lock.json', 'utf8').replace(/("version"\s*:\s*")[^"]+("\s*,)/g, (match, first, last) => ++count <= 2 ? `${first}${stable}${last}` : match);
    writeFileSync('package.json', pkg);
    writeFileSync('package-lock.json', lock);
    writeFileSync('client-release-approval.json', JSON.stringify({ version: 1, ownerTwoDeviceApproved: true, dataCompatible: true, testedBetaCommit: testedCommit, clientSourceHash: clientSourceHash() }, null, 2) + '\n');
    git('add', '--', 'package.json', 'package-lock.json', 'client-release-approval.json');
    git('commit', '-m', `release: prepare approved stable ${stable} from ${testedCommit.slice(0, 8)}`);
    console.log('Stable source prepared; no release published. Run release.bat for the guarded stable pipeline.');
  }
} catch (error) { console.error(error.message); process.exitCode = 1; }
