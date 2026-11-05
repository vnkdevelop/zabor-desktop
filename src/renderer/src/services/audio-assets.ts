export const DEEPFILTER_WASM_ASSET = 'pkg/df_bg.wasm'
export const DEEPFILTER_MODEL_ASSET = 'models/DeepFilterNet3_onnx.tar.gz'

export interface DeepFilterPayload {
  wasmBytes: ArrayBuffer
  modelBytes: ArrayBuffer
}

let dfWasmBytes: ArrayBuffer | null = null
let dfModelBytes: ArrayBuffer | null = null
let sileroModelBytes: ArrayBuffer | null = null
let sileroInFlight: Promise<void> | null = null
let deepfilterPreload: Promise<void> | null = null
let sileroPreload: Promise<void> | null = null
const inFlightAssets = new Map<string, Promise<ArrayBuffer | null>>()
let rustVoicingModule: Promise<WebAssembly.Module | null> | null = null

export function getRustVoicingModule(): Promise<WebAssembly.Module | null> {
  if (!rustVoicingModule) {
    rustVoicingModule = (async () => {
      if (typeof window.windowControls.getClientRuntime !== 'function') return null
      const runtime = await window.windowControls.getClientRuntime()
      if (!runtime.rustVoicing) return null
      const bytes = await window.windowControls.loadRustVoicing()
      if (!bytes) throw new Error('rust-voicing-asset-missing')
      return await WebAssembly.compile(toArrayBuffer(bytes))
    })().catch(error => {
      console.warn('[AudioAssets] Rust voicing unavailable; preserving legacy DSP:', error)
      rustVoicingModule = null
      return null
    })
  }
  return rustVoicingModule
}

function toArrayBuffer(bytes: Uint8Array): ArrayBuffer {
  return (bytes.byteOffset === 0 && bytes.byteLength === bytes.buffer.byteLength
    ? bytes.buffer
    : bytes.slice().buffer) as ArrayBuffer
}

async function readDeepFilterAsset(rel: string): Promise<ArrayBuffer | null> {
  try {
    const bytes = await window.windowControls.loadDeepFilterAsset(rel)
    if (bytes && bytes.byteLength > 0) return toArrayBuffer(bytes)
    console.error(`[AudioAssets] Bundled DeepFilter asset "${rel}" is missing from this installation`)
  } catch (error) {
    console.error(`[AudioAssets] Bundled DeepFilter asset "${rel}" could not be read:`, error)
  }
  return null
}

function cacheDeepFilterAsset(rel: string, buffer: ArrayBuffer): void {
  if (rel === DEEPFILTER_MODEL_ASSET) {
    if (!dfModelBytes) dfModelBytes = buffer.slice(0)
    return
  }
  if (rel === DEEPFILTER_WASM_ASSET && !dfWasmBytes) dfWasmBytes = buffer.slice(0)
}

export async function getDeepFilterAsset(rel: string): Promise<ArrayBuffer | null> {
  if (rel === DEEPFILTER_MODEL_ASSET && dfModelBytes) return dfModelBytes.slice(0)
  if (rel === DEEPFILTER_WASM_ASSET && dfWasmBytes) return dfWasmBytes.slice(0)

  const pending = inFlightAssets.get(rel)
  if (pending) {
    const buffer = await pending
    return buffer ? buffer.slice(0) : null
  }

  const request = readDeepFilterAsset(rel).finally(() => inFlightAssets.delete(rel))
  inFlightAssets.set(rel, request)
  const buffer = await request
  if (buffer) cacheDeepFilterAsset(rel, buffer)
  return buffer
}

export function getDeepFilterPayload(): DeepFilterPayload | null {
  if (!dfWasmBytes || !dfModelBytes) return null
  return {
    wasmBytes: dfWasmBytes.slice(0),
    modelBytes: dfModelBytes.slice(0)
  }
}

export async function getSileroModel(): Promise<Uint8Array> {
  if (!sileroModelBytes) {
    if (!sileroInFlight) {
      sileroInFlight = window.windowControls.loadSileroModel()
        .then(bytes => {
          if (!bytes || bytes.byteLength === 0) throw new Error('Silero model file is empty')
          sileroModelBytes = toArrayBuffer(bytes)
        })
        .finally(() => { sileroInFlight = null })
    }
    await sileroInFlight
  }
  if (!sileroModelBytes) throw new Error('Silero model is unavailable')
  return new Uint8Array(sileroModelBytes.slice(0))
}

function preloadDeepFilterAssets(): Promise<void> {
  if (dfWasmBytes && dfModelBytes) return Promise.resolve()
  if (!deepfilterPreload) {
    const startedAt = performance.now()
    deepfilterPreload = Promise.all([
      dfWasmBytes ? null : getDeepFilterAsset(DEEPFILTER_WASM_ASSET),
      dfModelBytes ? null : getDeepFilterAsset(DEEPFILTER_MODEL_ASSET)
    ])
      .then(() => {
        if (!dfWasmBytes || !dfModelBytes) {
          deepfilterPreload = null
          return
        }
        console.info(
          `[AudioAssets] DeepFilterNet3 assets ready: wasm ${(dfWasmBytes.byteLength / 1_048_576).toFixed(1)} MB, ` +
          `model ${(dfModelBytes.byteLength / 1_048_576).toFixed(1)} MB in ${Math.round(performance.now() - startedAt)}ms`
        )
      })
      .catch(error => {
        deepfilterPreload = null
        console.warn('[AudioAssets] DeepFilter asset preload failed:', error)
      })
  }
  return deepfilterPreload
}

function preloadSileroAsset(): Promise<void> {
  if (sileroModelBytes) return Promise.resolve()
  if (!sileroPreload) {
    const startedAt = performance.now()
    sileroPreload = getSileroModel()
      .then(() => {
        const size = sileroModelBytes ? (sileroModelBytes.byteLength / 1_048_576).toFixed(1) : '?'
        console.info(`[AudioAssets] Silero model ready: ${size} MB in ${Math.round(performance.now() - startedAt)}ms`)
      })
      .catch(error => {
        sileroPreload = null
        console.warn('[AudioAssets] Silero model preload failed:', error)
      })
  }
  return sileroPreload
}

export function preloadNoiseAssets(model: 'deepfilter' | 'rnnoise' | 'analyzer' = 'deepfilter'): Promise<void> {
  const jobs = [preloadSileroAsset()]
  if (model === 'deepfilter') jobs.push(preloadDeepFilterAssets())
  return Promise.all(jobs).then(() => undefined)
}
