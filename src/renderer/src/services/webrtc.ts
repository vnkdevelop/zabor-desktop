import { signalRService } from './signalr'
import { useAppStore } from '../store/useAppStore'
import i18n from '../i18n'
import processorUrl from './deepfilter-processor?worker&url'

type SpeakingEntry = {
  timer: NodeJS.Timeout
  stream: MediaStream
  /** Аудио-узлы VAD-цепочки для корректного disconnect при очистке */
  nodes: AudioNode[]
}

export class WebRTCManager {
  private localStream: MediaStream | null = null
  private rawStream: MediaStream | null = null

  private peerConnections: Map<string, RTCPeerConnection> = new Map()
  private audioElements: Map<string, HTMLAudioElement> = new Map()
  /** Буфер ICE-кандидатов, пришедших до setRemoteDescription */
  private pendingCandidates: Map<string, RTCIceCandidateInit[]> = new Map()
  /** Таймеры переподключения — хранятся на уровне класса чтобы отменять при явном дисконнекте */
  private dcTimers: Map<string, NodeJS.Timeout> = new Map()
  /** Таймеры ICE-таймаута для renegotiation */
  private iceTimeoutTimers: Map<string, NodeJS.Timeout> = new Map()
  /** Счётчик попыток renegotiation на каждого пира */
  private retryCount: Map<string, number> = new Map()
  /** Максимум попыток renegotiation */
  private static readonly MAX_ICE_RETRIES = 2
  /** Таймаут ICE-соединения (мс) — если за это время не `connected`, делаем renegotiation */
  private static readonly ICE_TIMEOUT_MS = 15000

  private currentDeviceId = 'default'
  private currentOutputDeviceId = 'default'
  private noiseSuppression = true

  private inputVolume = 100
  private outputVolume = 100
  private isDeafened = false

  private processedContext: AudioContext | null = null
  private processedSource: MediaStreamAudioSourceNode | null = null
  private inputGainNode: GainNode | null = null
  private dfNode: AudioWorkletNode | null = null

  private vadContext: AudioContext | null = null
  private speakingIntervals: Map<string, SpeakingEntry> = new Map()

