const { spawnSync } = require('child_process')
const path = require('path')
const fs = require('fs')

const workspaceRoot = path.resolve(__dirname, '..')

try {
  const nativeModuleDir = path.join(workspaceRoot, 'node_modules', 'electron-native-screenshare')
  if (!fs.existsSync(nativeModuleDir)) {
    console.log('[rebuild-native-audio] electron-native-screenshare not installed, skipping.')
    process.exit(0)
  }

  const targetReleaseDir = path.join(nativeModuleDir, 'build', 'Release')
  const targetNodeFile = path.join(targetReleaseDir, 'electron_native_screenshare.node')
  const prebuiltFile = path.join(workspaceRoot, 'build', 'native', 'electron_native_screenshare.node')

  if (fs.existsSync(prebuiltFile)) {
    fs.mkdirSync(targetReleaseDir, { recursive: true })
    fs.copyFileSync(prebuiltFile, targetNodeFile)
    console.log('[rebuild-native-audio] Prebuilt native module applied.')
  }

  const electronPkg = path.join(workspaceRoot, 'node_modules', 'electron', 'package.json')
  if (fs.existsSync(electronPkg)) {
    const electronVersion = require(electronPkg).version
    let nodeGypScript
    try {
      nodeGypScript = require.resolve('node-gyp/bin/node-gyp.js', { paths: [workspaceRoot] })
    } catch {
      // node-gyp not found
    }

    if (nodeGypScript) {
      console.log(`[rebuild-native-audio] Attempting native rebuild for Electron ${electronVersion}...`)
      const result = spawnSync(process.execPath, [
        nodeGypScript,
        'rebuild',
        '--directory', nativeModuleDir,
        '--runtime=electron',
        `--target=${electronVersion}`,
        '--dist-url=https://electronjs.org/headers'
      ], {
        cwd: workspaceRoot,
        env: process.env,
        stdio: 'inherit'
      })

      if (result.status === 0) {
        console.log('[rebuild-native-audio] Native module compiled successfully.')
        process.exit(0)
      } else {
        console.warn('[rebuild-native-audio] Native compilation did not succeed, keeping prebuilt/fallback.')
      }
    }
  }

  process.exit(0)
} catch (err) {
  console.warn('[rebuild-native-audio] Rebuild notice (non-fatal):', err.message)
  process.exit(0)
}

