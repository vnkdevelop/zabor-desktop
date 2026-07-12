import { signalRService } from './signalr'
import { useAppStore } from '../store/useAppStore'
import i18n from '../i18n'
import processorUrl from './deepfilter-processor?worker&url'

type SpeakingEntry = {
  timer: NodeJS.Timeout
  stream: MediaStream
  
  nodes: AudioNode[]
}

function optimizeSDP(sdp: string): string {
  let lines = sdp.split('\r\n')

  const opusRegex = /a=rtpmap:(\d+)\s+opus\/48000\/2/i
  const audioMatch = sdp.match(opusRegex)
  if (audioMatch) {
    const pt = audioMatch[1]
    let fmtpFound = false
    for (let i = 0; i < lines.length; i++) {
      if (lines[i].startsWith(`a=fmtp:${pt}`)) {
        lines[i] = `a=fmtp:${pt} maxaveragebitrate=96000;useinbandfec=1;usedtx=0;cbr=0;ptime=20;minptime=10;stereo=0;sprop-maxcapturerate=48000`
        fmtpFound = true
        break
      }
    }
    if (!fmtpFound) {
      for (let i = 0; i < lines.length; i++) {
        if (lines[i].startsWith(`a=rtpmap:${pt}`)) {
          lines.splice(i + 1, 0, `a=fmtp:${pt} maxaveragebitrate=96000;useinbandfec=1;usedtx=0;cbr=0;ptime=20;minptime=10;stereo=0;sprop-maxcapturerate=48000`)
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
      lines.splice(audioSectionIdx + 1, 0, 'b=AS:96')
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
    let h264Payloads: string[] = []
    let h264Fmtps: Record<string, string> = {}
    let h264RtpMaps: Record<string, string> = {}

    for (let i = videoSectionIdx + 1; i < lines.length; i++) {
      if (lines[i].startsWith('m=')) break
      const rtpmapMatch = lines[i].match(/^a=rtpmap:(\d+)\s+H264\/90000/i)
      if (rtpmapMatch) {
        const pt = rtpmapMatch[1]
        h264Payloads.push(pt)
        h264RtpMaps[pt] = lines[i]
      }
      const fmtpMatch = lines[i].match(/^a=fmtp:(\d+)\s+(.+)/i)
      if (fmtpMatch) {
        const pt = fmtpMatch[1]
        h264Fmtps[pt] = lines[i]
      }
    }

    if (h264Payloads.length > 0) {
      let filteredLines: string[] = []
      let skipVideoTracks = false

      for (let i = 0; i < lines.length; i++) {
        if (i === videoSectionIdx) {
          const parts = lines[i].split(' ')
          const newVideoLine = `${parts[0]} ${parts[1]} ${parts[2]} ${h264Payloads.join(' ')}`
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
            if (ptMatch && h264Payloads.includes(ptMatch[1])) {
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
  private localStream: MediaStream | null = null
  private rawStream: MediaStream | null = null
  public localVideoStream: MediaStream | null = null
  private statsInterval: NodeJS.Timeout | null = null
  private streamGainNodes: Map<string, GainNode> = new Map()
  private streamSourceNodes: Map<string, MediaStreamAudioSourceNode> = new Map()
  private streamAudioElements: Map<string, HTMLAudioElement> = new Map()

  private peerConnections: Map<string, RTCPeerConnection> = new Map()
  private audioElements: Map<string, HTMLAudioElement> = new Map()
  
  private pendingCandidates: Map<string, RTCIceCandidateInit[]> = new Map()
  
  private dcTimers: Map<string, NodeJS.Timeout> = new Map()
  
  private iceTimeoutTimers: Map<string, NodeJS.Timeout> = new Map()
  
  private retryCount: Map<string, number> = new Map()
  
  private static readonly MAX_ICE_RETRIES = 2
  
  private static readonly ICE_TIMEOUT_MS = 15000

  private currentDeviceId = 'default'
  private currentStreamQuality: '1080p' | '720p' = '720p'
  private currentOutputDeviceId = 'default'
  private noiseSuppression = true

  private inputVolume = 100
  private outputVolume = 100
  private isDeafened = false

  private processedContext: AudioContext | null = null
  private processedSource: MediaStreamAudioSourceNode | null = null
  private inputGainNode: GainNode | null = null
  private dfNode: AudioWorkletNode | null = null

  private calibratedThresholdOn = parseFloat(localStorage.getItem('zabor_threshold_on') || '0.008')
  private calibratedThresholdOff = parseFloat(localStorage.getItem('zabor_threshold_off') || '0.003')
  private calibratedAttenuationLimit = parseInt(localStorage.getItem('zabor_attenuation_limit') || '45')
  private calibratedNoiseFloor = parseFloat(localStorage.getItem('zabor_base_noise_floor') || '0.003')
  private thresholdMode = localStorage.getItem('zabor_threshold_mode') || 'auto'
  private manualThresholdValue = parseFloat(localStorage.getItem('zabor_manual_threshold_value') || '50')
  private activeStartPromise: Promise<boolean> | null = null

  private rawAnalyserNode: AnalyserNode | null = null
  private silenceMonitorInterval: NodeJS.Timeout | null = null
  private silenceCounterMs = 0
  private isSilenceWarningActive = false

  private speakingIntervals: Map<string, SpeakingEntry> = new Map()

  
  private outputMixContext: AudioContext | null = null
  private outputCompressor: DynamicsCompressorNode | null = null
  private mixAudioElement: HTMLAudioElement | null = null
  private userGainNodes: Map<string, GainNode> = new Map()
  private userSourceNodes: Map<string, MediaStreamAudioSourceNode> = new Map()

  private readonly config: RTCConfiguration = {
    iceServers: [
      { urls: 'stun:stun.l.google.com:19302' },
      { urls: 'stun:stun1.l.google.com:19302' },
      { urls: 'stun:stun.cloudflare.com:3478' },
      { urls: 'stun:stun.twilio.com:3478' },
      
      { urls: 'turn:150.241.64.108:3478?transport=udp', username: 'zabor', credential: 'mvtxbJo45sc8_turn' },
      { urls: 'turn:150.241.64.108:3478?transport=tcp', username: 'zabor', credential: 'mvtxbJo45sc8_turn' }
    ],
    bundlePolicy: 'max-bundle',
    iceCandidatePoolSize: 4
  }

  

  private getThresholdParams(gainFactor: number) {
    let activeThresholdOn = this.calibratedThresholdOn
    let activeThresholdOff = this.calibratedThresholdOff
    if (this.thresholdMode === 'manual') {
      const mapped = 0.0005 * Math.pow(10, (this.manualThresholdValue / 50))
      activeThresholdOn = mapped
      activeThresholdOff = mapped * 0.6
    }
    return {
      thresholdOn: activeThresholdOn * gainFactor,
      thresholdOff: activeThresholdOff * gainFactor,
      attenuationLimit: this.calibratedAttenuationLimit,
      noiseFloor: this.calibratedNoiseFloor * gainFactor,
      thresholdMode: this.thresholdMode,
      manualThresholdValue: this.manualThresholdValue
    }
  }

  private async createProcessedStream(rawStream: MediaStream): Promise<MediaStream> {
    this.cleanupProcessedStream()

    const ctx = new AudioContext({ sampleRate: 48000, latencyHint: 'interactive' })
    this.processedContext = ctx
    if (ctx.state === 'suspended') {
      await ctx.resume().catch(() => { })
    }
    const destination = ctx.createMediaStreamDestination()

    let dfNode: AudioWorkletNode | null = null
    try {
      await ctx.audioWorklet.addModule(processorUrl)
      dfNode = new AudioWorkletNode(ctx, 'deepfilter-processor')
      this.dfNode = dfNode

      const me = useAppStore.getState().currentUser
      dfNode.port.onmessage = (event) => {
        if (event.data.type === 'vad' && me) {
          useAppStore.getState().setSpeakingStatus(me.id, event.data.isSpeaking)
          signalRService.setSpeakingState(event.data.isSpeaking)
        }
      }
    } catch (e) {
      console.warn('[WebRTC] Failed to load deepfilter-processor.js, running without it.', e)
    }

    const source = ctx.createMediaStreamSource(rawStream)
    this.processedSource = source

    const compressor = ctx.createDynamicsCompressor()
    compressor.threshold.value = -12
    compressor.knee.value = 10
    compressor.ratio.value = 2
    compressor.attack.value = 0.005
    compressor.release.value = 0.150

    const highpass = ctx.createBiquadFilter()
    highpass.type = 'highpass'
    highpass.frequency.value = 140
    highpass.Q.value = 0.707

    const lowpass = ctx.createBiquadFilter()
    lowpass.type = 'lowpass'
    lowpass.frequency.value = 7500
    lowpass.Q.value = 0.707

    const peaking = ctx.createBiquadFilter()
    peaking.type = 'peaking'
    peaking.frequency.value = 3000
    peaking.Q.value = 1.0
    peaking.gain.value = 2

    const limiter = ctx.createDynamicsCompressor()
    limiter.threshold.value = -0.5
    limiter.knee.value = 0
    limiter.ratio.value = 20
    limiter.attack.value = 0.001
    limiter.release.value = 0.050

    const inputGain = ctx.createGain()
    const gainFactor = Math.max(0.01, this.inputVolume / 100)
    inputGain.gain.value = gainFactor
    this.inputGainNode = inputGain

    try {
      const rawAnalyser = ctx.createAnalyser()
      rawAnalyser.fftSize = 256
      source.connect(rawAnalyser)
      this.rawAnalyserNode = rawAnalyser
    } catch (e) {
      console.warn('[WebRTC] Failed to create raw analyser node for silence monitoring:', e)
    }

    let currentNode: AudioNode = source

    currentNode.connect(inputGain)
    currentNode = inputGain

    currentNode.connect(highpass)
    highpass.connect(lowpass)
    currentNode = lowpass

    if (this.dfNode) {
      const store = useAppStore.getState()
      const isMuted = store.currentUser?.isMuted || store.currentUser?.isServerMuted || false
      this.dfNode.port.postMessage({
        type: 'setConfig',
        noiseSuppression: this.noiseSuppression,
        isMuted: isMuted
      })
      this.dfNode.port.postMessage({
        type: 'setCalibratedParams',
        ...this.getThresholdParams(gainFactor)
      })
      currentNode.connect(this.dfNode)
      currentNode = this.dfNode
    }

    currentNode.connect(compressor)
    compressor.connect(peaking)
    peaking.connect(limiter)
    limiter.connect(destination)

    return destination.stream
  }

  private cleanupProcessedStream() {
    this.stopSilenceMonitor()
    if (this.dfNode) {
      this.dfNode.port.close()
      this.dfNode.disconnect()
      this.dfNode = null
    }

    if (this.processedContext && this.processedContext.state !== 'closed') {
      this.processedContext.close().catch(() => { })
    }
    this.processedContext = null
    this.processedSource = null
    this.inputGainNode = null
  }

  public setInputVolume(volume: number) {
    this.inputVolume = volume
    const gainFactor = Math.max(0.01, volume / 100)
    if (this.inputGainNode) {
      this.inputGainNode.gain.value = gainFactor
    }
    if (this.dfNode) {
      this.dfNode.port.postMessage({
        type: 'setCalibratedParams',
        ...this.getThresholdParams(gainFactor)
      })
    }
  }

  public setMicThresholdParams(mode: 'auto' | 'manual', manualValue: number) {
    localStorage.setItem('zabor_threshold_mode', mode)
    localStorage.setItem('zabor_manual_threshold_value', manualValue.toString())
    this.thresholdMode = mode
    this.manualThresholdValue = manualValue
    this.updateThresholds()
  }

  private updateThresholds() {
    const gainFactor = Math.max(0.01, this.inputVolume / 100)
    if (this.dfNode) {
      this.dfNode.port.postMessage({
        type: 'setCalibratedParams',
        ...this.getThresholdParams(gainFactor)
      })
    }
  }

  public setOutputVolume(volume: number) {
    this.outputVolume = volume
    this.userGainNodes.forEach((_, userId) => this.updateRemoteVolume(userId))
  }

  public setDeafened(deafened: boolean) {
    this.isDeafened = deafened
    if (this.mixAudioElement) {
      this.mixAudioElement.muted = deafened
    }
  }

  private updateRemoteVolume(userId: string) {
    const gainNode = this.userGainNodes.get(userId)
    if (!gainNode) return
    const userVol = useAppStore.getState().userVolumes[userId] ?? 100
    gainNode.gain.value = Math.max(0, Math.min(4.0, 1.35 * (this.outputVolume / 100) * (userVol / 100)))
  }

  public setNoiseSuppression(enabled: boolean) {
    this.noiseSuppression = enabled
    if (this.dfNode) {
      this.dfNode.port.postMessage({ type: 'setConfig', noiseSuppression: enabled })
    }
  }

  

  private setupVAD(stream: MediaStream, userId: string, isLocal: boolean) {
    this.clearVAD(userId)

    try {
      let contextToUse: AudioContext | null = null
      if (isLocal) {
        if (!this.processedContext || this.processedContext.state === 'closed') {
          this.processedContext = new AudioContext({ sampleRate: 48000, latencyHint: 'interactive' })
        }
        contextToUse = this.processedContext
      } else {
        this.initOutputMixer()
        contextToUse = this.outputMixContext
      }

      if (!contextToUse) return
      if (contextToUse.state === 'suspended') contextToUse.resume().catch(() => { })

      const source = contextToUse.createMediaStreamSource(stream)

      const bp1 = contextToUse.createBiquadFilter()
      bp1.type = 'highpass'; bp1.frequency.value = 85; bp1.Q.value = 0.5

      const bp2 = contextToUse.createBiquadFilter()
      bp2.type = 'lowpass'; bp2.frequency.value = 8000; bp2.Q.value = 0.5

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

      const avgTh = isLocal ? 2.5 : 1.5
      const peakTh = isLocal ? 7 : 5

      const check = () => {
        const store = useAppStore.getState()
        if (isLocal && (store.currentUser?.isMuted || store.currentUser?.isServerMuted)) {
          if (wasSpeaking) {
            wasSpeaking = false; voiceFrames = 0
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

        if (isLocal) {
          if (peak === 0) {
            silenceFrames++
          } else {
            silenceFrames = 0
            hasWarnedSilence = false
          }

          if (silenceFrames > 150 && !hasWarnedSilence) {
            const toastMsg = i18n.t('toasts.micNotHearing', 'Вас не слышно, проверьте микрофон')
            store.setSystemToast(toastMsg)
            setTimeout(() => {
              const currentStore = useAppStore.getState()
              if (currentStore.systemToast === toastMsg) {
                currentStore.setSystemToast(null)
              }
            }, 4000)
            hasWarnedSilence = true
          }
        }

        const isVoice = avg >= avgTh || peak >= peakTh
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
          if (isLocal) signalRService.setSpeakingState(speaking)
        }
      }

      const timer = setInterval(check, 30)
      this.speakingIntervals.set(userId, { timer, stream, nodes: vadNodes })
    } catch (e) { console.error('[VAD] setup failed', e) }
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

  

  public async getAudioDevices() {
    try {
      await navigator.mediaDevices.getUserMedia({ audio: true })
      const devices = await navigator.mediaDevices.enumerateDevices()
      return {
        inputs: devices.filter(d => d.kind === 'audioinput'),
        outputs: devices.filter(d => d.kind === 'audiooutput')
      }
    } catch { return { inputs: [], outputs: [] } }
  }

  public setInputDevice(deviceId: string) { this.currentDeviceId = deviceId }

  public setOutputDevice(deviceId: string) {
    this.currentOutputDeviceId = deviceId
    if (this.mixAudioElement && typeof (this.mixAudioElement as any).setSinkId === 'function') {
      (this.mixAudioElement as any).setSinkId(deviceId).catch(() => { })
    }
  }

  public async calibrateMic(durationMs?: number): Promise<{ noiseFloor: number; peakNoise: number }> {
    const isFirstRun = localStorage.getItem('zabor_mic_calibrated') !== 'true';
    const actualDurationMs = durationMs !== undefined ? durationMs : (isFirstRun ? 5000 : 2000);

    let stream: MediaStream;
    let shouldStopStream = false;

    try {
      if (this.rawStream && this.rawStream.getAudioTracks().length > 0 && this.rawStream.getAudioTracks()[0].readyState === 'live') {
        stream = this.rawStream;
      } else {
        try {
          const constraints: MediaTrackConstraints = {
            channelCount: 1,
            echoCancellation: true,
            noiseSuppression: false,
            autoGainControl: false
          }
          if (this.currentDeviceId && this.currentDeviceId !== 'default') {
            constraints.deviceId = { exact: this.currentDeviceId }
          }
          stream = await navigator.mediaDevices.getUserMedia({
            audio: constraints,
            video: false
          });
        } catch {
          try {
            const devices = await navigator.mediaDevices.enumerateDevices()
            const audioInputs = devices.filter(d => d.kind === 'audioinput' && d.deviceId)
            if (audioInputs.length > 0) {
              const firstDevice = audioInputs.find(d => d.deviceId !== 'default') || audioInputs[0]
              stream = await navigator.mediaDevices.getUserMedia({
                audio: {
                  deviceId: { exact: firstDevice.deviceId },
                  channelCount: 1,
                  echoCancellation: true,
                  noiseSuppression: false,
                  autoGainControl: false
                },
                video: false
              });
            } else {
              throw new Error('No audio input devices found')
            }
          } catch {
            try {
              stream = await navigator.mediaDevices.getUserMedia({
                audio: {
                  echoCancellation: true,
                  noiseSuppression: false,
                  autoGainControl: false
                },
                video: false
              });
            } catch {
              try {
                stream = await navigator.mediaDevices.getUserMedia({
                  audio: true,
                  video: false
                });
              } catch {
                stream = createSilentAudioStream();
              }
            }
          }
        }
        shouldStopStream = true;
      }

      const audioContext = new AudioContext({ sampleRate: 48000 });
      const source = audioContext.createMediaStreamSource(stream);

      const highpass = audioContext.createBiquadFilter();
      highpass.type = 'highpass';
      highpass.frequency.value = 140;
      highpass.Q.value = 0.707;

      const lowpass = audioContext.createBiquadFilter();
      lowpass.type = 'lowpass';
      lowpass.frequency.value = 7500;
      lowpass.Q.value = 0.707;

      const analyser = audioContext.createAnalyser();
      analyser.fftSize = 512;

      source.connect(highpass);
      highpass.connect(lowpass);
      lowpass.connect(analyser);

      const bufferLength = analyser.fftSize;
      const dataArray = new Float32Array(bufferLength);
      const windowRmsValues: number[] = [];

      const intervalTime = 50; 
      const steps = actualDurationMs / intervalTime;

      const checkRms = () => {
        analyser.getFloatTimeDomainData(dataArray);
        let sumSquares = 0;
        for (let i = 0; i < bufferLength; i++) {
          sumSquares += dataArray[i] * dataArray[i];
        }
        const rms = Math.sqrt(sumSquares / bufferLength);
        windowRmsValues.push(rms);
      };

      for (let i = 0; i < steps; i++) {
        await new Promise(resolve => setTimeout(resolve, intervalTime));
        checkRms();
      }

      source.disconnect();
      highpass.disconnect();
      lowpass.disconnect();
      analyser.disconnect();

      if (shouldStopStream) {
        stream.getTracks().forEach(t => t.stop());
      }
      await audioContext.close();

      if (windowRmsValues.length === 0) {
        throw new Error('No audio data collected during calibration');
      }

      const rawSorted = [...windowRmsValues].sort((a, b) => a - b);
      const cleanLength = Math.floor(rawSorted.length * 0.8);
      if (cleanLength === 0) {
        throw new Error('Not enough audio data collected during calibration');
      }
      const sortedRms = rawSorted.slice(0, cleanLength);

      const halfLength = Math.floor(sortedRms.length * 0.5);
      let noiseFloorSum = 0;
      for (let i = 0; i < Math.max(1, halfLength); i++) {
        noiseFloorSum += sortedRms[i];
      }
      let noiseFloor = noiseFloorSum / Math.max(1, halfLength);
      
      const peakNoiseIndex = Math.max(0, Math.floor(sortedRms.length * 0.95) - 1);
      const peakNoise = sortedRms[peakNoiseIndex] || 0.005;

      if (!isFirstRun) {
        const savedFloorRaw = localStorage.getItem('zabor_base_noise_floor');
        if (savedFloorRaw) {
          const savedFloor = parseFloat(savedFloorRaw);
          if (!isNaN(savedFloor)) {
            noiseFloor = 0.6 * savedFloor + 0.4 * noiseFloor;
          }
        }
      }

      this.calibratedThresholdOn = Math.max(0.008, peakNoise * 2.0 + 0.002);
      this.calibratedThresholdOff = Math.max(0.004, peakNoise * 1.3 + 0.001);
      this.calibratedAttenuationLimit = 100;
      this.calibratedNoiseFloor = noiseFloor;

      localStorage.setItem('zabor_mic_calibrated', 'true');
      localStorage.setItem('zabor_base_noise_floor', noiseFloor.toString());
      localStorage.setItem('zabor_threshold_on', this.calibratedThresholdOn.toString());
      localStorage.setItem('zabor_threshold_off', this.calibratedThresholdOff.toString());
      localStorage.setItem('zabor_attenuation_limit', this.calibratedAttenuationLimit.toString());

      this.updateThresholds();

      return { noiseFloor, peakNoise };
    } catch (e) {
      throw e;
    }
  }

  public async startLocalStream(deviceId?: string, useNS?: boolean, forceRestart = false): Promise<boolean> {
    if (deviceId !== undefined) this.currentDeviceId = deviceId
    if (useNS !== undefined) this.noiseSuppression = useNS

    if (this.activeStartPromise) {
      return this.activeStartPromise
    }

    const run = async () => {
      if (!forceRestart && this.localStream && this.localStream.getAudioTracks().length > 0 && this.localStream.getAudioTracks().every(t => t.readyState === 'live')) {
        return true
      }

      try {
        if (this.rawStream) { this.rawStream.getTracks().forEach(t => t.stop()); this.rawStream = null }
        if (this.localStream) { this.localStream.getTracks().forEach(t => t.stop()); this.localStream = null }
        this.cleanupProcessedStream()

        let raw: MediaStream
        try {
          const constraints: MediaTrackConstraints = {
            channelCount: 1,
            echoCancellation: true,
            noiseSuppression: !this.noiseSuppression,
            autoGainControl: false,
            // @ts-ignore
            googHighpassFilter: false,
            googEchoCancellation2: false,
            googAudioMirroring: false
          }
          if (this.currentDeviceId && this.currentDeviceId !== 'default') {
            constraints.deviceId = { exact: this.currentDeviceId }
          }
          raw = await navigator.mediaDevices.getUserMedia({
            audio: constraints,
            video: false
          })
        } catch {
          try {
            const devices = await navigator.mediaDevices.enumerateDevices()
            const audioInputs = devices.filter(d => d.kind === 'audioinput' && d.deviceId)
            if (audioInputs.length > 0) {
              const firstDevice = audioInputs.find(d => d.deviceId !== 'default') || audioInputs[0]
              raw = await navigator.mediaDevices.getUserMedia({
                audio: {
                  deviceId: { exact: firstDevice.deviceId },
                  channelCount: 1,
                  echoCancellation: true,
                  noiseSuppression: !this.noiseSuppression,
                  autoGainControl: false
                },
                video: false
              })
            } else {
              throw new Error('No audio input devices found')
            }
          } catch {
            try {
              raw = await navigator.mediaDevices.getUserMedia({
                audio: {
                  echoCancellation: true,
                  noiseSuppression: !this.noiseSuppression,
                  autoGainControl: false,
                  // @ts-ignore
                  googHighpassFilter: false,
                  googEchoCancellation2: false,
                  googAudioMirroring: false
                },
                video: false
              })
            } catch {
              try {
                raw = await navigator.mediaDevices.getUserMedia({
                  audio: true,
                  video: false
                })
              } catch {
                raw = createSilentAudioStream()
              }
            }
          }
        }

        this.rawStream = raw
        const rawTrack = raw.getAudioTracks()[0]
        if (rawTrack) rawTrack.contentHint = 'speech'

        this.localStream = await this.createProcessedStream(raw)

        const localTrack = this.localStream.getAudioTracks()[0]
        if (localTrack) localTrack.contentHint = 'speech'

        if (this.processedContext && this.processedContext.state === 'suspended') {
          await this.processedContext.resume().catch(() => { })
        }

        this.startSilenceMonitor()

        const me = useAppStore.getState().currentUser
        if (me && this.rawStream && !this.dfNode) this.setupVAD(this.rawStream, me.id, true)

        return true
      } catch (e) {
        throw new Error(`MIC_ACCESS_FAILED: ${(e as Error).message}`)
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
          }
        }
      } catch (e) {
        throw e 
      }
    }
  }

  public stopLocalStream() {
    const me = useAppStore.getState().currentUser
    if (me) this.clearVAD(me.id)
    this.stopSilenceMonitor()
    if (this.localStream) { this.localStream.getTracks().forEach(t => t.stop()); this.localStream = null }
    if (this.rawStream) { this.rawStream.getTracks().forEach(t => t.stop()); this.rawStream = null }
    this.cleanupProcessedStream()
    this.leaveAll()
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
            const toastMsg = i18n.t('toasts.micNotHearing', 'Вас не слышно, проверьте микрофон');
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
    if (this.dfNode) {
      this.dfNode.port.postMessage({ type: 'setConfig', isMuted: isMuted })
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
      gainNode.gain.value = Math.max(0, Math.min(2, (this.outputVolume / 100) * (volume / 100)))
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
    this.outputCompressor = this.outputMixContext.createDynamicsCompressor()

    
    this.outputCompressor.threshold.value = -1.0
    this.outputCompressor.knee.value = 0
    this.outputCompressor.ratio.value = 20
    this.outputCompressor.attack.value = 0.001
    this.outputCompressor.release.value = 0.100

    const dest = this.outputMixContext.createMediaStreamDestination()
    this.outputCompressor.connect(dest)

    this.mixAudioElement = new Audio()
    this.mixAudioElement.autoplay = true
    this.mixAudioElement.srcObject = dest.stream
    this.mixAudioElement.muted = this.isDeafened
    if (this.currentOutputDeviceId !== 'default' && typeof (this.mixAudioElement as any).setSinkId === 'function') {
      (this.mixAudioElement as any).setSinkId(this.currentOutputDeviceId).catch(() => { })
    }
    this.mixAudioElement.play().catch(err => {
      console.warn('[WebRTC] mixAudioElement play failed:', err)
    })
  }

  private setupPeerHandlers(pc: RTCPeerConnection, userId: string) {
    pc.ontrack = (event) => {
      const stream = event.streams[0] || new MediaStream([event.track])
      this.initOutputMixer()

      if (event.track.kind === 'video') {
        useAppStore.getState().setRemoteVideoStream(userId, stream)
        return
      }

      const hasVideo = stream && stream.getVideoTracks().length > 0

      if (hasVideo) {
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
          try { this.streamGainNodes.get(userId)?.disconnect() } catch { }
        }

        const source = this.outputMixContext!.createMediaStreamSource(stream)
        const gain = this.outputMixContext!.createGain()
        source.connect(gain)
        gain.connect(this.outputCompressor!)

        this.streamSourceNodes.set(userId, source)
        this.streamGainNodes.set(userId, gain)
        this.updateRemoteStreamVolume(userId)
      } else {
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
        }

        const source = this.outputMixContext!.createMediaStreamSource(stream)
        const gain = this.outputMixContext!.createGain()
        source.connect(gain)
        gain.connect(this.outputCompressor!)

        this.userSourceNodes.set(userId, source)
        this.userGainNodes.set(userId, gain)
        this.updateRemoteVolume(userId)
      }
    }

    pc.onicecandidate = (e) => {
      if (e.candidate) signalRService.sendIceCandidate(userId, JSON.stringify(e.candidate))
    }

    const checkState = () => {
      const st = pc.connectionState
      const iceSt = pc.iceConnectionState

      if (st === 'connected' || iceSt === 'connected' || iceSt === 'completed') {
        useAppStore.getState().setWebRTCConnectionStatus(userId, true)
        this.clearIceTimeout(userId)
        this.retryCount.delete(userId)
      } else if (st === 'failed' || iceSt === 'failed') {
        useAppStore.getState().setWebRTCConnectionStatus(userId, false)
        this.attemptRenegotiation(userId)
      } else if (st === 'disconnected' || iceSt === 'disconnected') {
        useAppStore.getState().setWebRTCConnectionStatus(userId, false)
        const existingTimer = this.dcTimers.get(userId)
        if (!existingTimer) {
          const t = setTimeout(() => {
            if (pc.connectionState === 'disconnected' || pc.iceConnectionState === 'disconnected') {
              this.attemptRenegotiation(userId)
            }
            this.dcTimers.delete(userId)
          }, 5000)
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
        this.attemptRenegotiation(userId)
      }
    }, WebRTCManager.ICE_TIMEOUT_MS)
    this.iceTimeoutTimers.set(userId, timer)
  }

  private clearIceTimeout(userId: string) {
    const t = this.iceTimeoutTimers.get(userId)
    if (t) { clearTimeout(t); this.iceTimeoutTimers.delete(userId) }
  }

  private attemptRenegotiation(userId: string) {
    const count = this.retryCount.get(userId) ?? 0
    if (count >= WebRTCManager.MAX_ICE_RETRIES) {
      this.disconnectFromPeer(userId)
      return
    }
    const nextCount = count + 1

    this.disconnectFromPeer(userId)
    this.retryCount.set(userId, nextCount)

    const me = useAppStore.getState().currentUser?.id
    if (me && me < userId) {
      this.connectToPeer(userId)
    }
  }

  public updateRemoteStreamVolume(userId: string) {
    const store = useAppStore.getState()
    const vol = store.streamVolumes[userId] ?? 100
    const gainNode = this.streamGainNodes.get(userId)
    if (gainNode) {
      gainNode.gain.value = Math.max(0, Math.min(2, (this.outputVolume / 100) * (vol / 100)))
    }
  }

  public setStreamVolumeRealtime(userId: string, volume: number) {
    const gainNode = this.streamGainNodes.get(userId)
    if (gainNode) {
      gainNode.gain.value = Math.max(0, Math.min(2, (this.outputVolume / 100) * (volume / 100)))
    }
  }

  public async startScreenShare(sourceId: string, quality: '1080p' | '720p') {
    this.currentStreamQuality = quality
    if (this.localVideoStream) {
      this.stopScreenShare()
    }
    const isScreen = sourceId.startsWith('screen')
    const width = quality === '1080p' ? 1920 : 1280
    const height = quality === '1080p' ? 1080 : 720
    const frameRate = quality === '1080p' ? 45 : 30

    try {
      const constraints: MediaStreamConstraints = {
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

      if (isScreen) {
        constraints.audio = {
          mandatory: {
            chromeMediaSource: 'desktop',
            chromeMediaSourceId: sourceId
          }
        } as any
      }

      const stream = await navigator.mediaDevices.getUserMedia(constraints)

      this.localVideoStream = stream
      const videoTrack = stream.getVideoTracks()[0]
      const audioTrack = stream.getAudioTracks()[0]

      if (videoTrack) {
        videoTrack.contentHint = 'motion'
      }

      if (audioTrack && this.processedContext) {
        const screenAudioSource = this.processedContext.createMediaStreamSource(new MediaStream([audioTrack]))
        const screenGainNode = this.processedContext.createGain()
        screenGainNode.gain.value = 0.8
        screenAudioSource.connect(screenGainNode)
        screenGainNode.connect(this.processedContext.destination)
      }

      for (const [userId, pc] of this.peerConnections.entries()) {
        if (videoTrack) pc.addTrack(videoTrack, stream)
        if (audioTrack) pc.addTrack(audioTrack, stream)
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
    if (this.localVideoStream) {
      this.localVideoStream.getTracks().forEach(track => {
        track.enabled = false
        track.stop()
      })
      this.localVideoStream = null
    }
    for (const [userId, pc] of this.peerConnections.entries()) {
      const senders = pc.getSenders()
      senders.forEach(sender => {
        if (sender.track && (sender.track.kind === 'video' || (sender.track.kind === 'audio' && sender.track !== this.localStream?.getAudioTracks()[0]))) {
          pc.removeTrack(sender)
        }
      })
      this.renegotiatePeer(pc, userId).catch(() => { })
    }
  }

  private async renegotiatePeer(pc: RTCPeerConnection, userId: string) {
    try {
      const offer = await pc.createOffer()
      const optimizedSDP = optimizeSDP(offer.sdp!)
      await pc.setLocalDescription({ type: 'offer', sdp: optimizedSDP })
      signalRService.sendWebRTCOffer(userId, JSON.stringify(pc.localDescription))
    } catch (e) {
      console.error(e)
    }
  }

  private startStatsMonitoring() {
    if (this.statsInterval) clearInterval(this.statsInterval)
    this.statsInterval = setInterval(async () => {
      for (const [userId, pc] of this.peerConnections.entries()) {
        if (pc.connectionState !== 'connected') continue
        try {
          const stats = await pc.getStats()
          let packetsLost = 0
          let rtt = 0
          let framesDropped = 0

          stats.forEach(report => {
            if (report.type === 'remote-inbound-rtp' && report.kind === 'video') {
              packetsLost = report.packetsLost || 0
              rtt = report.roundTripTime || 0
            }
            if (report.type === 'outbound-rtp' && report.kind === 'video') {
              framesDropped = report.framesDropped || 0
            }
          })

          const sender = pc.getSenders().find(s => s.track?.kind === 'video')
          if (sender) {
            const params = sender.getParameters()
            if (!params.encodings || params.encodings.length === 0) params.encodings = [{}]
            let changed = false

            const is1080p = this.currentStreamQuality === '1080p'
            const normalBitrate = is1080p ? 4500000 : 2500000
            const normalFramerate = is1080p ? 45 : 30

            if (params.encodings[0].priority !== 'medium') {
              params.encodings[0].priority = 'medium'
              changed = true
            }

            if (packetsLost > 5 || rtt > 0.25) {
              if (params.encodings[0].scaleResolutionDownBy !== 3.0) {
                params.encodings[0].scaleResolutionDownBy = 3.0
                params.encodings[0].maxBitrate = 600000
                params.encodings[0].maxFramerate = 15
                changed = true
              }
            } else if (packetsLost > 2 || rtt > 0.15) {
              if (params.encodings[0].scaleResolutionDownBy !== 2.0) {
                params.encodings[0].scaleResolutionDownBy = 2.0
                params.encodings[0].maxBitrate = 1200000
                params.encodings[0].maxFramerate = 25
                changed = true
              }
            } else {
              if (params.encodings[0].scaleResolutionDownBy !== 1.0 || params.encodings[0].maxBitrate !== normalBitrate || params.encodings[0].maxFramerate !== normalFramerate) {
                params.encodings[0].scaleResolutionDownBy = 1.0
                params.encodings[0].maxBitrate = normalBitrate
                params.encodings[0].maxFramerate = normalFramerate
                changed = true
              }
            }

            if (changed) {
              await sender.setParameters(params)
            }
          }

          if (framesDropped > 50) {
            const store = useAppStore.getState()
            const toastMsg = i18n.t('toasts.streamPerfIssue', 'Проблемы с производительностью, рекомендуется снизить качество')
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

  public async connectToPeer(userId: string) {
    if (this.peerConnections.has(userId)) return

    const pc = new RTCPeerConnection(this.config)
    this.peerConnections.set(userId, pc)

    if (this.localStream) {
      this.localStream.getTracks().forEach(track => {
        pc.addTrack(track, this.localStream!)
      })
    }
    if (this.localVideoStream) {
      this.localVideoStream.getTracks().forEach(track => {
        pc.addTrack(track, this.localVideoStream!)
      })
    }

    this.setupPeerHandlers(pc, userId)
    this.startIceTimeout(userId)

    try {
      const offer = await pc.createOffer()
      const optimizedSDP = optimizeSDP(offer.sdp!)
      await pc.setLocalDescription({ type: 'offer', sdp: optimizedSDP })
      signalRService.sendWebRTCOffer(userId, JSON.stringify(pc.localDescription))
    } catch (e) {
      console.error('[WebRTC] connectToPeer failed', e)
      this.disconnectFromPeer(userId)
    }
  }

  public async handleOffer(senderId: string, offerStr: string) {
    const store = useAppStore.getState()
    const isIncomingFromSender = store.incomingCall && store.incomingCall.callerId === senderId
    const isActiveCallWithSender = store.currentCallUser && store.currentCallUser.id === senderId
    if (!store.currentChannelId && !isIncomingFromSender && !isActiveCallWithSender) {
      const callStatus = store.callStatus
      if (callStatus !== 'connected') return
    }

    let pc = this.peerConnections.get(senderId)
    if (!pc) {
      pc = new RTCPeerConnection(this.config)
      this.peerConnections.set(senderId, pc)

      if (this.localStream) {
        this.localStream.getTracks().forEach(track => {
          pc!.addTrack(track, this.localStream!)
        })
      }
      if (this.localVideoStream) {
        this.localVideoStream.getTracks().forEach(track => {
          pc!.addTrack(track, this.localVideoStream!)
        })
      }

      this.setupPeerHandlers(pc, senderId)
      this.startIceTimeout(senderId)
    }

    try {
      const offer = JSON.parse(offerStr)
      offer.sdp = optimizeSDP(offer.sdp)
      await pc.setRemoteDescription(new RTCSessionDescription(offer))
      const answer = await pc.createAnswer()
      const optimizedAnswerSDP = optimizeSDP(answer.sdp!)
      await pc.setLocalDescription({ type: 'answer', sdp: optimizedAnswerSDP })
      await this.drainPendingCandidates(senderId)
      signalRService.sendWebRTCAnswer(senderId, JSON.stringify(pc.localDescription))
    } catch (e) {
      console.error('[WebRTC] handleOffer failed', e)
      this.disconnectFromPeer(senderId)
    }
  }

  public async handleAnswer(senderId: string, answerStr: string) {
    const pc = this.peerConnections.get(senderId)
    if (pc) {
      try {
        const answer = JSON.parse(answerStr)
        answer.sdp = optimizeSDP(answer.sdp)
        await pc.setRemoteDescription(new RTCSessionDescription(answer))
        await this.drainPendingCandidates(senderId)
      } catch (e) {
        console.error('[WebRTC] handleAnswer failed', e)
      }
    }
  }

  public async handleIceCandidate(senderId: string, candidateStr: string) {
    const pc = this.peerConnections.get(senderId)
    let candidate: RTCIceCandidateInit
    try { candidate = JSON.parse(candidateStr) } catch { return }

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

  public disconnectFromPeer(userId: string) {
    useAppStore.getState().setWebRTCConnectionStatus(userId, false)

    this.clearIceTimeout(userId)
    this.retryCount.delete(userId)
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

    const streamAudio = this.streamAudioElements.get(userId)
    if (streamAudio) { streamAudio.pause(); streamAudio.srcObject = null; this.streamAudioElements.delete(userId) }

    const streamSource = this.streamSourceNodes.get(userId)
    if (streamSource) { try { streamSource.disconnect() } catch { }; this.streamSourceNodes.delete(userId) }

    const streamGain = this.streamGainNodes.get(userId)
    if (streamGain) { try { streamGain.disconnect() } catch { }; this.streamGainNodes.delete(userId) }

    useAppStore.getState().setRemoteVideoStream(userId, null)

    this.pendingCandidates.delete(userId)
    this.clearVAD(userId)
  }

  public cleanupRemoteStream(userId: string) {
    const streamAudio = this.streamAudioElements.get(userId)
    if (streamAudio) { streamAudio.pause(); streamAudio.srcObject = null; this.streamAudioElements.delete(userId) }

    const streamSource = this.streamSourceNodes.get(userId)
    if (streamSource) { try { streamSource.disconnect() } catch { }; this.streamSourceNodes.delete(userId) }

    const streamGain = this.streamGainNodes.get(userId)
    if (streamGain) { try { streamGain.disconnect() } catch { }; this.streamGainNodes.delete(userId) }

    useAppStore.getState().setRemoteVideoStream(userId, null)
  }

  public leaveAll() {
    this.peerConnections.forEach((_, uid) => this.disconnectFromPeer(uid))
  }
}

export const webrtc = new WebRTCManager()