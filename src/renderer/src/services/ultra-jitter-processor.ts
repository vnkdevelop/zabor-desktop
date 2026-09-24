export {}

declare abstract class AudioWorkletProcessor {
  readonly port: MessagePort
  abstract process(
    inputs: Float32Array[][],
    outputs: Float32Array[][],
    parameters: Record<string, Float32Array>
  ): boolean
  constructor()
}

declare function registerProcessor(
  name: string,
  processorCtor: new () => AudioWorkletProcessor
): void

type AudioChunkMessage = {
  type: 'audio'
  buffer: ArrayBuffer
  channels: number
}

type ControlMessage = {
  type: 'reset'
}

type InboundMessage = AudioChunkMessage | ControlMessage

const SAMPLE_RATE = 48000
const RING_FRAMES = SAMPLE_RATE
const MIN_TARGET_FRAMES = Math.round(SAMPLE_RATE * 0.010)
const START_TARGET_FRAMES = Math.round(SAMPLE_RATE * 0.012)
const MAX_TARGET_FRAMES = Math.round(SAMPLE_RATE * 0.060)
const UNDERRUN_GROW_FRAMES = Math.round(SAMPLE_RATE * 0.005)
const TARGET_DECAY_FRAMES = Math.round(SAMPLE_RATE * 0.002)
const DROP_MARGIN_FRAMES = Math.round(SAMPLE_RATE * 0.020)
const RESTART_STARVED_FRAMES = Math.round(SAMPLE_RATE * 0.150)
const FADE_STEP = 1 / Math.round(SAMPLE_RATE * 0.002)
const DRIFT_GAIN = 0.05
const MAX_RATE_DEVIATION = 0.02
const MIN_READABLE_FRAMES = 2
const CLEAN_DECAY_FRAMES = SAMPLE_RATE
const STATS_INTERVAL_FRAMES = Math.round(SAMPLE_RATE * 0.25)

class UltraJitterProcessor extends AudioWorkletProcessor {
  private readonly left = new Float32Array(RING_FRAMES)
  private readonly right = new Float32Array(RING_FRAMES)
  private readPosition = 0
  private writeIndex = 0
  private availableFrames = 0
  private playing = false
  private starvedFrames = 0
  private fadeGain = 0
  private lastLeft = 0
  private lastRight = 0
  private targetFrames = START_TARGET_FRAMES
  private underruns = 0
  private cleanFrames = 0
  private statsCountdown = STATS_INTERVAL_FRAMES

  constructor() {
    super()
    this.port.onmessage = (event: MessageEvent<InboundMessage>) => {
      const message = event.data
      if (message.type === 'reset') { this.reset(); return }
      if (message.type !== 'audio' || !(message.buffer instanceof ArrayBuffer)) return
      if (message.channels !== 1 && message.channels !== 2) return

      const samples = new Float32Array(message.buffer)
      const frames = Math.floor(samples.length / message.channels)
      if (frames === 0) return

      if (this.availableFrames < MIN_READABLE_FRAMES) this.spliceAtReadHead()

      for (let frame = 0; frame < frames; frame++) {
        const sampleIndex = frame * message.channels
        this.left[this.writeIndex] = samples[sampleIndex]
        this.right[this.writeIndex] = message.channels === 2 ? samples[sampleIndex + 1] : samples[sampleIndex]
        this.writeIndex = (this.writeIndex + 1) % RING_FRAMES
        this.availableFrames++
      }
      if (!this.playing && this.availableFrames >= this.targetFrames) {
        this.playing = true
        this.starvedFrames = 0
      }

      const ceiling = this.targetFrames + DROP_MARGIN_FRAMES
      if (this.availableFrames > ceiling) this.dropOldest(this.availableFrames - this.targetFrames)
    }
  }

