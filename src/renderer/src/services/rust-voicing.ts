interface VoicingExports extends WebAssembly.Exports {
  memory: WebAssembly.Memory
  zabor_voicing_abi: () => number
  zabor_voicing_create: () => number
  zabor_voicing_input: () => number
  zabor_voicing_result: () => number
  zabor_voicing_process: (writeIndex: number, filled: number) => number
  zabor_voicing_dispose: () => void
}

export class RustVoicing {
  private readonly exports: VoicingExports
  private readonly input: Float32Array
  private readonly result: Float64Array
  private disposed = false
  public lag = 0

  constructor(module: WebAssembly.Module) {
    const instance = new WebAssembly.Instance(module, {})
    this.exports = instance.exports as VoicingExports
    if (this.exports.zabor_voicing_abi() !== 1 || this.exports.zabor_voicing_create() !== 1) throw new Error('rust-voicing-abi')
    const buffer = this.exports.memory.buffer
    const input = this.exports.zabor_voicing_input()
    const result = this.exports.zabor_voicing_result()
    if (!input || !result || input % 4 || result % 8 || input + 2048 > buffer.byteLength || result + 16 > buffer.byteLength) {
      this.exports.zabor_voicing_dispose()
      throw new Error('rust-voicing-memory')
    }
    this.input = new Float32Array(buffer, input, 512)
    this.result = new Float64Array(buffer, result, 2)
  }

  measure(ring: Float32Array, writeIndex: number, filled: number): number {
    if (this.disposed || ring.length !== 512 || this.input.buffer !== this.exports.memory.buffer) throw new Error('rust-voicing-state')
    this.input.set(ring)
    if (this.exports.zabor_voicing_process(writeIndex, filled) !== 1) throw new Error('rust-voicing-process')
    this.lag = this.result[1]
    return this.result[0]
  }

  dispose(): void {
    if (this.disposed) return
    this.disposed = true
    this.exports.zabor_voicing_dispose()
  }
}
