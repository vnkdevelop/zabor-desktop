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
  private readonly BUFFER_SIZE = 4800
  private readonly SAMPLE_RATE = 48000

  private denoiser: StandaloneDeepFilter | null = null
  private denoiserReady = false

  private readonly frameToProcess: Float32Array
  private readonly processedFrame: Float32Array

  private isMuted = false
  private noiseSuppression = true

  // VAD Thresholds & Smoothing
  private rmsSmoothed = 0
  private GATE_THRESHOLD_ON = 0.012  // Чувствительный порог включения
  private GATE_THRESHOLD_OFF = 0.005 // Порог удержания
  private lastVadSent = false

  private overflowCount = 0

  private readonly HOLD_FRAMES = 50 // ~500ms удержания гейта после завершения речи
  private framesSinceLastVoice = this.HOLD_FRAMES

  // VCA-эмуляция (Гибридный гейт для суммарных -60dB)
  private currentGain = 1.0
  private readonly TARGET_GAIN_ON = 1.0
  private readonly TARGET_GAIN_OFF = 0.0316 // -30 dB. В сумме с DF (-30dB) дает -60 dB

  // Экспоненциальные огибающие: Время атаки (15мс) и релиза (600мс)
  private readonly attackCoef = Math.exp(-1.0 / (this.SAMPLE_RATE * 0.015))
  private readonly releaseCoef = Math.exp(-1.0 / (this.SAMPLE_RATE * 0.60))

  // Медленная автоматическая регулировка усиления (АРУ / AGC)
  private agcGain = 1.0
  private readonly TARGET_SPEECH_RMS = 0.06 // Целевой уровень среднеквадратичного значения речи
  private readonly MAX_AGC_GAIN = 3.0       // Максимальный буст (+10дБ)
  private readonly MIN_AGC_GAIN = 0.5       // Минимальный уровень (-6дБ)
  private speechRmsAccumulator = 0
  private speechRmsCount = 0
  private attenuationLimit = 35

  constructor() {
    super()
    this.inputBuffer = new Float32Array(this.BUFFER_SIZE)
    this.outputBuffer = new Float32Array(this.BUFFER_SIZE)
    this.frameToProcess = new Float32Array(this.FRAME_SIZE)
    this.processedFrame = new Float32Array(this.FRAME_SIZE)
    this.outputWriteIndex = this.FRAME_SIZE // Pre-fill with 480 samples of silence

    this.port.onmessage = (event) => {
      if (event.data.type === 'loadWasm') {
        this.initDeepFilter()
      } else if (event.data.type === 'setConfig') {
        if (event.data.noiseSuppression !== undefined) {
          this.noiseSuppression = event.data.noiseSuppression
        }
        if (event.data.isMuted !== undefined) {
          const nextMuted = event.data.isMuted
          if (nextMuted && !this.isMuted) {
            this.inputBuffer.fill(0)
            this.outputBuffer.fill(0)
            this.inputReadIndex = 0
            this.inputWriteIndex = 0
            this.outputReadIndex = 0
            this.outputWriteIndex = this.FRAME_SIZE // Pre-fill with 480 samples of silence
            this.rmsSmoothed = 0
            this.currentGain = this.TARGET_GAIN_OFF
            this.framesSinceLastVoice = this.HOLD_FRAMES
            if (this.lastVadSent) {
              this.port.postMessage({ type: 'vad', isSpeaking: false })
              this.lastVadSent = false
            }
          }
          this.isMuted = nextMuted
        }
      } else if (event.data.type === 'setCalibratedParams') {
        if (event.data.thresholdOn !== undefined) {
          this.GATE_THRESHOLD_ON = event.data.thresholdOn
        }
        if (event.data.thresholdOff !== undefined) {
          this.GATE_THRESHOLD_OFF = event.data.thresholdOff
        }
        if (event.data.attenuationLimit !== undefined) {
          this.attenuationLimit = event.data.attenuationLimit
          if (this.denoiserReady && this.denoiser) {
            try {
              this.denoiser.setAttenuationLimit(event.data.attenuationLimit)
              console.log(`[DeepFilterProcessor] Applied calibrated attenuation limit: ${event.data.attenuationLimit}dB`)
            } catch (e) {
              console.warn('[DeepFilterProcessor] Failed to set attenuation limit:', e)
            }
          }
        }
      }
    }
    this.initDeepFilter()
  }

  private async initDeepFilter() {
    if (this.denoiserReady) return
    try {
      this.denoiser = new StandaloneDeepFilter({
        attenuationLimit: this.attenuationLimit, // Изначальный или уже калиброванный лимит
        postFilterBeta: 0.09  // Оптимальное сглаживание артефактов (песка)
      })
      await this.denoiser.initialize()
      this.denoiserReady = true
      this.port.postMessage({ type: 'ready' })
      console.log(`[DeepFilterProcessor] Neural net initialized successfully with attenuationLimit: ${this.attenuationLimit}dB`)
    } catch (e) {
      console.error('[DeepFilterProcessor] Failed to load DeepFilterNet:', e)
    }
  }

  private pushToBuffer(buffer: Float32Array, data: Float32Array, writeIndex: number, readIndex: number): number {
    const availableSpace = (readIndex - writeIndex - 1 + this.BUFFER_SIZE) % this.BUFFER_SIZE
    if (availableSpace < data.length) {
      this.overflowCount++
      if (this.overflowCount % 100 === 1) {
        console.warn(`DeepFilterProcessor: ring buffer overflow (×${this.overflowCount})`)
      }
      return writeIndex
    }
    for (let i = 0; i < data.length; i++) {
      buffer[writeIndex] = data[i]
      writeIndex = (writeIndex + 1) % this.BUFFER_SIZE
    }
    return writeIndex
  }

  private pullFromBuffer(buffer: Float32Array, data: Float32Array, writeIndex: number, readIndex: number): number {
    const availableData = (writeIndex - readIndex + this.BUFFER_SIZE) % this.BUFFER_SIZE
    if (availableData < data.length) {
      data.fill(0)
      return readIndex
    }
    for (let i = 0; i < data.length; i++) {
      data[i] = buffer[readIndex]
      readIndex = (readIndex + 1) % this.BUFFER_SIZE
    }
    return readIndex
  }

  process(inputs: Float32Array[][], outputs: Float32Array[][]): boolean {
    const input = inputs[0]
    const output = outputs[0]

    if (!input?.length || !output?.length) return true

    const inputChannel = input[0]
    const outputChannel = output[0]

    // Полное отсечение аудио при мьюте (Аппаратный гейт)
    if (this.isMuted) {
      outputChannel.fill(0)
      return true
    }

    this.inputWriteIndex = this.pushToBuffer(this.inputBuffer, inputChannel, this.inputWriteIndex, this.inputReadIndex)

    while ((this.inputWriteIndex - this.inputReadIndex + this.BUFFER_SIZE) % this.BUFFER_SIZE >= this.FRAME_SIZE) {
      this.inputReadIndex = this.pullFromBuffer(this.inputBuffer, this.frameToProcess, this.inputWriteIndex, this.inputReadIndex)

      if (this.noiseSuppression && this.denoiserReady && this.denoiser) {
        const cleanFrame = this.denoiser.processAudio(this.frameToProcess)
        this.processedFrame.set(cleanFrame)
      } else {
        this.processedFrame.set(this.frameToProcess)
      }

      // Вычисление RMS для определения голоса на RAW фрейме
      let sumSquares = 0
      for (let i = 0; i < this.FRAME_SIZE; i++) {
        sumSquares += this.frameToProcess[i] * this.frameToProcess[i]
      }
      const currentRms = Math.sqrt(sumSquares / this.FRAME_SIZE)

      // Сглаживание RMS
      this.rmsSmoothed = 0.3 * currentRms + 0.7 * this.rmsSmoothed

      // Гистерезис VAD
      let isVoiceFrame = false
      if (this.framesSinceLastVoice < this.HOLD_FRAMES) {
        isVoiceFrame = this.rmsSmoothed > this.GATE_THRESHOLD_OFF
      } else {
        isVoiceFrame = this.rmsSmoothed > this.GATE_THRESHOLD_ON
      }

      if (isVoiceFrame) {
        this.framesSinceLastVoice = 0
      } else {
        this.framesSinceLastVoice++
      }

      const isSpeaking = this.framesSinceLastVoice < this.HOLD_FRAMES
      if (isSpeaking !== this.lastVadSent) {
        this.port.postMessage({ type: 'vad', isSpeaking })
        this.lastVadSent = isSpeaking
      }

      // АРУ (AGC) накопление данных
      if (isSpeaking) {
        this.speechRmsAccumulator += currentRms
        this.speechRmsCount++

        if (this.speechRmsCount >= 100) { // Каждую секунду активной речи (100 фреймов по 10мс)
          const avgSpeechRms = this.speechRmsAccumulator / this.speechRmsCount
          this.speechRmsAccumulator = 0
          this.speechRmsCount = 0

          if (avgSpeechRms > 0.001) {
            const targetAgc = this.TARGET_SPEECH_RMS / avgSpeechRms
            const clampedTarget = Math.max(this.MIN_AGC_GAIN, Math.min(this.MAX_AGC_GAIN, targetAgc))
            // Медленная подстройка
            this.agcGain = 0.85 * this.agcGain + 0.15 * clampedTarget
          }
        }
      } else {
        this.speechRmsAccumulator = 0
        this.speechRmsCount = 0
      }

      // Гибридный VCA-экспандер и автоматическое усиление (AGC)
      if (this.noiseSuppression) {
        const overallTarget = isSpeaking ? this.agcGain : this.TARGET_GAIN_OFF

        for (let i = 0; i < this.FRAME_SIZE; i++) {
          if (overallTarget > this.currentGain) {
            // Быстрая атака
            this.currentGain = this.attackCoef * this.currentGain + (1 - this.attackCoef) * overallTarget
          } else {
            // Плавный релиз
            this.currentGain = this.releaseCoef * this.currentGain + (1 - this.releaseCoef) * overallTarget
          }
          this.processedFrame[i] *= this.currentGain
        }
      } else {
        // Если шумоподавление выключено, всё равно применяем выравнивание громкости (AGC)
        for (let i = 0; i < this.FRAME_SIZE; i++) {
          this.processedFrame[i] *= this.agcGain
        }
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

registerProcessor('deepfilter-processor', DeepFilterProcessor)