import { signalRService } from './signalr'
import { useAppStore } from '../store/useAppStore'
import i18n from '../i18n'
import manualGateProcessorUrl from './manual-gate-processor?worker&url'
import micPipelineDeepFilterUrl from './mic-pipeline-deepfilter?worker&url'
import micPipelineRnnoiseUrl from './mic-pipeline-rnnoise?worker&url'
import micTestCaptureProcessorUrl from './mic-test-capture-processor?worker&url'
import playbackBufferProcessorUrl from './playback-buffer-processor?worker&url'
import VadWorker from './silero-vad.worker?worker'
import {
  getDeepFilterAsset,
  getDeepFilterPayload,
  getSileroModel,
  preloadNoiseAssets,
  type DeepFilterPayload
} from './audio-assets'

export const VAD_CALIBRATION_ENABLED: boolean = false
const FIXED_SILERO_THRESHOLD = 0.4

const DEFAULT_SILERO_THRESHOLD = 0.4
const MIN_VOICE_CALIBRATION_WINDOWS = 12
const VAD_CALIBRATION_MARGIN_RATIO = 0.42
const VAD_CALIBRATION_MARGIN_FLOOR = 0.06
const VAD_CALIBRATION_MIN_THRESHOLD = 0.4
const VAD_CALIBRATION_MAX_THRESHOLD = 0.85

const vadThresholdFromVoiceLow = (voiceLow: number): number => {
  const margin = Math.max(VAD_CALIBRATION_MARGIN_FLOOR, voiceLow * VAD_CALIBRATION_MARGIN_RATIO)
  return Math.max(
    VAD_CALIBRATION_MIN_THRESHOLD,
    Math.min(VAD_CALIBRATION_MAX_THRESHOLD, voiceLow - margin)
  )
}

export type SmartNoiseModel = 'deepfilter' | 'rnnoise'
const SMART_MODEL_STORAGE_KEY = 'zabor_smart_noise_model'
const SUPPRESSION_STRENGTH_STORAGE_KEY = 'zabor_suppression_strength_db'
const SPEECH_ANALYZER_STORAGE_KEY = 'zabor_speech_analyzer_enabled'
const ECHO_CANCELLATION_STORAGE_KEY = 'zabor_echo_cancellation_enabled'
const RELAY_ONLY_STORAGE_KEY = 'zabor_relay_only_ice'

export const MIN_SUPPRESSION_STRENGTH_DB = 5
export const MAX_SUPPRESSION_STRENGTH_DB = 30

const DEEPFILTER_LOCAL_BASE = 'zabor-local://deepfilternet3'
const DEEPFILTER_ASSETS = new Set(['pkg/df_bg.wasm', 'models/DeepFilterNet3_onnx.tar.gz'])
const DEEPFILTER_MIN_ATTEN = MIN_SUPPRESSION_STRENGTH_DB
const DEEPFILTER_MAX_ATTEN = MAX_SUPPRESSION_STRENGTH_DB
const SUPPRESSION_SOFT_KNEE_DB = 24
const SUPPRESSION_ABOVE_KNEE_SLOPE = 0.5
const DEEPFILTER_SMART_DEFAULT_ATTEN = 15
const DEEPFILTER_MANUAL_ATTEN = 7
const DEEPFILTER_POST_FILTER_BETA = 0.02
const DEEPFILTER_MAX_POST_FILTER_BETA = 0.05
const TARGET_SPEECH_TO_NOISE_DB = 55
const ALC_TARGET_DBFS = -20
const TARGET_RESIDUAL_NOISE_DBFS = ALC_TARGET_DBFS - TARGET_SPEECH_TO_NOISE_DB
const ASSUMED_SPEECH_LEVEL_DBFS = -30
const DEFAULT_VAD_TRACKER_SEED = 0.05
const MIN_CALIBRATION_FRAMES = 20
const PLAYBACK_MAKEUP_GAIN_DB = 0
const PLAYBACK_VOICE_LIMIT_DB = -1.5
const PLAYBACK_STREAM_LIMIT_DB = -1.5
const PLAYBACK_VOICE_SPREAD = 0
const PLAYBACK_SEAT_GLIDE_S = 0.08
const OPUS_AUDIO_BITRATE = 64_000
const MIC_CAPTURE_TIMEOUT_MS = 10_000
const SILERO_MODEL_LOAD_TIMEOUT_MS = 10_000
const VAD_WORKER_READY_TIMEOUT_MS = 15_000
const CALIBRATION_PREPARE_TIMEOUT_MS = 20_000
const CALIBRATION_CLEANUP_TIMEOUT_MS = 5_000
const RUMBLE_GUARD_HZ = 70
const RUMBLE_GUARD_QS = [0.5177, 0.7071, 1.9319]
const PRE_ENGINE_SEED_FIELDS = [
  'engineInputGain',
  'engineInputSpeechRms',
  'engineInputPeakEnvelope',
  'noiseRmsHigh'
] as const
const ALC_SEED_FIELDS = [
  'alcGain',
  'alcSpeechRms',
  'alcPeakEnvelope'
] as const
const MIC_TEST_DURATION_MS = 5_000
const MIC_TEST_CAPTURE_GRACE_MS = 4_000
const MIC_TEST_PIPELINE_FLUSH_MS = 280
const MIC_TEST_MONITOR_SETTLE_MS = 120
const MIC_TEST_SEEK_TAIL_S = 0.05

export const MIC_TEST_SILENCE_DBFS = -70

export type MicTestClip = {
  durationSeconds: number
  peakDb: number
}

type SpeakingEntry = {
  timer: NodeJS.Timeout
  stream: MediaStream

  nodes: AudioNode[]
}

type CalibrationResult = {
  vadThreshold?: number
  voiceLow?: number
  voiceMedian?: number
  voiceHigh?: number
  peakProbability?: number
  acceptedVoiceWindows?: number
  noiseFloor?: number
  lowNoise?: number
  peakNoise?: number
  attenuationLimit?: number
  zeroCrossingRate?: number
  spectralTilt?: number
  acceptedFrames?: number
  rejectedSpeechFrames?: number
  noiseVadMedian?: number
  noiseVadHigh?: number
}

type StoredVoiceProfile = {
  version: 41
  timestamp: number
  vadThreshold: number
  voiceLow: number
  voiceMedian: number
  voiceHigh: number
}

type StoredEnvironmentProfile = {
  version: number
  timestamp: number
  noiseFloor: number
  lowNoise: number
  peakNoise: number
  attenuationLimit: number
  zeroCrossingRate: number
  spectralTilt: number
  noiseVadMedian?: number
  noiseVadHigh?: number
}

type AudioDevices = {
  inputs: MediaDeviceInfo[]
  outputs: MediaDeviceInfo[]
}

type AudioDeviceChangeResult = AudioDevices & {
  inputDeviceId: string
  outputDeviceId: string
}

export type CalibrationFailureCode =
  | 'CALIBRATION_ENGINE_UNAVAILABLE'
  | 'CALIBRATION_NO_MIC'
  | 'CALIBRATION_BUSY'
  | 'CALIBRATION_TIMEOUT'
  | 'CALIBRATION_NEEDS_VOICE'
  | 'CALIBRATION_NEEDS_SILENCE'

export class CalibrationError extends Error {
  constructor(public readonly code: CalibrationFailureCode, public readonly detail = '') {
    super(detail ? `${code}: ${detail}` : code)
    this.name = 'CalibrationError'
  }
}

export function describeMediaError(error: unknown): string {
  if (error && typeof error === 'object' && 'name' in error) {
    const named = error as { name?: string, message?: string }
    return `${named.name || 'Error'}: ${named.message || ''}`.trim()
  }
  return error instanceof Error ? error.message : String(error)
}

export type MicrophoneErrorKind = 'micNoAccess' | 'micBusy' | 'micNotFound' | 'unknown'

export function classifyMicrophoneError(detail: string): MicrophoneErrorKind {
  if (/NotReadableError|TrackStartError|AbortError/i.test(detail)) return 'micBusy'
  if (/NotFoundError|DevicesNotFoundError|OverconstrainedError/i.test(detail)) return 'micNotFound'
  if (/NotAllowedError|PermissionDeniedError|SecurityError|MIC_ACCESS_FAILED/i.test(detail)) return 'micNoAccess'
  return 'unknown'
}

function withTimeout<T>(
  operation: Promise<T>,
  timeoutMs: number,
  message: string,
  onLate?: (value: T) => void
): Promise<T> {
  let timer: number | undefined
  let timedOut = false
  const guard = new Promise<never>((_, reject) => {
    timer = window.setTimeout(() => {
      timedOut = true
      reject(new Error(message))
    }, timeoutMs)
  })
  if (onLate) {
    operation.then(value => { if (timedOut) onLate(value) }).catch(() => { })
  }
  return Promise.race([operation, guard]).finally(() => {
    if (timer !== undefined) window.clearTimeout(timer)
  })
}

const SILENCE_SUPPRESSION_OFF = { voiceActivityDetection: false } as RTCOfferOptions
const SILENCE_SUPPRESSION_OFF_WITH_ICE_RESTART = {
  iceRestart: true,
  voiceActivityDetection: false
} as RTCOfferOptions

function optimizeSDP(sdp: string, isRelayOnly = false): string {
  let lines = sdp.split('\r\n')

  if (isRelayOnly) {
    lines = lines.filter(line => {
      if (line.startsWith('a=candidate:')) {
        return line.includes('typ relay')
      }
      return true
    })
  }

  const opusRegex = /a=rtpmap:(\d+)\s+opus\/48000\/2/i
  const audioMatch = sdp.match(opusRegex)
  if (audioMatch) {
    const pt = audioMatch[1]
    const opusFmtp = `useinbandfec=1;usedtx=0;maxaveragebitrate=${OPUS_AUDIO_BITRATE};maxplaybackrate=48000;sprop-maxcapturerate=48000;stereo=0;sprop-stereo=0;cbr=0;minptime=10`
    let fmtpFound = false
    for (let i = 0; i < lines.length; i++) {
      if (lines[i].startsWith(`a=fmtp:${pt}`)) {
        lines[i] = `a=fmtp:${pt} ${opusFmtp}`
        fmtpFound = true
        break
      }
    }
    if (!fmtpFound) {
      for (let i = 0; i < lines.length; i++) {
        if (lines[i].startsWith(`a=rtpmap:${pt}`)) {
          lines.splice(i + 1, 0, `a=fmtp:${pt} ${opusFmtp}`)
          break
        }
      }
    }
    let audioSectionIdx = -1
    for (let i = 0; i < lines.length; i++) {
      if (lines[i].startsWith('m=audio')) {
        audioSectionIdx = i
        break
      }
    }
    if (audioSectionIdx !== -1) {
      const comfortNoisePayloads = new Set<string>()
      for (const line of lines) {
        const comfortNoiseMatch = line.match(/^a=rtpmap:(\d+)\s+CN\/\d+/i)
        if (comfortNoiseMatch) comfortNoisePayloads.add(comfortNoiseMatch[1])
      }
      const mediaParts = lines[audioSectionIdx].split(' ')
      const payloads = mediaParts.slice(3).filter(payload => !comfortNoisePayloads.has(payload))
      lines[audioSectionIdx] = [...mediaParts.slice(0, 3), pt, ...payloads.filter(payload => payload !== pt)].join(' ')
      if (comfortNoisePayloads.size > 0) {
        lines = lines.filter(line => {
          const attributeMatch = line.match(/^a=(?:rtpmap|fmtp|rtcp-fb):(\d+)/)
          return !attributeMatch || !comfortNoisePayloads.has(attributeMatch[1])
        })
      }

      let audioSectionEnd = lines.length
      for (let i = audioSectionIdx + 1; i < lines.length; i++) {
        if (lines[i].startsWith('m=')) {
          audioSectionEnd = i
          break
        }
      }
      for (let i = audioSectionEnd - 1; i > audioSectionIdx; i--) {
        if (/^b=(AS|TIAS):/i.test(lines[i]) || /^a=ptime:/i.test(lines[i])) lines.splice(i, 1)
      }
      audioSectionEnd = lines.length
      for (let i = audioSectionIdx + 1; i < lines.length; i++) {
        if (lines[i].startsWith('m=')) {
          audioSectionEnd = i
          break
        }
      }
      let bandwidthInsertIndex = audioSectionEnd
      for (let i = audioSectionIdx + 1; i < audioSectionEnd; i++) {
        if (lines[i].startsWith('a=')) {
          bandwidthInsertIndex = i
          break
        }
      }
      lines.splice(
        bandwidthInsertIndex,
        0,
        `b=AS:${Math.ceil(OPUS_AUDIO_BITRATE / 1000)}`,
        `b=TIAS:${OPUS_AUDIO_BITRATE}`,
        'a=ptime:20'
      )
    }
  }

  let videoSectionIdx = -1
  for (let i = 0; i < lines.length; i++) {
    if (lines[i].startsWith('m=video')) {
      videoSectionIdx = i
      break
    }
  }

  if (videoSectionIdx !== -1) {
    const h264Payloads: string[] = []
    const fecPayloads: string[] = []
    const rtxCandidates: string[] = []
    const aptByPayload: Record<string, string> = {}

    for (let i = videoSectionIdx + 1; i < lines.length; i++) {
      if (lines[i].startsWith('m=')) break
      const rtpmapMatch = lines[i].match(/^a=rtpmap:(\d+)\s+([A-Za-z0-9-]+)\/90000/i)
      if (rtpmapMatch) {
        const pt = rtpmapMatch[1]
        const codec = rtpmapMatch[2].toLowerCase()
        if (codec === 'h264') h264Payloads.push(pt)
        else if (codec === 'rtx') rtxCandidates.push(pt)
        else if (codec === 'red' || codec === 'ulpfec') fecPayloads.push(pt)
      }
      const aptMatch = lines[i].match(/^a=fmtp:(\d+)\s+apt=(\d+)/i)
      if (aptMatch) aptByPayload[aptMatch[1]] = aptMatch[2]
    }

    if (h264Payloads.length > 0) {
      const keptPayloads = new Set([...h264Payloads, ...fecPayloads])
      const rtxPayloads: string[] = []
      for (const pt of rtxCandidates) {
        const apt = aptByPayload[pt]
        if (apt && keptPayloads.has(apt)) {
          rtxPayloads.push(pt)
          keptPayloads.add(pt)
        }
      }
      const orderedPayloads = [...h264Payloads, ...rtxPayloads, ...fecPayloads]

      const filteredLines: string[] = []
      let skipVideoTracks = false

      for (let i = 0; i < lines.length; i++) {
        if (i === videoSectionIdx) {
          const parts = lines[i].split(' ')
          const newVideoLine = `${parts[0]} ${parts[1]} ${parts[2]} ${orderedPayloads.join(' ')}`
          filteredLines.push(newVideoLine)
          skipVideoTracks = true
          continue
        }
        if (skipVideoTracks && lines[i].startsWith('m=')) {
          skipVideoTracks = false
        }
        if (skipVideoTracks) {
          if (lines[i].startsWith('a=rtpmap:') || lines[i].startsWith('a=fmtp:') || lines[i].startsWith('a=rtcp-fb:')) {
            const ptMatch = lines[i].match(/^a=(?:rtpmap|fmtp|rtcp-fb):(\d+)/i)
            if (ptMatch && keptPayloads.has(ptMatch[1])) {
              filteredLines.push(lines[i])
            }
          } else {
            filteredLines.push(lines[i])
          }
        } else {
          filteredLines.push(lines[i])
        }
      }
      lines = filteredLines
    }
  }

  return lines.join('\r\n')
}

function createSilentAudioStream(): MediaStream {
  const ctx = new (window.AudioContext || (window as any).webkitAudioContext)()
  const dst = ctx.createMediaStreamDestination()
  const osc = ctx.createOscillator()
  const gain = ctx.createGain()
  gain.gain.value = 0
  osc.connect(gain)
  gain.connect(dst)
  osc.start()
  return dst.stream
}

export class WebRTCManager {
  private static readonly CALIBRATION_SCHEMA_VERSION = 41
  private static readonly CALIBRATION_SCHEMA_KEY = 'zabor_mic_calibration_schema'
  private static readonly CALIBRATION_PROFILE_MAX_AGE_MS = 90 * 24 * 60 * 60 * 1000
  private localStream: MediaStream | null = null
  private rawStream: MediaStream | null = null
  public localVideoStream: MediaStream | null = null
  private statsInterval: NodeJS.Timeout | null = null
  private streamGainNodes: Map<string, GainNode> = new Map()
  private streamSourceNodes: Map<string, MediaStreamAudioSourceNode> = new Map()
  private streamDelayNodes: Map<string, DelayNode> = new Map()
  private streamAudioElements: Map<string, HTMLAudioElement> = new Map()
  private streamCaptureContext: AudioContext | null = null
  private streamCaptureNode: AudioWorkletNode | null = null
  private streamCaptureDestination: MediaStreamAudioDestinationNode | null = null
  private removeStreamAudioListener: (() => void) | null = null

  private streamSyncInterval: NodeJS.Timeout | null = null
  private streamSyncOffsetEma: Map<string, number> = new Map()
  private streamSyncSkew: Map<string, number> = new Map()
  private viewerStates: Map<string, 'watching' | 'preview'> = new Map()
  private viewInterestTimer: NodeJS.Timeout | null = null
  private reportedViewStates: Map<string, 'watching' | 'preview'> = new Map()

  private peerConnections: Map<string, RTCPeerConnection> = new Map()
  private audioElements: Map<string, HTMLAudioElement> = new Map()
  private lastPacketsLost: Map<string, number> = new Map()
  private lastPacketsSent: Map<string, number> = new Map()
  private lossLadderStep: Map<string, number> = new Map()
  private lossBreachCount: Map<string, number> = new Map()
  private lossCleanCount: Map<string, number> = new Map()
  private appliedVideoProfiles: Map<string, string> = new Map()

  private pendingCandidates: Map<string, RTCIceCandidateInit[]> = new Map()

  private dcTimers: Map<string, NodeJS.Timeout> = new Map()

  private iceTimeoutTimers: Map<string, NodeJS.Timeout> = new Map()

  private retryCount: Map<string, number> = new Map()

  private iceRestartInFlight: Set<string> = new Set()

  private pendingRenegotiation: Set<string> = new Set()

  private static readonly MAX_ICE_RETRIES = 4

  private static readonly ICE_TIMEOUT_MS = 15000
  private static readonly DISCONNECTED_GRACE_MS = 10000
  private static readonly PEER_RELEVANCE_WAIT_MS = 5000
  private static readonly PENDING_CANDIDATES_PER_PEER = 64

  private currentDeviceId = 'default'
  private currentStreamQuality: 'high' | 'low' | 'camera' = 'low'
  private currentOutputDeviceId = 'default'
  private noiseSuppression = true

  private inputVolume = 100
  private outputVolume = 100
  private isDeafened = false

  private processedContext: AudioContext | null = null
  private processedSource: MediaStreamAudioSourceNode | null = null
  private micRumbleFilters: BiquadFilterNode[] = []
  private micOutputTap: AudioNode | null = null
  private micTestCaptureModules = new WeakSet<AudioContext>()
  private micTestRun: Promise<MicTestClip> | null = null
  private micTestBuffer: AudioBuffer | null = null
  private micTestContext: AudioContext | null = null
  private micTestGain: GainNode | null = null
  private micTestSource: AudioBufferSourceNode | null = null
  private micTestPlaying = false
  private micTestOffset = 0
  private micTestStartedAt = 0
  private micTestGeneration = 0
  private micTestEndedListener: (() => void) | null = null
  private readonly peerConnectStartedAt = new Map<string, number>()
  private inputGainNode: GainNode | null = null
  private manualGateNode: AudioWorkletNode | null = null
  private audioProcessorError: string | null = null
  private calibratedPreGainNode: GainNode | null = null
  private micNode: AudioWorkletNode | null = null
  private micNodeReady = false
  private readonly micLevelSeeds = new Map<string, Record<string, number>>()
  private micEngineError: string | null = null
  private lastMicCaptureError: string | null = null
  private lastReportedMicError: string | null = null
  private vadWorker: Worker | null = null
  private vadProbabilityHandler: ((data: {
    probability: number
    sequence?: number
    endFrameId?: number
    windowRms?: number
  }) => void) | null = null
  private vadWorkerInitPromise: Promise<void> | null = null
  private vadEpoch = 0
  private voiceProbeCollector: ((probability: number, windowRms: number) => void) | null = null