  private reset() {
    this.readPosition = 0
    this.writeIndex = 0
    this.availableFrames = 0
    this.playing = false
    this.starvedFrames = 0
    this.fadeGain = 0
    this.lastLeft = 0
    this.lastRight = 0
    this.targetFrames = START_TARGET_FRAMES
    this.cleanFrames = 0
  }

  private spliceAtReadHead() {
    const readIndex = Math.floor(this.readPosition)
    this.readPosition = readIndex
    this.writeIndex = readIndex
    this.availableFrames = 0
  }

  private dropOldest(frames: number) {
    const dropped = Math.min(frames, this.availableFrames)
    this.readPosition = (Math.floor(this.readPosition) + dropped) % RING_FRAMES
    this.availableFrames -= dropped
  }

  private readFrame(channel: Float32Array, position: number): number {
    const base = Math.floor(position)
    const fraction = position - base
    const current = channel[base % RING_FRAMES]
    if (fraction === 0) return current
    const next = channel[(base + 1) % RING_FRAMES]
    return current + (next - current) * fraction
  }

  private emitStats() {
    this.port.postMessage({
      type: 'stats',
      bufferMs: (this.availableFrames / SAMPLE_RATE) * 1000,
      targetMs: (this.targetFrames / SAMPLE_RATE) * 1000,
      underruns: this.underruns
    })
  }

  process(_inputs: Float32Array[][], outputs: Float32Array[][]): boolean {
    const output = outputs[0]
    if (!output?.length) return true

    const frames = output[0].length
    const stereo = output[1]

    this.statsCountdown -= frames
    if (this.statsCountdown <= 0) {
      this.statsCountdown += STATS_INTERVAL_FRAMES
      this.emitStats()
    }

    if (!this.playing) {
      this.fadeGain = 0
      this.lastLeft = 0
      this.lastRight = 0
      output.forEach(channel => channel.fill(0))
      return true
    }

    const error = (this.availableFrames - this.targetFrames) / this.targetFrames
    const rate = Math.max(
      1 - MAX_RATE_DEVIATION,
      Math.min(1 + MAX_RATE_DEVIATION, 1 + DRIFT_GAIN * error)
    )

    for (let frame = 0; frame < frames; frame++) {
      if (this.availableFrames < MIN_READABLE_FRAMES) {
        if (this.starvedFrames === 0) {
          this.underruns++
          this.targetFrames = Math.min(MAX_TARGET_FRAMES, this.targetFrames + UNDERRUN_GROW_FRAMES)
        }
        this.starvedFrames++
        this.cleanFrames = 0
        if (this.starvedFrames >= RESTART_STARVED_FRAMES) this.playing = false
        if (this.fadeGain > 0) this.fadeGain = Math.max(0, this.fadeGain - FADE_STEP)
        output[0][frame] = this.lastLeft * this.fadeGain
        if (stereo) stereo[frame] = this.lastRight * this.fadeGain
        continue
      }

      this.starvedFrames = 0
      this.cleanFrames++
      if (this.cleanFrames >= CLEAN_DECAY_FRAMES && this.targetFrames > MIN_TARGET_FRAMES) {
        this.cleanFrames = 0
        this.targetFrames = Math.max(MIN_TARGET_FRAMES, this.targetFrames - TARGET_DECAY_FRAMES)
      }
      if (this.fadeGain < 1) this.fadeGain = Math.min(1, this.fadeGain + FADE_STEP)
      this.lastLeft = this.readFrame(this.left, this.readPosition)
      this.lastRight = stereo ? this.readFrame(this.right, this.readPosition) : this.lastLeft
      output[0][frame] = this.lastLeft * this.fadeGain
      if (stereo) stereo[frame] = this.lastRight * this.fadeGain

      const advanced = this.readPosition + rate
      const consumed = Math.floor(advanced) - Math.floor(this.readPosition)
      this.readPosition = advanced % RING_FRAMES
      this.availableFrames -= consumed
    }

    return true
  }
}

registerProcessor('ultra-jitter-processor', UltraJitterProcessor)
