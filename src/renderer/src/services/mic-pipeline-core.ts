import { TonalNoiseSuppressor, VoiceShaper } from './voice-shaping'

const WASM_ASSET = 'pkg/df_bg.wasm'
const MODEL_ASSET = 'models/DeepFilterNet3_onnx.tar.gz'

export type NoiseAssetReader = (asset: string) => Promise<ArrayBuffer | null>

export interface NoiseEngine {
  readonly label: string
  readonly supportsAttenuationLimit: boolean
  readonly isReady: boolean
  readonly stageName: string
  readonly frame: number
  start(read: NoiseAssetReader, attenuationLimitDb: number, postFilterBeta: number): Promise<void>
  process(frame: Float32Array): Float32Array | null
  setAttenuationLimit(limitDb: number): void
  setPostFilterBeta(beta: number): void
}

let engineFactory: (() => NoiseEngine) | null = null

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

if (typeof globalThis.performance === 'undefined') {
  (globalThis as any).performance = {
    now: () => Date.now()
  }
}

class MicPipelineProcessor extends AudioWorkletProcessor {
  private inputBuffer: Float32Array
  private outputBuffer: Float32Array
  private inputReadIndex = 0
  private inputWriteIndex = 0
  private outputReadIndex = 0
  private outputWriteIndex = 0

  private readonly FRAME_SIZE = 480
  private readonly BUFFER_SIZE = 24000

  private readonly engine: NoiseEngine = (engineFactory as () => NoiseEngine)()

  private readonly frameToProcess: Float32Array
  private readonly processedFrame: Float32Array
  private readonly engineInputFrame: Float32Array
  private monoInput = new Float32Array(0)

  private isMuted = false
  private monitorWhileMuted = false
  private noiseSuppression = true

  private rmsSmoothed = 0
  private lastVadSent = false
  private overflowCount = 0
  private denoiserErrorCount = 0
  private readonly SILENCE_REPORT_FRAMES = 300
  private silentOutputFrames = 0

  private readonly VAD_FRAME_SIZE = 512
  private readonly VAD_ON_MIN = 0.4
  private readonly VAD_ON_MAX = 0.85
  private readonly VAD_ON_MARGIN = 0.05
  private readonly VAD_OFF_RATIO = 0.4
  private readonly VAD_OFF_MIN = 0.16
  private readonly VAD_UNVOICED_ON = 0.8
  private readonly VAD_SEMI_VOICED_ON = 0.65
  private readonly VAD_FIXED_SEMI_MARGIN = 0.06
  private readonly VAD_FIXED_UNVOICED_MARGIN = 0.14
  private readonly NOISE_PROB_QUANTILE = 0.9
  private readonly noiseProbHistory = new Float32Array(64)
  private readonly noiseProbScratch = new Float32Array(64)
  private noiseProbHistoryIndex = 0
  private noiseProbHistoryCount = 0
  private readonly NOISE_TRACKER_SETTLE_WINDOWS = 8
  private readonly VAD_ATTACK_RESULTS = 2
  private readonly VAD_ATTACK_GAP_MAX = 3
  private readonly VAD_WINDOW_FRAMES = 3.2
  private readonly VAD_RELEASE_MIN_RESULTS = 5
  private readonly VAD_RELEASE_MAX_RESULTS = 12
  private readonly MANUAL_HOLD_FRAMES = 38
  private readonly DECISION_DELAY_FRAMES = 24
  private readonly LOOKAHEAD_DELAY_FRAMES = 1
  private readonly SPEECH_PREROLL_FRAMES = 14
  private gateOpen = false
  private noiseProbHigh = 0.05
  private vadFixedThreshold = 0
  private vadOnThreshold = this.VAD_ON_MIN
  private vadOffThreshold = this.VAD_OFF_MIN
  private vadSemiVoicedOn = this.VAD_SEMI_VOICED_ON
  private vadUnvoicedOn = this.VAD_UNVOICED_ON
  private speechSegmentOpen = false
  private consecutiveVadSpeechResults = 0
  private attackGapWindows = 0
  private attackFirstWindowEndFrameId = -1
  private consecutiveVadSilenceResults = 0
  private closedWindowsSinceSpeech = 0
  private segmentPeakProbability = 0
  private readonly IMPULSE_CLAMP_RATIO = 4
  private readonly VOICE_PROB_FALL = 0.08
  private readonly VOICE_PROB_RISE = 0.004
  private readonly VOICE_PROB_MARGIN = 0.06
  private readonly VOICE_PROB_MIN_WINDOWS = 8
  private voiceProbLow = 0.9
  private segmentWindows = 0
  private loggedVadOnThreshold = -1
  private lastVadSequence = -1
  private audioFrameId = 0

  private readonly PITCH_MIN_LAG = 40
  private readonly PITCH_MAX_LAG = 229
  private readonly PITCH_WINDOW = 256
  private readonly VOICING_SPEECH_MIN = 0.45
  private readonly VOICING_HOLD_WINDOWS = 6
  private readonly VOICING_LAG_DRIFT_MAX = 0.25
  private readonly pitchBuffer = new Float32Array(512)
  private readonly pitchScratch = new Float32Array(512)
  private pitchWriteIndex = 0
  private pitchFilled = 0
  private voicing = 0
  private voicingLag = 0
  private previousVoicingLag = 0
  private voicingHoldWindows = 0

  private readonly voicingRingFrameIds = new Int32Array(12).fill(-1)
  private readonly voicingRingVoiced = new Uint8Array(12)
  private voicingRingWriteIndex = 0

  private readonly TRANSIENT_ATTACK_DB = 15
  private readonly TRANSIENT_CREST = 6
  private readonly TRANSIENT_DECAY_FRAMES = 6
  private readonly TRANSIENT_DECAY_RATIO = 0.25
  private readonly TRANSIENT_HOLD_FRAMES = 5
  private previousFrameRms = 0
  private transientHoldFrames = 0
  private transientCandidateFrameId = -1
  private transientCandidateRms = 0
  private transientCandidateAge = 0
  private transientCandidateAttackDb = 0
  private transientCandidateCrest = 0
  private rejectedTransientFrames = 0

  private readonly NOISE_RMS_QUANTILE = 0.9
  private readonly noiseRmsHistory = new Float32Array(200)
  private readonly noiseRmsScratch = new Float32Array(200)
  private noiseRmsHistoryIndex = 0
  private noiseRmsHistoryCount = 0
  private readonly NOISE_RMS_REFRESH_FRAMES = 5
  private readonly NOISE_TRACKER_SETTLE_FRAMES = 25
  private readonly AUDIBLE_MIN_RMS = 0.0006
  private noiseRmsHigh = 0.003
  private closedFramesSinceSpeech = 0

  private readonly ALC_TARGET_RMS = 0.065
  private readonly ALC_PEAK_CEILING = 0.891
  private readonly ALC_MAX_GAIN = 5.0
  private readonly ALC_MIN_SPEECH_RMS = 0.0015
  private readonly ALC_RMS_RATE = 0.004
  private readonly ALC_PEAK_FALL = 0.004
  private readonly ALC_GAIN_SMOOTH = 0.008
  private readonly ALC_LIMIT_ATTACK = 1 / 24
  private readonly ALC_LIMIT_RELEASE = 1 / 5760
  private readonly ALC_SEED_FRAMES = 50
  private readonly ALC_SEED_GAIN_SMOOTH = 0.05
  private readonly ALC_OUTPUT_CEILING = 0.999
  private alcSpeechRms = 0
  private alcPeakEnvelope = 0
  private alcSpeechFrames = 0
  private alcGain = 1
  private alcAppliedGain = 1
  private alcDownstreamGain = 1
  private alcLoggedGain = 1
  private alcLogFrames = 0

  private readonly voiceShaper = new VoiceShaper()

  private readonly toneSuppressor = new TonalNoiseSuppressor()
  private readonly TONE_ADAPT_NOISE_RATIO = 2
  private readonly TONE_LOG_INTERVAL_FRAMES = 300
  private toneLogFrames = 0
  private toneLoggedHumCount = -1
  private toneLoggedWhineCount = -1

  private readonly ENGINE_INPUT_TARGET_RMS = 0.1
  private readonly ENGINE_INPUT_PEAK_CEILING = 0.7
  private readonly ENGINE_INPUT_MAX_GAIN = 31.62
  private readonly ENGINE_INPUT_MIN_SPEECH_RMS = 0.0015
  private readonly ENGINE_INPUT_RMS_RATE = 0.004
  private readonly ENGINE_INPUT_PEAK_FALL = 0.004
  private readonly ENGINE_INPUT_GAIN_SMOOTH = 0.008
  private readonly ENGINE_INPUT_SEED_FRAMES = 50
  private readonly ENGINE_INPUT_SEED_GAIN_SMOOTH = 0.05
  private readonly ENGINE_INPUT_LEARN_FLOOR_RATIO = 0.35
  private readonly ENGINE_GAIN_SLEW = Math.pow(10, 0.25 / 20)
  private readonly ENGINE_SAMPLE_CEILING = 0.999
  private engineInputSpeechRms = 0
  private engineInputPeakEnvelope = 0
  private engineInputSpeechFrames = 0
  private engineInputGain = 1
  private engineAppliedGain = 1
  private engineInputLoggedGain = 1
  private engineInputLogFrames = 0

  private readonly LEVEL_REPORT_FRAMES = 100
  private levelReportFrames = 0

