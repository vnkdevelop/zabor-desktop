import assert from 'node:assert/strict';
import { test } from 'node:test';
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { join } from 'node:path';

test('beta installer owns the replacement splash and does not close it before relaunch', () => {
  const installer = readFileSync('build/installer-beta.nsh', 'utf8');
  assert.ok(installer.includes('FindWindow $0 "" "ZaborUpdaterBetaInstall"'));
  assert.ok(installer.includes("System::Call 'kernel32::GetCurrentProcessId() i .r2'"));
  assert.ok(installer.includes('ZaborUpdaterBeta.exe" 0 beta $2'));
  assert.doesNotMatch(installer, /taskkill[^\r\n]*ZaborUpdaterBeta/);
  assert.ok(installer.includes('"open" "--updated"'));
  const native = readFileSync('build/ZaborUpdater.cs', 'utf8');
  const beta = native.slice(native.indexOf('                if (beta) {'), native.indexOf('// Primary closing mechanism:'));
  assert.ok(beta.includes('installerProcess.HasExited'));
  assert.ok(beta.includes('zp.MainWindowHandle != IntPtr.Zero'));
  assert.ok(beta.includes('zp.StartTime.ToUniversalTime() >= startedAt'));
  assert.doesNotMatch(beta, /Elapsed\.TotalSeconds\s*>\s*(?:10|15)/);
  assert.ok(beta.includes('ShouldCloseBetaSplash(installerRunning, replacementWindowVisible)'));
});

test('native beta splash policy preserves progress until install ends and the replacement window is visible', t => {
  if (process.platform !== 'win32') return t.skip('Windows native compiler check');
  const framework = join(process.env.WINDIR || 'C:/Windows', 'Microsoft.NET/Framework64/v4.0.30319');
  const csc = join(framework, 'csc.exe');
  if (!existsSync(csc)) return t.skip('Native .NET Framework compiler unavailable');
  const base = join(process.env.LOCALAPPDATA, 'Temp/opencode');
  mkdirSync(base, { recursive: true });
  const root = mkdtempSync(join(base, 'zabor-updater-policy-'));
  try {
    const code = join(root, 'policy.cs');
    const output = join(root, 'policy.exe');
    writeFileSync(join(root, 'ZaborUpdater.cs'), readFileSync('build/ZaborUpdater.cs'));
    writeFileSync(code, 'using System; class SplashPolicyTest { static void Main() { for(int i=0;i<10000;i++) { if(ZaborUpdater.Program.ShouldCloseBetaSplash(true,false)||ZaborUpdater.Program.ShouldCloseBetaSplash(true,true)||ZaborUpdater.Program.ShouldCloseBetaSplash(false,false)||!ZaborUpdater.Program.ShouldCloseBetaSplash(false,true)) throw new Exception("invalid-splash-lifecycle"); } Console.WriteLine("native-beta-splash-policy-passed"); } }');
    execFileSync('cmd.exe', ['/c', csc, '/nologo', '/target:exe', '/main:SplashPolicyTest', '/out:policy.exe', `/lib:${join(framework, 'WPF')}`,
      '/r:PresentationCore.dll', '/r:PresentationFramework.dll', '/r:WindowsBase.dll', '/r:System.dll', '/r:System.Xaml.dll', 'ZaborUpdater.cs', 'policy.cs'], { cwd: root, stdio: 'pipe' });
    assert.match(execFileSync('cmd.exe', ['/c', output], { encoding: 'utf8' }), /native-beta-splash-policy-passed/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
