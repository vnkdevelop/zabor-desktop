/// <reference lib="webworker" />
import { StandaloneDeepFilter } from 'deepfilter-standalone'

// AudioWorklet-типы не включены в стандартный lib TypeScript — объявляем вручную
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
  private readonly BUFFER_SIZE = 4800 // 10 фреймов при 48kHz

  private denoiser: StandaloneDeepFilter | null = null
  private denoiserReady = false

  /**
   * Предвыделенные рабочие буферы — исключают аллокации в горячем пути process().
   * new Float32Array() каждый вызов провоцирует GC-паузы каждые ~3ms.
   */
  private readonly frameToProcess: Float32Array
  private readonly processedFrame: Float32Array

  private dfState: unknown = null // WASM-инстанс DeepFilterNet
  private wasmLoaded = false

  private vadThreshold = 0.01
  private lastVadSent = false

  /** Счётчик переполнений для rate-limit предупреждений (не спамим в hot path) */
  private overflowCount = 0

  private noiseSuppression = true
  private framesSinceLastVoice = 0
  private currentGain = 0
  private readonly HOLD_FRAMES = 40 // 400ms (40 фреймов по 10мс)
  private readonly ATTACK_STEP = 1.0 / (this.FRAME_SIZE * 2) // Плавное нарастание за ~20мс (2 фрейма)
  private readonly RELEASE_STEP = 1.0 / (this.FRAME_SIZE * 20) // Плавное затухание за ~200мс (20 фреймов)

  constructor() {
    super()
    this.inputBuffer = new Float32Array(this.BUFFER_SIZE)
    this.outputBuffer = new Float32Array(this.BUFFER_SIZE)
    this.frameToProcess = new Float32Array(this.FRAME_SIZE)
    this.processedFrame = new Float32Array(this.FRAME_SIZE)

    this.port.onmessage = (event) => {
      if (event.data.type === 'loadWasm') {
        this.initDeepFilter()
      } else if (event.data.type === 'setConfig') {
        this.noiseSuppression = event.data.noiseSuppression
      }
    }

    // Инициализируем нейросеть сразу при старте воркера
    this.initDeepFilter()
  }

  private async initDeepFilter() {
    if (this.denoiserReady) return
    try {
      this.denoiser = new StandaloneDeepFilter({
        attenuationLimit: 100, // Глубокое подавление
        postFilterBeta: 0.02
      })
      await this.denoiser.initialize()
      this.denoiserReady = true
      this.wasmLoaded = true
      this.port.postMessage({ type: 'ready' })
      console.log('[DeepFilterProcessor] Neural net initialized successfully')
    } catch (e) {
      console.error('[DeepFilterProcessor] Failed to load DeepFilterNet:', e)
    }
  }

  /** Записывает samples в кольцевой буфер. Возвращает обновлённый writeIndex. */
  private pushToBuffer(
    buffer: Float32Array,
    data: Float32Array,
    writeIndex: number,
    readIndex: number
  ): number {
    const availableSpace = (readIndex - writeIndex - 1 + this.BUFFER_SIZE) % this.BUFFER_SIZE
    if (availableSpace < data.length) {
      this.overflowCount++
      // Rate-limit: логируем раз в 100 переполнений, чтобы не спамить в hot path
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

  /** Читает samples из кольцевого буфера. Возвращает обновлённый readIndex. */
  private pullFromBuffer(
    buffer: Float32Array,
    data: Float32Array,
    writeIndex: number,
    readIndex: number
  ): number {
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

    // 1. Пишем 128 новых семплов в кольцевой входной буфер
    this.inputWriteIndex = this.pushToBuffer(
      this.inputBuffer,
      inputChannel,
      this.inputWriteIndex,
      this.inputReadIndex
    )

    // 2. Обрабатываем фреймы по 480 семплов (требование DeepFilterNet @ 48kHz)
    while (
      (this.inputWriteIndex - this.inputReadIndex + this.BUFFER_SIZE) % this.BUFFER_SIZE >=
      this.FRAME_SIZE
    ) {
      this.inputReadIndex = this.pullFromBuffer(
        this.inputBuffer,
        this.frameToProcess,
        this.inputWriteIndex,
        this.inputReadIndex
      )

      // Встроенный умный Noise Gate с плавным фейдом + VAD
      let sumSquares = 0
      for (let i = 0; i < this.FRAME_SIZE; i++) {
        sumSquares += this.frameToProcess[i] * this.frameToProcess[i]
      }
      const rms = Math.sqrt(sumSquares / this.FRAME_SIZE)
      
      // Порог VAD. Используем this.vadThreshold для управления чувствительностью.
      const isVoiceFrame = rms > this.vadThreshold

      if (isVoiceFrame) {
        this.framesSinceLastVoice = 0
      } else {
        this.framesSinceLastVoice++
      }

      // Отправляем VAD статус в UI
      const isSpeaking = this.framesSinceLastVoice < this.HOLD_FRAMES
      if (isSpeaking !== this.lastVadSent) {
        this.port.postMessage({ type: 'vad', isSpeaking })
        this.lastVadSent = isSpeaking
      }

      // Если шумоподавление включено и нейросеть готова, очищаем кадр
      if (this.noiseSuppression && this.denoiserReady && this.denoiser) {
        const cleanFrame = this.denoiser.processAudio(this.frameToProcess)
        this.processedFrame.set(cleanFrame)
      } else {
        // Иначе просто копируем
        this.processedFrame.set(this.frameToProcess)
      }

      if (this.noiseSuppression) {
        // Плавный Noise Gate поверх очищенного звука
        const targetGain = isSpeaking ? 1.0 : 0.0
        
        for (let i = 0; i < this.FRAME_SIZE; i++) {
          if (this.currentGain < targetGain) {
            this.currentGain = Math.min(targetGain, this.currentGain + this.ATTACK_STEP)
          } else if (this.currentGain > targetGain) {
            this.currentGain = Math.max(targetGain, this.currentGain - this.RELEASE_STEP)
          }
          
          // Применяем gain и убираем микроскопический шум (приравниваем к 0, чтобы Opus включил DTX)
          const sample = this.processedFrame[i] * this.currentGain
          this.processedFrame[i] = Math.abs(sample) < 1e-6 ? 0.0 : sample
        }
      }

      this.outputWriteIndex = this.pushToBuffer(
        this.outputBuffer,
        this.processedFrame,
        this.outputWriteIndex,
        this.outputReadIndex
      )
    }

    // 3. Отдаём 128 семплов из выходного буфера в Web Audio API
    const availableOutput =
      (this.outputWriteIndex - this.outputReadIndex + this.BUFFER_SIZE) % this.BUFFER_SIZE

    if (availableOutput >= outputChannel.length) {
      this.outputReadIndex = this.pullFromBuffer(
        this.outputBuffer,
        outputChannel,
        this.outputWriteIndex,
        this.outputReadIndex
      )
    } else {
      // Стартовая задержка накопления фреймов — пишем тишину
      outputChannel.fill(0)
    }

    return true
  }
}

registerProcessor('deepfilter-processor', DeepFilterProcessor)
