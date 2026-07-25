import { env, InferenceSession, Tensor } from 'onnxruntime-web'

env.wasm.wasmPaths = './'

const SAMPLE_RATE = 16000
const FRAME_SIZE = 512
const CONTEXT_SIZE = 64
const STATE_SIZE = 2 * 1 * 128

let session: InferenceSession | null = null
const state = new Float32Array(STATE_SIZE)
const context = new Float32Array(CONTEXT_SIZE)
const modelInput = new Float32Array(CONTEXT_SIZE + FRAME_SIZE)
const sampleRateTensor = new Tensor(
  'int64',
  BigInt64Array.from([BigInt(SAMPLE_RATE)]),
  [1]
)
let inferenceQueue = Promise.resolve()

interface ProcessMessage {
  type: 'process'
  audioFrame: Float32Array
  sequence: number
}

function resetModelState(): void {
  state.fill(0)
  context.fill(0)
  modelInput.fill(0)
}

async function processFrame(message: ProcessMessage): Promise<void> {
  if (!session || message.audioFrame.length !== FRAME_SIZE) return

  let maxPeak = 0
  for (let i = 0; i < FRAME_SIZE; i++) {
    const absVal = Math.abs(message.audioFrame[i])
    if (absVal > maxPeak) maxPeak = absVal
  }

  try {
    modelInput.set(context, 0)
    modelInput.set(message.audioFrame, CONTEXT_SIZE)

    const results = await session.run({
      input: new Tensor('float32', modelInput, [1, modelInput.length]),
      state: new Tensor('float32', state, [2, 1, 128]),
      sr: sampleRateTensor
    })

    const rawProb = Number(results.output.data[0])
    const probability = Number.isFinite(rawProb) ? Math.max(0, Math.min(1, rawProb)) : 0

    const newStateData = results.stateN.data as Float32Array
    state.set(newStateData)
    context.set(message.audioFrame.subarray(FRAME_SIZE - CONTEXT_SIZE))

    if (probability < 0.25) {
      for (let i = 0; i < STATE_SIZE; i++) {
        state[i] *= 0.70
      }
    }

    if (probability < 0.20 && maxPeak > 0.25) {
      state.fill(0)
      context.fill(0)
    }

    self.postMessage({
      type: 'probability',
      probability,
      sequence: message.sequence
    })
  } catch (error) {
    resetModelState()
    self.postMessage({ type: 'error', error: String(error) })
  }
}

self.onmessage = (event: MessageEvent) => {
  const message = event.data

  if (message.type === 'init') {
    const { modelUrl, wasmPath } = message
    if (wasmPath) env.wasm.wasmPaths = wasmPath

    inferenceQueue = inferenceQueue.then(async () => {
      try {
        session = await InferenceSession.create(modelUrl, {
          executionProviders: ['wasm'],
          graphOptimizationLevel: 'all'
        })
        resetModelState()
        self.postMessage({ type: 'ready' })
      } catch (error) {
        self.postMessage({ type: 'error', error: String(error) })
      }
    })
  } else if (message.type === 'process') {
    inferenceQueue = inferenceQueue.then(() => processFrame(message as ProcessMessage))
  } else if (message.type === 'reset') {
    inferenceQueue = inferenceQueue.then(() => resetModelState())
  }
}
