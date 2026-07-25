import { StandaloneDeepFilter } from 'deepfilter-standalone'

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

class DeepFilterProcessor extends AudioWorkletProcessor {
  private inputBuffer: Float32Array
  private outputBuffer: Float32Array
  private inputReadIndex = 0
  private inputWriteIndex = 0
  private outputReadIndex = 0
  private outputWriteIndex = 0

  private readonly FRAME_SIZE = 480
  private readonly BUFFER_SIZE = 24000
  private readonly SAMPLE_RATE = 48000

  private denoiser: StandaloneDeepFilter | null = null
  private denoiserReady = false

  private readonly frameToProcess: Float32Array
  private readonly processedFrame: Float32Array

  private isMuted = false
  private noiseSuppression = true

  private rmsSmoothed = 0
  private lastVadSent = false
  private overflowCount = 0

  private readonly VAD_FRAME_SIZE = 512
  private readonly VAD_ON_THRESHOLD = 0.28
  private readonly VAD_IMMEDIATE_THRESHOLD = 0.38
  private readonly VAD_OFF_THRESHOLD = 0.16
  private readonly VAD_HOLD_FRAMES = 30
  private vadOnThreshold = this.VAD_ON_THRESHOLD
  private vadHoldFrames = 0
  private consecutiveVoiceResults = 0
  private lastVadSequence = -1

  private attenuationLimit = 65
  private postFilterBeta = 0.05

  private delayFrames: Float32Array[] = []
  private delaySpeaking: boolean[] = []
  private delayWriteIndex = 0
  private delayReadIndex = 0
  private delayCount = 0
  private readonly LOOKAHEAD_FRAMES = 24
  private currentGain = 0.0

  private thresholdMode = 'auto'
  private noiseFloorEstimate = 0.003
  private sileroVadEnabled = false
  private sileroVadProbability = 0.0
  private readonly vad16kBuffer = new Float32Array(this.VAD_FRAME_SIZE)
  private vad16kWriteIndex = 0
  private vadSequence = 0

  private dsMem0 = 0
  private dsMem1 = 0

  constructor() {
    super()
    this.inputBuffer = new Float32Array(this.BUFFER_SIZE)
    this.outputBuffer = new Float32Array(this.BUFFER_SIZE)
    this.frameToProcess = new Float32Array(this.FRAME_SIZE)
    this.processedFrame = new Float32Array(this.FRAME_SIZE)
    this.outputWriteIndex = this.FRAME_SIZE * (1 + this.LOOKAHEAD_FRAMES)

    for (let i = 0; i <= this.LOOKAHEAD_FRAMES; i++) {
      this.delayFrames.push(new Float32Array(this.FRAME_SIZE))
      this.delaySpeaking.push(false)
    }

    this.port.onmessage = (event) => {
      if (event.data.type === 'loadWasm') {
        this.initDeepFilter()
      } else if (event.data.type === 'setConfig') {
        if (event.data.noiseSuppression !== undefined) {
          this.noiseSuppression = event.data.noiseSuppression
        }
        if (event.data.sileroVadEnabled !== undefined) {
          this.sileroVadEnabled = event.data.sileroVadEnabled
        }
        if (event.data.isMuted !== undefined) {
          const nextMuted = event.data.isMuted
          if (nextMuted && !this.isMuted) {
            this.inputBuffer.fill(0)
            this.outputBuffer.fill(0)
            this.inputReadIndex = 0
            this.inputWriteIndex = 0
            this.outputReadIndex = 0
            this.outputWriteIndex = this.FRAME_SIZE * (1 + this.LOOKAHEAD_FRAMES)
            this.rmsSmoothed = 0
            this.currentGain = 0.0
            this.vadHoldFrames = 0
            this.consecutiveVoiceResults = 0
            this.lastVadSequence = -1
            this.delayWriteIndex = 0
            this.delayReadIndex = 0
            this.delayCount = 0
            this.delaySpeaking.fill(false)
            this.vad16kWriteIndex = 0
            this.vad16kBuffer.fill(0)
            this.sileroVadProbability = 0.0
            this.port.postMessage({ type: 'resetVad' })
            if (this.lastVadSent) {
              this.port.postMessage({ type: 'vad', isSpeaking: false })
              this.lastVadSent = false
            }
          }
          this.isMuted = nextMuted
        }
      } else if (event.data.type === 'setCalibratedParams') {
        if (event.data.thresholdMode !== undefined) {
          this.thresholdMode = event.data.thresholdMode
        }
        if (event.data.manualThresholdValue !== undefined && this.thresholdMode === 'manual') {
          const sensitivity = Math.max(0, Math.min(100, event.data.manualThresholdValue))
          this.vadOnThreshold = 0.12 + (100 - sensitivity) * 0.003
        } else if (this.thresholdMode === 'auto') {
          this.vadOnThreshold = this.VAD_ON_THRESHOLD
        }
        if (event.data.attenuationLimit !== undefined) {
          this.attenuationLimit = Math.max(65, Math.min(100, event.data.attenuationLimit))
          if (this.denoiserReady && this.denoiser) {
            try {
              this.denoiser.setAttenuationLimit(this.attenuationLimit)
            } catch {
            }
          }
        }
        if (event.data.postFilterBeta !== undefined) {
          this.postFilterBeta = Math.max(0.0, Math.min(1.0, event.data.postFilterBeta))
          if (this.denoiserReady && this.denoiser) {
            try {
              (this.denoiser as any).setPostFilterBeta?.(this.postFilterBeta)
            } catch {
            }
          }
        }
      } else if (event.data.type === 'setSileroVadProbability') {
        if (this.isMuted || !this.sileroVadEnabled) return
        const sequence = Number(event.data.sequence)
        if (!Number.isFinite(sequence) || sequence <= this.lastVadSequence) return

        this.lastVadSequence = sequence
        this.sileroVadProbability = Math.max(0, Math.min(1, Number(event.data.probability) || 0))

        if (this.sileroVadProbability >= this.vadOnThreshold) {
          this.consecutiveVoiceResults++
        } else if (this.sileroVadProbability < this.VAD_OFF_THRESHOLD) {
          this.consecutiveVoiceResults = 0
        }

        const isVoiceOnset = this.sileroVadProbability >= this.VAD_IMMEDIATE_THRESHOLD ||
          (this.consecutiveVoiceResults >= 2 && this.sileroVadProbability >= this.vadOnThreshold)

        if (isVoiceOnset) {
          this.vadHoldFrames = this.VAD_HOLD_FRAMES
          if (!this.lastVadSent) {
            this.port.postMessage({ type: 'vad', isSpeaking: true })
            this.lastVadSent = true
          }
        } else if (this.vadHoldFrames > 0 && this.sileroVadProbability >= this.VAD_OFF_THRESHOLD) {
          this.vadHoldFrames = Math.max(this.vadHoldFrames, 18)
        }
      }
    }
    this.initDeepFilter()
  }

