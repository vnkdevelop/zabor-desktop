'use strict'

let native = null
let loadError = null

try {
  native = require('./build/Release/zabor_low_latency_audio.node')
} catch (e) {
  loadError = e && e.message ? e.message : String(e)
}

function available() {
  return Boolean(native)
}

function probe() {
  if (!native) return { ok: false, error: loadError || 'native module not built' }
  try {
    return native.probe()
  } catch (e) {
    return { ok: false, error: e && e.message ? e.message : String(e) }
  }
}

module.exports = { available, probe, loadError }