  private calibratedVadThreshold = DEFAULT_SILERO_THRESHOLD
  private calibratedNoiseFloor = 0.003
  private calibratedPreGainDb = 0
  private readonly RAW_MIC_BOOST = 1.7
  private calibratedVadTrackerSeed = DEFAULT_VAD_TRACKER_SEED
  private hasVoiceCalibration = false
  private calibrationDeviceId = 'default'
  private vadWorkerReady = false
  private calibrationInProgress = false
  private calibrationSuppressesSpeaking = false
  private localSpeakingState = false
  private thresholdMode = localStorage.getItem('zabor_threshold_mode') || 'auto'
  private manualThresholdValue = this.normalizeManualThreshold(parseFloat(localStorage.getItem('zabor_manual_threshold_value') || '-42'))
  private smartModel: SmartNoiseModel = localStorage.getItem(SMART_MODEL_STORAGE_KEY) === 'deepfilter'
    ? 'deepfilter'
    : 'rnnoise'
  private suppressionStrengthDb = this.normalizeSuppressionStrength(
    parseFloat(localStorage.getItem(SUPPRESSION_STRENGTH_STORAGE_KEY) || String(DEEPFILTER_SMART_DEFAULT_ATTEN))
  )
  private speechAnalyzerEnabled = localStorage.getItem(SPEECH_ANALYZER_STORAGE_KEY) !== 'false'
  private echoCancellationEnabled = localStorage.getItem(ECHO_CANCELLATION_STORAGE_KEY) === 'true'
  private calibrationForcesAnalyzer = false
  private activeStartPromise: Promise<boolean> | null = null

  private backgroundContext: AudioContext | null = null
  private backgroundSource: MediaStreamAudioSourceNode | null = null
  private backgroundAnalyser: AnalyserNode | null = null
  private backgroundMeterGain: GainNode | null = null
  private micMeterInterval: NodeJS.Timeout | null = null
  private micLevelDb = -100
  private micLevelListeners = new Set<(db: number) => void>()

  private rawAnalyserNode: AnalyserNode | null = null
  private silenceMonitorInterval: NodeJS.Timeout | null = null
  private silenceCounterMs = 0
  private isSilenceWarningActive = false
  private speakingIntervals: Map<string, SpeakingEntry> = new Map()

  private outputMixContext: AudioContext | null = null
  private outputPeakGuard: WaveShaperNode | null = null
  private outputBusGain: GainNode | null = null
  private mixAudioElement: HTMLAudioElement | null = null
  private userGainNodes: Map<string, GainNode> = new Map()
  private userLimiterNodes: Map<string, DynamicsCompressorNode> = new Map()
  private userPannerNodes: Map<string, StereoPannerNode> = new Map()
  private voiceSeatOrder: string[] = []
  private streamLimiterNodes: Map<string, DynamicsCompressorNode> = new Map()
  private userSourceNodes: Map<string, MediaStreamAudioSourceNode> = new Map()
  private defaultInputFingerprint: string | null = null
  private defaultOutputFingerprint: string | null = null
  private deviceChangePromise: Promise<AudioDeviceChangeResult> | null = null

  private readonly config: RTCConfiguration = {
    iceServers: [
      { urls: 'stun:stun.l.google.com:19302' },
      { urls: 'stun:stun1.l.google.com:19302' },
      { urls: 'stun:stun.cloudflare.com:3478' }
    ],
    bundlePolicy: 'max-bundle',
    iceCandidatePoolSize: 4
  }

  private turnServers: RTCIceServer[] = []
  private ownStunServers: RTCIceServer[] = []
  private relayOnlyIce = localStorage.getItem(RELAY_ONLY_STORAGE_KEY) === 'true'
  private relayWarningShownAt = 0
  private turnExpiresAt = 0
  private turnUserId: string | null = null
  private turnFetch: Promise<void> | null = null
  private turnRetryAfter = 0

  private static readonly TURN_REFRESH_MARGIN_MS = 60 * 60 * 1000
  private static readonly TURN_RETRY_COOLDOWN_MS = 30_000
  private static readonly FALLBACK_STUN_URL = 'stun:stun.cloudflare.com:3478'

  private deriveStunServers(servers: RTCIceServer[]): RTCIceServer[] {
    const derived = new Set<string>()
    for (const server of servers) {
      const urls = Array.isArray(server.urls) ? server.urls : [server.urls]
      for (const url of urls) {
        if (typeof url !== 'string') continue
        const match = url.match(/^turn:([^?]+?)(?:\?transport=(\w+))?$/i)
        if (!match) continue
        const transport = match[2]?.toLowerCase()
        if (transport && transport !== 'udp') continue
        derived.add(`stun:${match[1]}`)
      }
    }
    return [...derived].map(urls => ({ urls }))
  }

  private async ensureIceServers(): Promise<void> {
    const userId = useAppStore.getState().currentUser?.id ?? null
    const fresh = this.turnServers.length > 0
      && this.turnUserId === userId
      && Date.now() < this.turnExpiresAt - WebRTCManager.TURN_REFRESH_MARGIN_MS
    if (fresh) return
    if (this.turnFetch) return this.turnFetch
    if (Date.now() < this.turnRetryAfter) return

    this.turnFetch = (async () => {
      try {
        const config = await signalRService.fetchIceServers()
        const servers = config?.iceServers?.filter(server => server?.urls?.length) ?? []
        if (servers.length > 0) {
          this.turnServers = servers
          this.ownStunServers = this.deriveStunServers(servers)
          this.turnExpiresAt = config!.expiresAtUnixMs
          this.turnUserId = userId
          this.turnRetryAfter = 0
        } else {
          this.turnRetryAfter = Date.now() + WebRTCManager.TURN_RETRY_COOLDOWN_MS
        }
      } catch {
        this.turnRetryAfter = Date.now() + WebRTCManager.TURN_RETRY_COOLDOWN_MS
      } finally {
        this.turnFetch = null
      }
    })()
    return this.turnFetch
  }

  private rtcConfig(): RTCConfiguration {
    if (this.relayOnlyIce) {
      if (this.turnServers.length === 0) this.warnRelayUnavailable()
      return {
        ...this.config,
        iceServers: [...this.turnServers],
        iceTransportPolicy: 'relay',
        iceCandidatePoolSize: 0
      }
    }
    if (this.turnServers.length === 0) return this.config
    if (this.ownStunServers.length === 0) {
      return { ...this.config, iceServers: [...(this.config.iceServers ?? []), ...this.turnServers] }
    }
    return {
      ...this.config,
      iceServers: [
        ...this.ownStunServers,
        { urls: WebRTCManager.FALLBACK_STUN_URL },
        ...this.turnServers
      ]
    }
  }

  private warnRelayUnavailable() {
    console.error('[WebRTC] relay-only mode is enabled but no TURN server is available, direct candidates stay disabled')
    if (Date.now() - this.relayWarningShownAt < 30_000) return
    this.relayWarningShownAt = Date.now()
    const message = i18n.t(
      'toasts.relayUnavailable',
      'скрытие IP включено, но сервер ретрансляции недоступен — соединение не будет установлено.'
    )
    useAppStore.getState().setSystemToast(message)
    setTimeout(() => {
      const store = useAppStore.getState()
      if (store.systemToast === message) store.setSystemToast(null)
    }, 6000)
  }

  public isRelayOnlyIce(): boolean {
    return this.relayOnlyIce
  }

  public setRelayOnlyIce(enabled: boolean) {
    if (this.relayOnlyIce === enabled) return
    this.relayOnlyIce = enabled
    localStorage.setItem(RELAY_ONLY_STORAGE_KEY, enabled ? 'true' : 'false')
    if (enabled) void this.ensureIceServers()
  }

  private getSmartGateParams(gainFactor: number) {
    return {
      thresholdMode: 'auto',
      vadThreshold: this.calibratedVadThreshold,
      vadTrackerSeed: this.calibratedVadTrackerSeed,
      vadFixedThreshold: VAD_CALIBRATION_ENABLED ? 0 : FIXED_SILERO_THRESHOLD,
      attenuationLimit: this.suppressionStrengthDb,
      adaptiveAttenuation: false,
      postFilterBeta: this.postFilterBetaForStrength(this.suppressionStrengthDb),
      noiseFloor: this.calibratedNoiseFloor,
      gainFactor,
      downstreamGain: this.effectiveInputGain()
    }
  }

  private postFilterBetaForStrength(strengthDb: number): number {
    const span = MAX_SUPPRESSION_STRENGTH_DB - MIN_SUPPRESSION_STRENGTH_DB
    const normalized = span > 0
      ? Math.max(0, Math.min(1, (strengthDb - MIN_SUPPRESSION_STRENGTH_DB) / span))
      : 0
    const shaped = normalized * normalized
    return DEEPFILTER_POST_FILTER_BETA +
      (DEEPFILTER_MAX_POST_FILTER_BETA - DEEPFILTER_POST_FILTER_BETA) * shaped
  }

  private getManualGateParams() {
    return { manualThresholdValue: this.manualThresholdValue }
  }

  private inputVolumeToGain(volume: number): number {
    const normalized = Math.max(0, Math.min(200, Number.isFinite(volume) ? volume : 100))
    return normalized / 100
  }

  private effectiveInputGain(): number {
    const base = this.inputVolumeToGain(this.inputVolume)
    return this.noiseSuppression ? base : base * this.RAW_MIC_BOOST
  }

  private normalizeManualThreshold(value: number): number {
    if (value >= 0) return -42
    return Math.max(-60, Math.min(-12, Number.isFinite(value) ? value : -42))
  }

  private normalizeSuppressionStrength(value: number): number {
    if (!Number.isFinite(value)) return DEEPFILTER_SMART_DEFAULT_ATTEN
    return Math.round(Math.max(MIN_SUPPRESSION_STRENGTH_DB, Math.min(MAX_SUPPRESSION_STRENGTH_DB, value)))
  }

  private isSmartMode(): boolean {
    return this.noiseSuppression && this.thresholdMode === 'auto'
  }

  private usesSmartWorklet(): boolean {
    if (this.noiseSuppression) {
      return this.thresholdMode === 'auto'
    }
    return this.analyzerFeedsWorklet()
  }

  private micGraphSignature(): string {
    if (this.noiseSuppression) {
      return this.thresholdMode === 'auto' ? `smart-${this.smartModel}` : 'manual-gate'
    }
    return this.analyzerFeedsWorklet() ? 'analyzer-only' : 'raw'
  }

  private hasSpeakingWorklet(): boolean {
    return Boolean(this.micNode || this.manualGateNode)
  }

  private calibrationStorageKey(deviceId: string): string {
    return `zabor_mic_calibration_v${WebRTCManager.CALIBRATION_SCHEMA_VERSION}:${encodeURIComponent(deviceId || 'default')}`
  }

  private migrateCalibrationStorage(): void {
    try {
      const currentVersion = Number(localStorage.getItem(WebRTCManager.CALIBRATION_SCHEMA_KEY)) || 0
      if (currentVersion === WebRTCManager.CALIBRATION_SCHEMA_VERSION) return

      for (let i = localStorage.length - 1; i >= 0; i--) {
        const key = localStorage.key(i)
        if (key?.startsWith('zabor_mic_calibration_v') ||
          key === 'zabor_calibrated_noise_floor' || key === 'zabor_calibrated_attenuation') {
          localStorage.removeItem(key)
        }
      }
      localStorage.setItem(WebRTCManager.CALIBRATION_SCHEMA_KEY, String(WebRTCManager.CALIBRATION_SCHEMA_VERSION))
    } catch (error) {
      console.warn('[WebRTC] Failed to migrate microphone calibration profiles', error)
    }
  }

  private readEnvironmentProfiles(deviceId = this.calibrationDeviceId): StoredEnvironmentProfile[] {
    try {
      const raw = localStorage.getItem(this.calibrationStorageKey(deviceId))
      const parsed = raw ? JSON.parse(raw) : null
      if (!Array.isArray(parsed?.profiles)) return []
      const oldestAllowedTimestamp = Date.now() - WebRTCManager.CALIBRATION_PROFILE_MAX_AGE_MS
      return parsed.profiles.filter((profile: StoredEnvironmentProfile) =>
        profile?.version === WebRTCManager.CALIBRATION_SCHEMA_VERSION &&
        Number.isFinite(profile.timestamp) && profile.timestamp >= oldestAllowedTimestamp &&
        Number.isFinite(profile.noiseFloor) && Number.isFinite(profile.attenuationLimit) &&
        Number.isFinite(profile.zeroCrossingRate) && Number.isFinite(profile.spectralTilt)
      )
    } catch {
      return []
    }
  }

  private environmentDistance(
    profile: StoredEnvironmentProfile,
    noiseFloor: number,
    lowNoise: number,
    peakNoise: number,
    zeroCrossingRate: number,
    spectralTilt: number
  ): number {
    const db = (value: number) => 20 * Math.log10(Math.max(1e-5, value))
    const profileSpread = profile.peakNoise / Math.max(1e-5, profile.lowNoise)
    const measuredSpread = peakNoise / Math.max(1e-5, lowNoise)
    return Math.abs(db(profile.noiseFloor) - db(noiseFloor)) +
      Math.abs(Math.log2(Math.max(0.25, profileSpread) / Math.max(0.25, measuredSpread))) * 2 +
      Math.abs(profile.zeroCrossingRate - zeroCrossingRate) * 20 +
      Math.abs(Math.log2(Math.max(0.05, profile.spectralTilt) / Math.max(0.05, spectralTilt))) * 2
  }

  private calculateAttenuationFromRoom(
    noiseFloor: number,
    lowNoise: number,
    peakNoise: number,
    noiseVadHigh = 0
  ): {
    attenuationLimit: number, requiredDb: number, speechLikeBonusDb: number,
    transientReliefDb: number, demandDb: number, shapedDb: number, roomDbfs: number
  } {
    const clamp01 = (value: number) => Math.max(0, Math.min(1, value))
    const roomDbfs = Math.max(
      20 * Math.log10(Math.max(1e-5, noiseFloor)),
      20 * Math.log10(Math.max(1e-5, peakNoise)) - 6
    )
    const measuredSnrDb = ASSUMED_SPEECH_LEVEL_DBFS - roomDbfs
    const requiredDb = TARGET_SPEECH_TO_NOISE_DB - measuredSnrDb

    const speechLikeBonusDb = 5 * clamp01((noiseVadHigh - 0.25) / 0.45)
    const stationarity = peakNoise / Math.max(1e-5, lowNoise)
    const transientReliefDb = clamp01((stationarity - 6) / 6)

    const demandDb = requiredDb + speechLikeBonusDb
    const shapedDb = demandDb <= SUPPRESSION_SOFT_KNEE_DB
      ? demandDb
      : SUPPRESSION_SOFT_KNEE_DB + (demandDb - SUPPRESSION_SOFT_KNEE_DB) * SUPPRESSION_ABOVE_KNEE_SLOPE

    const attenuationLimit = Math.max(DEEPFILTER_MIN_ATTEN, Math.min(
      SUPPRESSION_SOFT_KNEE_DB,
      Math.round(shapedDb - transientReliefDb)
    ))
    return { attenuationLimit, requiredDb, speechLikeBonusDb, transientReliefDb, demandDb, shapedDb, roomDbfs }
  }

  private loadCalibration(deviceId: string, deviceLabel = '') {
    this.migrateCalibrationStorage()
    const normalizedDeviceId = deviceId && deviceId !== 'default'
      ? deviceId
      : `default:${deviceLabel.trim().toLowerCase() || 'unknown'}`
    this.calibrationDeviceId = normalizedDeviceId
    this.calibratedVadThreshold = VAD_CALIBRATION_ENABLED ? DEFAULT_SILERO_THRESHOLD : FIXED_SILERO_THRESHOLD
    this.hasVoiceCalibration = false

    if (!VAD_CALIBRATION_ENABLED) {
      this.updateThresholds()
      return
    }

    try {
      const raw = localStorage.getItem(this.calibrationStorageKey(normalizedDeviceId))
      const profile = raw ? JSON.parse(raw) as StoredVoiceProfile : null
      if (
        profile?.version === WebRTCManager.CALIBRATION_SCHEMA_VERSION &&
        Number.isFinite(profile.vadThreshold)
      ) {
        this.calibratedVadThreshold = Number.isFinite(profile.voiceLow) && profile.voiceLow > 0
          ? vadThresholdFromVoiceLow(profile.voiceLow)
          : Math.max(VAD_CALIBRATION_MIN_THRESHOLD, Math.min(VAD_CALIBRATION_MAX_THRESHOLD, profile.vadThreshold))
        this.hasVoiceCalibration = true
      }
    } catch { }
    this.updateThresholds()
  }

  public resetMicCalibration() {
    this.calibratedVadThreshold = VAD_CALIBRATION_ENABLED ? DEFAULT_SILERO_THRESHOLD : FIXED_SILERO_THRESHOLD
    this.hasVoiceCalibration = false
    try {
      for (let i = localStorage.length - 1; i >= 0; i--) {
        const key = localStorage.key(i)
        if (key?.startsWith('zabor_mic_calibration_v') || key === 'zabor_calibrated_noise_floor' || key === 'zabor_calibrated_attenuation') {
          localStorage.removeItem(key)
        }
      }
      localStorage.setItem(WebRTCManager.CALIBRATION_SCHEMA_KEY, String(WebRTCManager.CALIBRATION_SCHEMA_VERSION))
    } catch { }
    this.updateThresholds()
  }

  private async loadDeepFilterAsset(rel: string): Promise<ArrayBuffer | null> {
    return getDeepFilterAsset(rel)
  }

  private analyzerFeedsWorklet(): boolean {
    return this.speechAnalyzerEnabled || this.calibrationForcesAnalyzer
  }

  private levelSeedKeys(): { preEngine: string; alc: string } {
    const device = this.currentDeviceId || 'default'
    return {
      preEngine: `pre:${device}`,
      alc: `alc:${device}:${this.noiseSuppression ? this.smartModel : 'raw'}`
    }
  }

  private mergeLevelSeed(key: string, state: Record<string, unknown>, fields: readonly string[]): void {
    const seed = { ...(this.micLevelSeeds.get(key) ?? {}) }
    let changed = false
    for (const field of fields) {
      const value = Number(state[field])
      if (Number.isFinite(value) && value > 0) {
        seed[field] = value
        changed = true
      }
    }
    if (changed) this.micLevelSeeds.set(key, seed)
  }

  private storeLevelSeed(state: Record<string, unknown>, keys: { preEngine: string; alc: string }): void {
    this.mergeLevelSeed(keys.preEngine, state, PRE_ENGINE_SEED_FIELDS)
    this.mergeLevelSeed(keys.alc, state, ALC_SEED_FIELDS)
  }

  private buildLevelSeed(keys: { preEngine: string; alc: string }): Record<string, number> | null {
    const merged = {
      ...(this.micLevelSeeds.get(keys.preEngine) ?? {}),
      ...(this.micLevelSeeds.get(keys.alc) ?? {})
    }
    return Object.keys(merged).length > 0 ? merged : null
  }

  private bindVadProbabilityHandler(micNode: AudioWorkletNode): void {
    this.vadProbabilityHandler = (data) => {
      if (this.micNode !== micNode) return
      micNode.port.postMessage({
        type: 'setSileroVadProbability',
        probability: data.probability,
        sequence: data.sequence,
        endFrameId: data.endFrameId,
        windowRms: data.windowRms
      })
    }
  }

  private applyAnalyzerToWorklet(): void {
    const micNode = this.micNode
    if (!micNode) return
    if (!this.analyzerFeedsWorklet()) {
      micNode.port.postMessage({ type: 'setConfig', sileroVadEnabled: false })
      this.vadProbabilityHandler = null
      return
    }
    this.bindVadProbabilityHandler(micNode)
    if (this.vadWorkerReady) {
      this.vadWorker?.postMessage({ type: 'reset' })
      micNode.port.postMessage({ type: 'setConfig', sileroVadEnabled: true })
      return
    }
    void this.ensureVadWorker()
      .then(() => {
        if (this.micNode !== micNode || !this.analyzerFeedsWorklet()) return
        this.vadWorker?.postMessage({ type: 'reset' })
        micNode.port.postMessage({ type: 'setConfig', sileroVadEnabled: true })
      })
      .catch(error => {
        if (this.micNode !== micNode) return
        this.audioProcessorError = `Silero VAD failed: ${error instanceof Error ? error.message : String(error)}`
        micNode.port.postMessage({ type: 'setConfig', sileroVadEnabled: false })
        console.warn('[WebRTC] Silero VAD is unavailable; using voicing-based speech detection:', error)
      })
  }

