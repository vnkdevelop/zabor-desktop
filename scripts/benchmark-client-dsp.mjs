import { performance } from 'node:perf_hooks';
import { mkdirSync, writeFileSync } from 'node:fs';
import { createPipeline, legacyMeasure, legacyState, pitchRing, signalQuantum, wasmModule } from '../tests/helpers/client-dsp-fixture.mjs';
import { RustVoicing } from '../src/renderer/src/services/rust-voicing.ts';

const percentile = (values, fraction) => { const sorted = [...values].sort((a, b) => a - b); return sorted[Math.max(0, Math.ceil(sorted.length * fraction) - 1)]; };
const rings = Array.from({ length: 64 }, (_, index) => pitchRing(index));
const legacy = legacyState(rings[0]);
const rust = new RustVoicing(wasmModule);
const records = [];
for (const mode of ['legacy', 'rust']) {
  for (let index = 0; index < 1000; index++) {
    if (mode === 'legacy') { legacy.pitchBuffer = rings[index % rings.length]; legacyMeasure.call(legacy); }
    else rust.measure(rings[index % rings.length], 0, 512);
  }
}
for (let repeat = 0; repeat < 6; repeat++) {
  for (const mode of repeat % 2 ? ['rust', 'legacy'] : ['legacy', 'rust']) {
    const samples = [];
    for (let index = 0; index < 3000; index++) {
      const ring = rings[index % rings.length];
      legacy.pitchBuffer = ring;
      const started = performance.now();
      if (mode === 'legacy') legacyMeasure.call(legacy);
      else rust.measure(ring, 0, 512);
      samples.push((performance.now() - started) * 1000);
    }
    records.push({ repeat, mode, samplesUs: samples });
  }
}
rust.dispose();
const pipeline = [];
const inputs = Array.from({ length: 3750 }, (_, index) => signalQuantum(index * 128));
for (let repeat = 0; repeat < 6; repeat++) {
  for (const mode of repeat % 2 ? ['rust', 'legacy'] : ['legacy', 'rust']) {
    const fixture = createPipeline(mode === 'rust');
    const output = new Float32Array(128);
    const samples = [];
    for (const input of inputs) {
      const started = performance.now();
      fixture.pipeline.process([[input]], [[output]]);
      samples.push((performance.now() - started) * 1000);
    }
    fixture.pipeline.port.onmessage({ data: { type: 'dispose' } });
    pipeline.push({ repeat, mode, samplesUs: samples });
  }
}
const summarize = rows => Object.fromEntries(['legacy', 'rust'].map(mode => {
  const values = rows.filter(row => row.mode === mode).flatMap(row => row.samplesUs);
  return [mode, { p50Us: percentile(values, 0.5), p95Us: percentile(values, 0.95), p99Us: percentile(values, 0.99), totalMs: values.reduce((sum, value) => sum + value, 0) / 1000 }];
}));
const report = { scope: 'same client math, actual WASM copy/call cost included; pipeline uses synthetic input and identity model, not real neural inference or network', repeats: 6, wasmMemoryBytes: 262144, kernel: summarize(records), pipeline: summarize(pipeline), records, pipelineRecords: pipeline };
mkdirSync('migration-artifacts', { recursive: true });
writeFileSync('migration-artifacts/client-dsp-comparison.json', JSON.stringify(report, null, 2));
console.log(JSON.stringify({ kernel: report.kernel, pipeline: report.pipeline, wasmMemoryBytes: report.wasmMemoryBytes }, null, 2));
