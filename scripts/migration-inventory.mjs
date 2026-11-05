import { readdirSync, readFileSync, mkdirSync, writeFileSync, statSync, existsSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { join, relative, resolve, extname } from 'node:path';
import ts from 'typescript';

const sourceFiles = [];
const assetFiles = [];
const normalizedPath = value => value.replaceAll('\\', '/');
const ignoredDirectories = new Set(['node_modules', 'target', 'bin', 'obj', '.git', 'graphify-out']);

function scan(root, sources, assets) {
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    if (entry.isSymbolicLink()) continue;
    const file = join(root, entry.name);
    if (entry.isDirectory()) {
      if (!ignoredDirectories.has(entry.name)) scan(file, sources, assets);
    } else if (/\.(ts|tsx|js|mjs|cjs|py|cs|go|rs|proto)$/.test(entry.name)) sources.push(file);
    else if (/\.(wasm|onnx|tar\.gz|node|exe|ico)$/.test(entry.name)) assets.push(file);
  }
}

function inspectTypeScript(file, root) {
  const text = readFileSync(file, 'utf8');
  const source = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true, file.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS);
  const entry = { path: normalizedPath(relative(root, file)), sha256: createHash('sha256').update(text).digest('hex'), symbols: [], calls: [] };
  function visit(node) {
    if ((ts.isClassDeclaration(node) || ts.isFunctionDeclaration(node) || ts.isMethodDeclaration(node) || ts.isInterfaceDeclaration(node)) && node.name) {
      const { line } = source.getLineAndCharacterOfPosition(node.getStart(source));
      entry.symbols.push({ name: node.name.getText(source), line: line + 1, kind: ts.SyntaxKind[node.kind] });
    }
    if (ts.isCallExpression(node)) {
      const callee = node.expression.getText(source);
      if (/^(t|test|describe)$|\.(handle|on|invoke|invokeCommand|invokeWithTimeout|safeInvoke|postMessage|setState)$|^(listen|registerProcessor)$/.test(callee)) {
        const first = node.arguments[0];
        const { line } = source.getLineAndCharacterOfPosition(node.getStart(source));
        entry.calls.push({ callee, name: first && ts.isStringLiteralLike(first) ? first.text : null, line: line + 1, dynamic: !first || !ts.isStringLiteralLike(first) });
      }
    }
    ts.forEachChild(node, visit);
  }
  visit(source);
  return entry;
}

const root = resolve('.');
for (const directory of ['src', 'build', 'tests', 'scripts', 'rust']) scan(join(root, directory), sourceFiles, assetFiles);
for (const directory of ['release', 'release/beta']) {
  if (!existsSync(directory)) continue;
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    if (entry.isFile() && entry.name.endsWith('.exe')) assetFiles.push(resolve(directory, entry.name));
  }
}
const inventory = {
  inventoryVersion: 1,
  scope: 'working-tree structural inventory; not functional acceptance or performance measurements',
  baselineCommit: execFileSync('git', ['rev-parse', 'migration-baseline-2026-11-05^{commit}'], { encoding: 'utf8' }).trim(),
  headCommit: execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim(),
  dirty: execFileSync('git', ['status', '--porcelain'], { encoding: 'utf8' }).trim().length > 0,
  sources: sourceFiles.sort().map(file => /\.(tsx?|[cm]?js)$/.test(file) ? inspectTypeScript(file, root) : {
    path: normalizedPath(relative(root, file)), sha256: createHash('sha256').update(readFileSync(file)).digest('hex')
  }),
  assets: assetFiles.sort().map(file => ({ path: normalizedPath(relative(root, file)), bytes: statSync(file).size, sha256: createHash('sha256').update(readFileSync(file)).digest('hex') })),
  lockfiles: ['package-lock.json', 'rust/Cargo.lock'].map(file => ({ path: file, sha256: createHash('sha256').update(readFileSync(file)).digest('hex') })),
  runtimeAcceptance: 'pending',
  backend: null
};
const backendIndex = process.argv.indexOf('--backend');
if (backendIndex >= 0) {
  if (!process.argv[backendIndex + 1]) throw new Error('--backend requires a source directory');
  const backendRoot = resolve(process.argv[backendIndex + 1]);
  const files = [];
  scan(join(backendRoot, 'src'), files, []);
  scan(join(backendRoot, 'tests'), files, []);
  inventory.backend = {
    history: 'not established by this read-only inventory',
    sources: files.sort().map(file => {
      const text = readFileSync(file, 'utf8');
      const symbols = extname(file) === '.cs'
        ? [...text.matchAll(/^\s*public\s+(?:(?:async|override|static|virtual|sealed)\s+)*[\w<>,?\[\].]+\s+(\w+)\s*\(/gm)].map(match => match[1])
        : [...text.matchAll(/^func\s+(?:\([^\n]+?\)\s+)?(\w+)\s*\(/gm)].map(match => match[1]);
      return { path: normalizedPath(relative(backendRoot, file)), sha256: createHash('sha256').update(text).digest('hex'), publicSymbols: symbols };
    })
  };
}
mkdirSync('migration-artifacts', { recursive: true });
writeFileSync('migration-artifacts/inventory.json', `${JSON.stringify(inventory, null, 2)}\n`);
console.log(JSON.stringify({ sources: inventory.sources.length, assets: inventory.assets.length, calls: inventory.sources.reduce((total, file) => total + (file.calls?.length ?? 0), 0), backendSources: inventory.backend?.sources.length ?? 0, output: 'migration-artifacts/inventory.json' }));