  // ── Output Mixer ──────────────────────────────────────────────
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
      // fallbacks
      { urls: 'turn:150.241.64.108:3478?transport=udp', username: 'zabor', credential: 'mvtxbJo45sc8_turn' },
      { urls: 'turn:150.241.64.108:3478?transport=tcp', username: 'zabor', credential: 'mvtxbJo45sc8_turn' }
    ],
    bundlePolicy: 'max-bundle'
  }

  // ── Audio Pipeline (Строгий порядок DSP по ТЗ) ────────────────

  private async createProcessedStream(rawStream: MediaStream): Promise<MediaStream> {
    this.cleanupProcessedStream()

    // 1. Частота дискретизации строго 48 кГц
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

    // 4. Компрессор динамического диапазона (Строго ПОСЛЕ нейросети)
    const compressor = ctx.createDynamicsCompressor()
    compressor.threshold.value = -24
    compressor.knee.value = 10
    compressor.ratio.value = 3
    compressor.attack.value = 0.005
    compressor.release.value = 0.150

    // 5. Параметрический эквалайзер
    const highpass = ctx.createBiquadFilter()
    highpass.type = 'highpass'
    highpass.frequency.value = 80 // Срез гула ниже 80 Гц

    const peaking = ctx.createBiquadFilter()
    peaking.type = 'peaking'
    peaking.frequency.value = 3000 // Подъем для разборчивости речи
    peaking.Q.value = 1.0
    peaking.gain.value = 2 // Легкий подъем

    // 6. Brickwall Limiter (Защита канала от клиппинга)
    const limiter = ctx.createDynamicsCompressor()
    limiter.threshold.value = -0.5
    limiter.knee.value = 0
    limiter.ratio.value = 20
    limiter.attack.value = 0.001
    limiter.release.value = 0.050

    // 7. Output Gain
    const inputGain = ctx.createGain()
    inputGain.gain.value = Math.max(0, Math.min(2, this.inputVolume / 100))
    this.inputGainNode = inputGain

    // Сборка графа DSP
    let currentNode: AudioNode = source

    // 2. Ядро DeepFilterNet3 + 3. Шумовой затвор
    if (this.dfNode) {
      const store = useAppStore.getState()
      const isMuted = store.currentUser?.isMuted || store.currentUser?.isServerMuted || false
      this.dfNode.port.postMessage({
        type: 'setConfig',
        noiseSuppression: this.noiseSuppression,
        isMuted: isMuted
      })
      currentNode.connect(this.dfNode)
      currentNode = this.dfNode
    }

    currentNode.connect(compressor)
    compressor.connect(highpass)
    highpass.connect(peaking)
    peaking.connect(limiter)
    limiter.connect(inputGain)
    inputGain.connect(destination)

    return destination.stream
  }

  private cleanupProcessedStream() {
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
    if (this.inputGainNode) {
      this.inputGainNode.gain.value = Math.max(0, Math.min(2, volume / 100))
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
    gainNode.gain.value = Math.max(0, Math.min(2, (this.outputVolume / 100) * (userVol / 100)))
  }

  public setNoiseSuppression(enabled: boolean) {
    this.noiseSuppression = enabled
    if (this.dfNode) {
      this.dfNode.port.postMessage({ type: 'setConfig', noiseSuppression: enabled })
    }
  }

  // ── VAD (Резервный Fallback) ──────────────────────────────────

  private setupVAD(stream: MediaStream, userId: string, isLocal: boolean) {
    this.clearVAD(userId)

    try {
      if (!this.vadContext || this.vadContext.state === 'closed') {
        this.vadContext = new AudioContext({ latencyHint: 'playback' })
      }
      if (this.vadContext.state === 'suspended') this.vadContext.resume().catch(() => { })

      const cloned = new MediaStream(stream.getAudioTracks().map(t => t.clone()))
      const source = this.vadContext.createMediaStreamSource(cloned)

      const bp1 = this.vadContext.createBiquadFilter()
      bp1.type = 'highpass'; bp1.frequency.value = 85; bp1.Q.value = 0.5

      const bp2 = this.vadContext.createBiquadFilter()
      bp2.type = 'lowpass'; bp2.frequency.value = 8000; bp2.Q.value = 0.5

      const analyser = this.vadContext.createAnalyser()
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

      const avgTh = isLocal ? 4 : 2
      const peakTh = isLocal ? 10 : 8

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
      this.speakingIntervals.set(userId, { timer, stream: cloned, nodes: vadNodes })
    } catch (e) { console.error('[VAD] setup failed', e) }
  }

  private clearVAD(userId: string) {
    const entry = this.speakingIntervals.get(userId)
    if (entry) {
      clearInterval(entry.timer)
      entry.nodes.forEach(n => { try { n.disconnect() } catch { } })
      entry.stream.getTracks().forEach(t => { t.stop(); t.enabled = false })
      this.speakingIntervals.delete(userId)
    }
    useAppStore.getState().setSpeakingStatus(userId, false)

    if (this.speakingIntervals.size === 0 && this.vadContext && this.vadContext.state !== 'closed') {
      this.vadContext.close().catch(() => { })
      this.vadContext = null
    }
  }

  // ── Devices ───────────────────────────────────────────────────

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

  // ── Local Stream ──────────────────────────────────────────────

  public async startLocalStream(deviceId?: string, useNS?: boolean, forceRestart = false): Promise<boolean> {
    if (deviceId !== undefined) this.currentDeviceId = deviceId
    if (useNS !== undefined) this.noiseSuppression = useNS

    if (!forceRestart && this.localStream && this.localStream.getAudioTracks().length > 0 && this.localStream.getAudioTracks().every(t => t.readyState === 'live')) {
      return true
    }

    try {
      if (this.rawStream) { this.rawStream.getTracks().forEach(t => t.stop()); this.rawStream = null }
      if (this.localStream) { this.localStream.getTracks().forEach(t => t.stop()); this.localStream = null }
      this.cleanupProcessedStream()

      const raw = await navigator.mediaDevices.getUserMedia({
        audio: {
          deviceId: this.currentDeviceId !== 'default' ? { exact: this.currentDeviceId } : undefined,
          sampleRate: 48000,
          channelCount: 1,
          echoCancellation: true, // WebRTC AEC - строго до нейросети
          noiseSuppression: !this.noiseSuppression, // Отключаем браузерный, если включен DF3
          autoGainControl: false,  // ВАЖНО: Выключаем для предотвращения заглатывания звука
          // @ts-ignore
          googHighpassFilter: false,
          googEchoCancellation2: false, // Агрессивный AEC выключен
          googAudioMirroring: false
        },
        video: false
      })

      this.rawStream = raw
      const rawTrack = raw.getAudioTracks()[0]
      if (rawTrack) rawTrack.contentHint = 'speech'

      this.localStream = await this.createProcessedStream(raw)

      const localTrack = this.localStream.getAudioTracks()[0]
      if (localTrack) localTrack.contentHint = 'speech'

      if (this.processedContext && this.processedContext.state === 'suspended') {
        await this.processedContext.resume().catch(() => { })
      }

      const me = useAppStore.getState().currentUser
      if (me && this.rawStream && !this.dfNode) this.setupVAD(this.rawStream, me.id, true)

      return true
    } catch (e) {
      console.error('[WebRTC] Mic error:', e)
      // Пробрасываем ошибку для слоя-фасада (Hot-swap отказоустойчивость)
      throw new Error(`MIC_ACCESS_FAILED: ${(e as Error).message}`)
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
        throw e // Проброс для обработки в UI
      }
    }
  }

  public stopLocalStream() {
    const me = useAppStore.getState().currentUser
    if (me) this.clearVAD(me.id)
    if (this.localStream) { this.localStream.getTracks().forEach(t => t.stop()); this.localStream = null }
    if (this.rawStream) { this.rawStream.getTracks().forEach(t => t.stop()); this.rawStream = null }
    this.cleanupProcessedStream()
    this.leaveAll()
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

  // ── Peer Connections ──────────────────────────────────────────

  private initOutputMixer() {
    if (this.outputMixContext) {
      if (this.outputMixContext.state === 'suspended') this.outputMixContext.resume().catch(() => { })
      return
    }
    this.outputMixContext = new AudioContext({ latencyHint: 'playback' })
    if (this.outputMixContext.state === 'suspended') {
      this.outputMixContext.resume().catch(() => { })
    }
    this.outputCompressor = this.outputMixContext.createDynamicsCompressor()

    // Лимитер миксера для защиты от перегруза при 10+ спикерах
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
      const remote = event.streams[0]
      this.setupVAD(remote, userId, false)

      this.initOutputMixer()

      let dummyAudio = this.audioElements.get(userId)
      if (!dummyAudio) {
        dummyAudio = new Audio()
        dummyAudio.autoplay = true
        dummyAudio.muted = true
        this.audioElements.set(userId, dummyAudio)
      }
      dummyAudio.srcObject = remote
      dummyAudio.play().catch(err => {
        console.warn(`[WebRTC] dummyAudio play failed for user ${userId}:`, err)
      })

      if (this.userSourceNodes.has(userId)) {
        try { this.userSourceNodes.get(userId)?.disconnect() } catch { }
        try { this.userGainNodes.get(userId)?.disconnect() } catch { }
      }

      const source = this.outputMixContext!.createMediaStreamSource(remote)
      const gain = this.outputMixContext!.createGain()

      source.connect(gain)
      gain.connect(this.outputCompressor!)

      this.userSourceNodes.set(userId, source)
      this.userGainNodes.set(userId, gain)

      this.updateRemoteVolume(userId)
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

  public async connectToPeer(userId: string) {
    if (this.peerConnections.has(userId)) return

    const pc = new RTCPeerConnection(this.config)
    this.peerConnections.set(userId, pc)

    if (this.localStream) {
      this.localStream.getTracks().forEach(track => {
        pc.addTrack(track, this.localStream!)
      })
    }

    this.setupPeerHandlers(pc, userId)
    this.startIceTimeout(userId)

    try {
      const offer = await pc.createOffer()
      await pc.setLocalDescription(offer)
      signalRService.sendWebRTCOffer(userId, JSON.stringify(pc.localDescription))
    } catch (e) {
      console.error('[WebRTC] connectToPeer failed', e)
      this.disconnectFromPeer(userId)
    }
  }

  public async handleOffer(senderId: string, offerStr: string) {
    const store = useAppStore.getState()
    if (!store.currentChannelId && store.currentCallUser?.id !== senderId) {
      const callStatus = store.callStatus
      if (callStatus !== 'connected') return
    }
    if (this.peerConnections.has(senderId)) {
      this.disconnectFromPeer(senderId)
    }

    const pc = new RTCPeerConnection(this.config)
    this.peerConnections.set(senderId, pc)

    if (this.localStream) {
      this.localStream.getTracks().forEach(track => {
        pc.addTrack(track, this.localStream!)
      })
    }

    this.setupPeerHandlers(pc, senderId)
    this.startIceTimeout(senderId)

    try {
      await pc.setRemoteDescription(new RTCSessionDescription(JSON.parse(offerStr)))
      const answer = await pc.createAnswer()
      await pc.setLocalDescription(answer)
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
        await pc.setRemoteDescription(new RTCSessionDescription(JSON.parse(answerStr)))
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

    this.pendingCandidates.delete(userId)
    this.clearVAD(userId)
  }

  public leaveAll() {
    this.peerConnections.forEach((_, uid) => this.disconnectFromPeer(uid))
  }
}

export const webrtc = new WebRTCManager()