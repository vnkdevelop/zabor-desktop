import { readFileSync } from 'node:fs';

const root = new URL('../../../../', import.meta.url);
const request = JSON.parse(readFileSync(0, 'utf8'));
const output = [];
console.log = () => {};

if (request.model === 'rnnoise') {
  const { default: Processor } = await import(new URL('node_modules/@timephy/rnnoise-wasm/dist/RnnoiseProcessor.js', root));
  const { default: init } = await import(new URL('node_modules/@timephy/rnnoise-wasm/dist/generated/rnnoise-sync.js', root));
  const processor = new Processor(init());
  for (const samples of request.frames) {
    const frame = Float32Array.from(samples);
    processor.processAudioFrame(frame, true);
    output.push(Array.from(frame));
  }
} else if (request.model === 'deepfilter') {
  const module = await import(new URL('node_modules/deepfilter-standalone/dist/df3/df.js', root));
  module.initSync(readFileSync(new URL('src/renderer/public/deepfilternet3/pkg/df_bg.wasm', root)));
  const handle = module.df_create(readFileSync(new URL('src/renderer/public/deepfilternet3/models/DeepFilterNet3_onnx.tar.gz', root)), request.strength);
  module.df_set_post_filter_beta(handle, request.beta);
  for (const samples of request.frames) output.push(Array.from(module.df_process_frame(handle, Float32Array.from(samples))));
} else { throw new Error('Unknown fixture model'); }
process.stdout.write(JSON.stringify(output));
