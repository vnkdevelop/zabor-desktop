import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync, mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { createHash } from 'node:crypto';
import { createInterface } from 'node:readline/promises';
import { pathToFileURL } from 'node:url';
import { loadEnv } from 'vite';
import { releaseState } from './release-guard.mjs';
import { releaseChannelForBranch, suggestReleaseVersion, validateReleaseVersion } from './release-plan.mjs';
import { writeBetaManifest } from './beta-manifest.mjs';
import { writeStableManifest } from './stable-manifest.mjs';
import { assertClientApproved } from './client-release-gate.mjs';

const repository = 'vnkdevelop/zabor-desktop';
const git = (...args) => execFileSync('git', args, { encoding: 'utf8', windowsHide: true }).trim();

function githubRelease(tag) {
  const result = spawnSync('gh', ['api', `repos/${repository}/releases/tags/${encodeURIComponent(tag)}`], { encoding: 'utf8', windowsHide: true });
  if (result.status === 0) return JSON.parse(result.stdout);
  if (result.stderr?.includes('404')) {
    for (let page = 1; page <= 20; page++) {
      const releases = JSON.parse(execFileSync('gh', ['api', `repos/${repository}/releases?per_page=100&page=${page}`], { encoding: 'utf8', windowsHide: true }));
      const found = releases.find(release => release.tag_name === tag);
      if (found) return found;
      if (releases.length < 100) return null;
    }
    throw new Error('Release lookup pagination limit reached');
  }
  throw new Error('GitHub release lookup failed');
}