  private async initDeepFilter() {
    if (this.denoiserReady) return
    try {
      this.denoiser = new StandaloneDeepFilter({
        attenuationLimit: 65,
        postFilterBeta: 0.05
      })
      await this.denoiser.initialize()
      this.denoiser.startStreaming()
      try {
        (this.denoiser as any).setPostFilterBeta?.(0.05)
      } catch {
      }
      this.denoiserReady = true
      this.port.postMessage({ type: 'ready' })
    } catch (e) {
    }
  }

  private pushToBuffer(buffer: Float32Array, data: Float32Array, writeIndex: number, readIndex: number): number {
    const availableSpace = (readIndex - writeIndex - 1 + this.BUFFER_SIZE) % this.BUFFER_SIZE
    if (availableSpace < data.length) {
      this.overflowCount++
      return writeIndex
    }
    const part1 = this.BUFFER_SIZE - writeIndex
    if (part1 >= data.length) {
      buffer.set(data, writeIndex)
      writeIndex = (writeIndex + data.length) % this.BUFFER_SIZE
    } else {
      buffer.set(data.subarray(0, part1), writeIndex)
      buffer.set(data.subarray(part1), 0)
      writeIndex = data.length - part1
    }
    return writeIndex
  }

  private pullFromBuffer(buffer: Float32Array, data: Float32Array, writeIndex: number, readIndex: number): number {
    const availableData = (writeIndex - readIndex + this.BUFFER_SIZE) % this.BUFFER_SIZE
    if (availableData < data.length) {
      data.fill(0)
      return readIndex
    }
    const part1 = this.BUFFER_SIZE - readIndex
    if (part1 >= data.length) {
      data.set(buffer.subarray(readIndex, readIndex + data.length))
      readIndex = (readIndex + data.length) % this.BUFFER_SIZE
    } else {
      data.set(buffer.subarray(readIndex, this.BUFFER_SIZE), 0)
      data.set(buffer.subarray(0, data.length - part1), part1)
      readIndex = data.length - part1
    }
    return readIndex
  }

