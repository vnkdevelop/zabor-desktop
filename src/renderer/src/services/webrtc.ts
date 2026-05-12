import { signalRService } from './signalr'
import { useAppStore } from '../store/useAppStore'
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
      { urls: 'turn:openrelay.metered.ca:80', username: 'openrelayproject', credential: 'openrelayproject' },
      { urls: 'turn:openrelay.metered.ca:443', username: 'openrelayproject', credential: 'openrelayproject' },
      { urls: 'turn:turn.bistri.com:80', username: 'homeo', credential: 'homeo' }
    ],
    bundlePolicy: 'max-bundle'
  }

  // ── Audio Pipeline (минималистичный) ──────────────────────────

  private async createProcessedStream(rawStream: MediaStream): Promise<MediaStream> {
    this.cleanupProcessedStream()

    const ctx = new AudioContext({ sampleRate: 48000, latencyHint: 'interactive' })
    this.processedContext = ctx
    const destination = ctx.createMediaStreamDestination()

    let dfNode: AudioWorkletNode | null = null
    try {
      await ctx.audioWorklet.addModule(processorUrl)
      dfNode = new AudioWorkletNode(ctx, 'deepfilter-processor')
      this.dfNode = dfNode

      const me = useAppStore.getState().currentUser
      dfNode.port.onmessage = (event) => {
        if (event.data.type === 'vad' && me) {
           const store = useAppStore.getState()
           if (store.currentUser?.isMuted || store.currentUser?.isServerMuted) {
             return
           }
           store.setSpeakingStatus(me.id, event.data.isSpeaking)
           signalRService.setSpeakingState(event.data.isSpeaking)
        }
      }
    } catch (e) {
      console.warn('[WebRTC] Failed to load deepfilter-processor.js, running without it.', e)
    }

    const source = ctx.createMediaStreamSource(rawStream)
    this.processedSource = source

    // 1. Source -> 2. DeepFilterNet -> 3. High-pass (80Hz)
    const highpass = ctx.createBiquadFilter()
    highpass.type = 'highpass'
    highpass.frequency.value = 80

    // 4. Peaking Filter (350Hz, Q=1.4, -3dB) - Remove boxiness
    const peaking = ctx.createBiquadFilter()
    peaking.type = 'peaking'
    peaking.frequency.value = 350
    peaking.Q.value = 1.4
    peaking.gain.value = -3

    // 5. High-Shelf Filter (8kHz, +3dB) - Add air
    const highShelf = ctx.createBiquadFilter()
    highShelf.type = 'highshelf'
    highShelf.frequency.value = 8000
    highShelf.gain.value = 3

    // 6. Soft Compressor
    const compressor = ctx.createDynamicsCompressor()
    compressor.threshold.value = -24
    compressor.knee.value = 10
    compressor.ratio.value = 3
    compressor.attack.value = 0.005
    compressor.release.value = 0.150

    // 7. Output Gain
    const inputGain = ctx.createGain()
    inputGain.gain.value = Math.max(0, Math.min(2, this.inputVolume / 100))
    this.inputGainNode = inputGain

    // 8. Soft Limiter (Мягкий лимитер для защиты от перегруза)
    const limiter = ctx.createDynamicsCompressor()
    limiter.threshold.value = -3
    limiter.knee.value = 5
    limiter.ratio.value = 20
    limiter.attack.value = 0.002
    limiter.release.value = 0.100

    // Connections
    let currentNode: AudioNode = source
    
    // Always route through processor for VAD and Noise Gate
    if (this.dfNode) {
      this.dfNode.port.postMessage({ type: 'setConfig', noiseSuppression: this.noiseSuppression })
      currentNode.connect(this.dfNode)
      currentNode = this.dfNode
    }

    currentNode.connect(highpass)
    highpass.connect(peaking)
    peaking.connect(highShelf)
    highShelf.connect(compressor)
    compressor.connect(inputGain)
    inputGain.connect(limiter)
    limiter.connect(destination)

    return destination.stream
  }

  private cleanupProcessedStream() {

    if (this.dfNode) {
      this.dfNode.port.close()
      this.dfNode.disconnect()
      this.dfNode = null
    }

    if (this.processedContext && this.processedContext.state !== 'closed') {
      this.processedContext.close().catch(() => {})
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
    gainNode.gain.value = Math.max(0, Math.min(4, (this.outputVolume / 100) * (userVol / 100)))
  }

  public setNoiseSuppression(enabled: boolean) {
    this.noiseSuppression = enabled
    if (this.dfNode) {
      this.dfNode.port.postMessage({ type: 'setConfig', noiseSuppression: enabled })
    }
    if (this.rawStream) {
      const track = this.rawStream.getAudioTracks()[0]
      if (track) {
        track.applyConstraints({ noiseSuppression: !enabled }).catch(() => {})
      }
    }
  }

  // ── VAD ───────────────────────────────────────────────────────

  private setupVAD(stream: MediaStream, userId: string, isLocal: boolean) {
    this.clearVAD(userId)

    try {
      if (!this.vadContext || this.vadContext.state === 'closed') {
        this.vadContext = new AudioContext({ latencyHint: 'playback' })
      }
      if (this.vadContext.state === 'suspended') this.vadContext.resume().catch(() => {})

      const cloned = new MediaStream(stream.getAudioTracks().map(t => t.clone()))
      const source = this.vadContext.createMediaStreamSource(cloned)

      const bp1 = this.vadContext.createBiquadFilter()
      bp1.type = 'highpass'; bp1.frequency.value = 85; bp1.Q.value = 0.5

      const bp2 = this.vadContext.createBiquadFilter()
      bp2.type = 'lowpass'; bp2.frequency.value = 8000; bp2.Q.value = 0.5

      const analyser = this.vadContext.createAnalyser()
      analyser.fftSize = 512
      analyser.smoothingTimeConstant = 0.6  // 0.3 → 0.6: лучше улавливает тихую речь

      source.connect(bp1)
      bp1.connect(bp2)
      bp2.connect(analyser)
      // Сохраняем узлы для явного disconnect в clearVAD (иначе утечка памяти в vadContext)
      const vadNodes: AudioNode[] = [source, bp1, bp2, analyser]

      const buf = new Uint8Array(analyser.fftSize)
      let lastVoice = 0
      let wasSpeaking = false
      let voiceFrames = 0
      let silenceFrames = 0     // Счётчик для теста "микрофон мёртв"
      let vadSilenceFrames = 0  // Отдельный счётчик для гистерезиса VAD
      let hasWarnedSilence = false

      const avgTh  = isLocal ? 4 : 2   // Повышаем порог для уменьшения чувствительности
      const peakTh = isLocal ? 10 : 8  // Повышаем порог для уменьшения чувствительности

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
            store.setSystemToast('Вас не слышно, проверьте микрофон')
            setTimeout(() => {
              const currentStore = useAppStore.getState()
              if (currentStore.systemToast === 'Вас не слышно, проверьте микрофон') {
                currentStore.setSystemToast(null)
              }
            }, 4000)
            hasWarnedSilence = true
          }
        }

        // Гистерезис VAD: voiceFrames сбрасывается только после ~10 тихих фреймов (300ms),
        // а не на каждом тихом фрейме. Устраняет ложные отключения на паузах между словами.
        const isVoice = avg >= avgTh || peak >= peakTh
        if (isVoice) {
          voiceFrames++
          vadSilenceFrames = 0
        } else {
          vadSilenceFrames++
          if (vadSilenceFrames >= 6) voiceFrames = 0  // 180ms тишины до сброса (было 300ms)
        }
        if (voiceFrames >= 2) lastVoice = Date.now()

        const speaking = (Date.now() - lastVoice) < 400  // hold 400ms
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
      // Отключаем узлы от vadContext — без этого они остаются в памяти до закрытия контекста
      entry.nodes.forEach(n => { try { n.disconnect() } catch {} })
      entry.stream.getTracks().forEach(t => { t.stop(); t.enabled = false })
      this.speakingIntervals.delete(userId)
    }
    useAppStore.getState().setSpeakingStatus(userId, false)

    // Закрываем vadContext когда больше нет активных VAD-сессий
    if (this.speakingIntervals.size === 0 && this.vadContext && this.vadContext.state !== 'closed') {
      this.vadContext.close().catch(() => {})
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
      (this.mixAudioElement as any).setSinkId(deviceId).catch(() => {})
    }
  }

  // ── Local Stream ──────────────────────────────────────────────

  public async startLocalStream(deviceId?: string, useNS?: boolean): Promise<boolean> {
    if (deviceId !== undefined) this.currentDeviceId = deviceId
    if (useNS !== undefined) this.noiseSuppression = useNS

    try {
      if (this.rawStream) { this.rawStream.getTracks().forEach(t => t.stop()); this.rawStream = null }
      if (this.localStream) { this.localStream.getTracks().forEach(t => t.stop()); this.localStream = null }
      this.cleanupProcessedStream()

      const raw = await navigator.mediaDevices.getUserMedia({
        audio: {
          deviceId: this.currentDeviceId !== 'default' ? { exact: this.currentDeviceId } : undefined,
          sampleRate: 48000,
          channelCount: 1,
          echoCancellation: true, // Обязательно, чтобы не было эха
          noiseSuppression: !this.noiseSuppression, // Если нейросеть выключена, включаем слабое встроенное
          autoGainControl: true, // Включаем AGC браузера для предотвращения перегруза (клиппинга) при громких звуках
          // @ts-ignore - Скрытые настройки Chromium
          googHighpassFilter: false, 
          
          googEchoCancellation2: true,
          
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

      const me = useAppStore.getState().currentUser
      // JS VAD запускается только если DeepFilterNet не загружен.
      // Когда dfNode активен с WASM, он сам отправляет VAD через port.onmessage.
      if (me && this.rawStream && !this.dfNode) this.setupVAD(this.rawStream, me.id, true)

      return true
    } catch (e) { 
      console.warn('[WebRTC] Mic error, continuing as listener:', e)
      return true 
    }
  }

  public async updateSettings(deviceId: string, useNS: boolean) {
    this.currentDeviceId = deviceId
    this.noiseSuppression = useNS

    if (this.localStream) {
      await this.startLocalStream(deviceId, useNS)
      for (const pc of this.peerConnections.values()) {
        const sender = pc.getSenders().find(s => s.track?.kind === 'audio')
        const newTrack = this.localStream?.getAudioTracks()[0]
        if (sender && newTrack) {
          await sender.replaceTrack(newTrack).catch(() => {})
        }
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
    if (this.localStream) this.localStream.getAudioTracks().forEach(t => { t.enabled = !isMuted })
    
    if (isMuted) {
      const store = useAppStore.getState()
      const me = store.currentUser
      if (me) {
        store.setSpeakingStatus(me.id, false)
        signalRService.setSpeakingState(false)
      }
    }
  }

  public setUserVolume(userId: string, volume: number) {
    useAppStore.getState().setUserVolume(userId, Math.max(0, Math.min(200, volume)))
    this.updateRemoteVolume(userId)
  }

  // ── Peer Connections ──────────────────────────────────────────

  private initOutputMixer() {
    if (this.outputMixContext) {
      if (this.outputMixContext.state === 'suspended') this.outputMixContext.resume().catch(() => {})
      return
    }
    this.outputMixContext = new AudioContext({ latencyHint: 'playback' })
    this.outputCompressor = this.outputMixContext.createDynamicsCompressor()
    
    // Настройки компрессора: прозрачный студийный лимитер (Transparent Safety Limiter)
    // Он не будет постоянно "сплющивать" звук, а сработает только при реальной угрозе перегруза
    this.outputCompressor.threshold.value = -6     // Срабатывает только на высоких пиках (когда голоса накладываются)
    this.outputCompressor.knee.value = 15          // Очень мягкое скругление (Soft Knee) для незаметного срабатывания
    this.outputCompressor.ratio.value = 10         // Высокое ратио, работает как защитный барьер (Limiter)
    this.outputCompressor.attack.value = 0.005     // 5мс — достаточно быстро для защиты, но сохраняет "живость" голоса
    this.outputCompressor.release.value = 0.250    // 250мс — плавное восстановление, убирает эффект "насоса" (pumping)

    const dest = this.outputMixContext.createMediaStreamDestination()
    this.outputCompressor.connect(dest)

    this.mixAudioElement = new Audio()
    this.mixAudioElement.autoplay = true
    this.mixAudioElement.srcObject = dest.stream
    this.mixAudioElement.muted = this.isDeafened
    if (this.currentOutputDeviceId !== 'default' && typeof (this.mixAudioElement as any).setSinkId === 'function') {
      (this.mixAudioElement as any).setSinkId(this.currentOutputDeviceId).catch(() => {})
    }
  }

  private setupPeerHandlers(pc: RTCPeerConnection, userId: string) {
    pc.ontrack = (event) => {
      const remote = event.streams[0]
      this.setupVAD(remote, userId, false)

      this.initOutputMixer()

      // Создаем "фиктивный" аудио элемент, чтобы браузер не убил WebRTC поток
      // И ставим его на mute (звук пойдет через AudioContext миксер)
      let dummyAudio = this.audioElements.get(userId)
      if (!dummyAudio) {
        dummyAudio = new Audio()
        dummyAudio.autoplay = true
        dummyAudio.muted = true
        this.audioElements.set(userId, dummyAudio)
      }
      dummyAudio.srcObject = remote

      // Если мы уже создавали узлы для этого юзера, очистим
      if (this.userSourceNodes.has(userId)) {
        try { this.userSourceNodes.get(userId)?.disconnect() } catch {}
        try { this.userGainNodes.get(userId)?.disconnect() } catch {}
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

  /** Таймаут ICE-подключения: если за ICE_TIMEOUT_MS не перешли в connected — renegotiation */
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

  /** Пробуем переподключиться к пиру (renegotiation). При исчерпании попыток — полный disconnect. */
  private attemptRenegotiation(userId: string) {
    const count = this.retryCount.get(userId) ?? 0
    if (count >= WebRTCManager.MAX_ICE_RETRIES) {
      this.retryCount.delete(userId)
      // Вместо дисконнекта, который оставит UI висеть навсегда, просто завершаем попытки. 
      // Состояние останется 'false', показывая "ПОДКЛЮЧЕНИЕ". 
      // При желании можно кикнуть пользователя или сбросить счётчик через время.
      return
    }
    this.retryCount.set(userId, count + 1)
    
    const oldPc = this.peerConnections.get(userId)
    if (oldPc) {
      oldPc.ontrack = null; oldPc.onicecandidate = null; oldPc.onconnectionstatechange = null; oldPc.oniceconnectionstatechange = null
      oldPc.close()
      this.peerConnections.delete(userId)
    }
    // НЕ удаляем pendingCandidates, так как они могут принадлежать к новому циклу renegotiation
    
    const me = useAppStore.getState().currentUser?.id
    // Во избежание glare-состояния (когда оба шлют offer одновременно),
    // заставляем инициировать переподключение только одного из пиров
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
    // BUGFIX: было &&, из-за чего оффер отбрасывался когда currentCallUser ещё
    // не был установлен (состояние гонки при acceptCall). Правильная проверка:
    // «нет ни канала, ни активного звонка от этого пользователя» → игнорируем
    if (!store.currentChannelId && store.currentCallUser?.id !== senderId) {
      // Дополнительная мягкая проверка: если это известный друг/юзер и мы в состоянии
      // 'connected' - возможно callUser ещё не прогрузился, даём шанс
      const callStatus = store.callStatus
      if (callStatus !== 'connected') return
    }
    let existingCandidates: RTCIceCandidateInit[] = []
    if (this.peerConnections.has(senderId)) {
      existingCandidates = this.pendingCandidates.get(senderId) ?? []
      this.disconnectFromPeer(senderId)
      // Восстанавливаем кандидаты, которые могли прийти за миллисекунды до оффера
      if (existingCandidates.length > 0) {
        this.pendingCandidates.set(senderId, existingCandidates)
      }
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
      // Сбрасываем кандидатов ТОЛЬКО ПОСЛЕ установки обоих Description
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
        // Сбрасываем ICE-кандидаты, пришедшие до установки remote description
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
      // PC ещё не создан — буферизуем кандидата
      const buf = this.pendingCandidates.get(senderId) ?? []
      buf.push(candidate)
      this.pendingCandidates.set(senderId, buf)
      return
    }

    // Кандидат пришёл до setRemoteDescription — буферизуем
    if (!pc.remoteDescription) {
      const buf = this.pendingCandidates.get(senderId) ?? []
      buf.push(candidate)
      this.pendingCandidates.set(senderId, buf)
      return
    }

    try { await pc.addIceCandidate(new RTCIceCandidate(candidate)) } catch {}
  }

  /** Сбрасываем буфер ICE-кандидатов после setRemoteDescription */
  private async drainPendingCandidates(userId: string): Promise<void> {
    const pc = this.peerConnections.get(userId)
    const candidates = this.pendingCandidates.get(userId)
    if (!pc || !candidates || candidates.length === 0) return
    this.pendingCandidates.delete(userId)
    for (const c of candidates) {
      try { await pc.addIceCandidate(new RTCIceCandidate(c)) } catch {}
    }
  }

  public disconnectFromPeer(userId: string) {
    useAppStore.getState().setWebRTCConnectionStatus(userId, false)

    // Отменяем все таймеры
    this.clearIceTimeout(userId)
    this.retryCount.delete(userId)
    const dcTimer = this.dcTimers.get(userId)
    if (dcTimer) { clearTimeout(dcTimer); this.dcTimers.delete(userId) }

    const pc = this.peerConnections.get(userId)
    if (pc) { pc.ontrack = null; pc.onicecandidate = null; pc.onconnectionstatechange = null; pc.oniceconnectionstatechange = null; pc.close(); this.peerConnections.delete(userId) }
    
    const audio = this.audioElements.get(userId)
    if (audio) { audio.pause(); audio.srcObject = null; this.audioElements.delete(userId) }
    
    const source = this.userSourceNodes.get(userId)
    if (source) { try { source.disconnect() } catch {}; this.userSourceNodes.delete(userId) }
    
    const gain = this.userGainNodes.get(userId)
    if (gain) { try { gain.disconnect() } catch {}; this.userGainNodes.delete(userId) }

    this.pendingCandidates.delete(userId)
    this.clearVAD(userId)
  }

  public leaveAll() {
    this.peerConnections.forEach((_, uid) => this.disconnectFromPeer(uid))
  }
}

export const webrtc = new WebRTCManager()