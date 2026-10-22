const { execSync } = require('child_process');
const { existsSync } = require('fs');
const { join } = require('path');

if (process.platform !== 'win32') {
  process.exit(0);
}

const cscPath = 'C:\\Windows\\Microsoft.NET\\Framework64\\v4.0.30319\\csc.exe';
const wpfDir = 'C:\\Windows\\Microsoft.NET\\Framework64\\v4.0.30319\\WPF';
const srcFile = join(__dirname, '..', 'build', 'ZaborUpdater.cs');
const outFile = join(__dirname, '..', 'build', 'ZaborUpdater.exe');

if (!existsSync(cscPath) || !existsSync(srcFile)) {
  console.warn('[build-splash] csc.exe or ZaborUpdater.cs not found, skipping');
  process.exit(0);
}

try {
  const cmd = `"${cscPath}" /target:winexe /optimize+ /platform:anycpu /out:"${outFile}" /lib:"${wpfDir}" /r:PresentationCore.dll /r:PresentationFramework.dll /r:WindowsBase.dll /r:System.dll /r:System.Xaml.dll "${srcFile}"`;
  execSync(cmd, { stdio: 'inherit' });
  console.log('[build-splash] Successfully compiled ZaborUpdater.exe');

  const simSrc = join(__dirname, '..', 'build', 'ZaborInputSimulator.cs');
  const simOut = join(__dirname, '..', 'build', 'ZaborInputSimulator.exe');
  if (existsSync(simSrc)) {
    const simCmd = `"${cscPath}" /target:exe /optimize+ /platform:x64 /out:"${simOut}" "${simSrc}"`;
    execSync(simCmd, { stdio: 'inherit' });
    console.log('[build-splash] Successfully compiled ZaborInputSimulator.exe');
  }
} catch (err) {
  console.error('[build-splash] Failed to compile ZaborUpdater.exe:', err);
  process.exit(1);
}
