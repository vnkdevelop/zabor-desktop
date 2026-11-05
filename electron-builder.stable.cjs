const pkg = require('./package.json');
const { execFileSync } = require('node:child_process');

module.exports = {
  ...pkg.build,
  extraMetadata: { zaborBuild: { channel: 'stable', commit: execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8', windowsHide: true }).trim(), runtime: pkg.zaborRuntime } },
  nsis: { ...pkg.build.nsis, include: 'build/installer-stable.nsh' },
  publish: null
};
