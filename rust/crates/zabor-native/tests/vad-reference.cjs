const { readFileSync } = require('node:fs');
const { resolve } = require('node:path');
const { pathToFileURL } = require('node:url');
const vm = require('node:vm');
const ts = require('typescript');

async function main() {
  const root = resolve(__dirname, '../../../..');
  const request = JSON.parse(readFileSync(0, 'utf8'));
  const compile = file => ts.transpileModule(readFileSync(resolve(root, file), 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
  const binary = {};
  vm.runInNewContext(compile('src/renderer/src/services/fvad-binary.ts'), { exports: binary });
  const fvad = await import(pathToFileURL(resolve(root, 'node_modules/@echogarden/fvad-wasm/fvad.js')));
  const exported = {};
  vm.runInNewContext(compile('src/renderer/src/services/webrtc-vad.ts'), { exports: exported, require: name => { if (name === '@echogarden/fvad-wasm') return fvad; if (name === './fvad-binary.ts') return binary; throw new Error(name); }, Buffer, console });
  const detector = new exported.WebRtcVadDetector();
  await detector.init();
  if (!detector.isReady()) throw new Error('Existing WebRTC VAD fixture failed to initialize');
  const output = [];
  try {
    for (let index = 0; index < request.frames.length; index++) {
      if (request.resets.includes(index)) detector.reset();
      output.push(detector.process(Float32Array.from(request.frames[index])));
    }
  } finally { detector.destroy(); }
  process.stdout.write(JSON.stringify(output));
}

main().catch(error => { console.error(error); process.exitCode = 1; });