  private ensureVadWorker(): Promise<void> {
    if (!this.analyzerFeedsWorklet()) return Promise.resolve()
    if (this.vadWorker && this.vadWorkerReady) return Promise.resolve()
    if (this.vadWorkerInitPromise) return this.vadWorkerInitPromise

    const startup = (async () => {
      const worker = new VadWorker()
      const wasmPath = new URL('./', window.location.href).href
      let resolveReady: (() => void) | null = null
      let rejectReady: ((error: Error) => void) | null = null
      const ready = new Promise<void>((resolve, reject) => {
        resolveReady = resolve
        rejectReady = reject
      })
      ready.catch(() => { })
      const readyTimeout = window.setTimeout(() => {
        rejectReady?.(new Error('Silero VAD initialization timed out'))
      }, VAD_WORKER_READY_TIMEOUT_MS)

      worker.onmessage = (event) => {
        const data = event.data
        if (data.type === 'probability') {
          this.vadWorkerReady = true
          const epoch = Number(data.epoch)
          if (Number.isFinite(epoch) && epoch !== this.vadEpoch) return
          this.voiceProbeCollector?.(Number(data.probability), Number(data.windowRms))
          this.vadProbabilityHandler?.(data)
        } else if (data.type === 'ready') {
          this.vadWorkerReady = true
          window.clearTimeout(readyTimeout)
          resolveReady?.()
          resolveReady = null
          rejectReady = null
          console.info('[WebRTC] Silero VAD Worker is ready', data.io ?? '')
        } else if (data.type === 'error') {
          console.error('[WebRTC] Silero VAD Worker error:', data.error)
          if (data.phase === 'initialization') {
            window.clearTimeout(readyTimeout)
            rejectReady?.(new Error(String(data.error)))
          }
        }
      }
      this.vadWorker = worker

      const model = await withTimeout(
        getSileroModel(),
        SILERO_MODEL_LOAD_TIMEOUT_MS,
        'Silero VAD model load timed out'
      )
      worker.postMessage({ type: 'init', model, wasmPath }, [model.buffer])
      await ready
    })()

    this.vadWorkerInitPromise = startup.then(() => {
      this.vadWorkerInitPromise = null
    }).catch(error => {
      this.terminateVadWorker()
      this.vadWorkerInitPromise = null
      throw error
    })
    return this.vadWorkerInitPromise
  }

  private terminateVadWorker() {
    this.vadProbabilityHandler = null
    this.vadWorkerReady = false
    if (this.vadWorker) {
      try { this.vadWorker.terminate() } catch { }
      this.vadWorker = null
    }
  }

  public warmUpSmartNoiseSuppression(enabled = this.noiseSuppression): void {
    if (this.noiseSuppression && this.thresholdMode !== 'auto') return
    const needsAnalyzer = this.analyzerFeedsWorklet()
    if (!enabled && !needsAnalyzer) return
    void preloadNoiseAssets(enabled ? this.smartModel : 'analyzer')
      .then(() => this.ensureVadWorker())
      .catch(error => console.warn('[WebRTC] Smart noise suppression warm-up failed:', error))
  }

  public warmUpConnectivity(): void {
    const startedAt = performance.now()
    void this.ensureIceServers()
      .then(() => {
        console.log(`[WebRTC] ICE servers warmed up in ${Math.round(performance.now() - startedAt)}ms ` +
          `(${this.turnServers.length} entries)`)
      })
      .catch(error => console.warn('[WebRTC] ICE server warm-up failed:', error))
  }

  private createPeakGuard(ctx: AudioContext): WaveShaperNode {
    const peakGuard = ctx.createWaveShaper()
    const peakCurve = new Float32Array(65_536)
    const linearLimit = 0.98
    const outputLimit = 0.995
    const softRange = 1 - linearLimit
    const normalization = 1 - Math.exp(-3)
    for (let i = 0; i < peakCurve.length; i++) {
      const sample = (i / (peakCurve.length - 1)) * 2 - 1
      const magnitude = Math.abs(sample)
      peakCurve[i] = magnitude <= linearLimit
        ? sample
        : Math.sign(sample) * (
          linearLimit +
          (outputLimit - linearLimit) *
          (1 - Math.exp(-3 * ((magnitude - linearLimit) / softRange))) /
          normalization
        )
    }
    peakGuard.curve = peakCurve
    peakGuard.oversample = '4x'
    return peakGuard
  }

  private createRumbleGuards(ctx: AudioContext): BiquadFilterNode[] {
    const guards = RUMBLE_GUARD_QS.map(quality => {
      const guard = ctx.createBiquadFilter()
      guard.type = 'highpass'
      guard.frequency.value = RUMBLE_GUARD_HZ
      guard.Q.value = quality
      return guard
    })
    for (let index = 1; index < guards.length; index++) guards[index - 1].connect(guards[index])
    return guards
  }

  private applyLocalSpeaking(isSpeaking: boolean) {
    if (isSpeaking && this.calibrationSuppressesSpeaking) return
    const store = useAppStore.getState()
    const inActiveSession = Boolean(store.currentChannelId || store.currentCallUser)
    if (!inActiveSession && isSpeaking) return
    if (isSpeaking === this.localSpeakingState) return
    this.localSpeakingState = isSpeaking
    const me = store.currentUser
    if (!me) return
    store.setSpeakingStatus(me.id, isSpeaking)
    signalRService.setSpeakingState(isSpeaking)
  }

  private publishMicLevel(db: number) {
    if (!Number.isFinite(db)) return
    this.micLevelDb = db
    this.micLevelListeners.forEach(listener => listener(db))
  }

  private async createProcessedStream(rawStream: MediaStream): Promise<MediaStream> {
    if (!this.usesSmartWorklet()) return this.createManualProcessedStream(rawStream)
    return this.createSmartProcessedStream(rawStream, this.smartModel)
  }

  private async createManualProcessedStream(rawStream: MediaStream): Promise<MediaStream> {
    this.cleanupProcessedStream()
    this.audioProcessorError = null

    const ctx = new AudioContext({ sampleRate: 48000, latencyHint: 'interactive' })
    this.processedContext = ctx
    if (ctx.sampleRate !== 48000) {
      const detail = `AudioContext runs at ${ctx.sampleRate}Hz, 48000Hz required`
      console.error(`[WebRTC] Audio processing requires 48000Hz, got ${ctx.sampleRate}Hz`)
      this.audioProcessorError = detail
      await ctx.close().catch(() => { })
      this.processedContext = null
      return createSilentAudioStream()
    }
    if (ctx.state === 'suspended') await ctx.resume().catch(() => { })

    const destination = ctx.createMediaStreamDestination()
    const track = destination.stream.getAudioTracks()[0]
    if (track) track.contentHint = 'speech'
    const source = ctx.createMediaStreamSource(rawStream)
    this.processedSource = source
    const inputGain = ctx.createGain()
    inputGain.gain.value = this.effectiveInputGain()
    this.inputGainNode = inputGain
    const peakGuard = this.createPeakGuard(ctx)
    this.micOutputTap = peakGuard

    try {
      const analyser = ctx.createAnalyser()
      analyser.fftSize = 256
      source.connect(analyser)
      this.rawAnalyserNode = analyser
    } catch (error) {
      console.warn('[WebRTC] Failed to create raw analyser node:', error)
    }

    const passthrough = () => {
      source.connect(inputGain)
      inputGain.connect(peakGuard)
      peakGuard.connect(destination)
      return destination.stream
    }

    if (!this.noiseSuppression) return passthrough()

    try {
      await ctx.audioWorklet.addModule(manualGateProcessorUrl)
      this.manualGateNode = new AudioWorkletNode(ctx, 'manual-gate-processor', {
        numberOfInputs: 1,
        numberOfOutputs: 1,
        outputChannelCount: [1],
        channelCount: 1,
        channelCountMode: 'explicit'
      })
    } catch (error) {
      this.audioProcessorError = `Manual gate worklet failed: ${error instanceof Error ? error.message : String(error)}`
      console.error('[WebRTC] Failed to create the manual gate:', error)
      return passthrough()
    }

    const gateNode = this.manualGateNode
    gateNode.port.onmessage = (event) => {
      if (event.data.type === 'vad') {
        this.applyLocalSpeaking(Boolean(event.data.isSpeaking))
      } else if (event.data.type === 'micLevelDb') {
        this.publishMicLevel(Number(event.data.db))
      }
    }

    const rumbleGuards = this.createRumbleGuards(ctx)
    this.micRumbleFilters = rumbleGuards

    const store = useAppStore.getState()
    const isMuted = Boolean(store.currentUser?.isMuted || store.currentUser?.isServerMuted)
    gateNode.port.postMessage({ type: 'setConfig', isMuted, ...this.getManualGateParams() })

    source.connect(rumbleGuards[0])
    rumbleGuards[rumbleGuards.length - 1].connect(inputGain)
    inputGain.connect(gateNode)
    gateNode.connect(peakGuard)
    peakGuard.connect(destination)
    this.localSpeakingState = false
    return destination.stream
  }

  private async createSmartProcessedStream(
    rawStream: MediaStream,
    model: SmartNoiseModel
  ): Promise<MediaStream> {
    this.cleanupProcessedStream()
    this.micEngineError = null
    this.audioProcessorError = null

    const levelSeedKeys = this.levelSeedKeys()
    const ctx = new AudioContext({ sampleRate: 48000, latencyHint: 'interactive' })
    this.processedContext = ctx

    if (ctx.sampleRate !== 48000) {
      const detail = `AudioContext runs at ${ctx.sampleRate}Hz, 48000Hz required`
      console.error(`[WebRTC] Audio processing requires 48000Hz, got ${ctx.sampleRate}Hz`)
      this.micEngineError = detail
      this.audioProcessorError = detail
      await ctx.close().catch(() => { })
      this.processedContext = null
      return createSilentAudioStream()
    }

    if (ctx.state === 'suspended') {
      await ctx.resume().catch(() => { })
    }

    const destination = ctx.createMediaStreamDestination()
    const localTrack = destination.stream.getAudioTracks()[0]
    if (localTrack) localTrack.contentHint = 'speech'
    const moduleUrl = model === 'deepfilter' ? micPipelineDeepFilterUrl : micPipelineRnnoiseUrl
    const processorName = model === 'deepfilter' ? 'mic-pipeline-deepfilter' : 'mic-pipeline-rnnoise'

    try {
      await ctx.audioWorklet.addModule(moduleUrl)
      this.micNode = new AudioWorkletNode(ctx, processorName, {
        numberOfInputs: 1,
        numberOfOutputs: 1,
        outputChannelCount: [1],
        channelCount: 1,
        channelCountMode: 'explicit',
        channelInterpretation: 'speakers'
      })
      this.micNodeReady = false
      const me = useAppStore.getState().currentUser
      if (me) {
        this.clearVAD(me.id)
      }
    } catch (e) {
      this.micEngineError = `AudioWorklet module failed: ${e instanceof Error ? e.message : String(e)}`
      console.error(`[WebRTC] Failed to load ${processorName}`, e)
    }

    if (this.micNode) {
      const graphEpoch = this.vadEpoch
      this.micNode.port.onmessage = (event) => {
        if (event.data.type === 'vad') {
          this.applyLocalSpeaking(Boolean(event.data.isSpeaking))
        } else if (event.data.type === 'audio16k') {
          if (this.vadWorker) {
            const audioFrame = event.data.audio as Float32Array
            this.vadWorker.postMessage({
              type: 'process',
              audioFrame,
              epoch: graphEpoch,
              sequence: event.data.sequence,
              endFrameId: event.data.endFrameId,
              windowRms: event.data.windowRms
            }, [audioFrame.buffer])
          }
        } else if (event.data.type === 'micLevelDb') {
          this.publishMicLevel(Number(event.data.db))
        } else if (event.data.type === 'levelState') {
          this.storeLevelSeed(event.data, levelSeedKeys)
        } else if (event.data.type === 'resetVad') {
          this.vadWorker?.postMessage({ type: 'reset' })
        } else if (event.data.type === 'fetchRequest') {
          const url = event.data.url as string
          const deliver = (buffer: ArrayBuffer | null) => {
            if (buffer) {
              this.micNode?.port.postMessage({ type: 'fetchResponse', url, buffer }, [buffer])
            } else {
              this.micNode?.port.postMessage({ type: 'fetchResponse', url, buffer: null })
            }
          }
          const localRel = url.startsWith(`${DEEPFILTER_LOCAL_BASE}/`)
            ? url.slice(DEEPFILTER_LOCAL_BASE.length + 1)
            : null
          if (localRel && DEEPFILTER_ASSETS.has(localRel)) {
            this.loadDeepFilterAsset(localRel)
              .then(deliver)
              .catch(err => {
                console.error('[WebRTC] DeepFilter asset load failed:', err)
                deliver(null)
              })
          } else {
            console.error('[WebRTC] Worklet asked for a non-bundled asset, refused:', url)
            deliver(null)
          }
        } else if (event.data.type === 'log') {
          console.log('[WebRTC Worklet Log]:', event.data.message)
        } else if (event.data.type === 'ready') {
          this.micNodeReady = true
          this.micEngineError = null
          console.log(`[WebRTC Worklet Log]: ${processorName} is ready.`)
        }
      }

      this.micNode.port.onmessageerror = (event) => {
        console.error('[WebRTC] Mic pipeline worklet could not deserialize a message:', event)
      }

      const initialStore = useAppStore.getState()
      this.micNode.port.postMessage({
        type: 'setConfig',
        noiseSuppression: this.noiseSuppression,
        sileroVadEnabled: false,
        isMuted: Boolean(initialStore.currentUser?.isMuted || initialStore.currentUser?.isServerMuted)
      })
      const levelSeed = this.buildLevelSeed(levelSeedKeys)
      if (levelSeed) {
        this.micNode.port.postMessage({ type: 'setLevelSeed', ...levelSeed })
      }

      const micNodeForAssets = this.micNode
      const sendPayload = (payload: DeepFilterPayload) => {
        micNodeForAssets.port.postMessage({
          type: 'loadWasm',
          assetBase: DEEPFILTER_LOCAL_BASE,
          wasmBytes: payload.wasmBytes,
          modelBytes: payload.modelBytes
        }, [payload.wasmBytes, payload.modelBytes])
      }
      if (this.noiseSuppression) {
        const payload = model === 'deepfilter' ? getDeepFilterPayload() : null
        if (payload) {
          sendPayload(payload)
        } else {
          micNodeForAssets.port.postMessage({ type: 'loadWasm', assetBase: DEEPFILTER_LOCAL_BASE })
          if (model === 'deepfilter') {
            void preloadNoiseAssets('deepfilter')
              .then(() => {
                if (this.micNode !== micNodeForAssets || this.micNodeReady) return
                const late = getDeepFilterPayload()
                if (late) sendPayload(late)
              })
              .catch(error => console.warn('[WebRTC] Late DeepFilter payload delivery failed:', error))
          }
        }
      }

      const micNode = this.micNode
      this.bindVadProbabilityHandler(micNode)

      const attachSileroVad = () => {
        if (this.micNode !== micNode) return
        if (!this.analyzerFeedsWorklet() || !this.vadWorkerReady) {
          micNode.port.postMessage({ type: 'setConfig', sileroVadEnabled: false })
          return
        }
        this.vadWorker?.postMessage({ type: 'reset' })
        micNode.port.postMessage({ type: 'setConfig', sileroVadEnabled: true })
      }

      if (!this.analyzerFeedsWorklet()) {
        micNode.port.postMessage({ type: 'setConfig', sileroVadEnabled: false })
      } else if (this.vadWorkerReady) {
        attachSileroVad()
      } else {
        micNode.port.postMessage({ type: 'setConfig', sileroVadEnabled: false })
        void this.ensureVadWorker()
          .then(attachSileroVad)
          .catch(e => {
            if (this.micNode !== micNode) return
            this.vadProbabilityHandler = null
            this.audioProcessorError = `Silero VAD failed: ${e instanceof Error ? e.message : String(e)}`
            micNode.port.postMessage({ type: 'setConfig', sileroVadEnabled: false })
            console.warn(`[WebRTC] Silero VAD is unavailable; using voicing-based speech detection: ${e instanceof Error ? e.message : String(e)}`)
          })
      }

      this.localSpeakingState = false
    }

    const source = ctx.createMediaStreamSource(rawStream)
    this.processedSource = source

    const highpass1 = ctx.createBiquadFilter()
    highpass1.type = 'highpass'
    highpass1.frequency.value = 60
    highpass1.Q.value = 0.707

    const rumbleGuards = this.createRumbleGuards(ctx)
    this.micRumbleFilters = rumbleGuards

    const peakGuard = this.createPeakGuard(ctx)
    this.micOutputTap = peakGuard

    const inputGain = ctx.createGain()
    const calibratedPreGain = ctx.createGain()
    calibratedPreGain.gain.value = Math.pow(10, this.calibratedPreGainDb / 20)
    this.calibratedPreGainNode = calibratedPreGain
    const gainFactor = this.inputVolumeToGain(this.inputVolume)

    inputGain.gain.value = this.effectiveInputGain()
    this.inputGainNode = inputGain

    try {
      const rawAnalyser = ctx.createAnalyser()
      rawAnalyser.fftSize = 256
      source.connect(rawAnalyser)
      this.rawAnalyserNode = rawAnalyser
    } catch (e) {
      console.warn('[WebRTC] Failed to create raw analyser node for silence monitoring:', e)
    }

    if (this.micNode) {
      this.micNode.port.postMessage({
        type: 'setCalibratedParams',
        ...this.getSmartGateParams(gainFactor),
      })
      if (this.noiseSuppression) {
        source.connect(rumbleGuards[0])
        rumbleGuards[rumbleGuards.length - 1].connect(this.micNode)
      } else {
        source.connect(this.micNode)
      }
      this.micNode.connect(calibratedPreGain)
      calibratedPreGain.connect(inputGain)
    } else {
      source.connect(inputGain)
      inputGain.gain.value = this.noiseSuppression ? 0 : gainFactor
      inputGain.connect(highpass1)
    }

    if (this.micNode) inputGain.connect(peakGuard)
    else highpass1.connect(peakGuard)
    peakGuard.connect(destination)

    return destination.stream
  }

  private cleanupProcessedStreamSourceOnly() {
    this.stopSilenceMonitor()
    if (this.inputGainNode) {
      try { this.inputGainNode.disconnect() } catch { }
      this.inputGainNode = null
    }
    if (this.micNode) {
      try { this.micNode.disconnect() } catch { }
    }
    if (this.manualGateNode) {
      try { this.manualGateNode.disconnect() } catch { }
    }
    if (this.processedSource) {
      try { this.processedSource.disconnect() } catch { }
      this.processedSource = null
    }
    for (const guard of this.micRumbleFilters) {
      try { guard.disconnect() } catch { }
    }
    this.micRumbleFilters = []
    if (this.micOutputTap) {
      try { this.micOutputTap.disconnect() } catch { }
      this.micOutputTap = null
    }
    if (this.rawAnalyserNode) {
      try { this.rawAnalyserNode.disconnect() } catch { }
      this.rawAnalyserNode = null
    }
  }

  private cleanupProcessedStream() {
    this.cleanupProcessedStreamSourceOnly()
    if (this.micNode) {
      try { this.micNode.port.close() } catch { }
      this.micNode = null
    }
    if (this.manualGateNode) {
      try { this.manualGateNode.port.close() } catch { }
      this.manualGateNode = null
    }
    this.voiceProbeCollector = null
    this.micNodeReady = false
    if (this.processedContext && this.processedContext.state !== 'closed') {
      this.processedContext.close().catch(() => { })
    }
    this.processedContext = null
    this.vadEpoch++
    this.vadProbabilityHandler = null
    this.vadWorker?.postMessage({ type: 'reset' })
    const me = useAppStore.getState().currentUser
    if (me) {
      useAppStore.getState().setSpeakingStatus(me.id, false)
      signalRService.setSpeakingState(false)
    }
    this.localSpeakingState = false
  }

  public setInputVolume(volume: number) {
    this.inputVolume = Math.max(0, Math.min(200, Number.isFinite(volume) ? volume : 100))
    const gainFactor = this.inputVolumeToGain(this.inputVolume)
    const effectiveGain = this.effectiveInputGain()

    if (this.inputGainNode) {
      this.inputGainNode.gain.value = effectiveGain
    }
    if (this.backgroundMeterGain) {
      this.backgroundMeterGain.gain.value = effectiveGain
    }
    if (this.micNode) {
      this.micNode.port.postMessage({
        type: 'setCalibratedParams',
        ...this.getSmartGateParams(gainFactor)
      })
    }
  }

  public setMicThresholdParams(mode: 'auto' | 'manual', manualValue: number) {
    const normalizedThreshold = this.normalizeManualThreshold(manualValue)
    const modeChanged = this.thresholdMode !== mode
    localStorage.setItem('zabor_threshold_mode', mode)
    localStorage.setItem('zabor_manual_threshold_value', normalizedThreshold.toString())
    this.thresholdMode = mode
    this.manualThresholdValue = normalizedThreshold
    this.updateThresholds()
    if (modeChanged) this.warmUpSmartNoiseSuppression()
    if (modeChanged && this.localStream) {
      void this.updateSettings(this.currentDeviceId, this.noiseSuppression)
    }
  }

