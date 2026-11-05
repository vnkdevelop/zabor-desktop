import { compareVersions, parseVersion } from '../src/main/updatePolicy.ts';

export function releaseChannelForBranch(branch, requested = 'auto') {
  const channel = branch === 'main' ? 'stable' : branch === 'beta/rust-migration' ? 'beta' : null;
  if (!channel || (requested !== 'auto' && requested !== channel)) throw new Error('Release must run on main or beta/rust-migration with its matching channel');
  return channel;
}

export function suggestReleaseVersion(version, channel, occupied = false) {
  const parsed = parseVersion(version);
  if (!parsed) throw new Error('Invalid package version');
  if (channel === 'stable') {
    if (parsed.prerelease.length && !occupied) return parsed.core.join('.');
    return occupied ? `${parsed.core[0]}.${parsed.core[1]}.${BigInt(parsed.core[2]) + 1n}` : version;
  }
  if (parsed.prerelease[0] === 'beta') {
    if (!occupied) return version;
    const parts = [...parsed.prerelease];
    const last = parts.at(-1);
    if (/^\d+$/.test(last)) parts[parts.length - 1] = String(BigInt(last) + 1n);
    else parts.push('1');
    return `${parsed.core.join('.')}-${parts.join('.')}`;
  }
  return `${parsed.core[0]}.${parsed.core[1]}.${BigInt(parsed.core[2]) + 1n}-beta.rust.1`;
}

export function validateReleaseVersion(next, current, channel) {
  const parsed = parseVersion(next);
  if (!parsed || parsed.normalized !== next || (channel === 'stable' ? parsed.prerelease.length !== 0 : parsed.prerelease[0] !== 'beta')) throw new Error('Version does not match release channel');
  if ((compareVersions(next, current) ?? -1) < 0) throw new Error('Publishing a version downgrade is not permitted');
}
