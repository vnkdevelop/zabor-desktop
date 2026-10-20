import fvad from '@echogarden/fvad-wasm'
import type { FvadModule } from '@echogarden/fvad-wasm'
import { FVAD_WASM_BASE64 } from './fvad-binary.ts'

export async function createWebRtcVadModule(): Promise<FvadModule> {
  let wasmBinary: ArrayBuffer
  if (typeof Buffer !== 'undefined') {
    const buf = Buffer.from(FVAD_WASM_BASE64, 'base64')
    wasmBinary = buf.buffer.slice(buf.byteOffset, buf.byteOffset + buf.byteLength)
  } else {
    const binaryStr = atob(FVAD_WASM_BASE64)
    const len = binaryStr.length
    const bytes = new Uint8Array(len)
    for (let i = 0; i < len; i++) {
      bytes[i] = binaryStr.charCodeAt(i)
    }
    wasmBinary = bytes.buffer
  }
  return fvad({ wasmBinary })
}

export interface WebRtcVadDetectorOptions {
  sampleRate?: number
  mode?: number
  hangoverMs?: number
}

export class WebRtcVadDetector {
  private mod: FvadModule | null = null
  private handle = 0
  private pcmPtr = 0
  public readonly sampleRate: number
  public readonly mode: number
  public readonly frameLength: number
  private readonly hangoverFrames: number
  private remainingHangoverFrames = 0
  private lastSpeakingState = false
  private isDestroyed = false

  constructor(options?: WebRtcVadDetectorOptions) {
    this.sampleRate = options?.sampleRate ?? 48000
    this.mode = options?.mode ?? 2
    this.frameLength = Math.round(this.sampleRate * 0.01) // 480 samples @ 48kHz (10ms)
    const hangoverMs = options?.hangoverMs ?? 150
    this.hangoverFrames = Math.max(1, Math.round(hangoverMs / 10))
  }

  public async init(): Promise<void> {
    if (this.mod && this.handle) return
    try {
      this.mod = await createWebRtcVadModule()
      this.handle = this.mod._fvad_new()
      if (!this.handle) {
        throw new Error('Failed to create WebRTC VAD instance')
      }

      const modeResult = this.mod._fvad_set_mode(this.handle, this.mode)
      if (modeResult !== 0) {
        throw new Error(`Failed to set WebRTC VAD mode to ${this.mode}`)
      }

      const rateResult = this.mod._fvad_set_sample_rate(this.handle, this.sampleRate)
      if (rateResult !== 0) {
        throw new Error(`Failed to set WebRTC VAD sample rate to ${this.sampleRate}`)
      }

      this.pcmPtr = this.mod._malloc(this.frameLength * 2)
      this.isDestroyed = false
    } catch (e) {
      console.warn('[WebRtcVadDetector] Failed to initialize:', e)
      this.destroy()
    }
  }

  public isReady(): boolean {
    return Boolean(this.mod && this.handle && this.pcmPtr && !this.isDestroyed)
  }

  public process(frame: Float32Array): boolean {
    if (!this.isReady() || this.isDestroyed) return false
    const mod = this.mod!
    const ptr = this.pcmPtr
    const heapOffset = ptr >> 1
    const len = this.frameLength

    const heap16 = mod.HEAP16
    const copyLen = Math.min(len, frame.length)
    for (let i = 0; i < copyLen; i++) {
      const s = Math.max(-1, Math.min(1, frame[i] || 0))
      heap16[heapOffset + i] = s < 0 ? Math.round(s * 32768) : Math.round(s * 32767)
    }
    for (let i = copyLen; i < len; i++) {
      heap16[heapOffset + i] = 0
    }

    const res = mod._fvad_process(this.handle, ptr, len)
    const isSpeech = res === 1

    if (isSpeech) {
      this.remainingHangoverFrames = this.hangoverFrames
      this.lastSpeakingState = true
      return true
    } else if (this.remainingHangoverFrames > 0) {
      this.remainingHangoverFrames--
      return true
    } else {
      this.lastSpeakingState = false
      return false
    }
  }

  public reset(): void {
    if (this.isReady()) {
      try {
        this.mod!._fvad_reset(this.handle)
        this.mod!._fvad_set_sample_rate(this.handle, this.sampleRate)
        this.mod!._fvad_set_mode(this.handle, this.mode)
      } catch { }
      this.remainingHangoverFrames = 0
      this.lastSpeakingState = false
    }
  }

  public destroy(): void {
    if (this.isDestroyed) return
    this.isDestroyed = true
    if (this.mod && this.pcmPtr) {
      try { this.mod._free(this.pcmPtr) } catch { }
      this.pcmPtr = 0
    }
    if (this.mod && this.handle) {
      try { this.mod._fvad_free(this.handle) } catch { }
      this.handle = 0
    }
    this.mod = null
  }
}

