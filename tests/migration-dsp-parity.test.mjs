import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, existsSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { resolve } from 'node:path';
import ts from 'typescript';

const sourceText = readFileSync('src/renderer/src/services/mic-pipeline-core.ts', 'utf8');
const source = ts.createSourceFile('mic-pipeline-core.ts', sourceText, ts.ScriptTarget.Latest, true);
const processor = source.statements.find(node => ts.isClassDeclaration(node) && node.name?.text === 'MicPipelineProcessor');
assert.ok(processor);
const field = name => processor.members.find(node => ts.isPropertyDeclaration(node) && node.name.getText(source) === name);
const numericField = name => {
  const value = field(name)?.initializer;
  assert.ok(value && ts.isNumericLiteral(value), name);
  return Number(value.text);
};
const bufferLength = name => {
  const value = field(name)?.initializer?.arguments?.[0];
  assert.ok(value && ts.isNumericLiteral(value), name);
  return Number(value.text);
};
const method = name => processor.members.find(node => ts.isMethodDeclaration(node) && node.name.getText(source) === name);
const compile = name => {
  const member = method(name);
  assert.ok(member?.body, name);
  const body = member.body.getText(source).slice(1, -1);
  return new Function(...member.parameters.map(parameter => parameter.name.getText(source)), body);
};
const decimate = compile('decimateVadSample');
const measure = compile('measureVoicing');
const constructor = processor.members.find(ts.isConstructorDeclaration);
const coefficientStatements = constructor.body.statements.filter(statement => {
  const text = statement.getText(source);
  return /^(const center|const cutoff|let tapSum)/.test(text) || (ts.isForStatement(statement) && text.includes('this.vadDecimatorTaps'));
});
assert.equal(coefficientStatements.length, 5);
const initializeCoefficients = new Function(coefficientStatements.map(statement => statement.getText(source)).join('\n'));
const binary = resolve(`rust/target/release/examples/parity_runner${process.platform === 'win32' ? '.exe' : ''}`);
assert.ok(existsSync(binary), 'Build the Rust parity runner with npm run test:rust first');

function legacy(input) {
  const state = {
    vadDecimatorTaps: new Float32Array(bufferLength('vadDecimatorTaps')), vadDecimatorHistory: new Float32Array(bufferLength('vadDecimatorHistory')), vadDecimatorWriteIndex: 0, vadDecimatorPhase: 0,
    pitchBuffer: new Float32Array(bufferLength('pitchBuffer')), pitchScratch: new Float32Array(bufferLength('pitchScratch')), pitchWriteIndex: 0, pitchFilled: 0,
    PITCH_WINDOW: numericField('PITCH_WINDOW'), PITCH_MIN_LAG: numericField('PITCH_MIN_LAG'), PITCH_MAX_LAG: numericField('PITCH_MAX_LAG'), voicingLag: 0
  };
  initializeCoefficients.call(state);
  const decimated = [];
  for (const sample of input) {
    const result = decimate.call(state, sample);
    if (result !== null) decimated.push(result);
  }
  const voicing = [];
  for (let offset = 0; offset < decimated.length; offset += 512) {
    for (const sample of decimated.slice(offset, offset + 512)) {
      state.pitchBuffer[state.pitchWriteIndex] = sample;
      state.pitchWriteIndex = (state.pitchWriteIndex + 1) % state.pitchBuffer.length;
      state.pitchFilled = Math.min(state.pitchBuffer.length, state.pitchFilled + 1);
    }
    voicing.push({ score: measure.call(state), lag: state.voicingLag });
  }
  return { taps: state.vadDecimatorTaps, decimated, voicing };
}

const cases = [
  ['empty', 0, () => 0],
  ['silence', 24000, () => 0],
  ['short warmup', 600, i => Math.sin(i * 0.2) * 0.1],
  ['pitch boundary before span', 1455, i => Math.sin(i * 0.02) * 0.1],
  ['pitch boundary at span', 1458, i => Math.sin(i * 0.02) * 0.1],
  ['quiet 200Hz speech-band tone', 24001, i => Math.sin(2 * Math.PI * 200 * i / 48000) * 0.0001],
  ['periodic voiced harmonics', 24000, i => Math.sin(2 * Math.PI * 160 * i / 48000) * 0.15 + Math.sin(2 * Math.PI * 320 * i / 48000) * 0.04],
  ['impulses and clipping', 24000, i => i % 240 === 0 ? 3 : -0.05],
  ['high-frequency rejection', 24000, i => Math.sin(2 * Math.PI * 12000 * i / 48000) * 0.5],
  ['deterministic noise', 24000, i => (((Math.imul(i + 1, 1664525) + 1013904223) >>> 0) / 4294967296 - 0.5) * 0.3]
];

for (const [name, length, sample] of cases) {
  test(`Rust vs actual legacy DSP: ${name}`, () => {
    const input = Float32Array.from({ length }, (_, i) => sample(i));
    const expected = legacy(input);
    const packet = Buffer.alloc((49 + input.length) * 4);
    let offset = 0;
    for (const buffer of [expected.taps, input]) {
      for (const value of buffer) { packet.writeFloatLE(value, offset); offset += 4; }
    }
    const result = spawnSync(binary, [], { input: packet, encoding: 'utf8', maxBuffer: 4 * 1024 * 1024, windowsHide: true });
    assert.equal(result.status, 0, result.stderr);
    const actual = JSON.parse(result.stdout);
    assert.equal(actual.decimated.length, expected.decimated.length);
    for (let i = 0; i < actual.decimated.length; i++) {
      assert.ok(Math.abs(actual.decimated[i] - expected.decimated[i]) <= 1e-12, `decimator sample ${i}`);
    }
    assert.equal(actual.voicing.length, expected.voicing.length);
    for (let i = 0; i < actual.voicing.length; i++) {
      assert.equal(actual.voicing[i].lag, expected.voicing[i].lag, `lag ${i}`);
      assert.ok(Math.abs(actual.voicing[i].score - expected.voicing[i].score) <= 1e-12, `voicing score ${i}`);
    }
  });
}