function updateVersion(version) {
  const packageText = readFileSync('package.json', 'utf8');
  const lockText = readFileSync('package-lock.json', 'utf8');
  const packageData = JSON.parse(packageText);
  const lockData = JSON.parse(lockText);
  const nextPackage = packageText.replace(/("version"\s*:\s*")[^"]+("\s*,)/, `$1${version}$2`);
  let count = 0;
  const nextLock = lockText.replace(/("version"\s*:\s*")[^"]+("\s*,)/g, (match, first, last) => ++count <= 2 ? `${first}${version}${last}` : match);
  if (packageData.version === version) return;
  if (lockData.packages?.['']?.name !== packageData.name || JSON.parse(nextLock).packages[''].version !== version) throw new Error('Package/lock version update mismatch');
  writeFileSync('package.json', nextPackage);
  writeFileSync('package-lock.json', nextLock);
  git('add', '--', 'package.json', 'package-lock.json');
  git('commit', '-m', `release: prepare ${version}`);
}

export async function runRelease(requested = 'auto', args = []) {
  let temporary;
  const helpers = new Map();
  let generatedHelpers = null;
  const yes = args.includes('--yes');
  const versionIndex = args.indexOf('--version');
  const requestedVersion = versionIndex < 0 ? null : args[versionIndex + 1];
  try {
    if (git('status', '--porcelain', '--untracked-files=all')) throw new Error('Commit reviewed changes before release; no automatic staging, stash, rebase or branch switching is performed');
    const branch = git('branch', '--show-current');
    const channel = releaseChannelForBranch(branch, requested);
    const origin = git('remote', 'get-url', 'origin');
    if (!/^(https:\/\/github\.com\/vnkdevelop\/zabor-desktop(?:\.git)?|git@github\.com:vnkdevelop\/zabor-desktop(?:\.git)?)$/.test(origin)) throw new Error('Origin must identify the expected GitHub repository without embedded credentials');
    const pkg = JSON.parse(readFileSync('package.json', 'utf8'));
    const planVersion = suggestReleaseVersion(pkg.version, channel);
    if (args.includes('--plan-only')) {
      console.log(JSON.stringify({ branch, channel, version: requestedVersion || planVersion, commit: git('rev-parse', 'HEAD') }));
      return;
    }
    if (channel === 'stable') assertClientApproved();
    const env = loadEnv('production', process.cwd(), '');
    if (!(process.env.ZABOR_CLIENT_SECRET || env.ZABOR_CLIENT_SECRET)) throw new Error('Official release attestation is not configured in the local .env/environment; no secrets are printed or copied');
    if (git('ls-files', '--', '.env')) throw new Error('.env must not be tracked');
    execFileSync('gh', ['auth', 'status'], { stdio: 'ignore', windowsHide: true });
    git('fetch', '--tags', 'origin');
    git('merge-base', '--is-ancestor', 'origin/main', 'HEAD');
    let version = requestedVersion || planVersion;
    let tag = `v${version}`;
    let existing = githubRelease(tag);
    const tagResult = spawnSync('git', ['rev-parse', '--verify', `refs/tags/${tag}^{commit}`], { encoding: 'utf8', windowsHide: true });
    const head = git('rev-parse', 'HEAD');
    const resumable = tagResult.status === 0 && tagResult.stdout.trim() === head && (!existing || existing.draft === true);
    if (!requestedVersion && (existing?.draft === false || tagResult.status === 0 && !resumable)) version = suggestReleaseVersion(pkg.version, channel, true);
    validateReleaseVersion(version, pkg.version, channel);
    if (!yes) {
      const rl = createInterface({ input: process.stdin, output: process.stdout });
      try {
        if (!requestedVersion) version = (await rl.question(`Release version [${version}]: `)).trim() || version;
        validateReleaseVersion(version, pkg.version, channel);
        if ((await rl.question(`Publish ${channel} ${version} from ${branch}? [y/N] `)).trim().toLowerCase() !== 'y') return;
      } finally { rl.close(); }
    }
    tag = `v${version}`;
    existing = githubRelease(tag);
    if (existing && existing.draft !== true) throw new Error('Published releases are immutable; choose the next version');
    updateVersion(version);
    const state = releaseState(channel);
    for (const file of ['build/ZaborUpdater.exe', 'build/ZaborInputSimulator.exe']) helpers.set(file, readFileSync(file));
    const localTag = spawnSync('git', ['rev-parse', '--verify', `refs/tags/${tag}^{commit}`], { encoding: 'utf8', windowsHide: true });
    if (localTag.status === 0 && localTag.stdout.trim() !== state.commit) throw new Error('Existing tag points to another commit; it will not be moved');
    const checks = 'npm run typecheck && npm test && npm run test:migration && npm run test:readiness && npm run test:rust && npm run test:client-rust && npm run test:audio-worklet';
    const result = spawnSync('cmd.exe', ['/d', '/c', `${checks} && npm run ${channel === 'beta' ? 'dist:win:beta' : 'dist:win'}`], { stdio: 'inherit', windowsHide: true });
    generatedHelpers = new Map([...helpers.keys()].map(file => [file, createHash('sha256').update(readFileSync(file)).digest('hex')]));
    if (result.status !== 0) throw new Error('Release checks or packaging failed; no tag/release was created');
    const manifest = channel === 'beta' ? await writeBetaManifest() : await writeStableManifest();
    if (manifest.commit !== state.commit) throw new Error('Commit changed during release');
    const directory = channel === 'beta' ? 'release/beta' : 'release';
    const manifestName = channel === 'beta' ? 'zabor-beta-manifest.json' : 'zabor-stable-manifest.json';
    const files = [join(directory, manifest.artifact.name), join(directory, manifestName)];
    const approvedTemp = join(process.env.LOCALAPPDATA || tmpdir(), 'Temp', 'opencode');
    mkdirSync(approvedTemp, { recursive: true });
    temporary = mkdtempSync(join(approvedTemp, 'zabor-release-'));
    const notes = join(temporary, 'notes.md');
    writeFileSync(notes, `${channel === 'beta' ? 'beta / prerelease' : 'stable'}\n\ncommit: ${state.commit}\ndata schema: ${manifest.dataSchema}\ndefault modules: ${manifest.enabledModules.join(', ') || 'legacy'}\n\n${channel === 'beta' ? 'Use separate test accounts. Stable profile import is not enabled. Run with --legacy-dsp after exiting the app for an A/B comparison. Backend remains .NET; SFU remains Go.\n' : 'Owner acceptance applies to the current client source fingerprint.\n'}`);
    git('push', 'origin', `HEAD:refs/heads/${branch}`);
    if (localTag.status !== 0) git('tag', '-a', tag, state.commit, '-m', `${channel} ${version} at ${state.commit}`);
    git('push', 'origin', `refs/tags/${tag}`);
    existing = githubRelease(tag);
    if (existing && existing.draft !== true) throw new Error('Release became public during preparation; assets will not be overwritten');
    if (!existing) {
      execFileSync('gh', ['release', 'create', tag, ...files, '--repo', repository, '--verify-tag', '--draft', '--latest=false',
        ...(channel === 'beta' ? ['--prerelease'] : []), '--title', `ZABOR ${channel} ${version}`, '--notes-file', notes], { stdio: 'inherit', windowsHide: true });
    } else {
      execFileSync('gh', ['release', 'upload', tag, ...files, '--repo', repository, '--clobber'], { stdio: 'inherit', windowsHide: true });
    }
    const uploaded = githubRelease(tag);
    const manifestBytes = readFileSync(files[1]);
    for (const file of [manifest.artifact, { name: manifestName, size: manifestBytes.byteLength, sha256: createHash('sha256').update(manifestBytes).digest('hex') }]) {
      const asset = uploaded?.assets?.find(asset => asset.name === file.name);
      if (!asset || asset.size !== file.size || asset.digest && asset.digest.toLowerCase() !== `sha256:${file.sha256}`) throw new Error('Uploaded artifact validation failed; draft retained for safe retry');
    }
    execFileSync('gh', ['release', 'edit', tag, '--repo', repository, '--draft=false', `--prerelease=${channel === 'beta'}`, `--latest=${channel === 'stable'}`], { stdio: 'inherit', windowsHide: true });
    console.log(`Published ${channel} ${tag}; existing tags and neighboring profiles were not changed`);
  } finally {
    if (generatedHelpers) {
      for (const [file, bytes] of helpers) {
        if (!existsSync(file)) { console.warn('Helper was removed outside the build; it was not recreated'); continue; }
        const current = createHash('sha256').update(readFileSync(file)).digest('hex');
        if (current === generatedHelpers.get(file)) writeFileSync(file, bytes);
        else console.warn('Helper changed outside the build; it was preserved for manual review');
      }
    }
    if (temporary && existsSync(temporary)) rmSync(temporary, { recursive: true, force: true });
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  runRelease(process.argv[2] || 'auto', process.argv.slice(3)).catch(error => { console.error(error.message); process.exitCode = 1; });
}