  private readonly speechRingSpeech: Uint8Array
  private readonly speechRingFrames: Float32Array[] = []
  private readonly speechRingFrameIds: Int32Array
  private readonly speechRingRms: Float32Array
  private readonly speechRingZcr: Float32Array
  private readonly speechRingTilt: Float32Array
  private readonly speechRingTransient: Uint8Array
  private speechRingWriteIndex = 0
  private speechRingCount = 0
  private gateGain = 0
  private readonly GATE_FLOOR = 0
  private readonly GATE_HOLD_FRAMES = 32
  private readonly GATE_ATTACK_COEFFICIENT = 1 / 48
  private readonly GATE_RELEASE_COEFFICIENT = 1 / 5760
  private readonly GATE_SILENCE_EPSILON = 0.004
  private gateHoldFrames = 0

  private readonly MIN_ATTEN_LIMIT = 5
  private readonly MAX_ATTEN_LIMIT = 30
  private attenuationLimit = 15
  private readonly MIN_POST_FILTER_BETA = 0.02
  private readonly MAX_POST_FILTER_BETA = 0.05
  private postFilterBeta = this.MIN_POST_FILTER_BETA
  private assetBase = 'zabor-local://deepfilternet3'
  private preloadedWasmBytes: ArrayBuffer | null = null
  private preloadedModelBytes: ArrayBuffer | null = null
  private engineStarting = false
  private readonly ENGINE_FIRST_ATTEMPT_SAMPLES = 4_800
  private readonly ENGINE_RETRY_MIN_SAMPLES = 48_000
  private readonly ENGINE_RETRY_MAX_SAMPLES = 240_000
  private engineRetrySamples = this.ENGINE_FIRST_ATTEMPT_SAMPLES
  private engineAttempts = 0

  private readonly TARGET_SPEECH_TO_NOISE_DB = 55
  private readonly ATTEN_KNEE_DB = 24
  private readonly ATTEN_ABOVE_KNEE_SLOPE = 0.5
  private readonly ATTEN_SLEW_DB_PER_SEC = 1
  private readonly ATTEN_MIN_SPEECH_FRAMES = 100
  private attenuationFloor = 15
  private attenuationTarget = 15
  private adaptiveAttenuationEnabled = false
  private attenuationLoggedLimit = 15

  private readonly DF_VETO_MARGIN_DB = 3
  private readonly DF_VETO_ENGAGE_FRAMES = 25
  private readonly DF_VETO_RELEASE_STEP = 2
  private readonly DF_VETO_MIN_REDUCTION_DB = 9
  private dfVetoFrames = 0
  private dfVetoLatched = false

  private thresholdMode = 'auto'
  private noiseFloorEstimate = 0.003
  private sileroVadEnabled = false
  private sileroVadHealthy = false
  private lastSileroResultFrameId = -1
  private sileroVadProbability = 0.0
  private readonly vad16kBuffer = new Float32Array(this.VAD_FRAME_SIZE)
  private vad16kWriteIndex = 0
  private vadSequence = 0
  private readonly vadDecimatorTaps = new Float32Array(49)
  private readonly vadDecimatorHistory = new Float32Array(49)
  private vadDecimatorWriteIndex = 0
  private vadDecimatorPhase = 0
  private vadWindowSquareSum = 0
  private vadWindowSampleCount = 0

  private calibrationFramesLeft = 0
  private readonly CALIBRATION_LEAD_IN_FRAMES = 20
  private readonly CALIBRATION_SPEECH_VAD_FLOOR = 0.08
  private readonly calibrationRms = new Float32Array(600)
  private readonly calibrationNoiseVad = new Float32Array(200)
  private calibrationTotalFrames = 0
  private calibrationStartFrameId = 0
  private calibrationCount = 0
  private calibrationNoiseVadCount = 0
  private calibrationRejectedSpeechFrames = 0
  private calibrationZcrSum = 0
  private calibrationSpectralTiltSum = 0
  private readonly calibrationSilenceHistory = new Float32Array(100)
  private readonly calibrationSilenceScratch = new Float32Array(100)
  private calibrationSilenceHistoryCount = 0
  private calibrationSilenceHistoryIndex = 0
  private calibrationSilenceLevel = 0

  private manualThresholdDb = -42
  private manualVadHoldFrames = 0
  private meterFrameCounter = 0
  private gainFactor = 1

