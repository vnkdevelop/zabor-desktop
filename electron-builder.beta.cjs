const base = require('./package.json').build;
const { execFileSync } = require('node:child_process');

module.exports = {
  ...base,
  appId: 'com.zabor.desktop.beta',
  productName: 'ZABOR Beta',
  extraMetadata: { zaborBuild: { channel: 'beta', commit: execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8', windowsHide: true }).trim() } },
  directories: { ...base.directories, output: 'release/beta' },
  extraResources: base.extraResources.map(resource => resource.to === 'ZaborUpdater.exe'
    ? { ...resource, to: 'ZaborUpdaterBeta.exe' } : resource),
  win: { ...base.win, executableName: 'ZABOR-Beta' },
  nsis: {
    ...base.nsis,
    allowToChangeInstallationDirectory: false,
    shortcutName: 'ZABOR Beta',
    artifactName: 'ZABOR-Beta-Setup-${version}-${arch}.${ext}',
    include: 'build/installer-beta.nsh',
    deleteAppDataOnUninstall: false
  },
  publish: null
};
