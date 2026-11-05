import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';
import ts from 'typescript';
import { RemoteControlProtocol, RC_OP_START } from '../src/renderer/src/services/remoteControl.ts';

const text = readFileSync(new URL('../src/renderer/src/services/webrtc.ts', import.meta.url), 'utf8');
const source = ts.createSourceFile('webrtc.ts', text, ts.ScriptTarget.Latest, true);
const manager = source.statements.find(node => ts.isClassDeclaration(node) && node.name?.text === 'WebRTCManager');
const member = manager.members.find(node => node.name?.getText(source) === 'prepareMicTest');
const compiled = ts.transpileModule(`class Engine { ${member.getText(source)} }`, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
const Engine = runInNewContext(`${compiled}\nEngine`, { DOMException, Error, console });

test('mic-test warmup recovers a replaced context without a catch-scope ReferenceError', async () => {
  const engine = new Engine();
  const first = { state: 'running' };
  const next = { state: 'running' };
  const captured = [];
  engine.processedContext = first;
  engine.prewarmLocalStream = async () => true;
  engine.ensureMicTestCaptureModule = async context => {
    captured.push(context);
    if (context === first) {
      engine.processedContext = next;
      throw new Error('synthetic-context-replacement');
    }
  };
  await engine.prepareMicTest();
  assert.deepEqual(captured, [first, next]);
});

test('mic-test cancellation with a closed context does not restart capture', async () => {
  const engine = new Engine();
  engine.processedContext = { state: 'closed' };
  engine.prewarmLocalStream = async () => { throw new DOMException('synthetic-cancel', 'AbortError'); };
  engine.ensureMicTestCaptureModule = async () => { throw new Error('unexpected-restart'); };
  await engine.prepareMicTest();
});

test('remote-control signal buffers remain reusable ordinary ArrayBuffers', () => {
  const first = RemoteControlProtocol.encodeSignal(RC_OP_START);
  assert.equal(first, RemoteControlProtocol.encodeSignal(RC_OP_START));
  assert.ok(first.buffer instanceof ArrayBuffer);
  assert.deepEqual([...first], [RC_OP_START]);
  const button = RemoteControlProtocol.encodeMouseButton(1, true);
  assert.ok(button.buffer instanceof ArrayBuffer);
});

test('fullscreen permission lookup is bound to the selected streamer and deleted-channel edit state stays local', () => {
  const app = readFileSync(new URL('../src/renderer/src/App.tsx', import.meta.url), 'utf8');
  const signal = readFileSync(new URL('../src/renderer/src/services/signalr.ts', import.meta.url), 'utf8');
  assert.equal((app.match(/remoteStreamsAllowControl\?\.\[streamerUser\.id\]/g) ?? []).length, 2);
  assert.ok(app.includes('!store.channels.some(channel => channel.id === editChannelId)'));
  assert.equal(signal.includes('setChannelEdit('), false);
  assert.equal(signal.includes("setModal('editChannel'"), false);
});
