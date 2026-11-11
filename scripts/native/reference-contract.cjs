const { execFileSync } = require('node:child_process');
const { createHash } = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');
const { gunzipSync } = require('node:zlib');

const repository = path.resolve(__dirname, '../..');
const inventoryPath = 'tests/native-reference/manifest.json';
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const fail = message => { throw new Error(`Native reference admission: ${message}`); };
const logicalPath = value => {
  if (typeof value !== 'string' || !value || value.includes('\\') || value.split('/').some(part => !/^[A-Za-z0-9_@+-][A-Za-z0-9_.@+-]*$/.test(part) || /[. ]$/.test(part) || /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(\.|$)/i.test(part))) fail(`invalid relative path: ${value}`);
  return value;
};
const absoluteLocal = value => typeof value === 'string' && (process.platform === 'win32' ? /^[A-Z]:[\\/]/.test(value) : path.isAbsolute(value)) && !/["%\r\n]/.test(value) && !value.replace(/\\/g, '/').includes('//') && !value.split(/[\\/]/).some(part => part === '.' || part === '..' || /[. ]$/.test(part));
const canonical = value => {
  if (!absoluteLocal(value)) fail('root must be an absolute, non-aliased local path');
  const normalized = path.normalize(value);
  const parsed = path.parse(normalized);
  let current = parsed.root;
  const volume = fs.lstatSync(current);
  if (!volume.isDirectory() || volume.isSymbolicLink() || fs.realpathSync.native(current) !== current) fail(`redirected/reparse volume: ${current}`);
  const ancestors = [current];
  for (const part of normalized.slice(parsed.root.length).split(path.sep).filter(Boolean)) {
    if (!fs.readdirSync(current).includes(part)) fail(`missing or case-aliased path: ${current}${part}`);
    current = path.join(current, part);
    ancestors.push(current);
    const stat = fs.lstatSync(current);
    if (stat.isSymbolicLink() || fs.realpathSync.native(current) !== current) fail(`redirected/reparse path: ${current}`);
  }
  if (process.platform === 'win32') {
    const helper = path.join(repository, 'scripts/native/reference-paths.py');
    execFileSync('cmd.exe', ['/d', '/v:off', '/s', '/c', `""python" "-B" "${helper}""`], { input: JSON.stringify(ancestors), windowsVerbatimArguments: true });
  }
  return normalized;
};
const checkedPath = (root, file) => {
  logicalPath(file);
  const result = canonical(path.join(root, file));
  const stat = fs.lstatSync(result);
  if (!stat.isFile() || stat.nlink !== 1) fail(`not an unaliased regular file: ${file}`);
  return result;
};
const git = args => {
  const parts = ['git', ...args];
  if (parts.some(value => /["%\r\n]/.test(value))) fail('invalid Git argument');
  return execFileSync('cmd.exe', ['/d', '/v:off', '/s', '/c', `"${parts.map(value => `"${value}"`).join(' ')}"`], { cwd: repository, windowsVerbatimArguments: true, maxBuffer: 32 * 1024 * 1024 });
};
const checkedBytes = (bytes, row) => {
  if (bytes.length !== row.size || hash(bytes) !== row.sha256) fail(`size/hash mismatch: ${row.path}`);
  return bytes;
};
const validateManifest = manifest => {
  if (manifest.version !== 1 || !/^[a-f0-9]{40}$/.test(manifest.sourceCommit) || !Array.isArray(manifest.files) || !manifest.files.length) fail('invalid manifest header');
  const paths = new Set();
  for (const row of manifest.files) {
    logicalPath(row.path);
    if (paths.has(row.path.toLowerCase()) || !Number.isSafeInteger(row.size) || row.size < 1 || !/^[a-f0-9]{64}$/.test(row.sha256) || !/^[a-f0-9]{64}$/.test(row.gitSha256) || !/^[a-f0-9]{40}$/.test(row.gitBlob) || !['exact', 'crlf'].includes(row.checkout)) fail(`invalid manifest entry: ${row.path}`);
    paths.add(row.path.toLowerCase());
    if (row.seed) logicalPath(row.seed);
  }
  if (!manifest.groups || Object.values(manifest.groups).some(files => !Array.isArray(files) || files.some(file => !paths.has(file.toLowerCase()) || !manifest.files.some(row => row.path === file)))) fail('invalid group closure');
  if (!manifest.bundle || !/^[a-f0-9]{64}$/.test(manifest.bundle.sha256)) fail('invalid frozen bundle');
  logicalPath(manifest.bundle.path);
  if (!manifest.packages || typeof manifest.captureGate !== 'string' || !manifest.captureGate || Object.values(manifest.packages).some(pin => typeof pin.version !== 'string' || !/^sha512-[A-Za-z0-9+/]+={0,2}$/.test(pin.integrity) || pin.admitted !== false)) fail('unreviewed package/capture admission');
  if (manifest.packageLock?.path !== 'package-lock.json' || !/^[a-f0-9]{40}$/.test(manifest.packageLock.gitBlob) || !/^[a-f0-9]{64}$/.test(manifest.packageLock.gitSha256)) fail('invalid package-lock provenance');
  return manifest;
};
const checkProvenance = (row, commit, readGit = git) => {
  const listing = readGit(['ls-tree', commit, '--', row.path]).toString('utf8').trim();
  if (listing !== `100644 blob ${row.gitBlob}\t${row.path}`) fail(`Git path/blob mismatch: ${row.path}`);
  const bytes = readGit(['cat-file', 'blob', row.gitBlob]);
  if (hash(bytes) !== row.gitSha256) fail(`Git hash mismatch: ${row.path}`);
  return bytes;
};
const openReference = (options = {}) => {
  const manifest = validateManifest(JSON.parse(fs.readFileSync(checkedPath(repository, inventoryPath), 'utf8')));
  const lock = JSON.parse(checkProvenance(manifest.packageLock, manifest.sourceCommit));
  for (const [name, pin] of Object.entries(manifest.packages)) {
    const original = lock.packages[`node_modules/${name}`];
    if (!original || original.version !== pin.version || original.integrity !== pin.integrity) fail(`package lock provenance mismatch: ${name}`);
  }
  const root = canonical(options.root === undefined ? (process.env.ZABOR_NATIVE_REFERENCE_ROOT || repository) : options.root);
  const rows = new Map(manifest.files.map(row => [row.path, row]));
  const validate = (file, bytes) => {
    const row = rows.get(logicalPath(file));
    if (!row) fail(`path outside retained closure: ${file}`);
    checkedBytes(bytes, row);
    const normalized = row.checkout === 'crlf' ? Buffer.from(bytes.toString('utf8').replace(/\r\n/g, '\n')) : bytes;
    if (!normalized.equals(checkProvenance(row, manifest.sourceCommit))) fail(`working/Git provenance mismatch: ${file}`);
    return bytes;
  };
  const filePath = file => {
    if (!rows.has(logicalPath(file))) fail(`path outside retained closure: ${file}`);
    const result = checkedPath(root, file);
    validate(file, fs.readFileSync(result));
    return result;
  };
  const read = file => validate(file, fs.readFileSync(filePath(file)));
  const group = name => {
    if (!Object.hasOwn(manifest.groups, name)) fail(`unknown group: ${name}`);
    return Object.fromEntries(manifest.groups[name].map(file => [file, hash(read(file))]));
  };
  const requirePackage = name => {
    const pin = manifest.packages[name];
    if (!pin) fail(`unlisted package: ${name}`);
    if (process.env.ZABOR_NATIVE_PACKAGE_REFERENCE_ROOT) return require(packageFile(name, 'package.json', true));
    fail(`package closure not frozen: ${name}@${pin.version}; lock integrity ${pin.integrity}; no checkout/node_modules fallback`);
  };
  let packageRoot;
  let packageRows;
  const packageFile = (name, file, entry = false) => {
    logicalPath(file);
    const pin = manifest.packages[name];
    if (!pin) fail(`unlisted package: ${name}`);
    if (!process.env.ZABOR_NATIVE_PACKAGE_REFERENCE_ROOT) return requirePackage(name);
    if (!packageRoot) {
      const candidate = canonical(process.env.ZABOR_NATIVE_PACKAGE_REFERENCE_ROOT);
      const script = checkedPath(repository, 'scripts/native/package-reference.py');
      const checked = JSON.parse(execFileSync('cmd.exe', ['/d', '/v:off', '/s', '/c', `""python" "-B" "${script}" "check" "--root" "${candidate}""`], { windowsVerbatimArguments: true, maxBuffer: 1024 * 1024 }));
      const bytes = fs.readFileSync(checkedPath(repository, 'tests/native-delivery/closure.json'));
      if (checked.root !== candidate || checked.closureSha256 !== hash(bytes) || checked.nativeClientReady !== false) fail('package reference verification mismatch');
      packageRows = new Map(JSON.parse(bytes).packages.map(row => [row.path, row]));
      packageRoot = candidate;
    }
    const directory = `node_modules/${name}`;
    const row = packageRows.get(directory);
    if (!row || row.version !== pin.version || row.integrity !== pin.integrity) fail(`package reference pin mismatch: ${name}`);
    let result = checkedPath(packageRoot, `${directory}/${file}`);
    if (entry) {
      const metadata = JSON.parse(fs.readFileSync(result));
      result = require.resolve(path.dirname(result));
      const relative = path.relative(path.join(packageRoot, directory), result).replaceAll('\\', '/');
      if (metadata.version !== pin.version || !row.files.some(item => item.path === relative)) fail(`package entry outside frozen inventory: ${name}`);
      result = checkedPath(packageRoot, `${directory}/${relative}`);
    } else if (!row.files.some(item => item.path === file)) fail(`package file outside frozen inventory: ${name}/${file}`);
    return result;
  };
  const packagePath = (name, file) => packageFile(name, file);
  const capture = () => { fail(manifest.captureGate); };
  const snapshot = name => {
    if (!Object.hasOwn(manifest.groups, name)) fail(`unknown group: ${name}`);
    return { root, files: Object.fromEntries(manifest.groups[name].map(file => [file, read(file).toString('utf8')])) };
  };
  return { root, manifest, path: filePath, read, text: file => read(file).toString('utf8'), group, snapshot, requirePackage, packagePath, capture, validate };
};
const freezeReference = destination => {
  if (!absoluteLocal(destination)) fail('invalid freeze destination');
  const target = path.normalize(destination);
  logicalPath(path.basename(target));
  canonical(path.dirname(target));
  if (fs.lstatSync(target, { throwIfNoEntry: false })) fail('existing freeze destination preserved');
  const reference = openReference({ root: repository });
  const bundleBytes = fs.readFileSync(checkedPath(repository, reference.manifest.bundle.path));
  if (hash(bundleBytes) !== reference.manifest.bundle.sha256) fail('frozen bundle hash mismatch');
  const bundle = JSON.parse(gunzipSync(bundleBytes).toString('utf8'));
  const bundled = reference.manifest.files.filter(row => !row.seed).map(row => row.path);
  if (Object.keys(bundle).length !== bundled.length || Object.keys(bundle).some(file => !bundled.includes(file))) fail('frozen bundle closure mismatch');
  const files = reference.manifest.files.map(row => {
    const bytes = row.seed ? fs.readFileSync(checkedPath(repository, row.seed)) : Buffer.from(bundle[row.path], 'base64');
    return [row.path, reference.validate(row.path, bytes)];
  });
  const directories = new Set();
  let createdDestination = false;
  try {
    fs.mkdirSync(target);
    createdDestination = true;
    directories.add(target);
    for (const [file, bytes] of files) {
      let directory = target;
      for (const part of file.split('/').slice(0, -1)) {
        directory = path.join(directory, part);
        if (!directories.has(directory)) {
          fs.mkdirSync(directory);
          directories.add(directory);
        }
      }
      const output = path.join(target, file);
      const handle = fs.openSync(output, 'wx');
      try { fs.writeFileSync(handle, bytes); } finally { fs.closeSync(handle); }
    }
    const frozen = openReference({ root: target });
    for (const [file] of files) frozen.read(file);
    return { root: target, files: files.length, manifestSha256: hash(fs.readFileSync(checkedPath(repository, inventoryPath))), cleanupPolicy: 'preserve-on-failure', packagesAdmitted: false, visualAcceptance: false };
  } catch (error) {
    const failure = new Error(`Freeze failed; no automatic deletion performed; inspect preserved destination: ${target}`, { cause: error });
    failure.code = 'NATIVE_REFERENCE_FREEZE_PRESERVED';
    failure.freezeFailure = { status: 'failed', destination: target, createdDestination, cleanupPolicy: 'preserve-no-object-bound-delete', cleanupPerformed: false, referenceValidated: false, cause: error.message };
    throw failure;
  }
};

module.exports = { openReference, freezeReference, validateManifest, logicalPath, checkProvenance };
if (require.main === module) {
  const [operation, value, ...extra] = process.argv.slice(2);
  try {
    if (extra.length) fail('unexpected arguments');
    if (operation === '--read' && value) process.stdout.write(openReference().read(value));
    else if (operation === '--snapshot' && value) process.stdout.write(JSON.stringify(openReference().snapshot(value)));
    else if (operation === '--check' && value) process.stdout.write(JSON.stringify(openReference().group(value)));
    else if (operation === '--freeze' && value) process.stdout.write(JSON.stringify(freezeReference(value)));
    else fail('usage: reference-contract.cjs --read PATH | --snapshot GROUP | --check GROUP | --freeze ABSOLUTE_DIRECTORY');
  } catch (error) {
    console.error(error.freezeFailure ? JSON.stringify({ message: error.message, code: error.code, ...error.freezeFailure }) : error.message);
    process.exitCode = 1;
  }
}
