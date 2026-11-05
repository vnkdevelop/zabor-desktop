import { readFileSync } from 'node:fs';
import { performance } from 'node:perf_hooks';
import { runInNewContext } from 'node:vm';
import { buildSync } from 'esbuild';
import ts from 'typescript';

const source = readFileSync('src/renderer/src/services/mic-pipeline-core.ts', 'utf8');
const parsed = ts.createSourceFile('mic-pipeline-core.ts', source, ts.ScriptTarget.Latest, true);
const processor = parsed.statements.find(node => ts.isClassDeclaration(node) && node.name?.text === 'MicPipelineProcessor');
const measure = processor.members.find(node => node.name?.getText(parsed) === 'measureVoicing');
export const legacyMeasure = new Function(measure.body.getText(parsed).slice(1, -1));
const bundled = buildSync({
  stdin: { contents: "import { registerMicPipeline } from './src/renderer/src/services/mic-pipeline-core.ts'; registerMicPipeline('fixture', () => globalThis.__fixtureEngine());", resolveDir: process.cwd(), loader: 'ts' },
  bundle: true, format: 'iife', platform: 'browser', target: 'es2022', write: false
}).outputFiles[0].text;
export const wasmModule = new WebAssembly.Module(readFileSync('src/renderer/public/rust-voicing/zabor_voicing.wasm'));

export function createPipeline(rust = false) {
  const messages = [];
  let Ctor;
  class Base {
    constructor() {
      this.port = { onmessage: null, onmessageerror: null, postMessage: data => messages.push(data) };
    }
  }
  runInNewContext(bundled, {
    AudioWorkletProcessor: Base, registerProcessor: (_name, value) => { Ctor = value; },
    WebAssembly, ArrayBuffer, Float32Array, Float64Array, Int32Array, Uint8Array,
    performance, console,
    __fixtureEngine: () => ({ label: 'fixture', supportsAttenuationLimit: false, isReady: true, stageName: 'ready', frame: 480, start: async () => {}, process: frame => frame, setAttenuationLimit: () => {}, setPostFilterBeta: () => {} })
  });
  const pipeline = new Ctor({ processorOptions: { rustVoicingModule: rust ? wasmModule : null } });
  return { pipeline, messages };
}

export function signalQuantum(offset, length = 128) {
  return Float32Array.from({ length }, (_, index) => {
    const sample = offset + index;
    const amplitude = sample % 48000 < 32000 ? 0.15 : 0.002;
    return amplitude * (Math.sin(2 * Math.PI * 170 * sample / 48000) + 0.25 * Math.sin(2 * Math.PI * 340 * sample / 48000));
  });
}

export function pitchRing(seed) {
  return Float32Array.from({ length: 512 }, (_, index) => Math.sin((index + seed) * (0.045 + seed * 0.00003)) * 0.1 + Math.sin((index * 13 + seed) * 0.371) * 0.003);
}

export function legacyState(ring, writeIndex = 0, filled = 512) {
  return { pitchBuffer: ring, pitchScratch: new Float32Array(512), pitchWriteIndex: writeIndex, pitchFilled: filled, PITCH_WINDOW: 256, PITCH_MIN_LAG: 40, PITCH_MAX_LAG: 229, voicingLag: 0, rustVoicing: null };
}