  private updateThresholds() {
    const gainFactor = this.inputVolumeToGain(this.inputVolume)

    if (this.micNode) {
      this.micNode.port.postMessage({
        type: 'setCalibratedParams',
        ...this.getSmartGateParams(gainFactor)
      })
    }
    if (this.manualGateNode) {
      this.manualGateNode.port.postMessage({
        type: 'setCalibratedParams',
        ...this.getManualGateParams()
      })
    }
  }

  public setOutputVolume(volume: number) {
    this.outputVolume = volume
    this.userGainNodes.forEach((_, userId) => this.updateRemoteVolume(userId))
    if (this.micTestGain) this.micTestGain.gain.value = this.micTestVolume()
  }

  public setDeafened(deafened: boolean) {
    this.isDeafened = deafened
    if (this.outputBusGain) {
      this.outputBusGain.gain.value = deafened ? 0 : Math.pow(10, PLAYBACK_MAKEUP_GAIN_DB / 20)
    }
    if (this.mixAudioElement) {
      this.mixAudioElement.muted = deafened
    }
  }

  private updateRemoteVolume(userId: string) {
    const store = useAppStore.getState()
    const isActive = store.activeStreamId === userId
    const streamGainNode = this.streamGainNodes.get(userId)
    if (streamGainNode) {
      const streamVol = isActive ? (store.streamVolumes[userId] ?? 100) : 0
      streamGainNode.gain.value = Math.max(0, Math.min(4.0, (this.outputVolume / 100) * (streamVol / 100)))
    }

    const gainNode = this.userGainNodes.get(userId)
    if (gainNode) {
      const vol = store.userVolumes[userId] ?? 100
      gainNode.gain.value = Math.max(0, Math.min(4.0, (this.outputVolume / 100) * (vol / 100)))
    }
  }

  public setNoiseSuppression(enabled: boolean) {
    if (this.noiseSuppression === enabled) return
    this.noiseSuppression = enabled
    if (this.inputGainNode) {
      this.inputGainNode.gain.value = this.effectiveInputGain()
    }
    if (this.backgroundMeterGain) {
      this.backgroundMeterGain.gain.value = this.effectiveInputGain()
    }
    this.warmUpSmartNoiseSuppression(enabled)
    if (this.localStream) void this.updateSettings(this.currentDeviceId, enabled)
  }

  public getSmartNoiseModel(): SmartNoiseModel {
    return this.smartModel
  }

  public setSmartNoiseModel(model: SmartNoiseModel) {
    if (model !== 'deepfilter' && model !== 'rnnoise') return
    if (this.smartModel === model) return
    this.smartModel = model
    localStorage.setItem(SMART_MODEL_STORAGE_KEY, model)
    this.warmUpSmartNoiseSuppression()
    if (this.localStream) void this.updateSettings(this.currentDeviceId, this.noiseSuppression)
  }

  public getSuppressionStrength(): number {
    return this.suppressionStrengthDb
  }

  public isSpeechAnalyzerEnabled(): boolean {
    return this.speechAnalyzerEnabled
  }

  public setSpeechAnalyzerEnabled(enabled: boolean) {
    if (this.speechAnalyzerEnabled === enabled) return
    const previousSignature = this.micGraphSignature()
    this.speechAnalyzerEnabled = enabled
    localStorage.setItem(SPEECH_ANALYZER_STORAGE_KEY, enabled ? 'true' : 'false')
    if (enabled) this.warmUpSmartNoiseSuppression()
    if (this.micGraphSignature() === previousSignature) {
      if (this.micNode) this.applyAnalyzerToWorklet()
      else if (!enabled && !this.calibrationForcesAnalyzer) this.terminateVadWorker()
      return
    }
    if (!enabled && !this.calibrationForcesAnalyzer) {
      this.micNode?.port.postMessage({ type: 'setConfig', sileroVadEnabled: false })
      this.terminateVadWorker()
    }
    if (this.localStream) void this.updateSettings(this.currentDeviceId, this.noiseSuppression)
  }

  public isEchoCancellationEnabled(): boolean {
    return this.echoCancellationEnabled
  }

  public setEchoCancellationEnabled(enabled: boolean) {
    if (this.echoCancellationEnabled === enabled) return
    this.echoCancellationEnabled = enabled
    localStorage.setItem(ECHO_CANCELLATION_STORAGE_KEY, enabled ? 'true' : 'false')
    if (this.localStream || this.rawStream) {
      void this.updateSettings(this.currentDeviceId, this.noiseSuppression)
    }
  }

  public setSuppressionStrength(db: number) {
    const normalized = this.normalizeSuppressionStrength(db)
    if (normalized === this.suppressionStrengthDb) return
    this.suppressionStrengthDb = normalized
    localStorage.setItem(SUPPRESSION_STRENGTH_STORAGE_KEY, normalized.toString())
    this.updateThresholds()
  }

  private setupVAD(stream: MediaStream, userId: string, isLocal: boolean) {
    this.clearVAD(userId)

    try {
      if (isLocal) {
        if (this.hasSpeakingWorklet()) {
          return
        }
        if (!this.processedContext || this.processedContext.state === 'closed') {
          this.processedContext = new AudioContext({ sampleRate: 48000, latencyHint: 'interactive' })
        }
        const contextToUse = this.processedContext
        if (contextToUse.state === 'suspended') contextToUse.resume().catch(() => { })

        const source = contextToUse.createMediaStreamSource(stream)
        const bp1 = contextToUse.createBiquadFilter()
        bp1.type = 'highpass'
        bp1.frequency.value = 85
        bp1.Q.value = 0.5

        const bp2 = contextToUse.createBiquadFilter()
        bp2.type = 'lowpass'
        bp2.frequency.value = 8000
        bp2.Q.value = 0.5

        const analyser = contextToUse.createAnalyser()
        analyser.fftSize = 512
        analyser.smoothingTimeConstant = 0.6

        source.connect(bp1)
        bp1.connect(bp2)
        bp2.connect(analyser)

        const vadNodes: AudioNode[] = [source, bp1, bp2, analyser]
        const buf = new Uint8Array(analyser.fftSize)
        let lastVoice = 0
        let wasSpeaking = false
        let voiceFrames = 0
        let silenceFrames = 0
        let vadSilenceFrames = 0
        let hasWarnedSilence = false

        const check = () => {
          const store = useAppStore.getState()
          if (store.currentUser?.isMuted || store.currentUser?.isServerMuted) {
            if (wasSpeaking) {
              wasSpeaking = false
              voiceFrames = 0
              store.setSpeakingStatus(userId, false)
              signalRService.setSpeakingState(false)
            }
            silenceFrames = 0
            return
          }

          analyser.getByteTimeDomainData(buf)
          let peak = 0, sum = 0
          for (let i = 0; i < buf.length; i++) {
            const s = Math.abs(buf[i] - 128)
            if (s > peak) peak = s
            sum += s
          }
          const avg = sum / buf.length

          if (peak === 0) {
            silenceFrames++
          } else {
            silenceFrames = 0
            hasWarnedSilence = false
          }

          if (silenceFrames > 150 && !hasWarnedSilence) {
            const toastMsg = i18n.t('toasts.micNotHearing', 'вас не слышно, проверьте микрофон')
            store.setSystemToast(toastMsg)
            setTimeout(() => {
              const currentStore = useAppStore.getState()
              if (currentStore.systemToast === toastMsg) {
                currentStore.setSystemToast(null)
              }
            }, 4000)
            hasWarnedSilence = true
          }

          const isVoice = avg >= 2.5 || peak >= 7
          if (isVoice) {
            voiceFrames++
            vadSilenceFrames = 0
          } else {
            vadSilenceFrames++
            if (vadSilenceFrames >= 6) voiceFrames = 0
          }
          if (voiceFrames >= 2) lastVoice = Date.now()

          const speaking = (Date.now() - lastVoice) < 400
          if (speaking !== wasSpeaking) {
            wasSpeaking = speaking
            store.setSpeakingStatus(userId, speaking)
            signalRService.setSpeakingState(speaking)
          }
        }

        const timer = setInterval(check, 30)
        this.speakingIntervals.set(userId, { timer, stream, nodes: vadNodes })
      } else {
        let wasSpeaking = false
        let lastSoundAt = 0
        const releaseHoldMs = 200
        const check = () => {
          const pc = this.peerConnections.get(userId)
          if (!pc) return
          const receiver = pc.getReceivers().find(r => r.track && r.track.kind === 'audio')
          if (!receiver) return
          try {
            const syncSources = receiver.getSynchronizationSources()
            if (syncSources && syncSources.length > 0) {
              const latestSource = syncSources[0]
              const currentPlayoutLevel = latestSource.audioLevel
              if (currentPlayoutLevel !== undefined) {
                const now = Date.now()
                if (currentPlayoutLevel > 0.001) lastSoundAt = now
                const isRemoteSpeaking = now - lastSoundAt <= releaseHoldMs
                if (isRemoteSpeaking !== wasSpeaking) {
                  wasSpeaking = isRemoteSpeaking
                  useAppStore.getState().setSpeakingStatus(userId, isRemoteSpeaking)
                }
              }
            }
          } catch { }
        }
        const timer = setInterval(check, 20)
        this.speakingIntervals.set(userId, { timer, stream, nodes: [] })
      }
    } catch (e) {
      console.error('[VAD] setup failed', e)
    }
  }

  private clearVAD(userId: string) {
    const entry = this.speakingIntervals.get(userId)
    if (entry) {
      clearInterval(entry.timer)
      entry.nodes.forEach(n => { try { n.disconnect() } catch { } })
      this.speakingIntervals.delete(userId)
    }
    useAppStore.getState().setSpeakingStatus(userId, false)
  }

  private toCleanAudioDevice(device: MediaDeviceInfo): MediaDeviceInfo {
    const cleanLabel = (device.label || '').replace(/^(Default|Communications|По умолчанию|Связь)[\s:\-]+/i, '').trim()
    return {
      deviceId: device.deviceId,
      kind: device.kind,
      label: cleanLabel,
      groupId: device.groupId,
      toJSON: () => ({ deviceId: device.deviceId, kind: device.kind, label: cleanLabel, groupId: device.groupId })
    } as MediaDeviceInfo
  }

  private filterAndDeduplicateDevices(devices: MediaDeviceInfo[]): MediaDeviceInfo[] {
    const result: MediaDeviceInfo[] = []
    const seenDeviceIds = new Set<string>()

    const validDevices = devices.filter(d =>
      d.deviceId &&
      d.deviceId !== 'default' &&
      d.deviceId !== 'communications'
    )

    const sortedDevices = [...validDevices].sort((a, b) => {
      const aHasPrefix = /^(Default|Communications|По умолчанию|Связь)[\s:\-]+/i.test(a.label || '')
      const bHasPrefix = /^(Default|Communications|По умолчанию|Связь)[\s:\-]+/i.test(b.label || '')
      if (aHasPrefix && !bHasPrefix) return 1
      if (!aHasPrefix && bHasPrefix) return -1
      return 0
    })

    for (const dev of sortedDevices) {
      if (!seenDeviceIds.has(dev.deviceId)) {
        seenDeviceIds.add(dev.deviceId)
        result.push(this.toCleanAudioDevice(dev))
      }
    }

    if (result.length === 0) {
      return devices.filter(device => device.deviceId).map(device => this.toCleanAudioDevice(device))
    }

    return result
  }

  private getDefaultDeviceFingerprint(devices: MediaDeviceInfo[], kind: MediaDeviceKind): string | null {
    const devicesOfKind = devices.filter(device => device.kind === kind)
    const device = devicesOfKind.find(item => item.deviceId === 'default')
      ?? devicesOfKind.find(item => item.deviceId === 'communications')
      ?? devicesOfKind[0]
    if (!device) return null
    return `${device.groupId}|${device.label}`
  }

  private toAudioDevices(devices: MediaDeviceInfo[]): AudioDevices {
    return {
      inputs: this.filterAndDeduplicateDevices(devices.filter(device => device.kind === 'audioinput')),
      outputs: this.filterAndDeduplicateDevices(devices.filter(device => device.kind === 'audiooutput'))
    }
  }

  public async getAudioDevices(): Promise<AudioDevices> {
    try {
      if (!this.rawStream?.getAudioTracks().some(track => track.readyState === 'live')) {
        await this.startBackgroundMic()
      }
      const devices = await navigator.mediaDevices.enumerateDevices()
      this.defaultInputFingerprint ??= this.getDefaultDeviceFingerprint(devices, 'audioinput')
      this.defaultOutputFingerprint ??= this.getDefaultDeviceFingerprint(devices, 'audiooutput')
      return this.toAudioDevices(devices)
    } catch (error) {
      console.error('[WebRTC] Failed to enumerate audio devices:', error)
      return { inputs: [], outputs: [] }
    }
  }

  public setInputDevice(deviceId: string) { this.currentDeviceId = deviceId }

  public subscribeMicLevel(listener: (db: number) => void): () => void {
    this.micLevelListeners.add(listener)
    listener(this.micLevelDb)
    return () => this.micLevelListeners.delete(listener)
  }

  private stopBackgroundMeter() {
    if (this.micMeterInterval) {
      clearInterval(this.micMeterInterval)
      this.micMeterInterval = null
    }
    if (this.backgroundSource) {
      try { this.backgroundSource.disconnect() } catch { }
      this.backgroundSource = null
    }
    if (this.backgroundMeterGain) {
      try { this.backgroundMeterGain.disconnect() } catch { }
      this.backgroundMeterGain = null
    }
    this.backgroundAnalyser = null
    if (this.backgroundContext && this.backgroundContext.state !== 'closed') {
      this.backgroundContext.close().catch(() => { })
    }
    this.backgroundContext = null
  }

  private startBackgroundMeter() {
    this.stopBackgroundMeter()
    if (!this.rawStream?.getAudioTracks().some(track => track.readyState === 'live')) return

    const context = new AudioContext({ sampleRate: 48000, latencyHint: 'interactive' })
    const source = context.createMediaStreamSource(this.rawStream)
    const analyser = context.createAnalyser()
    analyser.fftSize = 512
    analyser.smoothingTimeConstant = 0
    const meterGain = context.createGain()
    meterGain.gain.value = this.inputVolumeToGain(this.inputVolume)
    source.connect(meterGain)
    meterGain.connect(analyser)
    const silentOutput = context.createGain()
    silentOutput.gain.value = 0
    analyser.connect(silentOutput)
    silentOutput.connect(context.destination)
    this.backgroundContext = context
    this.backgroundSource = source
    this.backgroundAnalyser = analyser
    this.backgroundMeterGain = meterGain
    if (context.state === 'suspended') context.resume().catch(() => { })

    const samples = new Float32Array(analyser.fftSize)
    let smoothedDb = -100
    this.micMeterInterval = setInterval(() => {
      if (!this.backgroundAnalyser) return
      this.backgroundAnalyser.getFloatTimeDomainData(samples)
      let sumSquares = 0
      for (let i = 0; i < samples.length; i++) sumSquares += samples[i] * samples[i]
      const rms = Math.sqrt(sumSquares / samples.length)
      const measuredDb = Math.max(-100, Math.min(0, 20 * Math.log10(Math.max(rms, 0.000001))))
      const smoothing = measuredDb > smoothedDb ? 0.55 : 0.18
      smoothedDb += (measuredDb - smoothedDb) * smoothing
      this.micLevelDb = smoothedDb
      this.micLevelListeners.forEach(listener => listener(smoothedDb))
    }, 50)
  }

  private buildMicConstraints(deviceId?: string, echoCancellation = true): MediaTrackConstraints {
    const useWebRtcNoiseSuppression = this.noiseSuppression && this.thresholdMode === 'manual'
    const constraints: MediaTrackConstraints = {
      channelCount: 1,
      echoCancellation,
      noiseSuppression: useWebRtcNoiseSuppression,
      autoGainControl: false,
      sampleRate: { ideal: 48000 },
      // @ts-ignore
      googAutoGainControl: false,
      googAutoGainControl2: false,
      googNoiseSuppression: useWebRtcNoiseSuppression,
      googNoiseSuppression2: useWebRtcNoiseSuppression,
      googTypingNoiseDetection: false,
      googHighpassFilter: false,
      googEchoCancellation: echoCancellation,
      googEchoCancellation2: echoCancellation,
      googAudioMirroring: false
    }
    if (deviceId && deviceId !== 'default') constraints.deviceId = { exact: deviceId }
    return constraints
  }

  private async captureMicAttempt(
    label: string,
    run: (echoCancellation: boolean) => Promise<MediaStream>
  ): Promise<MediaStream> {
    const capture = (echoCancellation: boolean) => withTimeout(
      run(echoCancellation),
      MIC_CAPTURE_TIMEOUT_MS,
      `MIC_CAPTURE_TIMEOUT: ${label}`,
      late => late.getTracks().forEach(track => track.stop())
    )

    const wantsEchoCancellation = this.echoCancellationEnabled
    const stream = await capture(wantsEchoCancellation)
    if (!wantsEchoCancellation) {
      return stream
    }

    const settings = stream.getAudioTracks()[0]?.getSettings() ?? {}
    const hitchhikers: string[] = []
    if (settings.autoGainControl === true) hitchhikers.push('AGC')
    const expectsNoiseSuppression = this.noiseSuppression && this.thresholdMode === 'manual'
    if (settings.noiseSuppression === true && !expectsNoiseSuppression) hitchhikers.push('noise suppression')

    if (hitchhikers.length === 0) {
      if (settings.echoCancellation !== true) {
        console.log(`[WebRTC] Echo cancellation unavailable on ${label}; capturing without it`)
      }
      return stream
    }

    console.warn(
      `[WebRTC] ${label} switched on ${hitchhikers.join(' + ')} together with echo ` +
      `cancellation; recapturing with AEC off so the voice stays unprocessed`
    )
    stream.getTracks().forEach(track => track.stop())
    return capture(false)
  }

  private async captureRawMicStream(): Promise<MediaStream> {
    const requestedDeviceId = this.currentDeviceId
    const attempts: Array<{
      label: string,
      keepsSelection: boolean,
      run: (echoCancellation: boolean) => Promise<MediaStream>
    }> = []

    if (requestedDeviceId && requestedDeviceId !== 'default') {
      attempts.push({
        label: `selected device ${requestedDeviceId}`,
        keepsSelection: true,
        run: echoCancellation => navigator.mediaDevices.getUserMedia({
          audio: this.buildMicConstraints(requestedDeviceId, echoCancellation),
          video: false
        })
      })
    }
    attempts.push({
      label: 'system default',
      keepsSelection: requestedDeviceId === 'default',
      run: echoCancellation => navigator.mediaDevices.getUserMedia({
        audio: this.buildMicConstraints(undefined, echoCancellation),
        video: false
      })
    })
    attempts.push({
      label: 'first available input',
      keepsSelection: false,
      run: async echoCancellation => {
        const devices = await navigator.mediaDevices.enumerateDevices()
        const inputs = devices.filter(device => device.kind === 'audioinput' && device.deviceId)
        const input = inputs.find(device => device.deviceId !== 'default' && device.deviceId !== 'communications')
          ?? inputs[0]
        if (!input) throw new DOMException('No audio input devices found', 'NotFoundError')
        return navigator.mediaDevices.getUserMedia({
          audio: this.buildMicConstraints(input.deviceId, echoCancellation),
          video: false
        })
      }
    })

    let firstError: unknown = null
    for (const attempt of attempts) {
      try {
        const stream = await this.captureMicAttempt(attempt.label, attempt.run)
        if (!attempt.keepsSelection) {
          this.currentDeviceId = 'default'
          console.warn(`[WebRTC] Microphone "${requestedDeviceId}" unavailable, captured via ${attempt.label}`)
        }
        this.lastMicCaptureError = null
        this.lastReportedMicError = null
        return stream
      } catch (error) {
        firstError ??= error
        console.warn(`[WebRTC] Microphone capture failed (${attempt.label}):`, error)
      }
    }

    this.lastMicCaptureError = describeMediaError(firstError)
    throw firstError instanceof Error
      ? firstError
      : new Error(`MIC_ACCESS_FAILED: ${this.lastMicCaptureError}`)
  }