  constructor() {
    super()
    this.inputBuffer = new Float32Array(this.BUFFER_SIZE)
    this.outputBuffer = new Float32Array(this.BUFFER_SIZE)
    this.frameToProcess = new Float32Array(this.FRAME_SIZE)
    this.processedFrame = new Float32Array(this.FRAME_SIZE)
    this.engineInputFrame = new Float32Array(this.FRAME_SIZE)
    this.speechRingSpeech = new Uint8Array(this.DECISION_DELAY_FRAMES + 1)
    this.speechRingFrameIds = new Int32Array(this.DECISION_DELAY_FRAMES + 1)
    this.speechRingFrameIds.fill(-1)
    this.speechRingRms = new Float32Array(this.DECISION_DELAY_FRAMES + 1)
    this.speechRingZcr = new Float32Array(this.DECISION_DELAY_FRAMES + 1)
    this.speechRingTilt = new Float32Array(this.DECISION_DELAY_FRAMES + 1)
    this.speechRingTransient = new Uint8Array(this.DECISION_DELAY_FRAMES + 1)
    for (let i = 0; i < this.speechRingSpeech.length; i++) {
      this.speechRingFrames.push(new Float32Array(this.FRAME_SIZE))
    }
    const center = (this.vadDecimatorTaps.length - 1) / 2
    const cutoff = 0.15
    let tapSum = 0
    for (let i = 0; i < this.vadDecimatorTaps.length; i++) {
      const offset = i - center
      const sinc = offset === 0
        ? 2 * cutoff
        : Math.sin(2 * Math.PI * cutoff * offset) / (Math.PI * offset)
      const window = 0.54 - 0.46 * Math.cos(2 * Math.PI * i / (this.vadDecimatorTaps.length - 1))
      const tap = sinc * window
      this.vadDecimatorTaps[i] = tap
      tapSum += tap
    }
    for (let i = 0; i < this.vadDecimatorTaps.length; i++) this.vadDecimatorTaps[i] /= tapSum
    this.outputWriteIndex = this.FRAME_SIZE * 2

    this.port.onmessageerror = () => {
      this.port.postMessage({
        type: 'log',
        message: 'Mic pipeline could not deserialize a port message; falling back to the asset bridge'
      })
      this.engineRetrySamples = 0
    }

    this.port.onmessage = (event) => {
      if (event.data.type === 'loadWasm') {
        if (typeof event.data.assetBase === 'string' && event.data.assetBase) {
          this.assetBase = event.data.assetBase
        }
        if (event.data.wasmBytes instanceof ArrayBuffer) {
          this.preloadedWasmBytes = event.data.wasmBytes
        }
        if (event.data.modelBytes instanceof ArrayBuffer) {
          this.preloadedModelBytes = event.data.modelBytes
        }
        this.engineRetrySamples = 0
        if (this.noiseSuppression) void this.startEngine()
      } else if (event.data.type === 'setLevelSeed') {
        this.applyLevelSeed(event.data)
      } else if (event.data.type === 'setConfig') {
        if (event.data.noiseSuppression !== undefined) {
          const nextNoiseSuppression = Boolean(event.data.noiseSuppression)
          if (nextNoiseSuppression !== this.noiseSuppression) {
            this.toneSuppressor.reset()
            this.toneLoggedHumCount = -1
            this.toneLoggedWhineCount = -1
            this.gateHoldFrames = 0
            this.dfVetoFrames = 0
            this.dfVetoLatched = false
          }
          this.noiseSuppression = nextNoiseSuppression
        }
        if (event.data.sileroVadEnabled !== undefined) {
          this.sileroVadEnabled = event.data.sileroVadEnabled
          if (!this.sileroVadEnabled) {
            this.sileroVadHealthy = false
            this.speechSegmentOpen = false
            this.consecutiveVadSpeechResults = 0
            this.consecutiveVadSilenceResults = 0
            this.attackGapWindows = 0
            this.attackFirstWindowEndFrameId = -1
            this.segmentPeakProbability = 0
            this.segmentWindows = 0
          }
        }
        if (event.data.monitorWhileMuted !== undefined) {
          this.monitorWhileMuted = Boolean(event.data.monitorWhileMuted)
        }
        if (event.data.isMuted !== undefined) {
          const nextMuted = event.data.isMuted
          if (nextMuted && !this.isMuted && (this.calibrationFramesLeft > 0 || this.monitorWhileMuted)) {
            this.outputBuffer.fill(0)
            if (this.lastVadSent) {
              this.port.postMessage({ type: 'vad', isSpeaking: false })
              this.lastVadSent = false
            }
          } else if (nextMuted && !this.isMuted) {
            this.inputBuffer.fill(0)
            this.outputBuffer.fill(0)
            this.inputReadIndex = 0
            this.inputWriteIndex = 0
            this.outputReadIndex = 0
            this.outputWriteIndex = this.FRAME_SIZE * 2
            this.rmsSmoothed = 0
            this.speechSegmentOpen = false
            this.consecutiveVadSpeechResults = 0
            this.attackGapWindows = 0
            this.attackFirstWindowEndFrameId = -1
            this.consecutiveVadSilenceResults = 0
            this.closedWindowsSinceSpeech = 0
            this.closedFramesSinceSpeech = 0
            this.segmentPeakProbability = 0
            this.segmentWindows = 0
            this.previousFrameRms = 0
            this.toneSuppressor.reset()
            this.toneLogFrames = 0
            this.toneLoggedHumCount = -1
            this.toneLoggedWhineCount = -1
            this.transientHoldFrames = 0
            this.transientCandidateFrameId = -1
            this.transientCandidateRms = 0
            this.transientCandidateAge = 0
            this.dfVetoFrames = 0
            this.dfVetoLatched = false
            this.sileroVadHealthy = false
            this.lastSileroResultFrameId = -1
            this.lastVadSequence = -1
            this.audioFrameId = 0
            this.speechRingWriteIndex = 0
            this.speechRingCount = 0
            this.gateGain = 0
            this.gateHoldFrames = 0
            this.gateOpen = false
            this.speechRingSpeech.fill(0)
            this.speechRingFrameIds.fill(-1)
            this.speechRingRms.fill(0)
            this.speechRingZcr.fill(0)
            this.speechRingTilt.fill(0)
            this.speechRingTransient.fill(0)
            for (const frame of this.speechRingFrames) frame.fill(0)
            this.vad16kWriteIndex = 0
            this.vad16kBuffer.fill(0)
            this.vadDecimatorHistory.fill(0)
            this.vadDecimatorWriteIndex = 0
            this.vadDecimatorPhase = 0
            this.vadWindowSquareSum = 0
            this.vadWindowSampleCount = 0
            this.pitchBuffer.fill(0)
            this.pitchWriteIndex = 0
            this.pitchFilled = 0
            this.voicing = 0
            this.voicingLag = 0
            this.previousVoicingLag = 0
            this.voicingHoldWindows = 0
            this.voicingRingFrameIds.fill(-1)
            this.voicingRingVoiced.fill(0)
            this.voicingRingWriteIndex = 0
            this.sileroVadProbability = 0.0
            this.port.postMessage({ type: 'resetVad' })
            if (this.lastVadSent) {
              this.port.postMessage({ type: 'vad', isSpeaking: false })
              this.lastVadSent = false
            }
          }
          this.isMuted = nextMuted
        }
      } else if (event.data.type === 'fetchResponse') {
        this.resolveAsset(String(event.data.url), event.data.buffer ?? null)
      } else if (event.data.type === 'setCalibratedParams') {
        if (event.data.thresholdMode !== undefined) {
          this.thresholdMode = event.data.thresholdMode
        }
        if (event.data.manualThresholdValue !== undefined && this.thresholdMode === 'manual') {
          const threshold = Number(event.data.manualThresholdValue)
          this.manualThresholdDb = Math.max(-60, Math.min(-12, Number.isFinite(threshold) ? threshold : -42))
        }
        if (event.data.vadFixedThreshold !== undefined) {
          const fixed = Number(event.data.vadFixedThreshold)
          const normalized = Number.isFinite(fixed) && fixed > 0 ? Math.min(0.95, fixed) : 0
          if (normalized !== this.vadFixedThreshold) {
            this.vadFixedThreshold = normalized
            this.refreshVadThresholds()
          }
        }
        if (event.data.vadTrackerSeed !== undefined && this.vadFixedThreshold <= 0) {
          const seed = Number(event.data.vadTrackerSeed)
          if (Number.isFinite(seed) && seed >= 0) {
            const seeded = Math.min(this.VAD_ON_MAX, seed)
            this.noiseProbHistory.fill(seeded)
            this.noiseProbHistoryIndex = 0
            this.noiseProbHistoryCount = this.noiseProbHistory.length
            this.noiseProbHigh = seeded
            this.refreshVadThresholds()
          }
        }
        if (event.data.attenuationLimit !== undefined) {
          const requested = Number(event.data.attenuationLimit)
          const previous = this.attenuationLimit
          this.attenuationLimit = Math.max(this.MIN_ATTEN_LIMIT, Math.min(this.MAX_ATTEN_LIMIT, event.data.attenuationLimit))
          this.attenuationFloor = this.attenuationLimit
          this.attenuationTarget = this.attenuationLimit
          this.attenuationLoggedLimit = this.attenuationLimit
          try {
            this.engine.setAttenuationLimit(this.attenuationLimit)
            if (this.attenuationLimit !== previous) {
              this.port.postMessage({
                type: 'log',
                message: `Suppression limit applied: ${this.attenuationLimit}dB (requested ${requested}dB)`
              })
            }
          } catch {
          }
        }
        if (event.data.adaptiveAttenuation !== undefined) {
          this.adaptiveAttenuationEnabled = Boolean(event.data.adaptiveAttenuation)
        }
        if (event.data.vadThreshold !== undefined && this.vadFixedThreshold <= 0) {
          const calibrated = Number(event.data.vadThreshold)
          if (Number.isFinite(calibrated) && calibrated > 0) {
            this.voiceProbLow = Math.max(
              this.VOICE_PROB_MARGIN + 0.02,
              Math.min(0.95, calibrated + this.VOICE_PROB_MARGIN)
            )
            this.refreshVadThresholds()
          }
        }
        if (event.data.postFilterBeta !== undefined) {
          const requestedBeta = Number(event.data.postFilterBeta)
          this.postFilterBeta = Number.isFinite(requestedBeta)
            ? Math.max(0, Math.min(this.MAX_POST_FILTER_BETA, requestedBeta))
            : this.MIN_POST_FILTER_BETA
          try {
            this.engine.setPostFilterBeta(this.postFilterBeta)
          } catch {
          }
        }
        if (event.data.gainFactor !== undefined) {
          const gainFactor = Number(event.data.gainFactor)
          this.gainFactor = Number.isFinite(gainFactor) && gainFactor > 0 ? gainFactor : 1
        }
        if (event.data.downstreamGain !== undefined) {
          const downstreamGain = Number(event.data.downstreamGain)
          this.alcDownstreamGain = Number.isFinite(downstreamGain) ? Math.max(1, downstreamGain) : 1
        }
        if (event.data.noiseFloor !== undefined) {
          const noiseFloor = Number(event.data.noiseFloor)
          if (Number.isFinite(noiseFloor) && noiseFloor > 0) {
            this.noiseFloorEstimate = Math.max(0.0001, Math.min(0.03, noiseFloor))
            this.noiseRmsHigh = this.noiseFloorEstimate * 2
            this.noiseRmsHistory.fill(this.noiseRmsHigh)
            this.noiseRmsHistoryIndex = 0
            this.noiseRmsHistoryCount = this.noiseRmsHistory.length
          }
        }
      } else if (event.data.type === 'startCalibration') {
        this.calibrationTotalFrames = Math.floor((event.data.durationMs || 2500) / 10)
        this.calibrationFramesLeft = this.calibrationTotalFrames
        this.calibrationStartFrameId = this.audioFrameId
        this.calibrationCount = 0
        this.calibrationNoiseVadCount = 0
        this.calibrationRejectedSpeechFrames = 0
        this.calibrationZcrSum = 0
        this.calibrationSpectralTiltSum = 0
        this.calibrationSilenceHistoryCount = 0
        this.calibrationSilenceHistoryIndex = 0
        this.calibrationSilenceLevel = 0
        this.sileroVadProbability = 0
        this.speechSegmentOpen = false
        this.consecutiveVadSpeechResults = 0
        this.attackGapWindows = 0
        this.attackFirstWindowEndFrameId = -1
        this.consecutiveVadSilenceResults = 0
        this.segmentPeakProbability = 0
        this.segmentWindows = 0
        this.port.postMessage({ type: 'log', message: 'Calibration started' })
      } else if (event.data.type === 'setSileroVadProbability') {
        if ((this.isMuted && this.calibrationFramesLeft <= 0 && !this.monitorWhileMuted) || !this.sileroVadEnabled) return
        const sequence = Number(event.data.sequence)
        const endFrameId = Number(event.data.endFrameId)
        const windowRms = Number(event.data.windowRms)
        if (!Number.isFinite(sequence) || sequence <= this.lastVadSequence) return

        this.lastVadSequence = sequence
        this.sileroVadHealthy = true
        this.lastSileroResultFrameId = Number.isFinite(endFrameId) ? endFrameId : this.audioFrameId
        this.sileroVadProbability = Math.max(0, Math.min(1, Number(event.data.probability) || 0))
        const probability = this.sileroVadProbability

        if (this.calibrationFramesLeft > 0 && Number.isFinite(endFrameId) && Number.isFinite(windowRms)) {
          const elapsedFrames = endFrameId - this.calibrationStartFrameId
          const insideWindow = elapsedFrames >= this.CALIBRATION_LEAD_IN_FRAMES &&
            elapsedFrames < this.calibrationTotalFrames
          const normalizedWindowRms = windowRms / this.gainFactor
          const likelySpeechWindow = probability >= this.CALIBRATION_SPEECH_VAD_FLOOR &&
            normalizedWindowRms >= Math.max(this.calibrationSilenceLevel * 8, 0.0008)
          if (insideWindow && !likelySpeechWindow &&
            this.calibrationNoiseVadCount < this.calibrationNoiseVad.length) {
            this.calibrationNoiseVad[this.calibrationNoiseVadCount++] = probability
          }
        }

        if (this.speechSegmentOpen) {
          this.closedWindowsSinceSpeech = 0
        } else if (this.vadFixedThreshold <= 0) {
          this.closedWindowsSinceSpeech++
          const roomWindow = this.voicingHoldWindows === 0 &&
            (!Number.isFinite(windowRms) || windowRms <= this.noiseRmsHigh * this.IMPULSE_CLAMP_RATIO)
          if (roomWindow && this.closedWindowsSinceSpeech > this.NOISE_TRACKER_SETTLE_WINDOWS) {
            this.noiseProbHistory[this.noiseProbHistoryIndex] = probability
            this.noiseProbHistoryIndex = (this.noiseProbHistoryIndex + 1) % this.noiseProbHistory.length
            this.noiseProbHistoryCount++
            const filled = Math.min(this.noiseProbHistoryCount, this.noiseProbHistory.length)
            const recent = this.noiseProbScratch.subarray(0, filled)
            recent.set(this.noiseProbHistory.subarray(0, filled))
            recent.sort()
            this.noiseProbHigh = recent[Math.floor((filled - 1) * this.NOISE_PROB_QUANTILE)]
            this.refreshVadThresholds()
          }
        }

        if (this.speechSegmentOpen) this.segmentWindows++

        const voicingIndex = Number.isFinite(endFrameId) ? this.findVoicingWindow(endFrameId) : -1
        const windowVoiced = voicingIndex >= 0
          ? this.voicingRingVoiced[voicingIndex] === 1
          : this.voicingHoldWindows > 0
        const openThreshold = windowVoiced
          ? this.vadOnThreshold
          : this.voicingHoldWindows > 0
            ? this.vadSemiVoicedOn
            : this.vadUnvoicedOn

        if (
          probability > this.segmentPeakProbability &&
          (this.speechSegmentOpen || probability >= this.vadOnThreshold)
        ) {
          this.segmentPeakProbability = probability
        }

        if (probability >= openThreshold) {
          if (!this.speechSegmentOpen && this.windowCarriedTransient(endFrameId)) {
            this.consecutiveVadSpeechResults = 0
            this.attackGapWindows = 0
            this.attackFirstWindowEndFrameId = -1
            this.consecutiveVadSilenceResults = 0
          } else {
            if (this.consecutiveVadSpeechResults === 0) {
              this.attackFirstWindowEndFrameId = Number.isFinite(endFrameId) ? endFrameId : -1
            }
            this.consecutiveVadSpeechResults++
            this.attackGapWindows = 0
            this.consecutiveVadSilenceResults = 0
            if (!this.speechSegmentOpen && this.consecutiveVadSpeechResults >= this.VAD_ATTACK_RESULTS) {
              this.speechSegmentOpen = true
              const firstWindowEnd = this.attackFirstWindowEndFrameId >= 0
                ? this.attackFirstWindowEndFrameId
                : (Number.isFinite(endFrameId) ? endFrameId : -1)
              if (firstWindowEnd >= 0) {
                this.markBufferedSpeechFrom(
                  firstWindowEnd -
                  Math.ceil(this.VAD_WINDOW_FRAMES) -
                  this.SPEECH_PREROLL_FRAMES
                )
              }
              this.attackGapWindows = 0
              this.attackFirstWindowEndFrameId = -1
            }
          }
        } else if (this.speechSegmentOpen) {
          this.consecutiveVadSpeechResults = 0
          this.attackGapWindows = 0
          this.attackFirstWindowEndFrameId = -1
          if (probability >= this.vadOffThreshold) {
            this.consecutiveVadSilenceResults = 0
          } else {
            this.consecutiveVadSilenceResults++
            if (this.consecutiveVadSilenceResults >= this.currentReleaseResults()) {
              this.speechSegmentOpen = false
              this.consecutiveVadSilenceResults = 0
              if (this.vadFixedThreshold <= 0 && this.segmentWindows >= this.VOICE_PROB_MIN_WINDOWS) {
                const rate = this.segmentPeakProbability < this.voiceProbLow
                  ? this.VOICE_PROB_FALL
                  : this.VOICE_PROB_RISE
                this.voiceProbLow += rate * (this.segmentPeakProbability - this.voiceProbLow)
                this.refreshVadThresholds()
              }
              this.segmentWindows = 0
              this.segmentPeakProbability = 0
            }
          }
        } else if (this.consecutiveVadSpeechResults > 0) {
          this.attackGapWindows++
          if (probability < this.vadOffThreshold || this.attackGapWindows > this.VAD_ATTACK_GAP_MAX) {
            this.consecutiveVadSpeechResults = 0
            this.attackGapWindows = 0
            this.attackFirstWindowEndFrameId = -1
          }
        }
      }
    }
  }

