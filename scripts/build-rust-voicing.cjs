const { existsSync, readFileSync, mkdirSync, copyFileSync, writeFileSync } = require('node:fs');
const { join } = require('node:path');
const { homedir } = require('node:os');
const { createHash } = require('node:crypto');
const { spawnSync } = require('node:child_process');

const root = join(__dirname, '..');
const files = ['rust/Cargo.toml', 'rust/Cargo.lock', 'rust/crates/zabor-dsp/Cargo.toml', 'rust/crates/zabor-dsp/src/lib.rs', 'rust/crates/zabor-dsp-wasm/Cargo.toml', 'rust/crates/zabor-dsp-wasm/src/lib.rs', 'scripts/build-rust-voicing.cjs'];
const hash = createHash('sha256');
for (const file of files) hash.update(file).update(readFileSync(join(root, file), 'utf8').replace(/\r\n/g, '\n'));
const sourceSha256 = hash.digest('hex');
const directory = join(root, 'src/renderer/public/rust-voicing');
const asset = join(directory, 'zabor_voicing.wasm');
const record = join(directory, 'manifest.json');
const digest = file => createHash('sha256').update(readFileSync(file)).digest('hex');
if (existsSync(asset) && existsSync(record)) {
  const metadata = JSON.parse(readFileSync(record, 'utf8'));
  if (metadata.sourceSha256 === sourceSha256 && metadata.sha256 === digest(asset)) {
    console.log('[rust-voicing] bundled WASM verified');
    process.exit(0);
  }
}
const binary = join(process.env.CARGO_HOME || join(homedir(), '.cargo'), 'bin', process.platform === 'win32' ? 'cargo.exe' : 'cargo');
const cargo = existsSync(binary) ? binary : 'cargo';
const result = spawnSync(cargo, ['build', '--release', '--locked', '--target', 'wasm32-unknown-unknown', '-p', 'zabor-dsp-wasm'], {
  cwd: join(root, 'rust'), stdio: 'inherit', windowsHide: true,
  env: { ...process.env, RUSTFLAGS: '-C link-arg=-z -C link-arg=stack-size=65536 -C link-arg=--initial-memory=262144 -C link-arg=--max-memory=262144' }
});
if (result.error) throw result.error;
if (result.status !== 0) process.exit(result.status || 1);
mkdirSync(directory, { recursive: true });
copyFileSync(join(root, 'rust/target/wasm32-unknown-unknown/release/zabor_dsp_wasm.wasm'), asset);
const bytes = readFileSync(asset);
const compiledModule = new WebAssembly.Module(bytes);
if (WebAssembly.Module.imports(compiledModule).length !== 0) throw new Error('Rust voicing WASM requires unexpected imports');
const instance = new WebAssembly.Instance(compiledModule, {});
if (instance.exports.zabor_voicing_abi() !== 1 || instance.exports.zabor_voicing_create() !== 1 || instance.exports.memory.buffer.byteLength !== 262144) throw new Error('Rust voicing ABI or memory mismatch');
instance.exports.zabor_voicing_dispose();
writeFileSync(record, JSON.stringify({ abiVersion: 1, sourceSha256, sha256: digest(asset), bytes: bytes.byteLength, memoryBytes: 262144 }, null, 2) + '\n');
console.log(`[rust-voicing] compiled ${bytes.byteLength} bytes, fixed 256 KiB memory`);