  private reportMicCaptureError(error: unknown): void {
    const detail = describeMediaError(error)
    this.lastMicCaptureError = detail
    if (this.lastReportedMicError === detail) return
    this.lastReportedMicError = detail

    const store = useAppStore.getState()
    const kind = classifyMicrophoneError(detail)
    const toastMsg = kind === 'micBusy'
      ? i18n.t('toasts.micBusy', 'микрофон занят другим приложением.')
      : kind === 'micNotFound'
        ? i18n.t('toasts.micNotFound', 'микрофон не найден. подключите устройство и попробуйте снова.')
        : kind === 'micNoAccess'
          ? i18n.t('toasts.micNoAccess', 'нет доступа к микрофону. проверьте разрешения в ОС.')
          : i18n.t('toasts.audioError', { message: detail, defaultValue: `ошибка аудио: ${detail}` })
    store.setSystemToast(toastMsg)
    setTimeout(() => {
      const currentStore = useAppStore.getState()
      if (currentStore.systemToast === toastMsg) currentStore.setSystemToast(null)
    }, 4000)
  }

  public async startBackgroundMic(deviceId?: string): Promise<boolean> {
    if (deviceId !== undefined) this.currentDeviceId = deviceId
    if (this.rawStream?.getAudioTracks().some(track => track.readyState === 'live')) {
      if (!this.localStream) this.startBackgroundMeter()
      return true
    }

    try {
      this.rawStream = await this.captureRawMicStream()
      const rawTrack = this.rawStream.getAudioTracks()[0]
      const settings = rawTrack?.getSettings()
      this.loadCalibration(settings?.deviceId || this.currentDeviceId, rawTrack?.label || '')
      this.startBackgroundMeter()
      return true
    } catch (error) {
      console.error('[WebRTC] Failed to initialize background microphone:', error)
      this.reportMicCaptureError(error)
      return false
    }
  }

  public setOutputDevice(deviceId: string) {
    this.currentOutputDeviceId = deviceId || 'default'
    void this.applyOutputDevice()
  }

  public async handleAudioDeviceChange(): Promise<AudioDeviceChangeResult> {
    if (this.deviceChangePromise) return this.deviceChangePromise

    const reconcile = async (): Promise<AudioDeviceChangeResult> => {
      const devices = await navigator.mediaDevices.enumerateDevices()
      const availableInputs = new Set(
        devices.filter(device => device.kind === 'audioinput').map(device => device.deviceId)
      )
      const availableOutputs = new Set(
        devices.filter(device => device.kind === 'audiooutput').map(device => device.deviceId)
      )

      const previousInputFingerprint = this.defaultInputFingerprint
      const previousOutputFingerprint = this.defaultOutputFingerprint
      const nextInputFingerprint = this.getDefaultDeviceFingerprint(devices, 'audioinput')
      const nextOutputFingerprint = this.getDefaultDeviceFingerprint(devices, 'audiooutput')
      this.defaultInputFingerprint = nextInputFingerprint
      this.defaultOutputFingerprint = nextOutputFingerprint

      const selectedInputMissing = this.currentDeviceId !== 'default' && !availableInputs.has(this.currentDeviceId)
      const selectedOutputMissing = this.currentOutputDeviceId !== 'default' && !availableOutputs.has(this.currentOutputDeviceId)
      if (selectedInputMissing) this.currentDeviceId = 'default'
      if (selectedOutputMissing) this.currentOutputDeviceId = 'default'

      const rawTrack = this.rawStream?.getAudioTracks()[0]
      const defaultInput = devices.find(device => device.kind === 'audioinput' && device.deviceId === 'default')
      const capturedGroupId = rawTrack?.getSettings().groupId
      const capturedWrongDefault = this.currentDeviceId === 'default' && Boolean(
        rawTrack && defaultInput?.groupId && capturedGroupId && defaultInput.groupId !== capturedGroupId
      )
      const defaultInputChanged = this.currentDeviceId === 'default' && previousInputFingerprint !== null &&
        nextInputFingerprint !== previousInputFingerprint
      const captureEnded = Boolean(this.rawStream && (!rawTrack || rawTrack.readyState !== 'live'))

      if (this.rawStream && (selectedInputMissing || defaultInputChanged || capturedWrongDefault || captureEnded)) {
        await this.updateSettings(this.currentDeviceId, this.noiseSuppression)
      }

      const defaultOutputChanged = this.currentOutputDeviceId === 'default' &&
        previousOutputFingerprint !== null && nextOutputFingerprint !== previousOutputFingerprint
      if (selectedOutputMissing || defaultOutputChanged) await this.applyOutputDevice()

      return {
        ...this.toAudioDevices(devices),
        inputDeviceId: this.currentDeviceId,
        outputDeviceId: this.currentOutputDeviceId
      }
    }

    this.deviceChangePromise = reconcile()
    try {
      return await this.deviceChangePromise
    } finally {
      this.deviceChangePromise = null
    }
  }

  private async applyOutputDevice() {
    const sinkId = this.currentOutputDeviceId === 'default' ? '' : this.currentOutputDeviceId

    if (this.outputMixContext && typeof (this.outputMixContext as any).setSinkId === 'function') {
      try {
        await (this.outputMixContext as any).setSinkId(sinkId)
      } catch (error) {
        console.warn(`[WebRTC] Failed to select output device "${sinkId}" on outputMixContext`, error)
      }
    }

    if (this.micTestContext) await this.applyMicTestSink(this.micTestContext)

    const audioElement = this.mixAudioElement
    if (!audioElement) return
    const setSinkId = (audioElement as HTMLAudioElement & {
      setSinkId?: (id: string) => Promise<void>
    }).setSinkId

    if (typeof setSinkId === 'function') {
      try {
        await setSinkId.call(audioElement, sinkId)
      } catch (error) {
        console.warn(`[WebRTC] Failed to select output device "${sinkId}"`, error)
        if (sinkId !== '') {
          this.currentOutputDeviceId = 'default'
          try {
            await setSinkId.call(audioElement, '')
          } catch (fallbackError) {
            console.warn('[WebRTC] Failed to select the default output device', fallbackError)
          }
        }
      }
    }

    if (this.outputMixContext?.state === 'suspended') {
      await this.outputMixContext.resume().catch(() => { })
    }
    await audioElement.play().catch(error => {
      console.warn('[WebRTC] mixAudioElement play failed:', error)
    })
  }

  public getCalibrationDiagnostics() {
    const rawTrack = this.rawStream?.getAudioTracks()[0]
    const rawSettings = rawTrack?.getSettings()
    return {
      inputDeviceId: this.currentDeviceId,
      outputDeviceId: this.currentOutputDeviceId,
      calibrationProfileKey: this.calibrationDeviceId,
      noiseSuppression: this.noiseSuppression,
      thresholdMode: this.thresholdMode,
      smartModel: this.smartModel,
      suppressionStrengthDb: this.suppressionStrengthDb,
      speechAnalyzerEnabled: this.speechAnalyzerEnabled,
      hasMicNode: Boolean(this.micNode),
      micNodeReady: this.micNodeReady,
      micEngineError: this.micEngineError,
      hasManualGateNode: Boolean(this.manualGateNode),
      audioProcessorError: this.audioProcessorError,
      sileroVadThreshold: this.calibratedVadThreshold,
      vadWorkerReady: this.vadWorkerReady,
      micCaptureError: this.lastMicCaptureError,
      processedContextState: this.processedContext?.state ?? null,
      processedContextSampleRate: this.processedContext?.sampleRate ?? null,
      capturedDeviceId: rawSettings?.deviceId ?? null,
      capturedSampleRate: rawSettings?.sampleRate ?? null,
      capturedChannelCount: rawSettings?.channelCount ?? null,
      capturedEchoCancellation: rawSettings?.echoCancellation ?? null,
      capturedAutoGainControl: rawSettings?.autoGainControl ?? null,
      capturedNoiseSuppression: rawSettings?.noiseSuppression ?? null,
      rawTrackLabel: rawTrack?.label ?? null,
      rawTrackState: rawTrack?.readyState ?? null,
      rawTrackEnabled: rawTrack?.enabled ?? null,
      rawTrackMuted: rawTrack?.muted ?? null,
      localTrackEnabled: this.localStream?.getAudioTracks()[0]?.enabled ?? null
    }
  }

  private activeCalibrationEngine(): SmartNoiseModel | 'manual' {
    if (!this.isSmartMode()) return 'manual'
    return this.smartModel
  }

  private postEngineConfig(config: Record<string, unknown>) {
    this.micNode?.port.postMessage({ type: 'setConfig', ...config })
    this.manualGateNode?.port.postMessage({ type: 'setConfig', ...config })
  }

  public isCalibrationAvailable(): boolean {
    return VAD_CALIBRATION_ENABLED
  }

  public async calibrateMic(durationMs = 4500, onStarted?: () => void): Promise<CalibrationResult> {
    if (!VAD_CALIBRATION_ENABLED) {
      throw new CalibrationError(
        'CALIBRATION_ENGINE_UNAVAILABLE',
        'Voice calibration is disabled, the VAD threshold is fixed'
      )
    }
    const wasInActiveSession = Boolean(useAppStore.getState().currentChannelId || useAppStore.getState().currentCallUser)
    const engine = this.activeCalibrationEngine()

    if (engine === 'manual') {
      throw new CalibrationError(
        'CALIBRATION_ENGINE_UNAVAILABLE',
        'Calibration requires smart noise suppression'
      )
    }

    const me = useAppStore.getState().currentUser
    const wasMuted = Boolean(me?.isMuted || me?.isServerMuted)
    let muteLifted = false
    const analyzerWasOff = !this.speechAnalyzerEnabled
    if (analyzerWasOff) this.calibrationForcesAnalyzer = true

    try {
      await withTimeout(
        this.prepareCalibrationEngine(),
        CALIBRATION_PREPARE_TIMEOUT_MS,
        `Calibration preparation timed out (${engine})`
      )

      if (wasMuted) {
        this.calibrationSuppressesSpeaking = true
        muteLifted = true
        this.postEngineConfig({ isMuted: false })
      }

      onStarted?.()
      return await this.calibrateViaVadWorker(durationMs)
    } catch (error) {
      if (error instanceof CalibrationError) throw error
      throw new CalibrationError(
        'CALIBRATION_TIMEOUT',
        error instanceof Error ? error.message : String(error)
      )
    } finally {
      if (analyzerWasOff) {
        this.calibrationForcesAnalyzer = false
        this.micNode?.port.postMessage({ type: 'setConfig', sileroVadEnabled: false })
        this.terminateVadWorker()
      }
      if (muteLifted) {
        const currentUser = useAppStore.getState().currentUser
        const stillMuted = Boolean(currentUser?.isMuted || currentUser?.isServerMuted)
        this.postEngineConfig({ isMuted: stillMuted })
        this.calibrationSuppressesSpeaking = false
        if (stillMuted && this.localSpeakingState) {
          this.localSpeakingState = false
          if (currentUser) {
            useAppStore.getState().setSpeakingStatus(currentUser.id, false)
            signalRService.setSpeakingState(false)
          }
        }
      }
      if (!wasInActiveSession) {
        await withTimeout(
          this.enterBackgroundMode(),
          CALIBRATION_CLEANUP_TIMEOUT_MS,
          'Background microphone restore timed out'
        ).catch(error => console.warn('[WebRTC] Calibration cleanup:', error))
      }
    }
  }

  private async prepareCalibrationEngine(): Promise<void> {
    if (!this.micNode) {
      try {
        if (this.localStream) {
          await this.updateSettings(this.currentDeviceId, this.noiseSuppression)
        } else {
          await this.startLocalStream(this.currentDeviceId, this.noiseSuppression, true)
        }
      } catch (error) {
        const detail = describeMediaError(error)
        console.error('[WebRTC] Calibration could not start the microphone:', detail)
        throw new CalibrationError('CALIBRATION_NO_MIC', detail)
      }
    }

    if (this.lastMicCaptureError) {
      throw new CalibrationError('CALIBRATION_NO_MIC', this.lastMicCaptureError)
    }

    await this.ensureVadWorker().catch(error => {
      console.warn('[WebRTC] Calibration could not start Silero VAD:', error)
    })
    if (this.vadWorkerReady) {
      this.micNode?.port.postMessage({ type: 'setConfig', sileroVadEnabled: true })
    }

    await this.waitForCalibrationReady()
  }

  private async waitForCalibrationReady(timeoutMs = 8000): Promise<void> {
    const startedAt = Date.now()
    const hasNode = () => Boolean(this.micNode)
    const isReady = () => hasNode() && this.vadWorkerReady

    while (!isReady() && Date.now() - startedAt < timeoutMs) {
      if (!hasNode() || this.audioProcessorError) break
      await new Promise(resolve => setTimeout(resolve, 100))
    }
    if (!isReady()) {
      throw new CalibrationError('CALIBRATION_ENGINE_UNAVAILABLE', this.audioProcessorError || (hasNode()
        ? 'Silero voice detection did not become ready'
        : 'Audio worklet node is unavailable'))
    }
  }

  private calibrateViaVadWorker(durationMs = 4500): Promise<CalibrationResult> {
    return new Promise((resolve, reject) => {
      if (!this.micNode || !this.vadWorkerReady) {
        reject(new CalibrationError(
          'CALIBRATION_ENGINE_UNAVAILABLE',
          this.audioProcessorError || 'Silero voice detection is not ready'
        ))
        return
      }
      if (this.calibrationInProgress) {
        reject(new CalibrationError('CALIBRATION_BUSY', 'Microphone calibration is already running'))
        return
      }
      if (!this.rawStream?.getAudioTracks().some(track => track.readyState === 'live' && track.enabled)) {
        reject(new CalibrationError('CALIBRATION_NO_MIC', this.lastMicCaptureError || 'The microphone track is not live'))
        return
      }

      const previousThreshold = this.calibratedVadThreshold
      const previousCalibrationState = this.hasVoiceCalibration
      const restorePreviousProfile = () => {
        this.calibratedVadThreshold = previousThreshold
        this.hasVoiceCalibration = previousCalibrationState
        this.updateThresholds()
      }

      this.calibrationInProgress = true
      const probabilities: number[] = []
      let peakProbability = 0
      const settleUntil = Date.now() + 150
      const collector = (probability: number, windowRms: number) => {
        if (Date.now() < settleUntil) return
        if (!Number.isFinite(probability) || !Number.isFinite(windowRms)) return
        if (windowRms < 0.0006 || probability < 0.4) return
        if (probabilities.length >= 512) return
        probabilities.push(probability)
        peakProbability = Math.max(peakProbability, probability)
      }
      this.voiceProbeCollector = collector

      window.setTimeout(() => {
        this.calibrationInProgress = false
        if (this.voiceProbeCollector !== collector) {
          reject(new CalibrationError(
            'CALIBRATION_ENGINE_UNAVAILABLE',
            this.micEngineError || 'The audio graph was rebuilt during calibration'
          ))
          return
        }
        this.voiceProbeCollector = null

        const values = probabilities.slice().sort((a, b) => a - b)
        const percentile = (p: number) => values.length
          ? values[Math.min(values.length - 1, Math.floor((values.length - 1) * p))]
          : 0
        const voiceLow = percentile(0.1)
        const voiceMedian = percentile(0.5)
        const voiceHigh = percentile(0.9)

        if (values.length < MIN_VOICE_CALIBRATION_WINDOWS || peakProbability < 0.4) {
          restorePreviousProfile()
          reject(new CalibrationError(
            'CALIBRATION_NEEDS_VOICE',
            `accepted ${values.length} voice windows, peak ${peakProbability.toFixed(3)}`
          ))
          return
        }

        this.calibratedVadThreshold = vadThresholdFromVoiceLow(voiceLow)
        this.hasVoiceCalibration = true
        this.updateThresholds()
        console.info('[WebRTC] DeepFilter voice calibration', {
          vadThreshold: Number(this.calibratedVadThreshold.toFixed(4)),
          voiceLow: Number(voiceLow.toFixed(4)),
          voiceMedian: Number(voiceMedian.toFixed(4)),
          voiceHigh: Number(voiceHigh.toFixed(4)),
          peakProbability: Number(peakProbability.toFixed(4)),
          acceptedVoiceWindows: values.length
        })

        try {
          const profile: StoredVoiceProfile = {
            version: WebRTCManager.CALIBRATION_SCHEMA_VERSION,
            timestamp: Date.now(),
            vadThreshold: this.calibratedVadThreshold,
            voiceLow,
            voiceMedian,
            voiceHigh
          }
          localStorage.setItem(this.calibrationStorageKey(this.calibrationDeviceId), JSON.stringify(profile))
        } catch (error) {
          console.error('Failed to save Silero calibration data', error)
        }

        resolve({
          vadThreshold: this.calibratedVadThreshold,
          voiceLow,
          voiceMedian,
          voiceHigh,
          peakProbability,
          acceptedVoiceWindows: values.length
        })
      }, durationMs)
    })
  }

  public async prewarmLocalStream(): Promise<boolean> {
    if (this.localStream && this.localStream.getAudioTracks().length > 0 && this.localStream.getAudioTracks().every(t => t.readyState === 'live')) {
      return true
    }
    return this.startLocalStream(undefined, undefined, false)
  }

  private setMonitorWhileMuted(enabled: boolean) {
    this.micNode?.port.postMessage({ type: 'setConfig', monitorWhileMuted: enabled })
    this.manualGateNode?.port.postMessage({ type: 'setConfig', monitorWhileMuted: enabled })
  }

  private async ensureMicTestCaptureModule(ctx: AudioContext): Promise<void> {
    if (this.micTestCaptureModules.has(ctx)) return
    await ctx.audioWorklet.addModule(micTestCaptureProcessorUrl)
    this.micTestCaptureModules.add(ctx)
  }

  private ensureMicTestPlayback(sampleRate: number): { ctx: AudioContext; gain: GainNode } {
    let ctx = this.micTestContext
    if (ctx && ctx.state !== 'closed' && this.micTestGain) {
      if (ctx.state === 'suspended') void ctx.resume().catch(() => { })
      return { ctx, gain: this.micTestGain }
    }

    this.releaseMicTestPlayback()
    try {
      ctx = new AudioContext({ sampleRate, latencyHint: 'interactive' })
    } catch (error) {
      console.warn(`[WebRTC] Mic test playback could not use ${sampleRate} Hz, falling back to default:`, error)
      ctx = new AudioContext({ latencyHint: 'interactive' })
    }
    this.micTestContext = ctx
    const gain = ctx.createGain()
    gain.gain.value = this.micTestVolume()
    gain.connect(ctx.destination)
    this.micTestGain = gain
    void this.applyMicTestSink(ctx)
    if (ctx.state === 'suspended') void ctx.resume().catch(() => { })

    return { ctx, gain }
  }

  private micTestVolume(): number {
    return Math.max(0, Math.min(2, this.outputVolume / 100))
  }

  private releaseMicTestPlayback() {
    this.detachMicTestSource()
    if (this.micTestGain) {
      try { this.micTestGain.disconnect() } catch { }
      this.micTestGain = null
    }
    if (this.micTestContext) {
      const ctx = this.micTestContext
      this.micTestContext = null
      void ctx.close().catch(() => { })
    }
  }

  private detachMicTestSource() {
    const source = this.micTestSource
    this.micTestSource = null
    if (!source) return
    source.onended = null
    try { source.stop() } catch { }
    try { source.disconnect() } catch { }
  }

  private async applyMicTestSink(ctx: AudioContext) {
    const target = ctx as AudioContext & { setSinkId?: (id: string) => Promise<void> }
    if (typeof target.setSinkId !== 'function') return
    const sinkId = this.currentOutputDeviceId === 'default' ? '' : this.currentOutputDeviceId
    try {
      await target.setSinkId(sinkId)
    } catch (error) {
      console.warn('[WebRTC] Mic test could not select the output device:', error)
    }
  }

  public isMicTestRecording(): boolean {
    return this.micTestRun !== null
  }

  public async prepareMicTest(): Promise<void> {
    try {
      if (!await this.prewarmLocalStream()) return
      const ctx = this.processedContext
      if (!ctx || ctx.state === 'closed') return
      await this.ensureMicTestCaptureModule(ctx)
    } catch (error) {
      console.warn('[WebRTC] Mic test could not warm up:', error)
    }
  }

  public async recordMicTest(onArmed?: () => void, durationMs = MIC_TEST_DURATION_MS): Promise<MicTestClip> {
    if (this.micTestRun) return this.micTestRun
    this.micTestRun = this.runMicTest(durationMs, onArmed)
    try {
      return await this.micTestRun
    } finally {
      this.micTestRun = null
    }
  }

