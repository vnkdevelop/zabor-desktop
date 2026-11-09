import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
import contract from '../../../../scripts/native/reference-contract.cjs';

const reference = contract.openReference();
reference.group('neural');
const request = JSON.parse(readFileSync(0, 'utf8'));
const output = [];
console.log = () => {};

if (request.model === 'rnnoise') {
  const { default: Processor } = await import(pathToFileURL(reference.packagePath('@timephy/rnnoise-wasm', 'dist/RnnoiseProcessor.js')));
  const { default: init } = await import(pathToFileURL(reference.packagePath('@timephy/rnnoise-wasm', 'dist/generated/rnnoise-sync.js')));
  const processor = new Processor(init());
  for (const samples of request.frames) {
    const frame = Float32Array.from(samples);
    processor.processAudioFrame(frame, true);
    output.push(Array.from(frame));
  }
} else if (request.model === 'deepfilter') {
  const module = await import(pathToFileURL(reference.packagePath('deepfilter-standalone', 'dist/df3/df.js')));
  module.initSync(reference.read('src/renderer/public/deepfilternet3/pkg/df_bg.wasm'));
  const handle = module.df_create(reference.read('src/renderer/public/deepfilternet3/models/DeepFilterNet3_onnx.tar.gz'), request.strength);
  module.df_set_post_filter_beta(handle, request.beta);
  for (const samples of request.frames) output.push(Array.from(module.df_process_frame(handle, Float32Array.from(samples))));
} else { throw new Error('Unknown fixture model'); }
process.stdout.write(JSON.stringify(output));
