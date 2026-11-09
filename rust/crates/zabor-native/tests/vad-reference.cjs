const { readFileSync } = require('node:fs');
const { pathToFileURL } = require('node:url');
const vm = require('node:vm');

async function main() {
  const reference = require('../../../../scripts/native/reference-contract.cjs').openReference();
  reference.group('vad');
  const ts = reference.requirePackage('typescript');
  const request = JSON.parse(readFileSync(0, 'utf8'));
  const compile = file => ts.transpileModule(reference.text(file), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
  const binary = {};
  vm.runInNewContext(compile('src/renderer/src/services/fvad-binary.ts'), { exports: binary });
  const fvad = await import(pathToFileURL(reference.packagePath('@echogarden/fvad-wasm', 'fvad.js')));
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
