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
let inferenceRunning = false
const pendingFrames: ProcessMessage[] = []
const MAX_PENDING_FRAMES = 4
let lastProcessedSequence = -1

let audioInputName = 'input'
let stateInputName = 'state'
let sampleRateInputName: string | null = 'sr'
let probabilityOutputName = 'output'
let stateOutputName = 'stateN'
let modelFeeds: Record<string, Tensor> | null = null

function resolveTensorNames(loaded: InferenceSession): void {
  const inputs = loaded.inputNames
  const outputs = loaded.outputNames
  const has = (name: string, needle: string) => name.toLowerCase().includes(needle)

  stateInputName = inputs.find(name => has(name, 'state')) ?? stateInputName
  sampleRateInputName = inputs.find(name => has(name, 'sr') || has(name, 'sample')) ?? null
  audioInputName = inputs.find(name =>
    name !== stateInputName && name !== sampleRateInputName) ?? audioInputName

  stateOutputName = outputs.find(name => has(name, 'state')) ?? stateOutputName
  probabilityOutputName = outputs.find(name => name !== stateOutputName) ?? probabilityOutputName
}

interface ProcessMessage {
  type: 'process'
  audioFrame: Float32Array
  sequence: number
  endFrameId: number
  windowRms: number
  epoch?: number
}

const VAD_GAIN = 6.5
const SATURATION_THRESHOLD = 0.65
const SATURATION_CEILING = 0.98
const SATURATION_HEADROOM = SATURATION_CEILING - SATURATION_THRESHOLD

function amplifyVadSample(sample: number): number {
  const scaled = sample * VAD_GAIN
  if (scaled > SATURATION_THRESHOLD) {
    const excess = scaled - SATURATION_THRESHOLD
    return SATURATION_THRESHOLD + (SATURATION_HEADROOM * excess) / (SATURATION_HEADROOM + excess)
  }
  if (scaled < -SATURATION_THRESHOLD) {
    const excess = -scaled - SATURATION_THRESHOLD
    return -SATURATION_THRESHOLD - (SATURATION_HEADROOM * excess) / (SATURATION_HEADROOM + excess)
  }
  return scaled
}

function resetModelState(): void {
  state.fill(0)
  context.fill(0)
  modelInput.fill(0)
  lastProcessedSequence = -1
}

async function processFrame(message: ProcessMessage): Promise<void> {
  if (!session || !modelFeeds || message.audioFrame.length !== FRAME_SIZE) return

  if (lastProcessedSequence >= 0 && message.sequence !== lastProcessedSequence + 1) {
    resetModelState()
  }
  lastProcessedSequence = message.sequence

  try {
    modelInput.set(context, 0)
    for (let i = 0; i < FRAME_SIZE; i++) {
      modelInput[CONTEXT_SIZE + i] = amplifyVadSample(message.audioFrame[i])
    }

    const results = await session.run(modelFeeds)

    const rawProb = Number(results[probabilityOutputName].data[0])
    const probability = Number.isFinite(rawProb) ? Math.max(0, Math.min(1, rawProb)) : 0

    const newStateData = results[stateOutputName].data as Float32Array
    state.set(newStateData)
    context.set(modelInput.subarray(modelInput.length - CONTEXT_SIZE))

    self.postMessage({
      type: 'probability',
      probability,
      epoch: message.epoch,
      sequence: message.sequence,
      endFrameId: message.endFrameId,
      windowRms: message.windowRms
    })
  } catch (error) {
    resetModelState()
    self.postMessage({ type: 'error', phase: 'inference', error: String(error) })
  }
}

interface InitMessage {
  type: 'init'
  model: Uint8Array
  wasmPath: string
}

async function drainFrames(message: ProcessMessage): Promise<void> {
  inferenceRunning = true
  let current: ProcessMessage | null = message
  while (current) {
    await processFrame(current)
    current = pendingFrames.shift() ?? null
  }
  inferenceRunning = false
}

self.onmessage = (event: MessageEvent) => {
  const message = event.data

  if (message.type === 'init') {
    const { model, wasmPath } = message as InitMessage
    if (wasmPath) env.wasm.wasmPaths = wasmPath

    InferenceSession.create(model, {
      executionProviders: ['wasm'],
      graphOptimizationLevel: 'all'
    }).then(createdSession => {
      session = createdSession
      resolveTensorNames(createdSession)
      resetModelState()
      modelFeeds = {
        [audioInputName]: new Tensor('float32', modelInput, [1, modelInput.length]),
        [stateInputName]: new Tensor('float32', state, [2, 1, 128])
      }
      if (sampleRateInputName) modelFeeds[sampleRateInputName] = sampleRateTensor
      self.postMessage({
        type: 'ready',
        io: {
          inputs: createdSession.inputNames,
          outputs: createdSession.outputNames,
          audioInput: audioInputName,
          stateInput: stateInputName,
          sampleRateInput: sampleRateInputName,
          probabilityOutput: probabilityOutputName,
          stateOutput: stateOutputName
        }
      })
    }).catch(error => self.postMessage({ type: 'error', phase: 'initialization', error: String(error) }))
  } else if (message.type === 'process') {
    const frame = message as ProcessMessage
    if (inferenceRunning) {
      if (pendingFrames.length >= MAX_PENDING_FRAMES) pendingFrames.splice(0, pendingFrames.length - 1)
      pendingFrames.push(frame)
    } else {
      void drainFrames(frame)
    }
  } else if (message.type === 'reset') {
    pendingFrames.length = 0
    lastProcessedSequence = -1
    resetModelState()
  }
}
