import { after, afterEach, beforeEach, test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, mkdirSync, writeFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import { createRequire } from 'node:module';
import { parseVersion, compareVersions, distributionChannel, distributionProfile, validateBetaManifest, isReleaseDownloadUrl } from '../src/main/updatePolicy.ts';
import { checkGitHubRelease, cleanUpdateDirectory, setupUpdater, isNewerVersion } from '../src/main/updater.ts';
import { app, ipcMain, ipcRenderer, MockBrowserWindow } from './helpers/electron-mock.mjs';
import { validateReleaseState } from '../scripts/release-guard.mjs';
import { createMockFetchResponse } from './helpers/fixtures.mjs';
import { createHash } from 'node:crypto';
import { spawnedProcesses, resetSpawnedProcesses } from './helpers/child-process-mock.mjs';
import ts from 'typescript';
import { execFileSync } from 'node:child_process';
import { releaseChannelForBranch, suggestReleaseVersion, validateReleaseVersion } from '../scripts/release-plan.mjs';

const originalFetch = globalThis.fetch;
const current = '3.7.2-beta.rust.1';
const version = '3.7.2-beta.rust.10';
const prefix = 'https://github.com/vnkdevelop/zabor-desktop/releases/download/';
const manifest = () => ({
  manifestVersion: 1, channel: 'beta', version, commit: 'a'.repeat(40), platform: 'win32', architecture: process.arch,
  dataSchema: 1, minimumDataSchema: 1, maximumDataSchema: 1, enabledModules: [],
  artifact: { name: `ZABOR-Beta-Setup-${version}-${process.arch}.exe`, size: 100, sha256: 'b'.repeat(64) }
});
const release = (v = version) => ({
  tag_name: `v${v}`, prerelease: true, draft: false,
  assets: [
    { name: 'zabor-beta-manifest.json', size: 500, browser_download_url: `${prefix}v${v}/zabor-beta-manifest.json` },
    { name: `ZABOR-Beta-Setup-${v}-${process.arch}.exe`, size: 100, browser_download_url: `${prefix}v${v}/ZABOR-Beta-Setup-${v}-${process.arch}.exe` }
  ]
});

beforeEach(() => { app.reset(); ipcMain.reset(); MockBrowserWindow.reset(); resetSpawnedProcesses(); app.setVersion(current); });
afterEach(() => { globalThis.fetch = originalFetch; app.cleanupTempDir(); });
after(() => app.cleanupTempDir());

test('SemVer precedence preserves numeric prerelease identifiers, prefixes and build metadata', () => {
  const ordered = ['1.0.0-alpha', '1.0.0-alpha.1', '1.0.0-alpha.beta', '1.0.0-beta', '1.0.0-beta.2', '1.0.0-beta.11', '1.0.0-rc.1', '1.0.0'];
  for (let i = 1; i < ordered.length; i++) assert.equal(compareVersions(ordered[i - 1], ordered[i]), -1);
  assert.equal(compareVersions('v3.7.2-beta.rust.2', version), -1);
  assert.equal(compareVersions(' V3.7.2+first ', '3.7.2+second'), 0);
  assert.equal(compareVersions('1.0.0-beta.9007199254740993', '1.0.0-beta.9007199254740992'), 1);
  assert.equal(isNewerVersion('3.6.4.1', '3.6.4'), true);
  assert.equal(isNewerVersion('3.6.4', ''), true);
});

test('malformed versions fail closed without coercion', () => {
  for (const value of ['', '1.2', '1.2.3evil', '01.2.3', '1.2.3-beta.01', '1.2.3+', '1.2.3-beta..1', '1.2.3.4-beta']) {
    assert.equal(parseVersion(value), null, value);
    assert.equal(isNewerVersion(value, '1.0.0'), false, value);
  }
});

test('new beta failures use bilingual lowercase translations without changing legacy errors', () => {
  const text = readFileSync('src/renderer/src/components/UpdaterWindow.tsx', 'utf8');
  const source = ts.createSourceFile('UpdaterWindow.tsx', text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const node = source.statements.find(node => ts.isFunctionDeclaration(node) && node.name?.text === 'betaUpdateErrorKey');
  assert.ok(node?.body);
  const keyForError = new Function('error', node.body.getText(source).slice(1, -1));
  const cases = [
    ['Unverified beta update', 'betaUnverified'],
    ['Invalid GitHub releases list', 'betaManifestInvalid'],
    ['Invalid GitHub pagination link', 'betaManifestInvalid'],
    ['GitHub release pagination limit reached', 'betaManifestInvalid'],
    ['Beta manifest too large', 'betaManifestInvalid'],
    ['Beta manifest error: 404', 'betaManifestInvalid'],
    ['installer-verification-failed:expected-sha256-missing', 'checksumMissing']
  ];
  for (const [error, key] of cases) {
    assert.equal(keyForError(error), `updater.${key}`);
    for (const language of ['ru', 'en']) {
      const value = JSON.parse(readFileSync(`src/renderer/src/locales/${language}.json`, 'utf8')).updater[key];
      assert.equal(typeof value, 'string');
      assert.equal(value, value.toLowerCase());
    }
  }
  assert.equal(keyForError(null), null);
  assert.equal(keyForError('existing legacy error'), null);
});

test('stable and beta profiles have distinct resource names', () => {
  const stable = distributionProfile('stable');
  const beta = distributionProfile('beta');
  for (const key of Object.keys(stable)) assert.notEqual(stable[key], beta[key], key);
  assert.equal(distributionChannel(version), 'beta');
  assert.equal(distributionChannel('3.7.2+beta'), 'stable');
});

test('beta cleanup never removes the neighboring stable updater resources', () => {
  const root = app.getPath('temp');
  for (const channel of ['beta', 'stable']) {
    const p = distributionProfile(channel);
    mkdirSync(join(root, p.updateDirectory));
    writeFileSync(join(root, `${p.updater}.exe`), 'fixture');
    writeFileSync(join(root, p.splashFile), 'fixture');
  }
  cleanUpdateDirectory();
  for (const channel of ['beta', 'stable']) {
    const p = distributionProfile(channel);
    assert.equal(existsSync(join(root, p.updateDirectory)), channel === 'stable');
    assert.equal(existsSync(join(root, `${p.updater}.exe`)), channel === 'stable');
    assert.equal(existsSync(join(root, p.splashFile)), channel === 'stable');
  }
});

test('manifest binds channel, schema, architecture, artifact and checksum', () => {
  assert.equal(validateBetaManifest(manifest(), version, process.arch), true);
  for (const patch of [{ channel: 'stable' }, { version: current }, { architecture: 'other' }, { minimumDataSchema: 2 }, { maximumDataSchema: 0 }, { dataSchema: 0 }, { enabledModules: ['invalid module'] }, { commit: 'main' }]) {
    assert.equal(validateBetaManifest({ ...manifest(), ...patch }, version, process.arch), false);
  }
  for (const patch of [{ name: 'setup.exe' }, { sha256: '' }, { size: -1 }]) {
    assert.equal(validateBetaManifest({ ...manifest(), artifact: { ...manifest().artifact, ...patch } }, version, process.arch), false);
  }
  assert.equal(isReleaseDownloadUrl(`${prefix}v${version}/file.exe`), true);
  for (const url of ['https://evil.test/file.exe', `${prefix}../file.exe`, `${prefix}v1/file.exe?redirect=1`]) assert.equal(isReleaseDownloadUrl(url), false);
});

test('old beta updater receives the honest legacy bridge before Rust-enabled beta', async () => {
  const old = execFileSync('git', ['show', '9e9f796:src/main/updatePolicy.ts'], { encoding: 'utf8' });
  const code = ts.transpileModule(old, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } }).outputText;
  const previous = await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);
  const bridgeVersion = '3.7.2-beta.rust.1';
  const rustVersion = '3.7.2-beta.rust.2';
  const bridge = { ...manifest(), version: bridgeVersion, artifact: { ...manifest().artifact, name: `ZABOR-Beta-Setup-${bridgeVersion}-${process.arch}.exe` } };
  const rust = { ...bridge, version: rustVersion, enabledModules: ['rust_voicing'], artifact: { ...bridge.artifact, name: `ZABOR-Beta-Setup-${rustVersion}-${process.arch}.exe` } };
  assert.equal(previous.validateBetaManifest(bridge, bridgeVersion, process.arch), true);
  assert.equal(previous.validateBetaManifest(rust, rustVersion, process.arch), false);
  assert.equal(validateBetaManifest(rust, rustVersion, process.arch), true);
  assert.equal(validateBetaManifest({ ...rust, enabledModules: ['future_client_module'], dataSchema: 2 }, rustVersion, process.arch), true);
  assert.equal(validateBetaManifest({ ...rust, minimumDataSchema: 2 }, rustVersion, process.arch), false);
  assert.equal(validateBetaManifest({ ...rust, enabledModules: ['rust_voicing', 'rust_voicing'] }, rustVersion, process.arch), false);
});

