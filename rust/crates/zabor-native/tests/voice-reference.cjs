const { readFileSync } = require('node:fs');
const { resolve } = require('node:path');
const vm = require('node:vm');
const ts = require('typescript');

const root = resolve(__dirname, '../../../..');
const payload = JSON.parse(readFileSync(0, 'utf8'));
const compile = file => ts.transpileModule(readFileSync(resolve(root, file), 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
const exported = {};
vm.runInNewContext(compile('src/renderer/src/services/voice-shaping.ts'), { exports: exported, sampleRate: 48000 });
const input = Float32Array.from(payload.samples);
const output = new Float32Array(input.length);
if (payload.kind === 'filters') {
  const tone = new exported.TonalNoiseSuppressor();
  const shaper = new exported.VoiceShaper();
  shaper.setSpeechReference(0.05);
  for (let i = 0; i < input.length; i++) output[i] = shaper.process(tone.process(input[i], true));
} else if (payload.kind === 'manual') {
  let Processor;
  class Base { port = { postMessage() {}, onmessage: null }; }
  vm.runInNewContext(compile('src/renderer/src/services/manual-gate-processor.ts'), { exports: {}, require: name => { if (name !== './voice-shaping') throw new Error(name); return exported; }, AudioWorkletProcessor: Base, registerProcessor: (_name, type) => { Processor = type; } });
  const processor = new Processor();
  for (let offset = 0; offset < input.length; offset += 128) {
    const block = offset / 128;
    processor.port.onmessage({ data: { type: 'setConfig', manualThresholdValue: -30, isMuted: block >= 2500 && block < 2750 } });
    processor.process([[input.subarray(offset, offset + 128)]], [[output.subarray(offset, offset + 128)]]);
  }
} else if (['smart-auto', 'smart-manual', 'smart-vad'].includes(payload.kind)) {
  let Processor;
  const windows = [];
  class Base { port = { postMessage(message) { if (message.type === 'audio16k') windows.push(message); }, onmessage: null }; }
  const pipeline = {};
  vm.runInNewContext(compile('src/renderer/src/services/mic-pipeline-core.ts'), { exports: pipeline, require: name => { if (name === './voice-shaping') return exported; if (name === './rust-voicing') return {}; throw new Error(name); }, sampleRate: 48000, AudioWorkletProcessor: Base, registerProcessor: (_name, type) => { Processor = type; } });
  pipeline.registerMicPipeline('reference', () => ({ label: 'reference', isReady: true, frame: 480, supportsAttenuationLimit: false, process: frame => frame, setAttenuationLimit() {}, setPostFilterBeta() {} }));
  const processor = new Processor();
  processor.port.onmessage({ data: { type: 'setConfig', noiseSuppression: true, sileroVadEnabled: payload.kind === 'smart-vad' } });
  processor.port.onmessage({ data: { type: 'setCalibratedParams', thresholdMode: payload.kind === 'smart-manual' ? 'manual' : 'auto', manualThresholdValue: -30, attenuationLimit: 15, adaptiveAttenuation: false, downstreamGain: 1, vadFixedThreshold: 0.4 } });
  for (let offset = 0; offset < input.length; offset += 480) {
    processor.process([[input.subarray(offset, offset + 480)]], [[output.subarray(offset, offset + 480)]]);
    for (const window of windows.splice(0)) processor.port.onmessage({ data: { ...window, type: 'setSileroVadProbability', probability: Math.fround(window.sequence >= 70 && window.sequence < 140 ? 0.8 : window.sequence >= 180 && window.sequence < 205 ? 0.5 : 0.05) } });
  }
} else { throw new Error('Unknown voice fixture'); }
process.stdout.write(Buffer.from(output.buffer).toString('base64'));
