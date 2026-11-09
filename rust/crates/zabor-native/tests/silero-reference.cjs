const { readFileSync } = require('node:fs');
const { dirname } = require('node:path');
const { pathToFileURL } = require('node:url');
const vm = require('node:vm');
const reference = require('../../../../scripts/native/reference-contract.cjs').openReference();
reference.group('silero');
const ts = reference.requirePackage('typescript');
const ort = reference.requirePackage('onnxruntime-web');

const request = JSON.parse(readFileSync(0, 'utf8'));
ort.env.wasm.numThreads = 1;
ort.env.wasm.proxy = false;
let session;
let receive;
const self = { postMessage(message) { receive?.(message); }, onmessage: null };
const runtime = {
  env: ort.env,
  Tensor: ort.Tensor,
  InferenceSession: { async create(...args) { session = await ort.InferenceSession.create(...args); return session; } },
};
const source = ts.transpileModule(reference.text('src/renderer/src/services/silero-vad.worker.ts'), {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
}).outputText;
vm.runInNewContext(source, {
  exports: {},
  require(name) { if (name !== 'onnxruntime-web') throw new Error(name); return runtime; },
  self, Float32Array, BigInt64Array, BigInt, Uint8Array,
});
const send = message => new Promise((accept, reject) => {
  receive = result => result.type === 'error' ? reject(new Error(result.error)) : accept(result);
  self.onmessage({ data: message });
});

(async () => {
  try {
    const wasmPath = pathToFileURL(`${dirname(reference.packagePath('onnxruntime-web', 'dist/ort.node.min.js'))}/`).href;
    await send({ type: 'init', model: reference.read('src/renderer/public/silero_vad.onnx'), wasmPath });
    const output = [];
    for (const packet of request.frames) {
      if (packet.reset) self.onmessage({ data: { type: 'reset' } });
      const result = await send({ type: 'process', audioFrame: Float32Array.from(packet.samples), sequence: packet.sequence, endFrameId: packet.sequence, windowRms: 0, epoch: 1 });
      if (result.type !== 'probability' || result.sequence !== packet.sequence) throw new Error('Unexpected Silero worker response');
      output.push(result.probability);
    }
    process.stdout.write(JSON.stringify(output));
  } finally { await session?.release(); }
})().catch(error => { console.error(error.message); process.exitCode = 1; });