test('one-click release planning respects branches, SemVer counters and occupied versions', () => {
  assert.equal(releaseChannelForBranch('main'), 'stable');
  assert.equal(releaseChannelForBranch('beta/rust-migration'), 'beta');
  assert.throws(() => releaseChannelForBranch('rust/client-beta'));
  assert.throws(() => releaseChannelForBranch('main', 'beta'));
  assert.equal(suggestReleaseVersion('3.7.2-beta.rust.9', 'beta', true), '3.7.2-beta.rust.10');
  assert.equal(suggestReleaseVersion('3.7.2-beta.rust.2', 'stable'), '3.7.2');
  assert.equal(suggestReleaseVersion('3.7.2-beta.rust.2', 'stable', true), '3.7.3');
  assert.equal(suggestReleaseVersion('3.7.2', 'beta'), '3.7.3-beta.rust.1');
  assert.doesNotThrow(() => validateReleaseVersion('3.7.2', '3.7.2-beta.rust.2', 'stable'));
  assert.throws(() => validateReleaseVersion('3.7.1', '3.7.2', 'stable'));
  for (const file of ['release.bat', 'release-beta.bat', 'scripts/release.mjs']) {
    assert.doesNotMatch(readFileSync(file, 'utf8'), /git add \.|--autostash|--rebase/);
  }
  const installer = readFileSync('build/installer-stable.nsh', 'utf8');
  assert.doesNotMatch(installer, /zabor-desktop-beta|ZABOR-Beta\.exe|Temp\\zabor\*/);
  assert.doesNotMatch(readFileSync('.github/workflows/build.yml', 'utf8'), /softprops\/action-gh-release/);
});