  process(inputs: Float32Array[][], outputs: Float32Array[][]): boolean {
    const input = inputs[0]
    const output = outputs[0]

    if (!input?.length || !output?.length) return true

    const inputChannel = input[0]
    const outputChannel = output[0]

    if (this.isMuted) {
      outputChannel.fill(0)
      return true
    }

    this.inputWriteIndex = this.pushToBuffer(this.inputBuffer, inputChannel, this.inputWriteIndex, this.inputReadIndex)

    while ((this.inputWriteIndex - this.inputReadIndex + this.BUFFER_SIZE) % this.BUFFER_SIZE >= this.FRAME_SIZE) {
      this.inputReadIndex = this.pullFromBuffer(this.inputBuffer, this.frameToProcess, this.inputWriteIndex, this.inputReadIndex)

      for (let i = 0; i < this.FRAME_SIZE; i += 3) {
        const s0 = this.frameToProcess[i]
        const s1 = this.frameToProcess[i + 1]
        const s2 = this.frameToProcess[i + 2]

        this.dsMem0 = 0.5 * this.dsMem0 + 0.25 * s0 + 0.25 * s1
        this.dsMem1 = 0.5 * this.dsMem1 + 0.25 * s1 + 0.25 * s2
        const filteredSample = 0.5 * (this.dsMem0 + this.dsMem1)

        const softClamped = filteredSample / (1.0 + Math.abs(filteredSample))
        this.vad16kBuffer[this.vad16kWriteIndex++] = Math.max(-0.85, Math.min(0.85, softClamped))

        if (this.vad16kWriteIndex === this.VAD_FRAME_SIZE) {
          const audioFrame = this.vad16kBuffer.slice()
          this.port.postMessage(
            { type: 'audio16k', audio: audioFrame, sequence: this.vadSequence++ },
            [audioFrame.buffer]
          )
          this.vad16kWriteIndex = 0
        }
      }

      if (this.noiseSuppression && this.denoiserReady && this.denoiser) {
        const cleanFrame = this.denoiser.processStreaming(this.frameToProcess)
        this.processedFrame.set(cleanFrame)
      } else {
        this.processedFrame.set(this.frameToProcess)
      }

      let sumSquares = 0
      for (let i = 0; i < this.FRAME_SIZE; i++) {
        sumSquares += this.frameToProcess[i] * this.frameToProcess[i]
      }
      const currentRms = Math.sqrt(sumSquares / this.FRAME_SIZE)
      this.rmsSmoothed = 0.2 * currentRms + 0.8 * this.rmsSmoothed

      const isSpeaking = this.sileroVadEnabled && this.vadHoldFrames > 0
      if (this.vadHoldFrames > 0) {
        this.vadHoldFrames--
      } else if (this.lastVadSent) {
        this.port.postMessage({ type: 'vad', isSpeaking: false })
        this.lastVadSent = false
      }

      if (!isSpeaking && currentRms < 0.03) {
        this.noiseFloorEstimate = 0.995 * this.noiseFloorEstimate + 0.005 * currentRms
        this.noiseFloorEstimate = Math.max(0.0001, Math.min(0.02, this.noiseFloorEstimate))
      }

      const delayBufferSize = this.LOOKAHEAD_FRAMES + 1
      const writeIdx = this.delayWriteIndex
      this.delayFrames[writeIdx].set(this.processedFrame)
      this.delaySpeaking[writeIdx] = isSpeaking
      this.delayWriteIndex = (writeIdx + 1) % delayBufferSize
      this.delayCount++

      if (this.delayCount > this.LOOKAHEAD_FRAMES) {
        const readIdx = this.delayReadIndex
        const oldestFrame = this.delayFrames[readIdx]

        let anySpeakingAhead = false
        for (let k = 0; k < delayBufferSize; k++) {
          if (k < this.delayCount) {
            const idx = (readIdx + k) % delayBufferSize
            if (this.delaySpeaking[idx]) {
              anySpeakingAhead = true
              break
            }
          }
        }

        const targetGain = anySpeakingAhead ? 1.0 : 0.0
        this.currentGain = 0.82 * this.currentGain + 0.18 * targetGain

        if (this.currentGain < 0.001) {
          oldestFrame.fill(0)
        } else if (this.currentGain < 0.999) {
          for (let i = 0; i < this.FRAME_SIZE; i++) {
            oldestFrame[i] *= this.currentGain
          }
        }

        this.outputWriteIndex = this.pushToBuffer(this.outputBuffer, oldestFrame, this.outputWriteIndex, this.outputReadIndex)
        this.delayReadIndex = (readIdx + 1) % delayBufferSize
        this.delayCount--
      }
    }

    const availableOutput = (this.outputWriteIndex - this.outputReadIndex + this.BUFFER_SIZE) % this.BUFFER_SIZE
    if (availableOutput >= outputChannel.length) {
      this.outputReadIndex = this.pullFromBuffer(this.outputBuffer, outputChannel, this.outputWriteIndex, this.outputReadIndex)
    } else {
      outputChannel.fill(0)
    }

    return true
  }
}

registerProcessor('deepfilter-processor', DeepFilterProcessor)