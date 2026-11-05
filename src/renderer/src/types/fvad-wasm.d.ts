declare module '@echogarden/fvad-wasm' {
  export interface FvadModule {
    readonly HEAP16: Int16Array
    _malloc(bytes: number): number
    _free(pointer: number): void
    _fvad_new(): number
    _fvad_free(handle: number): void
    _fvad_reset(handle: number): void
    _fvad_set_mode(handle: number, mode: number): number
    _fvad_set_sample_rate(handle: number, sampleRate: number): number
    _fvad_process(handle: number, pointer: number, length: number): number
  }

  export default function fvad(options?: { wasmBinary?: ArrayBuffer }): Promise<FvadModule>
}