test('cancel/retry waits for the previous update writer and cannot be overwritten by a late completion', async t => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  app.setVersion('3.7.1');
  const { setupUpdater } = await import('../src/main/updater.ts?cancel-retry-owner');
  setupUpdater(() => new MockBrowserWindow());
  const url = `${prefix}v3.7.2/ZABOR-Setup-3.7.2.exe`;
  let finishFetch;
  globalThis.fetch = () => new Promise(resolve => { finishFetch = resolve; });
  const first = ipcRenderer.invoke('start-update-download', url, '3.7.2');
  assert.equal(await ipcRenderer.invoke('cancel-update-download'), true);
  const premature = await ipcRenderer.invoke('start-update-download', url, '3.7.2');
  assert.equal(premature.success, false);
  assert.equal(premature.message, 'Already downloading');
  finishFetch(createMockFetchResponse({ bodyStream: new ReadableStream({ start(controller) { controller.close(); } }) }));
  assert.equal((await first).success, false);
  globalThis.fetch = async () => createMockFetchResponse({ contentLength: 3, bodyStream: new ReadableStream({ start(controller) { controller.enqueue(Buffer.from([1, 2, 3])); controller.close(); } }) });
  const next = await ipcRenderer.invoke('start-update-download', url, '3.7.2');
  assert.equal(next.success, true);
  assert.equal((await ipcRenderer.invoke('get-update-status')).status, 'downloaded');
});

test('beta discovery follows pagination and never uses releases/latest', async () => {
  const visited = [];
  globalThis.fetch = async url => {
    visited.push(url);
    if (url.includes('page=2')) return createMockFetchResponse({ jsonBody: [release()] });
    if (url.includes('api.github.com')) {
      const response = createMockFetchResponse({ jsonBody: [{ ...release('9.0.0'), prerelease: false }] });
      response.headers.set('link', '<https://api.github.com/repos/vnkdevelop/zabor-desktop/releases?per_page=100&page=2>; rel="next"');
      return response;
    }
    return createMockFetchResponse({ jsonBody: manifest() });
  };
  const result = await checkGitHubRelease();
  assert.equal(result.updateAvailable, true);
  assert.equal(result.updateInfo.version, version);
  assert.equal(result.updateInfo.sha256, manifest().artifact.sha256);
  assert.equal(visited.some(url => url.endsWith('/latest')), false);
});

test('absence of compatible beta does not offer stable or downgrade', async () => {
  globalThis.fetch = async () => createMockFetchResponse({ jsonBody: [{ ...release('9.0.0'), prerelease: false }, release('3.7.2-beta.rust.0')] });
  assert.equal((await checkGitHubRelease()).updateAvailable, false);
});