  private refreshVadThresholds() {
    if (this.vadFixedThreshold > 0) {
      const on = this.vadFixedThreshold
      this.vadOnThreshold = on
      this.vadOffThreshold = Math.max(this.VAD_OFF_MIN, Math.min(on - 0.02, on * this.VAD_OFF_RATIO))
      this.vadSemiVoicedOn = Math.max(on, Math.min(this.VAD_SEMI_VOICED_ON, on + this.VAD_FIXED_SEMI_MARGIN))
      this.vadUnvoicedOn = Math.max(on, Math.min(this.VAD_UNVOICED_ON, on + this.VAD_FIXED_UNVOICED_MARGIN))
      if (Math.abs(on - this.loggedVadOnThreshold) >= 0.02) {
        this.loggedVadOnThreshold = on
        this.port.postMessage({
          type: 'log',
          message: `VAD gate fixed at ${on.toFixed(2)}/${this.vadOffThreshold.toFixed(2)} ` +
            `for voiced, ${this.vadSemiVoicedOn.toFixed(2)} half-voiced, ` +
            `${this.vadUnvoicedOn.toFixed(2)} unvoiced`
        })
      }
      return
    }
    const room = Math.max(this.VAD_ON_MIN, Math.min(this.VAD_ON_MAX, this.noiseProbHigh + this.VAD_ON_MARGIN))
    const on = Math.max(this.VAD_ON_MIN, Math.min(room, this.voiceProbLow - this.VOICE_PROB_MARGIN))
    this.vadOnThreshold = on
    this.vadOffThreshold = Math.max(this.VAD_OFF_MIN, Math.min(on - 0.02, on * this.VAD_OFF_RATIO))
    const semiVoiced = Math.max(on, Math.min(this.VAD_SEMI_VOICED_ON, Math.max(on + 0.04, this.voiceProbLow * 0.72)))
    const unvoiced = Math.max(semiVoiced, Math.min(this.VAD_UNVOICED_ON, Math.max(semiVoiced + 0.06, this.voiceProbLow * 1.05)))
    this.vadSemiVoicedOn = semiVoiced
    this.vadUnvoicedOn = unvoiced
    if (Math.abs(on - this.loggedVadOnThreshold) >= 0.02) {
      this.loggedVadOnThreshold = on
      this.port.postMessage({
        type: 'log',
        message: `VAD gate ${on.toFixed(2)}/${this.vadOffThreshold.toFixed(2)} ` +
          `for voiced, ${semiVoiced.toFixed(2)} half-voiced, ${unvoiced.toFixed(2)} unvoiced ` +
          `(room ${this.noiseProbHigh.toFixed(2)}, quietest phrase ${this.voiceProbLow.toFixed(2)})`
      })
    }
  }

  private currentReleaseResults(): number {
    const span = Math.max(0.05, 0.9 - this.vadOnThreshold)
    const confidence = Math.max(0, Math.min(1, (this.segmentPeakProbability - this.vadOnThreshold) / span))
    return this.VAD_RELEASE_MIN_RESULTS +
      Math.round((this.VAD_RELEASE_MAX_RESULTS - this.VAD_RELEASE_MIN_RESULTS) * confidence)
  }

  private refreshAttenuationLimit() {
    if (!this.adaptiveAttenuationEnabled || !this.engine.supportsAttenuationLimit) return
    if (!this.engine.isReady) return
    if (this.alcSpeechRms <= 0 || this.alcSpeechFrames < this.ATTEN_MIN_SPEECH_FRAMES) return
    if (this.noiseRmsHistoryCount < this.noiseRmsHistory.length) return
    if (this.noiseRmsHigh <= 0) return

    const measuredSnrDb = 20 * Math.log10(this.alcSpeechRms / this.noiseRmsHigh)
    const demandDb = this.TARGET_SPEECH_TO_NOISE_DB - measuredSnrDb
    const shapedDb = demandDb <= this.ATTEN_KNEE_DB
      ? demandDb
      : this.ATTEN_KNEE_DB + (demandDb - this.ATTEN_KNEE_DB) * this.ATTEN_ABOVE_KNEE_SLOPE
    this.attenuationTarget = Math.max(this.attenuationFloor, Math.min(this.MAX_ATTEN_LIMIT, shapedDb))

    if (this.attenuationTarget <= this.attenuationLimit) return
    const step = this.ATTEN_SLEW_DB_PER_SEC * (this.FRAME_SIZE / 48000)
    this.attenuationLimit = Math.min(this.attenuationTarget, this.attenuationLimit + step)

    if (this.attenuationLimit - this.attenuationLoggedLimit < 1) return
    this.attenuationLoggedLimit = this.attenuationLimit
    try {
      this.engine.setAttenuationLimit(this.attenuationLimit)
    } catch {
      return
    }
    this.port.postMessage({
      type: 'log',
      message: `Suppression ${this.attenuationLimit.toFixed(0)}dB ` +
        `(target ${this.attenuationTarget.toFixed(0)}, calibrated floor ${this.attenuationFloor.toFixed(0)}): ` +
        `measured SNR ${measuredSnrDb.toFixed(0)}dB, room ` +
        `${(20 * Math.log10(Math.max(1e-6, this.noiseRmsHigh))).toFixed(0)}dBFS`
    })
  }

  private findVoicingWindow(endFrameId: number): number {
    for (let i = 0; i < this.voicingRingFrameIds.length; i++) {
      if (this.voicingRingFrameIds[i] === endFrameId) return i
    }
    return -1
  }

  private windowCarriedTransient(endFrameId: number): boolean {
    if (!Number.isFinite(endFrameId)) return this.transientHoldFrames > 0
    const firstFrameId = endFrameId - Math.ceil(this.VAD_WINDOW_FRAMES)
    for (let i = 0; i < this.speechRingTransient.length; i++) {
      const frameId = this.speechRingFrameIds[i]
      if (frameId >= firstFrameId && frameId <= endFrameId && this.speechRingTransient[i] === 1) return true
    }
    return false
  }

  private markBufferedSpeechFrom(firstFrameId: number) {
    for (let i = 0; i < this.speechRingSpeech.length; i++) {
      const frameId = this.speechRingFrameIds[i]
      if (frameId >= firstFrameId && this.speechRingTransient[i] === 0) this.speechRingSpeech[i] = 1
    }
  }