  private async runMicTest(durationMs: number, onArmed?: () => void): Promise<MicTestClip> {
    this.pauseMicTest()
    this.micTestBuffer = null
    const generation = this.micTestGeneration

    const started = await this.prewarmLocalStream()
    const ctx = this.processedContext
    const tap = this.micOutputTap
    if (!started || !ctx || !tap || ctx.state === 'closed') throw new Error('MIC_TEST_NO_MIC')
    if (ctx.state === 'suspended') await ctx.resume().catch(() => { })

    await this.ensureMicTestCaptureModule(ctx)
    const capture = new AudioWorkletNode(ctx, 'mic-test-capture-processor', {
      numberOfInputs: 1,
      numberOfOutputs: 1,
      outputChannelCount: [1],
      channelCount: 1,
      channelCountMode: 'explicit'
    })
    const sink = ctx.createMediaStreamDestination()
    capture.connect(sink)

    let timeout: ReturnType<typeof setTimeout> | null = null
    const captured = new Promise<Float32Array<ArrayBuffer>>((resolve, reject) => {
      capture.port.onmessage = event => {
        if (event.data?.type === 'captured') resolve(event.data.pcm as Float32Array<ArrayBuffer>)
      }
      capture.onprocessorerror = () => reject(new Error('MIC_TEST_CAPTURE_FAILED'))
      timeout = setTimeout(() => reject(new Error('MIC_TEST_TIMEOUT')), durationMs + MIC_TEST_CAPTURE_GRACE_MS)
    })

    try {
      tap.connect(capture)
      this.setMonitorWhileMuted(true)
      await new Promise(resolve => setTimeout(resolve, MIC_TEST_MONITOR_SETTLE_MS))
      if (generation !== this.micTestGeneration) throw new Error('MIC_TEST_CANCELLED')
      onArmed?.()
      await new Promise(resolve => setTimeout(resolve, MIC_TEST_PIPELINE_FLUSH_MS))
      if (generation !== this.micTestGeneration) throw new Error('MIC_TEST_CANCELLED')
      capture.port.postMessage({
        type: 'start',
        samples: Math.round((durationMs / 1000) * ctx.sampleRate)
      })
      const pcm = await captured
      if (generation !== this.micTestGeneration) throw new Error('MIC_TEST_CANCELLED')
      if (pcm.length === 0) throw new Error('MIC_TEST_EMPTY')

      let peak = 0
      for (let i = 0; i < pcm.length; i++) {
        const magnitude = pcm[i] < 0 ? -pcm[i] : pcm[i]
        if (magnitude > peak) peak = magnitude
      }

      const playback = this.ensureMicTestPlayback(ctx.sampleRate)
      const buffer = playback.ctx.createBuffer(1, pcm.length, ctx.sampleRate)
      buffer.copyToChannel(pcm, 0)
      this.micTestBuffer = buffer
      this.micTestOffset = 0

      return {
        durationSeconds: buffer.duration,
        peakDb: Math.round(20 * Math.log10(Math.max(peak, 1e-6)))
      }
    } catch (error) {
      if (generation !== this.micTestGeneration) throw new Error('MIC_TEST_CANCELLED')
      throw error
    } finally {
      if (timeout) clearTimeout(timeout)
      this.setMonitorWhileMuted(false)
      capture.port.onmessage = null
      capture.onprocessorerror = null
      try { capture.port.postMessage({ type: 'stop' }) } catch { }
      try { tap.disconnect(capture) } catch { }
      try { capture.disconnect() } catch { }
      for (const track of sink.stream.getTracks()) {
        try { track.stop() } catch { }
      }
    }
  }

  public onMicTestEnded(listener: (() => void) | null) {
    this.micTestEndedListener = listener
  }

  public getMicTestDuration(): number {
    return this.micTestBuffer?.duration ?? 0
  }

  public isMicTestPlaying(): boolean {
    return this.micTestPlaying
  }

  public getMicTestPosition(): number {
    const buffer = this.micTestBuffer
    if (!buffer) return 0
    if (!this.micTestPlaying || !this.micTestContext) return Math.min(this.micTestOffset, buffer.duration)
    const elapsed = this.micTestContext.currentTime - this.micTestStartedAt
    return Math.max(0, Math.min(buffer.duration, elapsed))
  }

  public playMicTest(offsetSeconds?: number): boolean {
    const buffer = this.micTestBuffer
    if (!buffer) return false

    const playback = this.ensureMicTestPlayback(buffer.sampleRate)
    this.detachMicTestSource()

    const requested = offsetSeconds === undefined ? this.micTestOffset : offsetSeconds
    const offset = requested >= buffer.duration - 0.02 ? 0 : Math.max(0, requested)

    const source = playback.ctx.createBufferSource()
    source.buffer = buffer
    source.connect(playback.gain)
    playback.gain.gain.value = this.micTestVolume()
    source.onended = () => {
      if (this.micTestSource !== source) return
      this.micTestSource = null
      this.micTestPlaying = false
      this.micTestOffset = buffer.duration
      this.micTestEndedListener?.()
    }

    this.micTestSource = source
    this.micTestOffset = offset
    this.micTestStartedAt = playback.ctx.currentTime - offset
    this.micTestPlaying = true
    source.start(0, offset)
    return true
  }

  public pauseMicTest() {
    if (!this.micTestPlaying) return
    this.micTestOffset = this.getMicTestPosition()
    this.micTestPlaying = false
    this.detachMicTestSource()
  }

  public seekMicTest(seconds: number) {
    const buffer = this.micTestBuffer
    if (!buffer) return
    const target = Math.max(0, Math.min(buffer.duration - MIC_TEST_SEEK_TAIL_S, seconds))
    if (this.micTestPlaying) {
      this.playMicTest(target)
      return
    }
    this.micTestOffset = target
  }

  public disposeMicTest() {
    this.micTestGeneration++
    this.micTestPlaying = false
    this.micTestOffset = 0
    this.micTestBuffer = null
    this.releaseMicTestPlayback()
    this.setMonitorWhileMuted(false)
  }

  public async startLocalStream(deviceId?: string, useNS?: boolean, forceRestart = false): Promise<boolean> {
    if (deviceId !== undefined) this.currentDeviceId = deviceId
    if (useNS !== undefined) this.noiseSuppression = useNS

    if (this.activeStartPromise) {
      return this.activeStartPromise
    }

    const run = async () => {
      const startedAt = performance.now()
      const laps: string[] = []
      let lapAt = startedAt
      const lap = (label: string) => {
        const now = performance.now()
        laps.push(`${label} ${Math.round(now - lapAt)}ms`)
        lapAt = now
      }
      if (!forceRestart && this.localStream && this.localStream.getAudioTracks().length > 0 && this.localStream.getAudioTracks().every(t => t.readyState === 'live')) {
        this.initOutputMixer()
        this.startSilenceMonitor()
        const me = useAppStore.getState().currentUser
        if (me && this.rawStream && !this.hasSpeakingWorklet() && !this.speakingIntervals.has(me.id)) {
          this.setupVAD(this.rawStream, me.id, true)
        }
        const isMuted = me?.isMuted || me?.isServerMuted || false
        this.toggleMute(isMuted)
        console.log(`[WebRTC] startLocalStream reused existing stream in ${Math.round(performance.now() - startedAt)}ms`)
        return true
      }

      try {
        this.initOutputMixer()
        this.stopBackgroundMeter()
        if (forceRestart && this.rawStream) { this.rawStream.getTracks().forEach(t => t.stop()); this.rawStream = null }
        if (this.localStream) { this.localStream.getTracks().forEach(t => t.stop()); this.localStream = null }
        this.cleanupProcessedStream()
        lap('cleanup')

        let raw = this.rawStream
        if (!raw?.getAudioTracks().some(track => track.readyState === 'live')) {
          try {
            raw = await this.captureRawMicStream()
          } catch (error) {
            this.reportMicCaptureError(error)
            raw = createSilentAudioStream()
          }
        }
        lap('getUserMedia')

        this.rawStream = raw
        const rawTrack = raw.getAudioTracks()[0]
        if (rawTrack) {
          rawTrack.contentHint = 'speech'
          const settings = rawTrack.getSettings()
          this.loadCalibration(settings.deviceId || this.currentDeviceId, rawTrack.label)
          console.info('[WebRTC] Microphone settings', {
            deviceId: settings.deviceId,
            sampleRate: settings.sampleRate,
            channelCount: settings.channelCount,
            echoCancellation: settings.echoCancellation,
            noiseSuppression: settings.noiseSuppression,
            autoGainControl: settings.autoGainControl
          })
        }

        this.localStream = await this.createProcessedStream(raw)
        lap('graph')

        const localTrack = this.localStream.getAudioTracks()[0]
        if (localTrack) {
          localTrack.contentHint = 'speech'
        }

        if (this.processedContext && this.processedContext.state === 'suspended') {
          await this.processedContext.resume().catch(() => { })
        }

        this.startSilenceMonitor()

        const me = useAppStore.getState().currentUser
        if (me && this.rawStream && !this.hasSpeakingWorklet()) this.setupVAD(this.rawStream, me.id, true)

        const isMuted = me?.isMuted || me?.isServerMuted || false
        this.toggleMute(isMuted)
        lap('meters')

        console.log(`[WebRTC] startLocalStream ${Math.round(performance.now() - startedAt)}ms: ${laps.join(', ')}`)
        return true
      } catch (e) {
        throw new Error(`MIC_ACCESS_FAILED: ${describeMediaError(e)}`)
      }
    }

    this.activeStartPromise = run()
    try {
      return await this.activeStartPromise
    } finally {
      this.activeStartPromise = null
    }
  }

  public async updateSettings(deviceId: string, useNS: boolean) {
    this.currentDeviceId = deviceId
    this.noiseSuppression = useNS

    if (this.localStream) {
      try {
        await this.startLocalStream(deviceId, useNS, true)
        for (const pc of this.peerConnections.values()) {
          const sender = pc.getSenders().find(s => s.track?.kind === 'audio')
          const newTrack = this.localStream?.getAudioTracks()[0]
          if (sender && newTrack) {
            await sender.replaceTrack(newTrack).catch(() => { })
            this.configureAudioSender(sender)
          }
        }
      } catch (e) {
        throw e
      }
    } else if (this.rawStream) {
      this.rawStream.getTracks().forEach(track => track.stop())
      this.rawStream = null
      this.stopBackgroundMeter()
      await this.startBackgroundMic(deviceId)
    }
  }

  public async setNoiseSuppressionMode(mode: 'auto' | 'manual') {
    this.thresholdMode = mode
    localStorage.setItem('zabor_threshold_mode', mode)
    if (this.localStream) await this.updateSettings(this.currentDeviceId, this.noiseSuppression)
  }

  public stopLocalStream() {
    const me = useAppStore.getState().currentUser
    if (me) this.clearVAD(me.id)
    this.stopSilenceMonitor()
    if (this.localStream) { this.localStream.getTracks().forEach(t => t.stop()); this.localStream = null }
    if (this.rawStream) { this.rawStream.getTracks().forEach(t => t.stop()); this.rawStream = null }
    this.stopBackgroundMeter()
    this.cleanupProcessedStream()
    this.leaveAll()
  }

  public async enterBackgroundMode(): Promise<void> {
    const me = useAppStore.getState().currentUser
    if (me) this.clearVAD(me.id)
    this.stopSilenceMonitor()
    this.applyLocalSpeaking(false)
    this.leaveAll()

    const hasLiveLocalStream = Boolean(
      this.localStream &&
      this.localStream.getAudioTracks().length > 0 &&
      this.localStream.getAudioTracks().every(track => track.readyState === 'live') &&
      this.processedContext &&
      this.processedContext.state !== 'closed'
    )

    if (!hasLiveLocalStream) {
      if (this.localStream) {
        this.localStream.getTracks().forEach(track => track.stop())
        this.localStream = null
      }
      this.cleanupProcessedStream()

      if (this.rawStream?.getAudioTracks().some(track => track.readyState === 'live')) {
        this.startBackgroundMeter()
      } else {
        await this.startBackgroundMic()
      }
    }
  }

  private startSilenceMonitor() {
    this.stopSilenceMonitor();
    this.silenceCounterMs = 0;
    this.isSilenceWarningActive = false;

    if (!this.rawAnalyserNode) return;

    const bufferLength = this.rawAnalyserNode.fftSize;
    const dataArray = new Float32Array(bufferLength);

    this.silenceMonitorInterval = setInterval(() => {
      const store = useAppStore.getState();
      const me = store.currentUser;

      if (!me || me.isMuted || me.isServerMuted) {
        this.silenceCounterMs = 0;
        return;
      }

      if (this.rawAnalyserNode) {
        try {
          this.rawAnalyserNode.getFloatTimeDomainData(dataArray);
          let sumSquares = 0;
          for (let i = 0; i < bufferLength; i++) {
            sumSquares += dataArray[i] * dataArray[i];
          }
          const rms = Math.sqrt(sumSquares / bufferLength);

          if (rms < 0.0002) {
            this.silenceCounterMs += 200;
          } else {
            this.silenceCounterMs = 0;
          }

          if (this.silenceCounterMs >= 15000 && !this.isSilenceWarningActive) {
            this.isSilenceWarningActive = true;
            const toastMsg = i18n.t('toasts.micNotHearing', 'вас не слышно, проверьте микрофон');
            store.setSystemToast(toastMsg);

            setTimeout(() => {
              const currentStore = useAppStore.getState();
              if (currentStore.systemToast === toastMsg) {
                currentStore.setSystemToast(null);
              }
              this.isSilenceWarningActive = false;
            }, 4000);

            this.silenceCounterMs = 0;
          }
        } catch (e) {
          console.warn('[WebRTC] Silence monitor error:', e);
        }
      }
    }, 200);
  }

  private stopSilenceMonitor() {
    if (this.silenceMonitorInterval) {
      clearInterval(this.silenceMonitorInterval);
      this.silenceMonitorInterval = null;
    }
    this.silenceCounterMs = 0;
    this.isSilenceWarningActive = false;
    this.rawAnalyserNode = null;
  }

  public toggleMute(isMuted: boolean) {
    if (this.micNode) {
      this.micNode.port.postMessage({ type: 'setConfig', isMuted: isMuted })
    }
    if (this.manualGateNode) {
      this.manualGateNode.port.postMessage({ type: 'setConfig', isMuted })
    }
    if (this.localStream) {
      this.localStream.getAudioTracks().forEach(t => { t.enabled = !isMuted })
    }
  }

  public setUserVolume(userId: string, volume: number) {
    useAppStore.getState().setUserVolume(userId, Math.max(0, Math.min(200, volume)))
    this.updateRemoteVolume(userId)
  }

  public setUserVolumeRealtime(userId: string, volume: number) {
    const gainNode = this.userGainNodes.get(userId)
    if (gainNode) {
      gainNode.gain.value = Math.max(0, Math.min(4.0, (this.outputVolume / 100) * (Math.max(0, Math.min(200, volume)) / 100)))
    }
  }

  private initOutputMixer() {
    if (this.outputMixContext) {
      if (this.outputMixContext.state === 'suspended') this.outputMixContext.resume().catch(() => { })
      return
    }
    try {
      this.outputMixContext = new AudioContext({ sampleRate: 48000, latencyHint: 'interactive' })
    } catch (e) {
      console.warn('[WebRTC] Failed to create outputMixContext at 48000Hz, falling back to default:', e)
      this.outputMixContext = new AudioContext({ latencyHint: 'interactive' })
    }
    if (this.outputMixContext.state === 'suspended') {
      this.outputMixContext.resume().catch(() => { })
    }
    this.outputBusGain = this.outputMixContext.createGain()
    this.outputBusGain.gain.value = this.isDeafened ? 0 : Math.pow(10, PLAYBACK_MAKEUP_GAIN_DB / 20)

    this.outputPeakGuard = this.createPeakGuard(this.outputMixContext)
    this.outputBusGain.connect(this.outputPeakGuard)

    const ctx = this.outputMixContext
    const hasContextSink = typeof (ctx as any).setSinkId === 'function'

    if (hasContextSink) {
      this.outputPeakGuard.connect(ctx.destination)
    } else {
      const dest = ctx.createMediaStreamDestination()
      const destTrack = dest.stream.getAudioTracks()[0]
      if (destTrack) destTrack.contentHint = 'music'
      this.outputPeakGuard.connect(dest)

      this.mixAudioElement = new Audio()
      this.mixAudioElement.autoplay = true
      this.mixAudioElement.srcObject = dest.stream
      this.mixAudioElement.muted = this.isDeafened
    }
    void this.applyOutputDevice()
  }

  private createPlaybackLimiter(
    thresholdDb: number,
    kneeDb: number,
    ratio: number,
    attackS: number,
    releaseS: number
  ): DynamicsCompressorNode {
    const limiter = this.outputMixContext!.createDynamicsCompressor()
    limiter.threshold.value = thresholdDb
    limiter.knee.value = kneeDb
    limiter.ratio.value = ratio
    limiter.attack.value = attackS
    limiter.release.value = releaseS
    return limiter
  }

  private applyVoiceSeats() {
    const ctx = this.outputMixContext
    if (!ctx) return
    this.voiceSeatOrder = this.voiceSeatOrder.filter(id => this.userPannerNodes.has(id))
    const seats = this.voiceSeatOrder
    const span = Math.max(1, seats.length - 1)
    const now = ctx.currentTime
    seats.forEach((id, index) => {
      const panner = this.userPannerNodes.get(id)
      if (!panner) return
      const position = seats.length < 2 ? 0 : ((index / span) * 2 - 1) * PLAYBACK_VOICE_SPREAD
      panner.pan.cancelScheduledValues(now)
      panner.pan.setTargetAtTime(position, now, PLAYBACK_SEAT_GLIDE_S)
    })
  }

  private setupPeerHandlers(pc: RTCPeerConnection, userId: string) {
    pc.ontrack = (event) => {
      if (userId === useAppStore.getState().currentUser?.id) {
        event.track.stop()
        this.disconnectFromPeer(userId)
        return
      }
      const stream = event.streams[0] || new MediaStream([event.track])
      this.initOutputMixer()

      if (event.track.kind === 'video') {
        useAppStore.getState().setRemoteVideoStream(userId, stream)
        return
      }

      const remoteVideoStream = useAppStore.getState().remoteVideoStreams[userId]
      const isScreenShareAudio = event.track.contentHint === 'music' ||
        (remoteVideoStream && event.streams[0] && event.streams[0].id === remoteVideoStream.id) ||
        (event.streams[0] && event.streams[0].getVideoTracks().length > 0)

      if (isScreenShareAudio) {
        event.track.contentHint = 'music'
        let dummyAudio = this.streamAudioElements.get(userId)
        if (!dummyAudio) {
          dummyAudio = new Audio()
          dummyAudio.autoplay = true
          dummyAudio.muted = true
          this.streamAudioElements.set(userId, dummyAudio)
        }
        dummyAudio.srcObject = stream
        dummyAudio.play().catch(() => { })

        if (this.streamSourceNodes.has(userId)) {
          try { this.streamSourceNodes.get(userId)?.disconnect() } catch { }
          try { this.streamDelayNodes.get(userId)?.disconnect() } catch { }
          try { this.streamGainNodes.get(userId)?.disconnect() } catch { }
          try { this.streamLimiterNodes.get(userId)?.disconnect() } catch { }
        }

        const source = this.outputMixContext!.createMediaStreamSource(new MediaStream([event.track]))
        const delay = new DelayNode(this.outputMixContext!, { maxDelayTime: 1.0, delayTime: 0 })
        const gain = this.outputMixContext!.createGain()
        const limiter = this.createPlaybackLimiter(PLAYBACK_STREAM_LIMIT_DB, 2, 20, 0.002, 0.080)
        source.connect(delay)
        delay.connect(limiter)
        limiter.connect(gain)
        gain.connect(this.outputBusGain!)

        this.streamSourceNodes.set(userId, source)
        this.streamDelayNodes.set(userId, delay)
        this.streamGainNodes.set(userId, gain)
        this.streamLimiterNodes.set(userId, limiter)
        this.streamSyncOffsetEma.delete(userId)
        this.streamSyncSkew.delete(userId)
        this.updateRemoteStreamVolume(userId)
        this.startStreamSyncMonitoring()
      } else {
        event.track.contentHint = 'speech'
        this.setupVAD(stream, userId, false)

        let dummyAudio = this.audioElements.get(userId)
        if (!dummyAudio) {
          dummyAudio = new Audio()
          dummyAudio.autoplay = true
          dummyAudio.muted = true
          this.audioElements.set(userId, dummyAudio)
        }
        dummyAudio.srcObject = stream
        dummyAudio.play().catch(() => { })

        if (this.userSourceNodes.has(userId)) {
          try { this.userSourceNodes.get(userId)?.disconnect() } catch { }
          try { this.userGainNodes.get(userId)?.disconnect() } catch { }
          try { this.userLimiterNodes.get(userId)?.disconnect() } catch { }
          try { this.userPannerNodes.get(userId)?.disconnect() } catch { }
        }

        const source = this.outputMixContext!.createMediaStreamSource(new MediaStream([event.track]))
        const gain = this.outputMixContext!.createGain()
        const limiter = this.createPlaybackLimiter(PLAYBACK_VOICE_LIMIT_DB, 2, 20, 0.002, 0.060)
        const panner = new StereoPannerNode(this.outputMixContext!, { pan: 0 })
        source.connect(limiter)
        limiter.connect(gain)
        gain.connect(panner)
        panner.connect(this.outputBusGain!)

        this.userSourceNodes.set(userId, source)
        this.userGainNodes.set(userId, gain)
        this.userLimiterNodes.set(userId, limiter)
        this.userPannerNodes.set(userId, panner)
        if (!this.voiceSeatOrder.includes(userId)) this.voiceSeatOrder.push(userId)
        this.updateRemoteVolume(userId)
        this.applyVoiceSeats()
      }
    }

    pc.onicecandidate = (e) => {
      if (!e.candidate) return
      if (this.relayOnlyIce) {
        const cand = e.candidate.candidate || ''
        if (!cand.includes('typ relay')) return
      }
      signalRService.sendIceCandidate(userId, JSON.stringify(e.candidate))
    }

    const checkState = () => {
      const st = pc.connectionState
      const iceSt = pc.iceConnectionState

      if (st === 'connected' || iceSt === 'connected' || iceSt === 'completed') {
        const startedAt = this.peerConnectStartedAt.get(userId)
        if (startedAt !== undefined) {
          this.peerConnectStartedAt.delete(userId)
          const elapsed = Math.round(performance.now() - startedAt)
          void pc.getStats().then(stats => {
            let pair = ''
            stats.forEach(report => {
              if (report.type !== 'candidate-pair' || !report.selected && report.state !== 'succeeded') return
              const local = stats.get(report.localCandidateId)
              const remote = stats.get(report.remoteCandidateId)
              if (local && remote) pair = `${local.candidateType}/${local.protocol} -> ${remote.candidateType}/${remote.protocol}`
            })
            console.log(`[WebRTC] peer ${userId} connected in ${elapsed}ms${pair ? ` via ${pair}` : ''}`)
          }).catch(() => console.log(`[WebRTC] peer ${userId} connected in ${elapsed}ms`))
        }
        useAppStore.getState().setWebRTCConnectionStatus(userId, true)
        this.clearIceTimeout(userId)
        this.retryCount.delete(userId)
        const dcTimer = this.dcTimers.get(userId)
        if (dcTimer) {
          clearTimeout(dcTimer)
          this.dcTimers.delete(userId)
        }
      } else if (st === 'failed' || iceSt === 'failed') {
        useAppStore.getState().setWebRTCConnectionStatus(userId, false)
        void this.attemptRenegotiation(userId)
      } else if (st === 'disconnected' || iceSt === 'disconnected') {
        useAppStore.getState().setWebRTCConnectionStatus(userId, false)
        const existingTimer = this.dcTimers.get(userId)
        if (!existingTimer) {
          const t = setTimeout(() => {
            if (pc.connectionState === 'disconnected' || pc.iceConnectionState === 'disconnected') {
              void this.attemptRenegotiation(userId)
            }
            this.dcTimers.delete(userId)
          }, WebRTCManager.DISCONNECTED_GRACE_MS)
          this.dcTimers.set(userId, t)
        }
      } else {
        useAppStore.getState().setWebRTCConnectionStatus(userId, false)
      }
    }

    pc.onconnectionstatechange = checkState
    pc.oniceconnectionstatechange = checkState
  }