test('beta rejects missing or incompatible manifests and GitHub checksum mismatches', async () => {
  for (const mode of ['missing', 'schema', 'digest', 'draft']) {
    const payload = release();
    if (mode === 'missing') payload.assets.shift();
    if (mode === 'digest') payload.assets[1].digest = `sha256:${'c'.repeat(64)}`;
    if (mode === 'draft') payload.draft = true;
    globalThis.fetch = async url => createMockFetchResponse({ jsonBody: url.includes('api.github.com') ? [payload] : { ...manifest(), ...(mode === 'schema' ? { minimumDataSchema: 2 } : {}) } });
    assert.equal((await checkGitHubRelease()).updateAvailable, false, mode);
  }
});

test('stable never offers a prerelease returned by an unexpected API response', async () => {
  app.setVersion('3.7.1');
  globalThis.fetch = async () => createMockFetchResponse({ jsonBody: release() });
  assert.equal((await checkGitHubRelease()).updateAvailable, false);
});

test('modern stable manifest selects the exact installer and binds schema/checksum without confusing helpers', async () => {
  app.setVersion('3.7.1');
  const stableVersion = '3.7.2';
  const stableManifest = { ...manifest(), channel: 'stable', version: stableVersion, artifact: { ...manifest().artifact, name: `ZABOR-Setup-${stableVersion}.exe` } };
  const api = { tag_name: `v${stableVersion}`, prerelease: false, assets: [
    { name: 'ZaborHelper.exe', size: 10, browser_download_url: `${prefix}v${stableVersion}/ZaborHelper.exe` },
    { name: stableManifest.artifact.name, size: 100, digest: `sha256:${stableManifest.artifact.sha256}`, browser_download_url: `${prefix}v${stableVersion}/${stableManifest.artifact.name}` },
    { name: 'zabor-stable-manifest.json', size: 500, browser_download_url: `${prefix}v${stableVersion}/zabor-stable-manifest.json` }
  ] };
  globalThis.fetch = async url => createMockFetchResponse({ jsonBody: url.includes('api.github.com') ? api : stableManifest });
  const result = await checkGitHubRelease();
  assert.equal(result.updateAvailable, true);
  assert.ok(result.updateInfo.downloadUrl.endsWith(stableManifest.artifact.name));
  assert.equal(result.updateInfo.sha256, stableManifest.artifact.sha256);
  api.assets[1].digest = `sha256:${'e'.repeat(64)}`;
  assert.equal((await checkGitHubRelease()).updateAvailable, false);
});

test('beta download IPC cannot bypass validated release discovery', async t => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  setupUpdater(() => new MockBrowserWindow());
  const result = await ipcRenderer.invoke('start-update-download', `${prefix}v${version}/setup.exe`, version);
  assert.equal(result.success, false);
  assert.equal(result.message, 'Unverified beta update');
});

test('verified beta installs from its own directory and uses the beta splash namespace', async t => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const { setupUpdater, cleanUpdateDirectory } = await import('../src/main/updater.ts?beta-verified');
  const window = new MockBrowserWindow();
  setupUpdater(() => window);
  const content = Buffer.alloc(100, 42);
  const digest = createHash('sha256').update(content).digest('hex');
  const approved = { ...manifest(), artifact: { ...manifest().artifact, sha256: digest } };
  globalThis.fetch = async url => {
    if (url.includes('api.github.com')) return createMockFetchResponse({ jsonBody: [release()] });
    if (url.endsWith('.json')) return createMockFetchResponse({ jsonBody: approved });
    return createMockFetchResponse({ contentLength: content.length, bodyStream: new ReadableStream({ start(controller) { controller.enqueue(content); controller.close(); } }) });
  };
  const check = await ipcRenderer.invoke('check-for-updates');
  assert.equal(check.updateAvailable, true);
  const result = await ipcRenderer.invoke('start-update-download', check.updateInfo.downloadUrl, check.updateInfo.version);
  assert.equal(result.success, true);
  assert.ok(result.filePath.includes(distributionProfile('beta').updateDirectory));
  globalThis.fetch = async () => createMockFetchResponse({ jsonBody: [] });
  assert.equal((await ipcRenderer.invoke('check-for-updates')).updateAvailable, false);
  assert.equal((await ipcRenderer.invoke('get-update-status')).status, 'downloaded');
  cleanUpdateDirectory();
  assert.equal(existsSync(result.filePath), true);
  assert.equal((await ipcRenderer.invoke('install-update')).success, true);
  assert.ok(spawnedProcesses.some(process => process.command.endsWith('ZaborUpdaterBeta.exe') && process.args.includes('beta')));
  assert.equal(spawnedProcesses.some(process => process.command.endsWith('ZaborUpdater.exe')), false);
});

