import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { homedir } from 'node:os';
import { spawnSync } from 'node:child_process';

const installedCargo = join(process.env.CARGO_HOME || join(homedir(), '.cargo'), 'bin', process.platform === 'win32' ? 'cargo.exe' : 'cargo');
const cargo = existsSync(installedCargo) ? installedCargo : 'cargo';
const steps = [
  ['fmt', '--all', '--', '--check'],
  ['clippy', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings'],
  ['test', '--workspace', '--locked'],
  ['build', '--release', '--locked', '-p', 'zabor-dsp', '--example', 'parity_runner']
];
for (const args of steps) {
  const result = spawnSync(cargo, args, { cwd: 'rust', stdio: 'inherit', windowsHide: true });
  if (result.status !== 0) {
    if (result.error) console.error(result.error.message);
    process.exit(result.status || 1);
  }
}
const result = spawnSync(process.execPath, ['--disable-warning=MODULE_TYPELESS_PACKAGE_JSON', '--test', 'tests/migration-dsp-parity.test.mjs'], { stdio: 'inherit', windowsHide: true });
process.exitCode = result.status || (result.error ? 1 : 0);