  private startIceTimeout(userId: string) {
    this.clearIceTimeout(userId)
    const timer = setTimeout(() => {
      this.iceTimeoutTimers.delete(userId)
      const pc = this.peerConnections.get(userId)
      if (pc && pc.connectionState !== 'connected') {
        void this.attemptRenegotiation(userId)
      }
    }, WebRTCManager.ICE_TIMEOUT_MS)
    this.iceTimeoutTimers.set(userId, timer)
  }

  private clearIceTimeout(userId: string) {
    const t = this.iceTimeoutTimers.get(userId)
    if (t) { clearTimeout(t); this.iceTimeoutTimers.delete(userId) }
  }

  private async attemptRenegotiation(userId: string) {
    if (this.iceRestartInFlight.has(userId)) return

    const pc = this.peerConnections.get(userId)
    if (!pc || pc.connectionState === 'closed') return

    const me = useAppStore.getState().currentUser?.id
    if (!me) return

    this.iceRestartInFlight.add(userId)
    try {
      const count = this.retryCount.get(userId) ?? 0

      if (me > userId && count === 0) {
        this.retryCount.set(userId, 1)
        this.startIceTimeout(userId)
        return
      }

      if (count < WebRTCManager.MAX_ICE_RETRIES) {
        this.retryCount.set(userId, count + 1)
        await this.renegotiatePeer(pc, userId, true)
        this.startIceTimeout(userId)
        return
      }

      this.disconnectFromPeer(userId, { preserveRemoteVideo: true, preserveRetryState: true })
      this.retryCount.set(userId, 0)
      await this.connectToPeer(userId, true)
    } finally {
      this.iceRestartInFlight.delete(userId)
    }
  }

  public updateRemoteStreamVolume(userId: string) {
    const store = useAppStore.getState()
    const isActive = store.activeStreamId === userId
    const vol = isActive ? (store.streamVolumes[userId] ?? 100) : 0
    const gainNode = this.streamGainNodes.get(userId)
    if (gainNode) {
      gainNode.gain.value = Math.max(0, Math.min(4.0, (this.outputVolume / 100) * (vol / 100)))
    }
  }

  public setStreamVolumeRealtime(userId: string, volume: number) {
    const store = useAppStore.getState()
    const isActive = store.activeStreamId === userId
    const gainNode = this.streamGainNodes.get(userId)
    if (gainNode) {
      const vol = isActive ? volume : 0
      gainNode.gain.value = Math.max(0, Math.min(4.0, (this.outputVolume / 100) * (Math.max(0, Math.min(200, vol)) / 100)))
    }
  }

  private async startIsolatedStreamAudio(sourceId: string): Promise<MediaStreamTrack> {
    this.stopIsolatedStreamAudio()

    const context = new AudioContext({ sampleRate: 48000, latencyHint: 'interactive' })
    if (context.sampleRate !== 48000) {
      await context.close().catch(() => { })
      throw new Error(`Stream audio requires 48000Hz, got ${context.sampleRate}Hz`)
    }

    try {
      await context.audioWorklet.addModule(playbackBufferProcessorUrl)
      const node = new AudioWorkletNode(context, 'playback-buffer-processor', {
        numberOfInputs: 0,
        numberOfOutputs: 1,
        outputChannelCount: [2]
      })
      const destination = context.createMediaStreamDestination()
      node.connect(destination)

      this.streamCaptureContext = context
      this.streamCaptureNode = node
      this.streamCaptureDestination = destination
      this.removeStreamAudioListener = window.windowControls.onStreamAudioData((data, metadata) => {
        if (metadata.sampleRate !== 48000 || metadata.bitsPerSample !== 32 || !metadata.isFloat) {
          console.warn('[WebRTC] Unsupported native stream audio format:', metadata)
          return
        }

        const copy = new Uint8Array(data.byteLength)
        copy.set(data)
        node.port.postMessage({
          type: 'audio',
          buffer: copy.buffer,
          channels: metadata.channels
        }, [copy.buffer])
      })

      await window.windowControls.startStreamAudioCapture(sourceId)
      if (context.state === 'suspended') await context.resume()

      const track = destination.stream.getAudioTracks()[0]
      if (!track) throw new Error('Native stream audio track was not created')
      track.contentHint = 'music'
      return track
    } catch (error) {
      this.stopIsolatedStreamAudio()
      throw error
    }
  }

  private stopIsolatedStreamAudio() {
    const wasCapturing = this.removeStreamAudioListener !== null
    if (this.removeStreamAudioListener) {
      try { this.removeStreamAudioListener() } catch { }
      this.removeStreamAudioListener = null
    }

    if (wasCapturing) {
      try { void window.windowControls.stopStreamAudioCapture().catch(() => { }) } catch { }
    }
    if (this.streamCaptureNode) {
      try { this.streamCaptureNode.disconnect() } catch { }
      this.streamCaptureNode = null
    }
    if (this.streamCaptureDestination) {
      try {
        this.streamCaptureDestination.stream.getTracks().forEach(track => {
          try {
            track.enabled = false
            track.stop()
          } catch { }
        })
      } catch { }
      this.streamCaptureDestination = null
    }
    if (this.streamCaptureContext && this.streamCaptureContext.state !== 'closed') {
      try { this.streamCaptureContext.close().catch(() => { }) } catch { }
    }
    this.streamCaptureContext = null
  }

  public async startScreenShare(sourceId: string, quality: 'high' | 'low' | 'camera', includeAudio: boolean) {
    this.currentStreamQuality = quality
    if (this.localVideoStream) {
      this.stopScreenShare()
    }
    const width = quality === 'high' ? 1920 : 1280
    const height = quality === 'high' ? 1080 : 720
    const frameRate = quality === 'high' ? 60 : 30

    try {
      let constraints: MediaStreamConstraints
      const isCamera = sourceId.startsWith('camera:')

      if (isCamera) {
        const deviceId = sourceId.slice(7)
        constraints = {
          video: {
            deviceId: deviceId ? { exact: deviceId } : undefined
          },
          audio: false
        }
      } else {
        constraints = {
          video: {
            mandatory: {
              chromeMediaSource: 'desktop',
              chromeMediaSourceId: sourceId,
              maxWidth: width,
              maxHeight: height,
              maxFrameRate: frameRate
            }
          } as any
        }
      }

      let stream: MediaStream
      try {
        stream = await navigator.mediaDevices.getUserMedia(constraints)
      } catch (err) {
        if (constraints.audio) {
          delete constraints.audio
          stream = await navigator.mediaDevices.getUserMedia(constraints)
        } else {
          throw err
        }
      }

      this.localVideoStream = stream
      const videoTrack = stream.getVideoTracks()[0]
      let audioTrack: MediaStreamTrack | undefined
      if (includeAudio && !isCamera) {
        audioTrack = await this.startIsolatedStreamAudio(sourceId)
        stream.addTrack(audioTrack)
      }

      if (videoTrack) {
        videoTrack.contentHint = 'motion'
      }

      for (const [userId, pc] of this.peerConnections.entries()) {
        if (videoTrack) {
          const sender = pc.addTrack(videoTrack, stream)
          try {
            if (!isCamera) {
              const params = sender.getParameters()
              if (!params.encodings || params.encodings.length === 0) params.encodings = [{}]
              params.degradationPreference = 'maintain-resolution'
              params.encodings[0].maxBitrate = quality === 'high' ? 6000000 : 2500000
              params.encodings[0].maxFramerate = quality === 'high' ? 60 : 30
              params.encodings[0].networkPriority = 'high'
              sender.setParameters(params).catch(() => { })
            }
          } catch { }
        }
        if (audioTrack) this.configureAudioSender(pc.addTrack(audioTrack, stream))
        await this.renegotiatePeer(pc, userId)
      }

      this.startStatsMonitoring()
      return true
    } catch (e) {
      this.stopScreenShare()
      throw e
    }
  }

  public stopScreenShare() {
    if (this.statsInterval) {
      clearInterval(this.statsInterval)
      this.statsInterval = null
    }

    const localVideo = this.localVideoStream
    const videoTracks = localVideo ? localVideo.getVideoTracks() : []
    const screenAudioTracks = localVideo ? localVideo.getAudioTracks().filter(t => t !== this.localStream?.getAudioTracks()[0]) : []

    for (const [userId, pc] of this.peerConnections.entries()) {
      try {
        const senders = pc.getSenders()
        for (const sender of senders) {
          const track = sender.track
          const isScreenVideo = (track && track.kind === 'video') || (track && videoTracks.includes(track))
          const isScreenAudio = (track && track.kind === 'audio' && track !== this.localStream?.getAudioTracks()[0]) || (track && screenAudioTracks.includes(track))
          if (isScreenVideo || isScreenAudio) {
            try { pc.removeTrack(sender) } catch { }
          }
        }
        void this.renegotiatePeer(pc, userId)
      } catch { }
    }

    if (this.localVideoStream) {
      try {
        this.localVideoStream.getTracks().forEach(track => {
          try {
            track.enabled = false
            track.stop()
          } catch { }
        })
      } catch { }
      this.localVideoStream = null
    }

    this.stopIsolatedStreamAudio()

    this.lastPacketsLost.clear()
    this.lastPacketsSent.clear()
    this.lossLadderStep.clear()
    this.lossBreachCount.clear()
    this.lossCleanCount.clear()
    this.appliedVideoProfiles.clear()
    this.viewerStates.clear()
  }

  private async renegotiatePeer(pc: RTCPeerConnection, userId: string, iceRestart = false): Promise<boolean> {
    try {
      if (pc.signalingState !== 'stable') {
        this.pendingRenegotiation.add(userId)
        return false
      }
      this.pendingRenegotiation.delete(userId)
      const offer = await pc.createOffer(
        iceRestart ? SILENCE_SUPPRESSION_OFF_WITH_ICE_RESTART : SILENCE_SUPPRESSION_OFF
      )
      const optimizedSDP = optimizeSDP(offer.sdp!, this.relayOnlyIce)
      await pc.setLocalDescription({ type: 'offer', sdp: optimizedSDP })
      signalRService.sendWebRTCOffer(userId, JSON.stringify(pc.localDescription))
      return true
    } catch (e) {
      console.error('[WebRTC] renegotiation failed', e)
      return false
    }
  }

  private flushPendingRenegotiation(userId: string) {
    if (!this.pendingRenegotiation.has(userId)) return
    const pc = this.peerConnections.get(userId)
    if (!pc || pc.signalingState !== 'stable') return
    this.pendingRenegotiation.delete(userId)
    void this.renegotiatePeer(pc, userId)
  }

  private static readonly SYNC_DEADBAND_S = 0.025
  private static readonly SYNC_MAX_AUDIO_DELAY_S = 0.5
  private static readonly SYNC_EMA_ALPHA = 0.15
  private static readonly SYNC_CORRECTION_GAIN = 0.5
  private static readonly SYNC_MAX_DELAY_STEP_S = 0.015
  private static readonly SYNC_DELAY_RAMP_S = 1.5

  private startStreamSyncMonitoring() {
    if (this.streamSyncInterval) return
    this.streamSyncInterval = setInterval(() => {
      void this.runStreamSyncTick()
    }, 1000)
  }

  private stopStreamSyncMonitoring() {
    if (!this.streamSyncInterval) return
    clearInterval(this.streamSyncInterval)
    this.streamSyncInterval = null
    this.streamSyncOffsetEma.clear()
    this.streamSyncSkew.clear()
  }

  private measureAudioVideoOffset(stats: RTCStatsReport): number | null {
    let audioPlayout = Number.NaN
    let videoPlayout = Number.NaN
    let audioJitter = Number.NaN
    let videoJitter = Number.NaN

    stats.forEach(report => {
      if (report.type !== 'inbound-rtp') return
      const emitted = report.jitterBufferEmittedCount
      const jitterDelay = typeof report.jitterBufferDelay === 'number' && typeof emitted === 'number' && emitted > 0
        ? report.jitterBufferDelay / emitted
        : Number.NaN
      if (report.kind === 'audio') {
        if (typeof report.estimatedPlayoutTimestamp === 'number') audioPlayout = report.estimatedPlayoutTimestamp
        audioJitter = jitterDelay
      } else if (report.kind === 'video') {
        if (typeof report.estimatedPlayoutTimestamp === 'number') videoPlayout = report.estimatedPlayoutTimestamp
        videoJitter = jitterDelay
      }
    })

    if (Number.isFinite(audioPlayout) && Number.isFinite(videoPlayout)) {
      return (audioPlayout - videoPlayout) / 1000
    }
    if (Number.isFinite(audioJitter) && Number.isFinite(videoJitter)) {
      return videoJitter - audioJitter
    }
    return null
  }

  private async runStreamSyncTick() {
    if (this.streamGainNodes.size === 0) {
      this.stopStreamSyncMonitoring()
      return
    }

    const ctx = this.outputMixContext
    if (!ctx) return

    for (const userId of this.streamGainNodes.keys()) {
      const pc = this.peerConnections.get(userId)
      const delayNode = this.streamDelayNodes.get(userId)
      if (!pc || !delayNode || pc.connectionState !== 'connected') continue

      try {
        const stats = await pc.getStats()
        if (!this.streamGainNodes.has(userId) || this.streamDelayNodes.get(userId) !== delayNode) continue
        const rawOffset = this.measureAudioVideoOffset(stats)
        if (rawOffset === null || !Number.isFinite(rawOffset)) continue

        const clampedOffset = Math.max(-0.5, Math.min(0.5, rawOffset))
        const previousEma = this.streamSyncOffsetEma.get(userId)
        const smoothed = previousEma === undefined
          ? clampedOffset
          : previousEma + WebRTCManager.SYNC_EMA_ALPHA * (clampedOffset - previousEma)
        this.streamSyncOffsetEma.set(userId, smoothed)

        let skew = this.streamSyncSkew.get(userId) ?? 0
        if (Math.abs(smoothed) > WebRTCManager.SYNC_DEADBAND_S) {
          skew = Math.max(
            0,
            Math.min(WebRTCManager.SYNC_MAX_AUDIO_DELAY_S, skew + WebRTCManager.SYNC_CORRECTION_GAIN * smoothed)
          )
          this.streamSyncSkew.set(userId, skew)
        }

        const wantedAudioDelay = Math.max(0, skew)
        const appliedAudioDelay = delayNode.delayTime.value
        const step = Math.max(
          -WebRTCManager.SYNC_MAX_DELAY_STEP_S,
          Math.min(WebRTCManager.SYNC_MAX_DELAY_STEP_S, wantedAudioDelay - appliedAudioDelay)
        )
        if (Math.abs(step) > 0.003) {
          const now = ctx.currentTime
          delayNode.delayTime.cancelScheduledValues(now)
          delayNode.delayTime.setValueAtTime(appliedAudioDelay, now)
          delayNode.delayTime.linearRampToValueAtTime(
            appliedAudioDelay + step,
            now + WebRTCManager.SYNC_DELAY_RAMP_S
          )
        }

        const videoReceiver = pc.getReceivers().find(r => r.track?.kind === 'video')
        if (videoReceiver && 'jitterBufferTarget' in videoReceiver && videoReceiver.jitterBufferTarget !== null) {
          videoReceiver.jitterBufferTarget = null
        }
      } catch { }
    }
  }

  public applyViewerState(viewerId: string, state: 'watching' | 'preview') {
    const normalized = state === 'preview' ? 'preview' : 'watching'
    if (this.viewerStates.get(viewerId) === normalized) return
    this.viewerStates.set(viewerId, normalized)
    if (this.localVideoStream && this.currentStreamQuality !== 'camera') {
      void this.applyVideoProfile(viewerId)
    }
  }

  public scheduleStreamViewInterestReport() {
    if (this.viewInterestTimer) clearTimeout(this.viewInterestTimer)
    this.viewInterestTimer = setTimeout(() => {
      this.viewInterestTimer = null
      this.reportStreamViewInterest()
    }, 300)
  }

  private reportStreamViewInterest() {
    const store = useAppStore.getState()
    const currentUserId = store.currentUser?.id
    const watchedId = store.activeStreamId
    const streamingPeers = Object.keys(store.remoteVideoStreams)

    for (const peerId of [...this.reportedViewStates.keys()]) {
      if (!streamingPeers.includes(peerId)) this.reportedViewStates.delete(peerId)
    }

    for (const peerId of streamingPeers) {
      if (peerId === currentUserId) continue
      if (!this.peerConnections.has(peerId)) continue
      const desired: 'watching' | 'preview' = peerId === watchedId ? 'watching' : 'preview'
      if (this.reportedViewStates.get(peerId) === desired) continue
      this.reportedViewStates.set(peerId, desired)
      signalRService.sendStreamViewState(peerId, desired)
    }
  }

  private static readonly VIDEO_LADDER = [
    { scale: 1.0, bitrateHigh: 6000000, bitrateLow: 2500000, fpsHigh: 60, fpsLow: 30 },
    { scale: 1.0, bitrateHigh: 3000000, bitrateLow: 1200000, fpsHigh: 60, fpsLow: 30 },
    { scale: 1.0, bitrateHigh: 1800000, bitrateLow: 700000, fpsHigh: 30, fpsLow: 20 },
    { scale: 2.0, bitrateHigh: 800000, bitrateLow: 400000, fpsHigh: 20, fpsLow: 15 }
  ]

  private static readonly PREVIEW_VIDEO_PROFILE = { scale: 4.0, bitrate: 150000, framerate: 2 }
  private static readonly LADDER_BREACHES_TO_DROP = 2
  private static readonly LADDER_CLEAN_TO_RECOVER = 4