test('beta checksum failure prevents splash and installer execution', async t => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const { setupUpdater } = await import('../src/main/updater.ts?beta-corrupted');
  setupUpdater(() => new MockBrowserWindow());
  globalThis.fetch = async url => {
    if (url.includes('api.github.com')) return createMockFetchResponse({ jsonBody: [release()] });
    if (url.endsWith('.json')) return createMockFetchResponse({ jsonBody: manifest() });
    return createMockFetchResponse({ contentLength: 100, bodyStream: new ReadableStream({ start(controller) { controller.enqueue(Buffer.alloc(100)); controller.close(); } }) });
  };
  const check = await ipcRenderer.invoke('check-for-updates');
  const download = await ipcRenderer.invoke('start-update-download', check.updateInfo.downloadUrl, check.updateInfo.version);
  assert.equal(download.success, true);
  const install = await ipcRenderer.invoke('install-update');
  assert.equal(install.success, false);
  assert.match(install.message, /sha256-mismatch/);
  assert.equal(spawnedProcesses.length, 0);
});

test('beta build metadata is preserved and cannot strip the expected checksum', async t => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const { setupUpdater } = await import('../src/main/updater.ts?beta-build-metadata');
  setupUpdater(() => new MockBrowserWindow());
  const metadataVersion = `${version}+build.5`;
  const approved = { ...manifest(), version: metadataVersion, artifact: { ...manifest().artifact, name: `ZABOR-Beta-Setup-${metadataVersion}-${process.arch}.exe` } };
  globalThis.fetch = async url => {
    if (url.includes('api.github.com')) return createMockFetchResponse({ jsonBody: [release(metadataVersion)] });
    if (url.endsWith('.json')) return createMockFetchResponse({ jsonBody: approved });
    return createMockFetchResponse({ contentLength: 100, bodyStream: new ReadableStream({ start(controller) { controller.enqueue(Buffer.alloc(100)); controller.close(); } }) });
  };
  const check = await ipcRenderer.invoke('check-for-updates');
  assert.equal(check.updateAvailable, true);
  const download = await ipcRenderer.invoke('start-update-download', check.updateInfo.downloadUrl, check.updateInfo.version);
  assert.equal(download.success, true);
  assert.ok(download.filePath.includes('+build.5'));
  const install = await ipcRenderer.invoke('install-update');
  assert.equal(install.success, false);
  assert.match(install.message, /sha256-mismatch/);
  assert.equal(spawnedProcesses.length, 0);
});

test('packaging fails on wrong branch, dirty tree, mismatched channel or invalid version', () => {
  assert.doesNotThrow(() => validateReleaseState('beta', version, 'beta/rust-migration', true));
  assert.doesNotThrow(() => validateReleaseState('stable', '3.7.1', 'main', true));
  for (const values of [['beta', version, 'main', true], ['stable', '3.7.1', 'beta/rust-migration', true], ['beta', '3.7.1', 'beta/rust-migration', true], ['stable', version, 'main', true], ['beta', version, 'beta/rust-migration', false]]) {
    assert.throws(() => validateReleaseState(...values));
  }
});

test('beta packaging keeps existing UI assets and does not use the stable NSIS hooks', () => {
  const require = createRequire(import.meta.url);
  const beta = require('../electron-builder.beta.cjs');
  const base = require('../package.json').build;
  assert.notEqual(beta.appId, base.appId);
  assert.equal(beta.win.executableName, 'ZABOR-Beta');
  assert.equal(beta.win.icon, base.win.icon);
  assert.equal(beta.nsis.deleteAppDataOnUninstall, false);
  const installer = readFileSync('build/installer-beta.nsh', 'utf8');
  assert.equal(installer.includes('zabor_backup'), false);
  assert.equal(installer.includes('taskkill /F /IM ZABOR.exe'), false);
  assert.equal(installer.includes('RMDir /r "$APPDATA\\ZABOR"'), false);
  assert.equal(installer.includes('RMDir /r "$LOCALAPPDATA\\Temp\\zabor*"'), false);
  assert.ok(installer.includes('RMDir /r "$APPDATA\\zabor-desktop-beta"'));
  const source = readFileSync('src/main/index.ts', 'utf8');
  assert.ok(source.indexOf("app.setPath('sessionData', betaUserData)") < source.indexOf('app.requestSingleInstanceLock()'));
  const script = readFileSync('release-beta.bat', 'utf8');
  assert.doesNotMatch(script, /git add \.|--autostash|--rebase/);
});
