import assert from 'node:assert/strict';
import { test } from 'node:test';
import { RustVoicing } from '../src/renderer/src/services/rust-voicing.ts';
import { createPipeline, legacyMeasure, legacyState, pitchRing, signalQuantum, wasmModule } from './helpers/client-dsp-fixture.mjs';

test('Rust WASM matches actual legacy voicing at ring offsets and warmup boundaries', () => {
  const rust = new RustVoicing(wasmModule);
  for (const filled of [0, 100, 485, 486, 512]) {
    for (const offset of [0, 1, 255, 511]) {
      for (let seed = 0; seed < 12; seed++) {
        const ring = pitchRing(seed);
        const state = legacyState(ring, offset, filled);
        const expected = legacyMeasure.call(state);
        const actual = rust.measure(ring, offset, filled);
        assert.ok(Math.abs(actual - expected) <= 1e-12);
        assert.equal(rust.lag, state.voicingLag);
      }
    }
  }
  rust.dispose();
  assert.throws(() => rust.measure(pitchRing(0), 0, 512));
});

test('complete mic processor keeps PCM, VAD windows and state equal with Rust voicing', () => {
  const legacy = createPipeline();
  const rust = createPipeline(true);
  assert.equal(rust.messages[0].runtime, 'rust-voicing');
  assert.equal(legacy.messages[0].runtime, 'legacy');
  legacy.pipeline.sileroVadEnabled = true;
  rust.pipeline.sileroVadEnabled = true;
  for (let quantum = 0; quantum < 3750; quantum++) {
    const input = signalQuantum(quantum * 128);
    const oldOutput = new Float32Array(128);
    const newOutput = new Float32Array(128);
    assert.equal(legacy.pipeline.process([[input]], [[oldOutput]]), true);
    assert.equal(rust.pipeline.process([[input]], [[newOutput]]), true);
    assert.deepEqual(newOutput, oldOutput);
    assert.equal(rust.pipeline.voicingLag, legacy.pipeline.voicingLag);
    assert.ok(Math.abs(rust.pipeline.voicing - legacy.pipeline.voicing) <= 1e-12);
    assert.equal(rust.pipeline.voicingHoldWindows, legacy.pipeline.voicingHoldWindows);
    assert.equal(rust.pipeline.vad16kWriteIndex, legacy.pipeline.vad16kWriteIndex);
  }
  const oldWindows = legacy.messages.filter(message => message.type === 'audio16k');
  const newWindows = rust.messages.filter(message => message.type === 'audio16k');
  assert.equal(oldWindows.length, newWindows.length);
  for (let index = 0; index < oldWindows.length; index++) {
    assert.deepEqual(newWindows[index].audio, oldWindows[index].audio);
    assert.equal(newWindows[index].sequence, oldWindows[index].sequence);
    assert.equal(newWindows[index].endFrameId, oldWindows[index].endFrameId);
    assert.equal(newWindows[index].windowRms, oldWindows[index].windowRms);
  }
  rust.pipeline.port.onmessage({ data: { type: 'dispose' } });
  assert.equal(rust.pipeline.process([[signalQuantum(0)]], [[new Float32Array(128)]]), false);
});

test('failed Rust instance falls back using the continuously maintained legacy ring', () => {
  const { pipeline, messages } = createPipeline(true);
  const ring = pitchRing(7);
  pipeline.pitchBuffer.set(ring);
  pipeline.pitchFilled = 512;
  pipeline.pitchWriteIndex = 255;
  pipeline.rustVoicing.exports.zabor_voicing_dispose();
  const expectedState = legacyState(ring, 255, 512);
  const expected = legacyMeasure.call(expectedState);
  const actual = pipeline.measureVoicing();
  assert.ok(Math.abs(actual - expected) <= 1e-12);
  assert.equal(pipeline.voicingLag, expectedState.voicingLag);
  assert.equal(pipeline.rustVoicing, null);
  assert.ok(messages.some(message => message.reason === 'rust-voicing-fallback'));
});

test('repeated Rust lifecycle does not grow WASM linear memory', () => {
  for (let index = 0; index < 100; index++) {
    const rust = new RustVoicing(wasmModule);
    const buffer = rust.exports.memory.buffer;
    assert.equal(buffer.byteLength, 262144);
    for (let frame = 0; frame < 16; frame++) rust.measure(pitchRing(frame), frame, 512);
    assert.equal(rust.exports.memory.buffer, buffer);
    rust.dispose();
    rust.dispose();
  }
});