export class WebRtcVadSidecar {
  private detector: WebRtcVadDetector | null = null
  private context: AudioContext | null = null
  private source: MediaStreamAudioSourceNode | null = null
  private tappedNode: AudioNode | null = null
  private processor: ScriptProcessorNode | null = null
  private silentGain: GainNode | null = null
  private readonly accumulator = new Float32Array(480)
  private accumulatedSamples = 0
  private onSpeakingChange: ((isSpeaking: boolean) => void) | null = null
  private isMuted = false
  private isActive = false
  private lastReportedSpeaking: boolean | null = null

  constructor(onSpeakingChange: (isSpeaking: boolean) => void) {
    this.onSpeakingChange = onSpeakingChange
  }

  public async start(context: AudioContext, target: AudioNode | MediaStream): Promise<void> {
    this.stop()
    if (!context || context.state === 'closed') return

    this.context = context
    try {
      if (!this.detector || !this.detector.isReady()) {
        this.detector = new WebRtcVadDetector({ sampleRate: 48000, mode: 2, hangoverMs: 150 })
        await this.detector.init()
      }

      if (!this.detector.isReady()) return

      let inputNode: AudioNode | null = null
      if (typeof MediaStream !== 'undefined' && target instanceof MediaStream) {
        this.source = context.createMediaStreamSource(target)
        inputNode = this.source
      } else if (typeof AudioNode !== 'undefined' && target instanceof AudioNode) {
        this.tappedNode = target
        inputNode = target
      } else if (target && typeof (target as any).getAudioTracks === 'function') {
        this.source = context.createMediaStreamSource(target as any)
        inputNode = this.source
      } else if (target && typeof (target as any).connect === 'function') {
        this.tappedNode = target as any
        inputNode = target as any
      }

      if (!inputNode) return

      if (typeof context.createScriptProcessor === 'function') {
        this.processor = context.createScriptProcessor(512, 1, 1)
        this.silentGain = context.createGain()
        this.silentGain.gain.value = 0

        this.processor.onaudioprocess = (event) => {
          if (!this.isActive || !this.detector) return
          if (this.isMuted) {
            this.report(false)
            return
          }

          const input = event.inputBuffer.getChannelData(0)
          let inputOffset = 0
          while (inputOffset < input.length) {
            const needed = 480 - this.accumulatedSamples
            const available = input.length - inputOffset
            const toCopy = Math.min(needed, available)

            this.accumulator.set(
              input.subarray(inputOffset, inputOffset + toCopy),
              this.accumulatedSamples
            )
            this.accumulatedSamples += toCopy
            inputOffset += toCopy

            if (this.accumulatedSamples === 480) {
              const isSpeaking = this.detector.process(this.accumulator)
              this.report(isSpeaking)
              this.accumulatedSamples = 0
            }
          }

          if (event.outputBuffer) {
            for (let ch = 0; ch < event.outputBuffer.numberOfChannels; ch++) {
              event.outputBuffer.getChannelData(ch).fill(0)
            }
          }
        }

        inputNode.connect(this.processor)
        this.processor.connect(this.silentGain)
        this.silentGain.connect(context.destination)
      }

      this.isActive = true
    } catch (e) {
      console.warn('[WebRtcVadSidecar] start failed gracefully:', e)
    }
  }

  private report(isSpeaking: boolean): void {
    if (this.lastReportedSpeaking === isSpeaking) return
    this.lastReportedSpeaking = isSpeaking
    if (this.onSpeakingChange) {
      this.onSpeakingChange(isSpeaking)
    }
  }

  public setMuted(muted: boolean): void {
    this.isMuted = muted
    if (muted) {
      this.accumulatedSamples = 0
      this.detector?.reset()
      this.report(false)
    }
  }

  public processDirectFrame(frame: Float32Array): boolean {
    if (!this.detector || this.isMuted) {
      this.report(false)
      return false
    }
    const isSpeaking = this.detector.process(frame)
    this.report(isSpeaking)
    return isSpeaking
  }

  public stop(): void {
    this.isActive = false
    if (this.source) {
      try { this.source.disconnect() } catch { }
      this.source = null
    }
    if (this.tappedNode && this.processor) {
      try { this.tappedNode.disconnect(this.processor) } catch { }
      this.tappedNode = null
    }
    if (this.processor) {
      try {
        this.processor.onaudioprocess = null
        this.processor.disconnect()
      } catch { }
      this.processor = null
    }
    if (this.silentGain) {
      try { this.silentGain.disconnect() } catch { }
      this.silentGain = null
    }
    if (this.detector) {
      this.detector.destroy()
      this.detector = null
    }
    this.accumulatedSamples = 0
    this.report(false)
    this.lastReportedSpeaking = null
  }

  public destroy(): void {
    this.stop()
  }
}
