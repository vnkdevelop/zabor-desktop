/// <reference lib="webworker" />

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

  constructor() {
    super()
    this.inputBuffer = new Float32Array(this.BUFFER_SIZE)
    this.outputBuffer = new Float32Array(this.BUFFER_SIZE)
    this.frameToProcess = new Float32Array(this.FRAME_SIZE)
    this.processedFrame = new Float32Array(this.FRAME_SIZE)

    this.port.onmessage = (event) => {
      if (event.data.type === 'loadWasm') {
        // Инициализация WASM-инстанса DeepFilterNet:
        // this.dfState = initDeepFilter(event.data.module, event.data.model)
        this.wasmLoaded = true
        this.port.postMessage({ type: 'ready' })
      }
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

      if (this.wasmLoaded) {
        // Реальная обработка через WASM (когда будет интегрировано):
        // df_process(this.dfState, this.frameToProcess, this.processedFrame)
        // isSpeaking = calculateVADFromMask(mask)

        // Временный bypass до интеграции WASM
        this.processedFrame.set(this.frameToProcess)

        // RMS-based VAD как fallback пока WASM не даёт маску
        let sumSquares = 0
        for (let i = 0; i < this.FRAME_SIZE; i++) {
          sumSquares += this.processedFrame[i] * this.processedFrame[i]
        }
        const isSpeaking = Math.sqrt(sumSquares / this.FRAME_SIZE) > this.vadThreshold

        if (isSpeaking !== this.lastVadSent) {
          this.port.postMessage({ type: 'vad', isSpeaking })
          this.lastVadSent = isSpeaking
        }
      } else {
        // Bypass без WASM — копируем без изменений.
        // VAD в этом режиме обрабатывается в JS-потоке через setupVAD().
        this.processedFrame.set(this.frameToProcess)
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
