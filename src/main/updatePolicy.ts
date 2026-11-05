export type DistributionChannel = 'stable' | 'beta';

export interface ParsedVersion {
  core: string[];
  prerelease: string[];
  normalized: string;
}

export interface BetaUpdateManifest {
  manifestVersion: 1;
  channel: 'beta';
  version: string;
  commit: string;
  platform: 'win32';
  architecture: string;
  dataSchema: number;
  minimumDataSchema: number;
  maximumDataSchema: number;
  enabledModules: string[];
  artifact: { name: string; size: number; sha256: string };
}

export const LOCAL_DATA_SCHEMA = 1;
export const BETA_MANIFEST_NAME = 'zabor-beta-manifest.json';
export const RELEASES_BASE_URL = 'https://github.com/vnkdevelop/zabor-desktop/releases/download/';

export function parseVersion(value: string, allowLegacyRevision = false): ParsedVersion | null {
  if (typeof value !== 'string') return null;
  const normalized = value.trim().replace(/^v/i, '');
  const match = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:\.(0|[1-9]\d*))?(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$/.exec(normalized);
  if (!match || (match[4] !== undefined && (!allowLegacyRevision || match[5] || match[6]))) return null;
  const prerelease = match[5]?.split('.') ?? [];
  if (prerelease.some(part => /^\d+$/.test(part) && part.length > 1 && part[0] === '0')) return null;
  return { core: match.slice(1, match[4] === undefined ? 4 : 5), prerelease, normalized };
}

function compareNumeric(a: string, b: string): number {
  return a.length === b.length ? (a === b ? 0 : a > b ? 1 : -1) : a.length > b.length ? 1 : -1;
}

export function compareVersions(a: string, b: string): number | null {
  const left = parseVersion(a, true);
  const right = parseVersion(b, true);
  if (!left || !right) return null;
  for (let i = 0; i < Math.max(left.core.length, right.core.length); i++) {
    const comparison = compareNumeric(left.core[i] ?? '0', right.core[i] ?? '0');
    if (comparison !== 0) return comparison;
  }
  if (!left.prerelease.length || !right.prerelease.length) {
    return left.prerelease.length === right.prerelease.length ? 0 : left.prerelease.length ? -1 : 1;
  }
  for (let i = 0; i < Math.max(left.prerelease.length, right.prerelease.length); i++) {
    const l = left.prerelease[i];
    const r = right.prerelease[i];
    if (l === undefined || r === undefined) return l === undefined ? -1 : 1;
    const ln = /^\d+$/.test(l);
    const rn = /^\d+$/.test(r);
    const comparison = ln && rn ? compareNumeric(l, r) : ln !== rn ? (ln ? -1 : 1) : l === r ? 0 : l > r ? 1 : -1;
    if (comparison !== 0) return comparison;
  }
  return 0;
}

export function distributionChannel(version: string): DistributionChannel {
  return parseVersion(version)?.prerelease[0] === 'beta' ? 'beta' : 'stable';
}

export function distributionProfile(channel: DistributionChannel) {
  return channel === 'beta'
    ? { appId: 'com.zabor.desktop.beta', profileName: 'zabor-desktop-beta', executable: 'ZABOR-Beta', updater: 'ZaborUpdaterBeta', updateDirectory: 'zabor-beta-update', splashFile: 'zabor-beta-install-splash.ps1' }
    : { appId: 'com.zabor.desktop', profileName: 'zabor-desktop', executable: 'ZABOR', updater: 'ZaborUpdater', updateDirectory: 'zabor-update', splashFile: 'zabor-install-splash.ps1' };
}

export function isReleaseDownloadUrl(value: unknown): value is string {
  if (typeof value !== 'string' || !value.startsWith(RELEASES_BASE_URL)) return false;
  try {
    const url = new URL(value);
    return url.origin === 'https://github.com' && !url.search && !url.hash && url.pathname.startsWith('/vnkdevelop/zabor-desktop/releases/download/') && url.pathname.split('/').length === 7;
  } catch {
    return false;
  }
}

export function validateBetaManifest(value: unknown, version: string, architecture: string): value is BetaUpdateManifest {
  if (!value || typeof value !== 'object') return false;
  const m = value as BetaUpdateManifest;
  return m.manifestVersion === 1 && m.channel === 'beta' && m.version === version &&
    distributionChannel(version) === 'beta' && /^[0-9a-f]{40}$/.test(m.commit) &&
    m.platform === 'win32' && m.architecture === architecture &&
    m.dataSchema === LOCAL_DATA_SCHEMA && Number.isSafeInteger(m.minimumDataSchema) && Number.isSafeInteger(m.maximumDataSchema) &&
    m.minimumDataSchema > 0 && m.minimumDataSchema <= LOCAL_DATA_SCHEMA && m.maximumDataSchema >= LOCAL_DATA_SCHEMA &&
    Array.isArray(m.enabledModules) && m.enabledModules.length === 0 &&
    !!m.artifact && m.artifact.name === `ZABOR-Beta-Setup-${version}-${architecture}.exe` &&
    Number.isSafeInteger(m.artifact.size) && m.artifact.size > 0 && /^[0-9a-f]{64}$/.test(m.artifact.sha256);
}
