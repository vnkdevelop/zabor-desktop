import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { resolve, sep } from 'node:path';
import { pathToFileURL } from 'node:url';
import { runInNewContext } from 'node:vm';
import ts from 'typescript';

const require = createRequire(import.meta.url);
const ort = require('onnxruntime-web');
ort.env.wasm.numThreads = 1;
ort.env.wasm.wasmPaths = pathToFileURL(resolve('node_modules/onnxruntime-web/dist') + sep).href;
ort.env.wasm.wasmBinary = readFileSync('node_modules/onnxruntime-web/dist/ort-wasm-simd-threaded.wasm');
const model = readFileSync('src/renderer/public/silero_vad.onnx');
const baseline = execFileSync('cmd.exe', ['/c', 'git', 'show', 'c5d0734:src/renderer/src/services/silero-vad.worker.ts'], { encoding: 'utf8' });
const current = readFileSync('src/renderer/src/services/silero-vad.worker.ts', 'utf8');

async function workerFixture(text) {
  const source = ts.createSourceFile('worker.ts', text, ts.ScriptTarget.Latest, true);
  const body = source.statements.filter(node => !ts.isImportDeclaration(node)).map(node => node.getText(source)).join('\n');
  const compiled = ts.transpileModule(body, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
  const sessions = [];
  const timings = [];
  const feedIdentities = new Set();
  let tensors = 0;
  let resolveReady;
  let rejectReady;
  const ready = new Promise((resolve, reject) => { resolveReady = resolve; rejectReady = reject; });
  const probabilities = [];
  class Tensor extends ort.Tensor {
    constructor(...args) { super(...args); tensors++; }
  }
  const self = {
    postMessage(message) {
      if (message.type === 'ready') resolveReady();
      else if (message.type === 'error') rejectReady(new Error(message.error));
      else if (message.type === 'probability') probabilities.push({ ...message });
    }
  };
  const InferenceSession = {
    async create(bytes, options) {
      const session = await ort.InferenceSession.create(bytes, options);
      sessions.push(session);
      return {
        inputNames: session.inputNames,
        outputNames: session.outputNames,
        async run(feeds) {
          feedIdentities.add(feeds);
          const start = performance.now();
          const result = await session.run(feeds);
          timings.push(performance.now() - start);
          return result;
        }
      };
    }
  };
  const worker = runInNewContext(`${compiled}\n({ initialize: () => self.onmessage({data:{type:'init',model,wasmPath:''}}), process: processFrame, state: () => Array.from(state), reset: () => self.onmessage({data:{type:'reset'}}) })`, {
    self, model, Tensor, InferenceSession, env: { wasm: {} }, Float32Array, Uint8Array, BigInt64Array,
    BigInt, Error, console
  });
  try {
    worker.initialize();
    await ready;
    return {
      worker, probabilities, timings,
      stats: () => ({ tensors, feeds: feedIdentities.size }),
      close: async () => { for (const session of sessions) await session.release(); }
    };
  } catch (error) {
    for (const session of sessions) await session.release();
    throw error;
  }
}

test('reused Silero input tensors preserve actual WASM model probabilities, history, reset and sequence gaps', async () => {
  const previous = await workerFixture(baseline);
  const updated = await workerFixture(current);
  try {
    for (let index = 0; index < 320; index++) {
      if (index === 180) {
        previous.worker.reset();
        updated.worker.reset();
      }
      const audioFrame = new Float32Array(512);
      for (let sample = 0; sample < audioFrame.length; sample++) {
        const time = index * 512 + sample;
        audioFrame[sample] = index % 80 < 20 ? 0 :
          0.045 * Math.sin(time * 2 * Math.PI * 170 / 16000) + 0.008 * Math.sin(time * 2 * Math.PI * 940 / 16000);
      }
      const message = { type: 'process', audioFrame, sequence: index >= 120 ? index + 3 : index, endFrameId: index * 512, windowRms: 0.032, epoch: 7 };
      await previous.worker.process(message);
      await updated.worker.process(message);
    }
    assert.equal(updated.probabilities.length, 320);
    for (let index = 0; index < 320; index++) {
      const old = previous.probabilities[index];
      const next = updated.probabilities[index];
      assert.ok(Math.abs(old.probability - next.probability) <= 1e-7, `probability frame ${index}`);
      for (const name of ['epoch', 'sequence', 'endFrameId', 'windowRms']) assert.equal(old[name], next[name]);
    }
    assert.deepEqual(Array.from(updated.worker.state()), Array.from(previous.worker.state()));
    assert.equal(previous.stats().tensors, 641);
    assert.equal(updated.stats().tensors, 3);
    assert.equal(previous.stats().feeds, 320);
    assert.equal(updated.stats().feeds, 1);
  } finally {
    await previous.close();
    await updated.close();
  }
});