  private markBufferedTransientFrom(firstFrameId: number) {
    if (firstFrameId < 0) return
    for (let i = 0; i < this.speechRingTransient.length; i++) {
      if (this.speechRingFrameIds[i] >= firstFrameId) this.speechRingTransient[i] = 1
    }
  }

  private decimateVadSample(sample: number): number | null {
    this.vadDecimatorHistory[this.vadDecimatorWriteIndex] = sample
    this.vadDecimatorWriteIndex = (this.vadDecimatorWriteIndex + 1) % this.vadDecimatorHistory.length
    this.vadDecimatorPhase++
    if (this.vadDecimatorPhase < 3) return null
    this.vadDecimatorPhase = 0

    let filtered = 0
    let historyIndex = (this.vadDecimatorWriteIndex - 1 + this.vadDecimatorHistory.length) % this.vadDecimatorHistory.length
    for (let i = 0; i < this.vadDecimatorTaps.length; i++) {
      filtered += this.vadDecimatorTaps[i] * this.vadDecimatorHistory[historyIndex]
      historyIndex = (historyIndex - 1 + this.vadDecimatorHistory.length) % this.vadDecimatorHistory.length
    }
    return Math.max(-1, Math.min(1, filtered))
  }

  private measureVoicing(): number {
    const size = this.pitchBuffer.length
    const window = this.PITCH_WINDOW
    const maxLag = this.PITCH_MAX_LAG
    const span = window + maxLag + 1
    this.voicingLag = 0
    if (this.pitchFilled < span) return 0

    const scratch = this.pitchScratch
    let index = (this.pitchWriteIndex - 1 + size) % size
    for (let i = 0; i < span; i++) {
      scratch[i] = this.pitchBuffer[index]
      index = (index - 1 + size) % size
    }

    let frameEnergy = 0
    for (let i = 0; i < window; i++) frameEnergy += scratch[i] * scratch[i]
    if (frameEnergy < 1e-9) return 0

    let lagEnergy = 0
    for (let j = this.PITCH_MIN_LAG; j < this.PITCH_MIN_LAG + window; j++) lagEnergy += scratch[j] * scratch[j]

    let best = 0
    let bestLag = 0
    for (let lag = this.PITCH_MIN_LAG; lag <= maxLag; lag++) {
      if (lagEnergy > 1e-9) {
        let correlation = 0
        for (let i = 0; i < window; i++) correlation += scratch[i] * scratch[i + lag]
        const score = correlation / Math.sqrt(frameEnergy * lagEnergy)
        if (score > best) {
          best = score
          bestLag = lag
        }
      }
      lagEnergy += scratch[lag + window] * scratch[lag + window] - scratch[lag] * scratch[lag]
    }
    this.voicingLag = bestLag
    return Math.max(0, Math.min(1, best))
  }

  private readonly pendingAssets = new Map<
    string,
    { resolvers: Array<(bytes: ArrayBuffer | null) => void>; samplesLeft: number }
  >()

  private readonly ASSET_BRIDGE_TIMEOUT_SAMPLES = 960_000

  private requestAsset(asset: string): Promise<ArrayBuffer | null> {
    const url = `${this.assetBase}/${asset}`
    return new Promise<ArrayBuffer | null>(resolve => {
      const pending = this.pendingAssets.get(url)
      if (pending) {
        pending.resolvers.push(resolve)
        return
      }
      this.pendingAssets.set(url, {
        resolvers: [resolve],
        samplesLeft: this.ASSET_BRIDGE_TIMEOUT_SAMPLES
      })
      this.port.postMessage({ type: 'fetchRequest', url })
    })
  }

  private resolveAsset(url: string, bytes: ArrayBuffer | null): void {
    const pending = this.pendingAssets.get(url)
    if (!pending) return
    this.pendingAssets.delete(url)
    for (const resolve of pending.resolvers) resolve(bytes)
  }

  private tickAssetBridge(samples: number): void {
    for (const [url, pending] of this.pendingAssets) {
      pending.samplesLeft -= samples
      if (pending.samplesLeft > 0) continue
      this.pendingAssets.delete(url)
      for (const resolve of pending.resolvers) resolve(null)
      this.port.postMessage({
        type: 'log',
        message: `Asset bridge timed out for ${url}; the engine will ask again`
      })
    }
  }

  private readonly readAsset = async (asset: string): Promise<ArrayBuffer | null> => {
    if (asset === WASM_ASSET && this.preloadedWasmBytes) {
      const bytes = this.preloadedWasmBytes
      this.preloadedWasmBytes = null
      return bytes
    }
    if (asset === MODEL_ASSET && this.preloadedModelBytes) {
      const bytes = this.preloadedModelBytes
      this.preloadedModelBytes = null
      return bytes
    }
    return this.requestAsset(asset)
  }

  private async startEngine(): Promise<void> {
    if (this.engine.isReady || this.engineStarting) return
    this.engineStarting = true
    this.engineAttempts++
    const attempt = this.engineAttempts
    const startedAt = performance.now()
    if (attempt === 1) {
      this.port.postMessage({ type: 'log', message: `${this.engine.label} initialization started` })
    }
    try {
      await this.engine.start(this.readAsset, this.attenuationLimit, this.postFilterBeta)
      this.port.postMessage({
        type: 'log',
        message: `${this.engine.label} initialized: frame=${this.engine.frame}, ` +
          `attenuation=${this.attenuationLimit}dB, postFilter=${this.postFilterBeta}, ` +
          `attempt ${attempt} in ${Math.round(performance.now() - startedAt)}ms`
      })
      this.port.postMessage({ type: 'ready' })
    } catch (error) {
      this.engineRetrySamples = Math.min(
        this.ENGINE_RETRY_MAX_SAMPLES,
        this.ENGINE_RETRY_MIN_SAMPLES * (1 << Math.min(attempt - 1, 3))
      )
      this.port.postMessage({
        type: 'log',
        message: `${this.engine.label} start failed on attempt ${attempt} at stage ` +
          `"${this.engine.stageName}": ${error instanceof Error ? error.message : String(error)}; ` +
          `retrying in ${(this.engineRetrySamples / 48000).toFixed(1)}s`
      })
    } finally {
      this.engineStarting = false
    }
  }

  private driveEngineStartup(samples: number): void {
    if (this.noiseSuppression && !this.engine.isReady && !this.engineStarting) {
      if (this.engineRetrySamples > 0) this.engineRetrySamples -= samples
      else void this.startEngine()
    }
    if (this.pendingAssets.size > 0) this.tickAssetBridge(samples)
  }

  private applyLevelSeed(seed: Record<string, unknown>): void {
    const positive = (value: unknown): number => {
      const parsed = Number(value)
      return Number.isFinite(parsed) && parsed > 0 ? parsed : 0
    }
    const alcSpeechRms = positive(seed.alcSpeechRms)
    if (alcSpeechRms > 0) {
      this.alcSpeechRms = Math.min(1, alcSpeechRms)
      this.alcPeakEnvelope = Math.min(1, positive(seed.alcPeakEnvelope) || this.alcSpeechRms)
      this.alcGain = Math.max(1, Math.min(this.ALC_MAX_GAIN, positive(seed.alcGain) || 1))
      this.alcAppliedGain = this.alcGain
      this.alcLoggedGain = this.alcGain
      this.alcSpeechFrames = this.ALC_SEED_FRAMES
    }
    const engineInputSpeechRms = positive(seed.engineInputSpeechRms)
    if (engineInputSpeechRms > 0) {
      this.engineInputSpeechRms = Math.min(1, engineInputSpeechRms)
      this.engineInputPeakEnvelope = Math.min(1, positive(seed.engineInputPeakEnvelope) || this.engineInputSpeechRms)
      this.engineInputGain = Math.max(1, Math.min(this.ENGINE_INPUT_MAX_GAIN, positive(seed.engineInputGain) || 1))
      this.engineAppliedGain = this.engineInputGain
      this.engineInputLoggedGain = this.engineInputGain
      this.engineInputSpeechFrames = this.ENGINE_INPUT_SEED_FRAMES
    }
    const noiseRmsHigh = positive(seed.noiseRmsHigh)
    if (noiseRmsHigh > 0) {
      this.noiseRmsHigh = Math.min(0.06, noiseRmsHigh)
      this.noiseRmsHistory.fill(this.noiseRmsHigh)
      this.noiseRmsHistoryIndex = 0
      this.noiseRmsHistoryCount = this.noiseRmsHistory.length
    }
  }

