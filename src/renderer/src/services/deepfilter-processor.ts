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

  // VAD Thresholds & Smoothing
  private rmsSmoothed = 0
  private GATE_THRESHOLD_ON = 0.015  // Чуть приподнятый дефолтный порог включения
  private GATE_THRESHOLD_OFF = 0.008 // Порог удержания
  private lastVadSent = false

  private overflowCount = 0

  private readonly HOLD_FRAMES = 20 // ~200ms удержания гейта после завершения речи (было 50 / 500ms)
  private framesSinceLastVoice = this.HOLD_FRAMES

  // VCA-эмуляция (Гибридный гейт для суммарного подавления)
  private currentGain = 1.0
  private readonly TARGET_GAIN_ON = 1.0
  private readonly TARGET_GAIN_OFF = 0.0 // Полная тишина при закрытом гейте (было 0.01 / -40 дБ)

  // Экспоненциальные огибающие: Время атаки (10мс) и релиза (30мс)
  private readonly attackCoef = Math.exp(-1.0 / (this.SAMPLE_RATE * 0.010)) // Было 15мс
  private readonly releaseCoef = Math.exp(-1.0 / (this.SAMPLE_RATE * 0.030)) // Было 600мс

  // Медленная автоматическая регулировка усиления (АРУ / AGC)
  private readonly agcGain = 1.0 // Зафиксировано на 1.0 для предотвращения скачков и пампинга
  private attenuationLimit = 100

  // Буфер lookahead для задержки голоса и качественного открытия гейта
  private delayBuffer: Array<{ frame: Float32Array; isSpeaking: boolean }> = []
  private readonly LOOKAHEAD_FRAMES = 8 // 80мс задержки

  constructor() {
    super()
    this.inputBuffer = new Float32Array(this.BUFFER_SIZE)
    this.outputBuffer = new Float32Array(this.BUFFER_SIZE)
    this.frameToProcess = new Float32Array(this.FRAME_SIZE)
    this.processedFrame = new Float32Array(this.FRAME_SIZE)
    this.outputWriteIndex = this.FRAME_SIZE * (1 + this.LOOKAHEAD_FRAMES) // Предзаполняем тишиной с учетом lookahead задержки

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
            this.outputWriteIndex = this.FRAME_SIZE * (1 + this.LOOKAHEAD_FRAMES)
            this.rmsSmoothed = 0
            this.currentGain = this.TARGET_GAIN_OFF
            this.framesSinceLastVoice = this.HOLD_FRAMES
            this.delayBuffer = []
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
        attenuationLimit: this.attenuationLimit,
        postFilterBeta: 0.05
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

      // Вычисление RMS для определения голоса.
      // Если шумоподавление включено, делаем замер на очищенном (processed) фрейме,
      // чтобы фоновый шум не приводил к ложным срабатываниям ВАД.
      let sumSquares = 0
      const analysisFrame = this.noiseSuppression ? this.processedFrame : this.frameToProcess
      for (let i = 0; i < this.FRAME_SIZE; i++) {
        sumSquares += analysisFrame[i] * analysisFrame[i]
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

      // Добавляем фрейм во временный буфер задержки
      this.delayBuffer.push({
        frame: new Float32Array(this.processedFrame),
        isSpeaking: isSpeaking
      })

      // Когда буфер заполнен, извлекаем самый старый и применяем гейт с lookahead
      if (this.delayBuffer.length > this.LOOKAHEAD_FRAMES) {
        const oldest = this.delayBuffer.shift()!
        
        // Lookahead: гейт открыт, если сам старый фрейм или любой из последующих в очереди активен
        const anySpeakingAhead = oldest.isSpeaking || this.delayBuffer.some(item => item.isSpeaking)

        // Отправляем VAD-сообщение хосту синхронно с выдачей фрейма
        if (anySpeakingAhead !== this.lastVadSent) {
          this.port.postMessage({ type: 'vad', isSpeaking: anySpeakingAhead })
          this.lastVadSent = anySpeakingAhead
        }

        if (this.noiseSuppression) {
          const overallTarget = anySpeakingAhead ? this.agcGain : this.TARGET_GAIN_OFF

          for (let i = 0; i < this.FRAME_SIZE; i++) {
            if (overallTarget > this.currentGain) {
              // Быстрая атака (10мс)
              this.currentGain = this.attackCoef * this.currentGain + (1 - this.attackCoef) * overallTarget
            } else {
              // Быстрый релиз (30мс)
              this.currentGain = this.releaseCoef * this.currentGain + (1 - this.releaseCoef) * overallTarget
            }
            oldest.frame[i] *= this.currentGain
          }
        }

        this.outputWriteIndex = this.pushToBuffer(this.outputBuffer, oldest.frame, this.outputWriteIndex, this.outputReadIndex)
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