  private videoProfileFor(userId: string) {
    if (this.viewerStates.get(userId) === 'preview') return WebRTCManager.PREVIEW_VIDEO_PROFILE
    const isHigh = this.currentStreamQuality === 'high'
    const maxStep = WebRTCManager.VIDEO_LADDER.length - 1
    const step = Math.max(0, Math.min(maxStep, this.lossLadderStep.get(userId) ?? 0))
    const rung = WebRTCManager.VIDEO_LADDER[step]
    return {
      scale: rung.scale,
      bitrate: isHigh ? rung.bitrateHigh : rung.bitrateLow,
      framerate: isHigh ? rung.fpsHigh : rung.fpsLow
    }
  }

  private async applyVideoProfile(userId: string, pc?: RTCPeerConnection | null) {
    const connection = pc ?? this.peerConnections.get(userId)
    if (!connection) return
    const sender = connection.getSenders().find(s => s.track?.kind === 'video')
    if (!sender) return

    const profile = this.videoProfileFor(userId)
    const signature = `${profile.scale}|${profile.bitrate}|${profile.framerate}`
    if (this.appliedVideoProfiles.get(userId) === signature) return

    try {
      const params = sender.getParameters()
      if (!params.encodings || params.encodings.length === 0) params.encodings = [{}]
      params.degradationPreference = 'maintain-resolution'
      params.encodings[0].priority = 'medium'
      params.encodings[0].networkPriority = 'high'
      params.encodings[0].scaleResolutionDownBy = profile.scale
      params.encodings[0].maxBitrate = profile.bitrate
      params.encodings[0].maxFramerate = profile.framerate
      await sender.setParameters(params)
      this.appliedVideoProfiles.set(userId, signature)
    } catch { }
  }

  private updateLossLadder(userId: string, fractionLost: number, rtt: number) {
    const desiredStep =
      fractionLost > 0.05 || rtt > 0.28 ? 3
        : fractionLost > 0.02 || rtt > 0.18 ? 2
          : fractionLost > 0.008 || rtt > 0.10 ? 1
            : 0
    const currentStep = this.lossLadderStep.get(userId) ?? 0

    if (desiredStep > currentStep) {
      const breaches = (this.lossBreachCount.get(userId) ?? 0) + 1
      this.lossCleanCount.set(userId, 0)
      if (breaches >= WebRTCManager.LADDER_BREACHES_TO_DROP) {
        this.lossLadderStep.set(userId, currentStep + 1)
        this.lossBreachCount.set(userId, 0)
      } else {
        this.lossBreachCount.set(userId, breaches)
      }
      return
    }

    this.lossBreachCount.set(userId, 0)

    if (desiredStep < currentStep) {
      const clean = (this.lossCleanCount.get(userId) ?? 0) + 1
      if (clean >= WebRTCManager.LADDER_CLEAN_TO_RECOVER) {
        this.lossLadderStep.set(userId, currentStep - 1)
        this.lossCleanCount.set(userId, 0)
      } else {
        this.lossCleanCount.set(userId, clean)
      }
      return
    }

    this.lossCleanCount.set(userId, 0)
  }

  private startStatsMonitoring() {
    if (this.statsInterval) clearInterval(this.statsInterval)
    this.statsInterval = setInterval(async () => {
      if (this.currentStreamQuality === 'camera') return
      for (const [userId, pc] of this.peerConnections.entries()) {
        if (pc.connectionState !== 'connected') continue
        try {
          const stats = await pc.getStats()
          let reportedFractionLost = -1
          let rtt = 0
          let framesDropped = 0
          let rawPacketsLost = 0
          let rawPacketsSent = 0

          stats.forEach(report => {
            if (report.type === 'remote-inbound-rtp' && report.kind === 'video') {
              rtt = report.roundTripTime || 0
              rawPacketsLost = report.packetsLost || 0
              if (typeof report.fractionLost === 'number') reportedFractionLost = report.fractionLost
            }
            if (report.type === 'outbound-rtp' && report.kind === 'video') {
              framesDropped = report.framesDropped || 0
              rawPacketsSent = report.packetsSent || 0
            }
          })

          const previousLost = this.lastPacketsLost.get(userId) ?? rawPacketsLost
          const previousSent = this.lastPacketsSent.get(userId) ?? rawPacketsSent
          this.lastPacketsLost.set(userId, rawPacketsLost)
          this.lastPacketsSent.set(userId, rawPacketsSent)

          let fractionLost = reportedFractionLost
          if (fractionLost < 0) {
            const lostDelta = Math.max(0, rawPacketsLost - previousLost)
            const sentDelta = Math.max(0, rawPacketsSent - previousSent)
            fractionLost = sentDelta > 0 ? Math.min(1, lostDelta / (sentDelta + lostDelta)) : 0
          }

          this.updateLossLadder(userId, fractionLost, rtt)
          await this.applyVideoProfile(userId, pc)

          if (framesDropped > 50) {
            const store = useAppStore.getState()
            const toastMsg = i18n.t('toasts.streamPerfIssue', 'проблемы с производительностью, рекомендуется снизить качество')
            store.setSystemToast(toastMsg)
            setTimeout(() => {
              if (store.systemToast === toastMsg) {
                store.setSystemToast(null)
              }
            }, 4000)
          }
        } catch (e) {
          console.warn(e)
        }
      }
    }, 2500)
  }

  private configureAudioSender(sender: RTCRtpSender) {
    try {
      const params = sender.getParameters()
      if (!params.encodings || params.encodings.length === 0) params.encodings = [{}]
      params.encodings[0].maxBitrate = OPUS_AUDIO_BITRATE
      params.encodings[0].networkPriority = 'high'
      params.encodings[0].priority = 'high'
      sender.setParameters(params)
        .then(() => {
          console.info('[WebRTC] Audio sender configured', {
            codec: 'opus/48000 mono',
            maxBitrate: sender.getParameters().encodings?.[0]?.maxBitrate,
            contentHint: sender.track?.contentHint
          })
        })
        .catch(error => console.warn('[WebRTC] Failed to configure audio sender', error))
    } catch (error) {
      console.warn('[WebRTC] Failed to read audio sender parameters', error)
    }
  }

  private isPeerRelevant(userId: string): boolean {
    const store = useAppStore.getState()
    if (store.currentCallUser?.id === userId || store.incomingCall?.callerId === userId) return true
    return Boolean(store.currentChannelId && store.voiceUsers.some(user => user.id === userId))
  }

  private async awaitPeerRelevance(userId: string): Promise<boolean> {
    if (this.isPeerRelevant(userId)) return true
    const startedAt = performance.now()
    const deadline = startedAt + WebRTCManager.PEER_RELEVANCE_WAIT_MS
    while (performance.now() < deadline) {
      await new Promise(resolve => setTimeout(resolve, 50))
      if (!useAppStore.getState().currentUser) return false
      if (this.isPeerRelevant(userId)) {
        console.log(`[WebRTC] peer ${userId} became known after ${Math.round(performance.now() - startedAt)}ms`)
        return true
      }
    }
    console.warn(`[WebRTC] peer ${userId} never appeared in the channel roster, signalling dropped`)
    return false
  }

  public async connectToPeer(userId: string, preserveRemoteVideoOnFailure = false) {
    if (userId === useAppStore.getState().currentUser?.id) return
    if (!this.isPeerRelevant(userId)) return
    if (this.peerConnections.has(userId)) return

    const startedAt = performance.now()
    this.peerConnectStartedAt.set(userId, startedAt)
    if (!this.localStream) {
      await this.startLocalStream().catch(() => { })
    }
    const iceStartedAt = performance.now()
    await this.ensureIceServers()
    const iceMs = Math.round(performance.now() - iceStartedAt)
    if (!this.isPeerRelevant(userId)) return

    const pc = new RTCPeerConnection(this.rtcConfig())
    this.peerConnections.set(userId, pc)

    if (this.localStream) {
      this.localStream.getTracks().forEach(track => {
        const sender = pc.addTrack(track, this.localStream!)
        if (track.kind === 'audio') {
          this.configureAudioSender(sender)
        }
      })
    }
    if (this.localVideoStream) {
      this.localVideoStream.getTracks().forEach(track => {
        const sender = pc.addTrack(track, this.localVideoStream!)
        if (track.kind === 'video') {
          try {
            const isCamera = this.currentStreamQuality === 'camera'
            if (!isCamera) {
              const params = sender.getParameters()
              if (!params.encodings || params.encodings.length === 0) params.encodings = [{}]
              params.degradationPreference = 'maintain-resolution'
              params.encodings[0].maxBitrate = this.currentStreamQuality === 'high' ? 6000000 : 2500000
              params.encodings[0].maxFramerate = this.currentStreamQuality === 'high' ? 60 : 30
              params.encodings[0].networkPriority = 'high'
              sender.setParameters(params).catch(() => { })
            }
          } catch { }
        } else if (track.kind === 'audio') {
          this.configureAudioSender(sender)
        }
      })
    } else {
      try {
        pc.addTransceiver('video', { direction: 'recvonly' })
        pc.addTransceiver('audio', { direction: 'recvonly' })
      } catch { }
    }

    this.setupPeerHandlers(pc, userId)
    this.startIceTimeout(userId)

    try {
      const offer = await pc.createOffer(SILENCE_SUPPRESSION_OFF)
      const optimizedSDP = optimizeSDP(offer.sdp!, this.relayOnlyIce)
      await pc.setLocalDescription({ type: 'offer', sdp: optimizedSDP })
      if (this.peerConnections.get(userId) !== pc || !this.isPeerRelevant(userId)) {
        this.disconnectFromPeer(userId)
        return
      }
      signalRService.sendWebRTCOffer(userId, JSON.stringify(pc.localDescription))
      console.log(`[WebRTC] offer to ${userId} sent in ${Math.round(performance.now() - startedAt)}ms (ice servers ${iceMs}ms)`)
    } catch (e) {
      console.error('[WebRTC] connectToPeer failed', e)
      this.peerConnectStartedAt.delete(userId)
      if (preserveRemoteVideoOnFailure) {
        this.startIceTimeout(userId)
      } else {
        this.disconnectFromPeer(userId)
      }
    }
  }

  public async handleOffer(senderId: string, offerStr: string) {
    const store = useAppStore.getState()
    if (senderId === store.currentUser?.id) return
    if (!await this.awaitPeerRelevance(senderId)) return

    let pc = this.peerConnections.get(senderId)
    if (pc && (pc.connectionState === 'closed' || pc.connectionState === 'failed' || pc.connectionState === 'disconnected')) {
      this.disconnectFromPeer(senderId, { preserveRemoteVideo: true })
      pc = undefined
    }
    if (!pc) {
      if (!this.peerConnectStartedAt.has(senderId)) this.peerConnectStartedAt.set(senderId, performance.now())
      if (!this.localStream) {
        await this.startLocalStream().catch(() => { })
      }
      await this.ensureIceServers()
      if (!this.isPeerRelevant(senderId)) return
      pc = new RTCPeerConnection(this.rtcConfig())
      this.peerConnections.set(senderId, pc)

      if (this.localStream) {
        this.localStream.getTracks().forEach(track => {
          const sender = pc!.addTrack(track, this.localStream!)
          if (track.kind === 'audio') {
            this.configureAudioSender(sender)
          }
        })
      }
      if (this.localVideoStream) {
        this.localVideoStream.getTracks().forEach(track => {
          const sender = pc!.addTrack(track, this.localVideoStream!)
          if (track.kind === 'video') {
            try {
              const isCamera = this.currentStreamQuality === 'camera'
              if (!isCamera) {
                const params = sender.getParameters()
                if (!params.encodings || params.encodings.length === 0) params.encodings = [{}]
                params.degradationPreference = 'maintain-resolution'
                params.encodings[0].maxBitrate = this.currentStreamQuality === 'high' ? 6000000 : 2500000
                params.encodings[0].maxFramerate = this.currentStreamQuality === 'high' ? 60 : 30
                params.encodings[0].networkPriority = 'high'
                sender.setParameters(params).catch(() => { })
              }
            } catch { }
          } else if (track.kind === 'audio') {
            this.configureAudioSender(sender)
          }
        })
      } else {
        try {
          pc.addTransceiver('video', { direction: 'recvonly' })
          pc.addTransceiver('audio', { direction: 'recvonly' })
        } catch { }
      }

      this.setupPeerHandlers(pc, senderId)
      this.startIceTimeout(senderId)
    }

    try {
      const offer = JSON.parse(offerStr)

      const offerCollision = pc.signalingState !== 'stable'
      if (offerCollision) {
        const me = store.currentUser?.id ?? ''
        const isPolitePeer = me > senderId
        if (!isPolitePeer) return
        await pc.setLocalDescription({ type: 'rollback' })
      }

      await pc.setRemoteDescription(new RTCSessionDescription(offer))
      const answer = await pc.createAnswer(SILENCE_SUPPRESSION_OFF)
      const optimizedAnswerSDP = optimizeSDP(answer.sdp!, this.relayOnlyIce)
      await pc.setLocalDescription({ type: 'answer', sdp: optimizedAnswerSDP })
      await this.drainPendingCandidates(senderId)
      if (this.peerConnections.get(senderId) !== pc || !this.isPeerRelevant(senderId)) {
        this.disconnectFromPeer(senderId)
        return
      }
      signalRService.sendWebRTCAnswer(senderId, JSON.stringify(pc.localDescription))
      this.flushPendingRenegotiation(senderId)

      if (this.localVideoStream) {
        const senders = pc.getSenders()
        const tracks = this.localVideoStream.getTracks()
        const hasUnnegotiated = tracks.some(t => {
          const s = senders.find(sender => sender.track === t)
          return !s || !s.transport
        })
        if (hasUnnegotiated) {
          void this.renegotiatePeer(pc, senderId)
        }
      }
    } catch (e) {
      console.error('[WebRTC] handleOffer failed', e)
      this.disconnectFromPeer(senderId, { preserveRemoteVideo: true })
      void this.connectToPeer(senderId, true)
    }
  }

  public async handleAnswer(senderId: string, answerStr: string) {
    if (senderId === useAppStore.getState().currentUser?.id) return
    if (!this.isPeerRelevant(senderId)) return
    const pc = this.peerConnections.get(senderId)
    if (pc) {
      try {
        const answer = JSON.parse(answerStr)
        await pc.setRemoteDescription(new RTCSessionDescription(answer))
        await this.drainPendingCandidates(senderId)
        this.flushPendingRenegotiation(senderId)
      } catch (e) {
        console.error('[WebRTC] handleAnswer failed', e)
      }
    }
  }

  public async handleIceCandidate(senderId: string, candidateStr: string) {
    if (senderId === useAppStore.getState().currentUser?.id) return
    let candidate: RTCIceCandidateInit
    try { candidate = JSON.parse(candidateStr) } catch { return }

    if (!this.isPeerRelevant(senderId)) {
      const early = this.pendingCandidates.get(senderId) ?? []
      if (early.length < WebRTCManager.PENDING_CANDIDATES_PER_PEER) {
        early.push(candidate)
        this.pendingCandidates.set(senderId, early)
      }
      return
    }
    const pc = this.peerConnections.get(senderId)
    if (!pc) {
      const buf = this.pendingCandidates.get(senderId) ?? []
      buf.push(candidate)
      this.pendingCandidates.set(senderId, buf)
      return
    }

    if (!pc.remoteDescription) {
      const buf = this.pendingCandidates.get(senderId) ?? []
      buf.push(candidate)
      this.pendingCandidates.set(senderId, buf)
      return
    }

    try { await pc.addIceCandidate(new RTCIceCandidate(candidate)) } catch { }
  }

  private async drainPendingCandidates(userId: string): Promise<void> {
    const pc = this.peerConnections.get(userId)
    const candidates = this.pendingCandidates.get(userId)
    if (!pc || !candidates || candidates.length === 0) return
    this.pendingCandidates.delete(userId)
    for (const c of candidates) {
      try { await pc.addIceCandidate(new RTCIceCandidate(c)) } catch { }
    }
  }

  public disconnectFromPeer(
    userId: string,
    options: { preserveRemoteVideo?: boolean; preserveRetryState?: boolean } = {}
  ) {
    useAppStore.getState().setWebRTCConnectionStatus(userId, false)

    this.clearIceTimeout(userId)
    if (!options.preserveRetryState) this.retryCount.delete(userId)
    const dcTimer = this.dcTimers.get(userId)
    if (dcTimer) { clearTimeout(dcTimer); this.dcTimers.delete(userId) }

    const pc = this.peerConnections.get(userId)
    if (pc) { pc.ontrack = null; pc.onicecandidate = null; pc.onconnectionstatechange = null; pc.oniceconnectionstatechange = null; pc.close(); this.peerConnections.delete(userId) }

    const audio = this.audioElements.get(userId)
    if (audio) { audio.pause(); audio.srcObject = null; this.audioElements.delete(userId) }

    const source = this.userSourceNodes.get(userId)
    if (source) { try { source.disconnect() } catch { }; this.userSourceNodes.delete(userId) }

    const gain = this.userGainNodes.get(userId)
    if (gain) { try { gain.disconnect() } catch { }; this.userGainNodes.delete(userId) }

    const limiter = this.userLimiterNodes.get(userId)
    if (limiter) { try { limiter.disconnect() } catch { }; this.userLimiterNodes.delete(userId) }

    const panner = this.userPannerNodes.get(userId)
    if (panner) { try { panner.disconnect() } catch { }; this.userPannerNodes.delete(userId) }
    this.applyVoiceSeats()

    const streamAudio = this.streamAudioElements.get(userId)
    if (streamAudio) { streamAudio.pause(); streamAudio.srcObject = null; this.streamAudioElements.delete(userId) }

    const streamSource = this.streamSourceNodes.get(userId)
    if (streamSource) { try { streamSource.disconnect() } catch { }; this.streamSourceNodes.delete(userId) }

    const streamGain = this.streamGainNodes.get(userId)
    if (streamGain) { try { streamGain.disconnect() } catch { }; this.streamGainNodes.delete(userId) }

    const streamLimiter = this.streamLimiterNodes.get(userId)
    if (streamLimiter) { try { streamLimiter.disconnect() } catch { }; this.streamLimiterNodes.delete(userId) }

    const streamDelay = this.streamDelayNodes.get(userId)
    if (streamDelay) { try { streamDelay.disconnect() } catch { }; this.streamDelayNodes.delete(userId) }
    this.streamSyncOffsetEma.delete(userId)
    this.streamSyncSkew.delete(userId)
    if (this.streamGainNodes.size === 0) this.stopStreamSyncMonitoring()

    if (!options.preserveRemoteVideo) {
      useAppStore.getState().setRemoteVideoStream(userId, null)
    }

    this.pendingCandidates.delete(userId)
    this.pendingRenegotiation.delete(userId)
    this.clearVAD(userId)
    this.lastPacketsLost.delete(userId)
    this.lastPacketsSent.delete(userId)
    this.lossLadderStep.delete(userId)
    this.lossBreachCount.delete(userId)
    this.lossCleanCount.delete(userId)
    this.appliedVideoProfiles.delete(userId)
    this.viewerStates.delete(userId)
    this.reportedViewStates.delete(userId)
  }

  public cleanupRemoteStream(userId: string) {
    const streamAudio = this.streamAudioElements.get(userId)
    if (streamAudio) { streamAudio.pause(); streamAudio.srcObject = null; this.streamAudioElements.delete(userId) }

    const streamSource = this.streamSourceNodes.get(userId)
    if (streamSource) { try { streamSource.disconnect() } catch { }; this.streamSourceNodes.delete(userId) }

    const streamGain = this.streamGainNodes.get(userId)
    if (streamGain) { try { streamGain.disconnect() } catch { }; this.streamGainNodes.delete(userId) }

    const streamLimiter = this.streamLimiterNodes.get(userId)
    if (streamLimiter) { try { streamLimiter.disconnect() } catch { }; this.streamLimiterNodes.delete(userId) }

    const streamDelay = this.streamDelayNodes.get(userId)
    if (streamDelay) { try { streamDelay.disconnect() } catch { }; this.streamDelayNodes.delete(userId) }
    this.streamSyncOffsetEma.delete(userId)
    this.streamSyncSkew.delete(userId)
    if (this.streamGainNodes.size === 0) this.stopStreamSyncMonitoring()

    useAppStore.getState().setRemoteVideoStream(userId, null)
  }

  public leaveAll() {
    this.peerConnections.forEach((_, uid) => this.disconnectFromPeer(uid))
    this.pendingCandidates.clear()
    this.pendingRenegotiation.clear()
  }
}

export const webrtc = new WebRTCManager()