  private reportLevelState(): void {
    const alcLearned = this.alcSpeechFrames > 0
    const engineLearned = this.engineInputSpeechFrames > 0
    this.port.postMessage({
      type: 'levelState',
      alcGain: alcLearned ? this.alcGain : 0,
      alcSpeechRms: alcLearned ? this.alcSpeechRms : 0,
      alcPeakEnvelope: alcLearned ? this.alcPeakEnvelope : 0,
      engineInputGain: engineLearned ? this.engineInputGain : 0,
      engineInputSpeechRms: engineLearned ? this.engineInputSpeechRms : 0,
      engineInputPeakEnvelope: engineLearned ? this.engineInputPeakEnvelope : 0,
      noiseRmsHigh: this.noiseRmsHistoryCount > 0 ? this.noiseRmsHigh : 0
    })
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

    this.driveEngineStartup(inputChannel.length)

    const measureWhileMuted = this.isMuted && (this.calibrationFramesLeft > 0 || this.monitorWhileMuted)
    if (this.isMuted && !measureWhileMuted) {
      if (this.lastVadSent) {
        this.port.postMessage({ type: 'vad', isSpeaking: false })
        this.lastVadSent = false
      }
      outputChannel.fill(0)
      return true
    }

    let monoInput = inputChannel
    if (input.length > 1) {
      if (this.monoInput.length !== inputChannel.length) this.monoInput = new Float32Array(inputChannel.length)
      this.monoInput.fill(0)
      for (const channel of input) {
        for (let i = 0; i < inputChannel.length; i++) this.monoInput[i] += channel[i] || 0
      }
      const scale = 1 / input.length
      for (let i = 0; i < this.monoInput.length; i++) this.monoInput[i] *= scale
      monoInput = this.monoInput
    }
    this.inputWriteIndex = this.pushToBuffer(this.inputBuffer, monoInput, this.inputWriteIndex, this.inputReadIndex)

    while ((this.inputWriteIndex - this.inputReadIndex + this.BUFFER_SIZE) % this.BUFFER_SIZE >= this.FRAME_SIZE) {
      this.inputReadIndex = this.pullFromBuffer(this.inputBuffer, this.frameToProcess, this.inputWriteIndex, this.inputReadIndex)

      const shapingInput = this.noiseSuppression
      const adaptingTones = shapingInput && !this.gateOpen &&
        this.previousFrameRms <= this.noiseRmsHigh * this.TONE_ADAPT_NOISE_RATIO

      for (let i = 0; i < this.FRAME_SIZE; i++) {
        const inputSample = shapingInput
          ? this.toneSuppressor.process(this.frameToProcess[i], adaptingTones)
          : this.frameToProcess[i]
        this.frameToProcess[i] = inputSample
        this.vadWindowSquareSum += inputSample * inputSample
        this.vadWindowSampleCount++
        const filteredSample = this.decimateVadSample(inputSample)
        if (filteredSample === null) continue
        this.vad16kBuffer[this.vad16kWriteIndex++] = filteredSample
        this.pitchBuffer[this.pitchWriteIndex] = filteredSample
        this.pitchWriteIndex = (this.pitchWriteIndex + 1) % this.pitchBuffer.length
        if (this.pitchFilled < this.pitchBuffer.length) this.pitchFilled++

        if (this.vad16kWriteIndex === this.VAD_FRAME_SIZE) {
          this.voicing = this.measureVoicing()
          const lag = this.voicingLag
          const previousLag = this.previousVoicingLag
          const lagContinuous = lag > 0 && previousLag > 0 &&
            Math.abs(lag - previousLag) / previousLag <= this.VOICING_LAG_DRIFT_MAX
          this.previousVoicingLag = lag
          const windowVoiced = this.voicing >= this.VOICING_SPEECH_MIN && lagContinuous
          if (windowVoiced) this.voicingHoldWindows = this.VOICING_HOLD_WINDOWS
          else if (this.voicingHoldWindows > 0) this.voicingHoldWindows--

          this.voicingRingFrameIds[this.voicingRingWriteIndex] = this.audioFrameId
          this.voicingRingVoiced[this.voicingRingWriteIndex] = windowVoiced ? 1 : 0
          this.voicingRingWriteIndex = (this.voicingRingWriteIndex + 1) % this.voicingRingFrameIds.length

          if (this.sileroVadEnabled) {
            const audioFrame = this.vad16kBuffer.slice()
            const windowRms = Math.sqrt(this.vadWindowSquareSum / Math.max(1, this.vadWindowSampleCount))
            this.port.postMessage(
              { type: 'audio16k', audio: audioFrame, sequence: this.vadSequence++, endFrameId: this.audioFrameId, windowRms },
              [audioFrame.buffer]
            )
          }
          this.vad16kWriteIndex = 0
          this.vadWindowSquareSum = 0
          this.vadWindowSampleCount = 0
        }
      }

      let sumSquares = 0
      let currentPeak = 0
      let lowBandState = 0
      let lowBandSquares = 0
      let highBandSquares = 0
      let zeroCrossings = 0
      for (let i = 0; i < this.FRAME_SIZE; i++) {
        const sample = this.frameToProcess[i]
        currentPeak = Math.max(currentPeak, Math.abs(sample))
        sumSquares += sample * sample
        lowBandState += 0.12 * (sample - lowBandState)
        const highBand = sample - lowBandState
        lowBandSquares += lowBandState * lowBandState
        highBandSquares += highBand * highBand
        if (i > 0 && (sample >= 0) !== (this.frameToProcess[i - 1] >= 0)) zeroCrossings++
      }
      const currentRms = Math.sqrt(sumSquares / this.FRAME_SIZE)
      const attackFactor = currentRms > this.rmsSmoothed ? 0.6 : 0.2
      this.rmsSmoothed = attackFactor * currentRms + (1 - attackFactor) * this.rmsSmoothed
      const currentDb = 20 * Math.log10(Math.max(this.rmsSmoothed, 0.000001))
      if (++this.meterFrameCounter >= 5) {
        this.meterFrameCounter = 0
        this.port.postMessage({ type: 'micLevelDb', db: Math.max(-100, Math.min(0, currentDb)) })
      }
      const currentZcr = zeroCrossings / (this.FRAME_SIZE - 1)
      const currentTilt = Math.sqrt(highBandSquares / Math.max(1e-12, lowBandSquares))

      if (shapingInput && ++this.toneLogFrames >= this.TONE_LOG_INTERVAL_FRAMES) {
        this.toneLogFrames = 0
        const humCount = this.toneSuppressor.activeHumCount
        const whineCount = this.toneSuppressor.activeWhineCount
        if (humCount !== this.toneLoggedHumCount || whineCount !== this.toneLoggedWhineCount) {
          this.toneLoggedHumCount = humCount
          this.toneLoggedWhineCount = whineCount
          this.port.postMessage({
            type: 'log',
            message: `Tonal notches hum=${humCount > 0 ? `${humCount}@${this.toneSuppressor.lowestHumHz.toFixed(1)}Hz` : 'off'} whine=${whineCount > 0 ? `${whineCount}@${this.toneSuppressor.strongestWhineHz.toFixed(0)}Hz` : 'off'}`
          })
        }
      }

      if (this.transientHoldFrames > 0) this.transientHoldFrames--
      const attackDb = 20 * Math.log10(currentRms / Math.max(1e-6, this.previousFrameRms))
      const crest = currentPeak / Math.max(1e-6, currentRms)
      const frameNominatesTransient = attackDb >= this.TRANSIENT_ATTACK_DB &&
        crest >= this.TRANSIENT_CREST &&
        currentRms >= Math.max(this.noiseRmsHigh * 2, this.AUDIBLE_MIN_RMS)
      this.previousFrameRms = currentRms

      let convictedTransient = false
      if (this.transientCandidateFrameId >= 0) {
        this.transientCandidateRms = Math.max(this.transientCandidateRms, currentRms)
        this.transientCandidateAge++
        if (currentRms <= this.transientCandidateRms * this.TRANSIENT_DECAY_RATIO) {
          convictedTransient = true
        } else if (this.transientCandidateAge >= this.TRANSIENT_DECAY_FRAMES) {
          this.transientCandidateFrameId = -1
        }
      } else if (frameNominatesTransient) {
        this.transientCandidateFrameId = this.audioFrameId
        this.transientCandidateRms = currentRms
        this.transientCandidateAge = 0
        this.transientCandidateAttackDb = attackDb
        this.transientCandidateCrest = crest
      }

      if (convictedTransient) {
        this.markBufferedTransientFrom(this.transientCandidateFrameId)
        this.transientCandidateFrameId = -1
        this.transientHoldFrames = this.TRANSIENT_HOLD_FRAMES
        this.rejectedTransientFrames++
        if (this.rejectedTransientFrames % 20 === 1) {
          this.port.postMessage({
            type: 'log',
            message: `VAD rejected transient at ${currentDb.toFixed(0)} dB ` +
              `(attack ${this.transientCandidateAttackDb.toFixed(0)} dB, ` +
              `crest ${this.transientCandidateCrest.toFixed(1)}, ` +
              `decayed ${(20 * Math.log10(Math.max(1e-6, currentRms) / Math.max(1e-6, this.transientCandidateRms))).toFixed(0)} dB ` +
              `in ${this.transientCandidateAge} frames)`
          })
        }
      }
      const ringTransient = convictedTransient || this.transientHoldFrames > 0
      const inTransient = ringTransient || this.transientCandidateFrameId >= 0

      if (this.sileroVadEnabled && this.sileroVadHealthy && this.lastSileroResultFrameId >= 0 &&
        this.audioFrameId - this.lastSileroResultFrameId > 120) {
        this.sileroVadHealthy = false
        this.speechSegmentOpen = false
        this.consecutiveVadSpeechResults = 0
        this.attackGapWindows = 0
        this.attackFirstWindowEndFrameId = -1
        this.consecutiveVadSilenceResults = 0
        this.segmentPeakProbability = 0
        this.segmentWindows = 0
      }

      if (this.gateOpen) {
        this.closedFramesSinceSpeech = 0
      } else {
        this.closedFramesSinceSpeech++
        if (this.closedFramesSinceSpeech > this.NOISE_TRACKER_SETTLE_FRAMES) {
          this.noiseRmsHistory[this.noiseRmsHistoryIndex] =
            Math.min(currentRms, this.noiseRmsHigh * this.IMPULSE_CLAMP_RATIO)
          this.noiseRmsHistoryIndex = (this.noiseRmsHistoryIndex + 1) % this.noiseRmsHistory.length
          this.noiseRmsHistoryCount++
          if (this.noiseRmsHistoryCount % this.NOISE_RMS_REFRESH_FRAMES === 0) {
            const filled = Math.min(this.noiseRmsHistoryCount, this.noiseRmsHistory.length)
            const recent = this.noiseRmsScratch.subarray(0, filled)
            recent.set(this.noiseRmsHistory.subarray(0, filled))
            recent.sort()
            this.noiseRmsHigh = recent[Math.floor((filled - 1) * this.NOISE_RMS_QUANTILE)]
          }
        }
      }

      if (this.thresholdMode === 'manual') {
        if (!inTransient && currentDb >= this.manualThresholdDb) {
          if (this.manualVadHoldFrames === 0) {
            this.markBufferedSpeechFrom(this.audioFrameId - this.SPEECH_PREROLL_FRAMES)
          }
          this.manualVadHoldFrames = this.MANUAL_HOLD_FRAMES
        } else if (this.manualVadHoldFrames > 0) {
          if (!inTransient && currentDb >= this.manualThresholdDb - 8) {
            this.manualVadHoldFrames = this.MANUAL_HOLD_FRAMES
          } else {
            this.manualVadHoldFrames--
          }
        }
      }

      const isSpeaking = this.thresholdMode === 'manual'
        ? this.manualVadHoldFrames > 0
        : this.sileroVadEnabled
          ? this.speechSegmentOpen
          : true
      this.gateOpen = isSpeaking

      const writeIndex = this.speechRingWriteIndex
      this.speechRingSpeech[writeIndex] = isSpeaking ? 1 : 0
      this.speechRingFrameIds[writeIndex] = this.audioFrameId
      this.speechRingRms[writeIndex] = currentRms
      this.speechRingZcr[writeIndex] = currentZcr
      this.speechRingTilt[writeIndex] = currentTilt
      this.speechRingTransient[writeIndex] = ringTransient ? 1 : 0
      this.speechRingFrames[writeIndex].set(this.frameToProcess)
      this.speechRingWriteIndex = (writeIndex + 1) % this.speechRingSpeech.length
      this.speechRingCount++
      this.audioFrameId++

      const delayFrames = this.sileroVadEnabled
        ? this.DECISION_DELAY_FRAMES
        : (this.noiseSuppression ? this.LOOKAHEAD_DELAY_FRAMES : 0)
      const readIndex = (writeIndex - delayFrames + this.speechRingSpeech.length) % this.speechRingSpeech.length
      const hasDelayedFrame = this.speechRingCount > delayFrames
      const delayedIsSpeech = hasDelayedFrame && this.speechRingSpeech[readIndex] === 1

      const delayedFrame = this.speechRingFrames[readIndex]
      let engineFrame = delayedFrame
      let engineGain = 1
      if (hasDelayedFrame && this.noiseSuppression) {
        let delayedPeak = 0
        for (let i = 0; i < this.FRAME_SIZE; i++) {
          const magnitude = delayedFrame[i] < 0 ? -delayedFrame[i] : delayedFrame[i]
          if (magnitude > delayedPeak) delayedPeak = magnitude
        }
        this.engineInputPeakEnvelope = delayedPeak > this.engineInputPeakEnvelope
          ? delayedPeak
          : this.engineInputPeakEnvelope +
            this.ENGINE_INPUT_PEAK_FALL * (delayedPeak - this.engineInputPeakEnvelope)
        engineGain = this.engineInputGain > this.engineAppliedGain
          ? Math.min(this.engineInputGain, this.engineAppliedGain * this.ENGINE_GAIN_SLEW)
          : Math.max(this.engineInputGain, this.engineAppliedGain / this.ENGINE_GAIN_SLEW)
        engineGain = Math.max(1, engineGain)
        this.engineAppliedGain = engineGain
        if (engineGain > 1) {
          const ceiling = this.ENGINE_SAMPLE_CEILING
          for (let i = 0; i < this.FRAME_SIZE; i++) {
            const scaled = delayedFrame[i] * engineGain
            this.engineInputFrame[i] = scaled > ceiling ? ceiling : scaled < -ceiling ? -ceiling : scaled
          }
          engineFrame = this.engineInputFrame
        }

        const delayedRms = this.speechRingRms[readIndex]
        const engineLearnFloor = Math.max(
          this.ENGINE_INPUT_MIN_SPEECH_RMS,
          this.engineInputSpeechRms * this.ENGINE_INPUT_LEARN_FLOOR_RATIO
        )
        if (delayedIsSpeech && delayedRms >= engineLearnFloor) {
          if (this.engineInputSpeechFrames === 0) {
            this.engineInputSpeechRms = delayedRms
          } else {
            const learnRms = Math.min(delayedRms, this.engineInputSpeechRms * this.IMPULSE_CLAMP_RATIO)
            this.engineInputSpeechRms += this.ENGINE_INPUT_RMS_RATE * (learnRms - this.engineInputSpeechRms)
          }
          const seeding = ++this.engineInputSpeechFrames <= this.ENGINE_INPUT_SEED_FRAMES

          const rmsGain = this.ENGINE_INPUT_TARGET_RMS /
            Math.max(this.ENGINE_INPUT_MIN_SPEECH_RMS, this.engineInputSpeechRms)
          const peakGain = this.ENGINE_INPUT_PEAK_CEILING /
            Math.max(this.ENGINE_INPUT_MIN_SPEECH_RMS, this.engineInputPeakEnvelope)
          const targetGain = Math.max(
            1,
            Math.min(this.ENGINE_INPUT_MAX_GAIN, Math.min(rmsGain, peakGain))
          )
          this.engineInputGain += (targetGain - this.engineInputGain) *
            (seeding ? this.ENGINE_INPUT_SEED_GAIN_SMOOTH : this.ENGINE_INPUT_GAIN_SMOOTH)

          if (++this.engineInputLogFrames >= 100) {
            this.engineInputLogFrames = 0
            if (Math.abs(20 * Math.log10(this.engineInputGain / this.engineInputLoggedGain)) >= 1) {
              this.engineInputLoggedGain = this.engineInputGain
              const dbfs = (value: number) => (20 * Math.log10(Math.max(1e-6, value))).toFixed(1)
              this.port.postMessage({
                type: 'log',
                message: `Model input +${(20 * Math.log10(engineGain)).toFixed(1)}dB: speech ` +
                  `${dbfs(this.engineInputSpeechRms)}dBFS -> ${dbfs(this.engineInputSpeechRms * engineGain)}dBFS, ` +
                  `peak ${dbfs(this.engineInputPeakEnvelope * engineGain)}dBFS`
              })
            }
          }
        }
      }

      let outputFrame: Float32Array | null = engineFrame
      let frameDenoised = false
      if (hasDelayedFrame && this.noiseSuppression && this.engine.isReady) {
        try {
          const denoised = this.engine.process(engineFrame)
          if (denoised) {
            outputFrame = denoised
            frameDenoised = true
          } else {
            outputFrame = null
          }
        } catch (error) {
          outputFrame = null
          this.denoiserErrorCount++
          if (this.denoiserErrorCount <= 3 || this.denoiserErrorCount % 100 === 0) {
            this.port.postMessage({
              type: 'log',
              message: `${this.engine.label} frame failed (${this.denoiserErrorCount}): ${error instanceof Error ? error.message : String(error)}`
            })
          }
        }
      }
      if (!hasDelayedFrame) {
        this.processedFrame.fill(0)
        this.gateGain = 0
      } else if (!this.noiseSuppression && !this.sileroVadEnabled) {
        this.processedFrame.set(delayedFrame)
        this.gateGain = 1
        this.gateHoldFrames = 0
      } else {
        const gateSource = outputFrame ?? engineFrame
        if (frameDenoised && outputFrame && this.engine.supportsAttenuationLimit) {
          let inputSquares = 0
          let outputSquares = 0
          for (let i = 0; i < this.FRAME_SIZE; i++) {
            inputSquares += engineFrame[i] * engineFrame[i]
            outputSquares += outputFrame[i] * outputFrame[i]
          }
          const vetoReductionDb = Math.max(
            this.DF_VETO_MIN_REDUCTION_DB,
            this.attenuationLimit - this.DF_VETO_MARGIN_DB
          )
          const emptiedByDenoiser = inputSquares > 1e-12 &&
            10 * Math.log10(Math.max(1e-12, outputSquares) / inputSquares) <= -vetoReductionDb
          if (delayedIsSpeech) {
            this.dfVetoFrames = 0
            this.dfVetoLatched = false
          } else if (emptiedByDenoiser) {
            if (this.dfVetoFrames < this.DF_VETO_ENGAGE_FRAMES) this.dfVetoFrames++
            if (this.dfVetoFrames >= this.DF_VETO_ENGAGE_FRAMES) this.dfVetoLatched = true
          } else {
            this.dfVetoFrames = Math.max(0, this.dfVetoFrames - this.DF_VETO_RELEASE_STEP)
            if (this.dfVetoFrames === 0) this.dfVetoLatched = false
          }
        } else {
          this.dfVetoFrames = 0
          this.dfVetoLatched = false
        }

        const speechNow = delayedIsSpeech
        if (speechNow) this.gateHoldFrames = this.GATE_HOLD_FRAMES
        else if (this.dfVetoLatched) this.gateHoldFrames = 0
        else if (this.gateHoldFrames > 0) this.gateHoldFrames--

        const targetGain = speechNow || this.gateHoldFrames > 0 ? 1 : this.GATE_FLOOR
        const engineOutputScale = 1 / engineGain
        for (let i = 0; i < this.FRAME_SIZE; i++) {
          const coefficient = targetGain > this.gateGain
            ? this.GATE_ATTACK_COEFFICIENT
            : this.GATE_RELEASE_COEFFICIENT
          this.gateGain += (targetGain - this.gateGain) * coefficient
          this.processedFrame[i] = gateSource[i] * engineOutputScale * this.gateGain
        }
        if (targetGain === this.GATE_FLOOR && this.gateGain <= this.GATE_SILENCE_EPSILON) {
          this.gateGain = this.GATE_FLOOR
        }
      }

      if ((this.noiseSuppression || this.sileroVadEnabled) && !this.isMuted && hasDelayedFrame) {
        const inputAudible = currentRms >= Math.max(this.AUDIBLE_MIN_RMS, this.noiseRmsHigh)
        if (this.gateGain <= 0.02 && inputAudible) this.silentOutputFrames++
        else this.silentOutputFrames = 0
        if (this.silentOutputFrames > 0 && this.silentOutputFrames % this.SILENCE_REPORT_FRAMES === 0) {
          this.port.postMessage({
            type: 'log',
            message: `Gate has been closed for ${(this.silentOutputFrames / 100).toFixed(0)}s on audible input: ` +
              `engine ${this.engine.isReady ? 'ready' : `NOT ready (${this.engine.stageName})`}, ` +
              `silero ${this.sileroVadEnabled ? (this.sileroVadHealthy ? 'healthy' : 'stalled') : 'off'}, ` +
              `mode ${this.thresholdMode}, segment ${this.gateOpen ? 'open' : 'closed'}, ` +
              `gate ${this.vadOnThreshold.toFixed(2)}/${this.vadSemiVoicedOn.toFixed(2)}/${this.vadUnvoicedOn.toFixed(2)}, ` +
              `peak ${this.segmentPeakProbability.toFixed(2)}, voicing ${this.voicing.toFixed(2)} ` +
              `(hold ${this.voicingHoldWindows}), veto ${this.dfVetoFrames}, overflow ${this.overflowCount}`
          })
        }
      } else {
        this.silentOutputFrames = 0
      }

      if (this.noiseSuppression) {
        let framePeak = 0
        let frameSquares = 0
        this.voiceShaper.setSpeechReference(
          this.alcSpeechFrames > 0 ? Math.max(this.ALC_MIN_SPEECH_RMS, this.alcSpeechRms) : 0
        )
        for (let i = 0; i < this.FRAME_SIZE; i++) {
          const sample = this.voiceShaper.process(this.processedFrame[i])
          this.processedFrame[i] = sample
          const magnitude = sample < 0 ? -sample : sample
          if (magnitude > framePeak) framePeak = magnitude
          frameSquares += sample * sample
        }

        this.alcPeakEnvelope = framePeak > this.alcPeakEnvelope
          ? framePeak
          : this.alcPeakEnvelope + this.ALC_PEAK_FALL * (framePeak - this.alcPeakEnvelope)

        const frameGain = framePeak > 0
          ? Math.min(
            this.alcGain,
            Math.max(0.1, this.ALC_PEAK_CEILING / (framePeak * this.alcDownstreamGain))
          )
          : this.alcGain
        for (let i = 0; i < this.FRAME_SIZE; i++) {
          const coefficient = frameGain < this.alcAppliedGain ? this.ALC_LIMIT_ATTACK : this.ALC_LIMIT_RELEASE
          this.alcAppliedGain += (frameGain - this.alcAppliedGain) * coefficient
          const amplified = this.processedFrame[i] * this.alcAppliedGain
          this.processedFrame[i] = amplified > this.ALC_OUTPUT_CEILING
            ? this.ALC_OUTPUT_CEILING
            : amplified < -this.ALC_OUTPUT_CEILING ? -this.ALC_OUTPUT_CEILING : amplified
        }

        const frameRms = Math.sqrt(frameSquares / this.FRAME_SIZE)
        if (this.gateGain > 0.5 && framePeak >= this.ALC_MIN_SPEECH_RMS) {
          if (this.alcSpeechFrames === 0) {
            this.alcSpeechRms = frameRms
          } else {
            const learnRms = Math.min(frameRms, this.alcSpeechRms * this.IMPULSE_CLAMP_RATIO)
            this.alcSpeechRms += this.ALC_RMS_RATE * (learnRms - this.alcSpeechRms)
          }
          const seeding = ++this.alcSpeechFrames <= this.ALC_SEED_FRAMES

          const rmsGain = this.ALC_TARGET_RMS / Math.max(this.ALC_MIN_SPEECH_RMS, this.alcSpeechRms)
          const peakGain = this.ALC_PEAK_CEILING /
            (Math.max(this.ALC_MIN_SPEECH_RMS, this.alcPeakEnvelope) * this.alcDownstreamGain)
          const targetGain = Math.max(1, Math.min(this.ALC_MAX_GAIN, Math.min(rmsGain, peakGain)))
          this.alcGain += (targetGain - this.alcGain) *
            (seeding ? this.ALC_SEED_GAIN_SMOOTH : this.ALC_GAIN_SMOOTH)

          if (++this.alcLogFrames >= 100) {
            this.alcLogFrames = 0
            const dbfs = (value: number) => (20 * Math.log10(Math.max(1e-6, value))).toFixed(1)
            if (Math.abs(20 * Math.log10(this.alcGain / this.alcLoggedGain)) >= 1) {
              this.alcLoggedGain = this.alcGain
              this.port.postMessage({
                type: 'log',
                message: `ALC ${dbfs(this.alcGain)}dB: active speech ${dbfs(this.alcSpeechRms)}dBFS -> ` +
                  `${dbfs(this.alcSpeechRms * this.alcGain)}dBFS, peak ${dbfs(this.alcPeakEnvelope * this.alcGain)}dBFS`
              })
            }
          }
        }
      }

      if (++this.levelReportFrames >= this.LEVEL_REPORT_FRAMES) {
        this.levelReportFrames = 0
        this.reportLevelState()
      }

      if (this.calibrationFramesLeft <= 0) this.refreshAttenuationLimit()

      if (this.calibrationFramesLeft > 0) {
        const elapsedFrames = this.calibrationTotalFrames - this.calibrationFramesLeft
        this.calibrationFramesLeft--
        const insideWindow = elapsedFrames >= this.CALIBRATION_LEAD_IN_FRAMES

        const normalizedRms = currentRms / this.gainFactor
        if (insideWindow) {
          this.calibrationSilenceHistory[this.calibrationSilenceHistoryIndex] = normalizedRms
          this.calibrationSilenceHistoryIndex =
            (this.calibrationSilenceHistoryIndex + 1) % this.calibrationSilenceHistory.length
          this.calibrationSilenceHistoryCount++
          if (this.calibrationSilenceHistoryCount % 10 === 0) {
            const filled = Math.min(this.calibrationSilenceHistoryCount, this.calibrationSilenceHistory.length)
            const recent = this.calibrationSilenceScratch.subarray(0, filled)
            recent.set(this.calibrationSilenceHistory.subarray(0, filled))
            recent.sort()
            this.calibrationSilenceLevel = recent[Math.floor((filled - 1) * 0.4)]
          }
        }
        const nearFieldSpeechFloor = Math.max(this.calibrationSilenceLevel * 8, 0.0008)
        const containsSpeech = this.sileroVadProbability >= this.CALIBRATION_SPEECH_VAD_FLOOR &&
          normalizedRms >= nearFieldSpeechFloor

        if (insideWindow && !containsSpeech && this.calibrationCount < this.calibrationRms.length) {
          this.calibrationRms[this.calibrationCount] = normalizedRms
          this.calibrationCount++
          this.calibrationZcrSum += currentZcr
          this.calibrationSpectralTiltSum += currentTilt
        } else if (insideWindow && containsSpeech) {
          this.calibrationRejectedSpeechFrames++
        }

        if (this.calibrationFramesLeft === 0) {
          const samples = Array.from(this.calibrationRms.subarray(0, this.calibrationCount)).sort((a, b) => a - b)
          const noiseVadSamples = Array.from(this.calibrationNoiseVad.subarray(0, this.calibrationNoiseVadCount)).sort((a, b) => a - b)
          const percentile = (values: number[], p: number) => values.length
            ? values[Math.min(values.length - 1, Math.floor((values.length - 1) * p))]
            : 0
          this.port.postMessage({
            type: 'calibrationResult',
            noiseFloor: percentile(samples, 0.5),
            lowNoise: percentile(samples, 0.2),
            peakNoise: percentile(samples, 0.95),
            noiseVadMedian: percentile(noiseVadSamples, 0.5),
            noiseVadHigh: percentile(noiseVadSamples, 0.95),
            zeroCrossingRate: this.calibrationZcrSum / Math.max(1, this.calibrationCount),
            spectralTilt: this.calibrationSpectralTiltSum / Math.max(1, this.calibrationCount),
            acceptedFrames: this.calibrationCount,
            rejectedSpeechFrames: this.calibrationRejectedSpeechFrames,
            silenceReference: this.calibrationSilenceLevel
          })
        }
      }

      if (measureWhileMuted && !this.monitorWhileMuted) {
        this.processedFrame.fill(0)
        this.gateGain = 0
      }

      let outboundSpeaking = false
      if (!this.isMuted && !measureWhileMuted && hasDelayedFrame) {
        let outboundSquares = 0
        for (let i = 0; i < this.FRAME_SIZE; i++) {
          const sample = this.processedFrame[i]
          outboundSquares += sample * sample
        }
        const outboundRms = Math.sqrt(outboundSquares / this.FRAME_SIZE)

        if (this.sileroVadEnabled) {
          outboundSpeaking = !this.dfVetoLatched &&
            this.gateGain > 0.05 &&
            (delayedIsSpeech || this.gateHoldFrames > 0)
        }
      }

      if (this.sileroVadEnabled && this.lastVadSent !== outboundSpeaking) {
        this.port.postMessage({ type: 'vad', isSpeaking: outboundSpeaking })
        this.lastVadSent = outboundSpeaking
      }

      this.outputWriteIndex = this.pushToBuffer(this.outputBuffer, this.processedFrame, this.outputWriteIndex, this.outputReadIndex)
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

export function registerMicPipeline(name: string, createEngine: () => NoiseEngine): void {
  engineFactory = createEngine
  registerProcessor(name, MicPipelineProcessor)
}
