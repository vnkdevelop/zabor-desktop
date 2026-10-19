import fvad from '@echogarden/fvad-wasm'
import type { FvadModule } from '@echogarden/fvad-wasm'
import { FVAD_WASM_BASE64 } from './fvad-binary'

export async function createWebRtcVadModule(): Promise<FvadModule> {
  let wasmBinary: ArrayBuffer
  if (typeof Buffer !== 'undefined') {
    wasmBinary = Buffer.from(FVAD_WASM_BASE64, 'base64').buffer
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
  private source: MediaStreamAudioSourceNode | null = null
  private analyser: AnalyserNode | null = null
  private pollTimer: any = null
  private readonly timeBuf = new Float32Array(1024)
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

    try {
      if (!this.detector || !this.detector.isReady()) {
        this.detector = new WebRtcVadDetector({ sampleRate: 48000, mode: 2, hangoverMs: 150 })
        await this.detector.init()
      }

      if (!this.detector.isReady()) return

      const analyser = context.createAnalyser()
      analyser.fftSize = 1024
      analyser.smoothingTimeConstant = 0
      this.analyser = analyser

      if (target instanceof MediaStream) {
        this.source = context.createMediaStreamSource(target)
        this.source.connect(analyser)
      } else if (target instanceof AudioNode) {
        target.connect(analyser)
      } else {
        return
      }

      this.isActive = true
      this.pollTimer = setInterval(() => {
        if (!this.isActive || !this.detector || !this.detector.isReady() || !this.analyser) return
        if (this.isMuted) {
          this.report(false)
          return
        }

        try {
          this.analyser.getFloatTimeDomainData(this.timeBuf)
          const voice1 = this.detector.process(this.timeBuf.subarray(0, 480))
          const voice2 = this.detector.process(this.timeBuf.subarray(480, 960))
          this.report(voice1 || voice2)
        } catch { }
      }, 20)
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
    if (this.pollTimer) {
      clearInterval(this.pollTimer)
      this.pollTimer = null
    }
    if (this.source) {
      try { this.source.disconnect() } catch { }
      this.source = null
    }
    if (this.analyser) {
      try { this.analyser.disconnect() } catch { }
      this.analyser = null
    }
    this.report(false)
    this.lastReportedSpeaking = null
  }

  public destroy(): void {
    this.stop()
    if (this.detector) {
      this.detector.destroy()
      this.detector = null
    }
  }
}
