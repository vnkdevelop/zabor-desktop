import * as realChildProcess from 'node:child_process';

export const spawnedProcesses = [];

export function spawn(command, args, options) {
  const mockChild = {
    pid: 99999,
    command,
    args: args || [],
    options: options || {},
    unrefCalled: false,
    on(_event, _handler) {
      return this;
    },
    unref() {
      this.unrefCalled = true;
    }
  };
  spawnedProcesses.push(mockChild);
  return mockChild;
}

export function resetSpawnedProcesses() {
  spawnedProcesses.length = 0;
}

export const execFile = realChildProcess.execFile;
export const exec = realChildProcess.exec;
export const fork = realChildProcess.fork;
export const execSync = realChildProcess.execSync;
export const spawnSync = realChildProcess.spawnSync;
export const execFileSync = realChildProcess.execFileSync;

export default {
  ...realChildProcess,
  spawn,
  spawnedProcesses,
  resetSpawnedProcesses
};
