const { existsSync, statSync } = require('node:fs');
const { join } = require('node:path');
const { spawnSync } = require('node:child_process');

if (process.platform !== 'win32') process.exit(0);
const root = join(__dirname, '..');
const source = join(root, 'build', 'ZaborInputSimulator.cs');
const output = join(root, 'build', 'ZaborInputSimulator.exe');
if (existsSync(output) && statSync(output).mtimeMs >= statSync(source).mtimeMs) process.exit(0);
const compiler = join(process.env.WINDIR || 'C:\\Windows', 'Microsoft.NET', 'Framework64', 'v4.0.30319', 'csc.exe');
if (!existsSync(compiler)) throw new Error('Windows .NET Framework compiler is required to build the input simulator');
const result = spawnSync(compiler, ['/nologo', '/target:exe', '/platform:x64', '/optimize+', `/out:${output}`, source], { stdio: 'inherit', windowsHide: true });
if (result.error) throw result.error;
if (result.status !== 0) process.exit(result.status || 1);
const check = spawnSync(output, ['--self-test'], { stdio: 'inherit', windowsHide: true });
if (check.status !== 0) process.exit(check.status || 1);
