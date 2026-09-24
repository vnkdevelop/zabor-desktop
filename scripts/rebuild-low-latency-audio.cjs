const { spawnSync } = require('child_process')
const path = require('path')
const fs = require('fs')

const workspaceRoot = path.resolve(__dirname, '..')
const moduleDir = path.join(workspaceRoot, 'native', 'low-latency-audio')

if (!fs.existsSync(moduleDir)) {
  console.warn('[low-latency-audio] module directory missing, skipping build')
  process.exit(0)
}

if (!fs.existsSync(path.join(moduleDir, 'node_modules', 'node-addon-api'))) {
  const install = spawnSync('npm', ['install', '--no-audit', '--no-fund'], {
    cwd: moduleDir,
    env: process.env,
    stdio: 'inherit',
    shell: process.platform === 'win32'
  })
  if (install.status !== 0) {
    console.warn('[low-latency-audio] dependency install failed, skipping native build')
    process.exit(0)
  }
}

let nodeGypScript
try {
  nodeGypScript = require.resolve('node-gyp/bin/node-gyp.js', { paths: [workspaceRoot, moduleDir] })
} catch (e) {
  console.warn('[low-latency-audio] node-gyp not found, skipping native build')
  process.exit(0)
}

const electronPkg = path.join(workspaceRoot, 'node_modules', 'electron', 'package.json')
const args = [nodeGypScript, 'rebuild', '--directory', moduleDir]

if (fs.existsSync(electronPkg)) {
  const electronVersion = require(electronPkg).version
  args.push('--runtime=electron', `--target=${electronVersion}`, '--dist-url=https://electronjs.org/headers')
}

const result = spawnSync(process.execPath, args, {
  cwd: workspaceRoot,
  env: process.env,
  stdio: 'inherit'
})

if (result.error) {
  console.warn('[low-latency-audio] native build failed:', result.error.message)
  process.exit(0)
}

process.exit(result.status ?? 0)
