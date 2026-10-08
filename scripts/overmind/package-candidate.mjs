#!/usr/bin/env node
// Local development candidate packaging and verification. No installs, downloads or activation.
import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import {
  chmod, copyFile, lstat, mkdir, mkdtemp, open, readFile, readdir, realpath, rename, rm, stat, writeFile,
} from 'node:fs/promises';
import { constants } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const SCRIPT = fileURLToPath(import.meta.url);
const SOURCE_ROOT = path.resolve(path.dirname(SCRIPT), '../..');
const BINARIES = ['codex', 'codex-code-mode-host', 'codex-linux-sandbox'];
const MIN_NODE = '22.19.0';
const MANIFEST = 'candidate-manifest.json';
const CHECKSUMS = 'checksums.sha256';
const OMIT_DIRECTORIES = new Set(['.git', '.github', '.cache', '__tests__', 'test', 'tests', 'coverage']);
const OMIT_FILES = /(?:\.map$|\.d\.(?:ts|mts|cts)$|^\.env(?:\.|$)|^\.npmrc$|^auth\.json$|\.(?:pem|key)$)/i;

function requireValue(condition, message) {
  if (!condition) throw new Error(message);
}

function inside(root, child) {
  const relative = path.relative(root, child);
  return relative === '' || (!relative.startsWith(`..${path.sep}`) && relative !== '..' && !path.isAbsolute(relative));
}

function relativePath(value) {
  requireValue(typeof value === 'string' && value.length > 0 && !/[\x00-\x1f\x7f\\]/.test(value), 'Invalid relative package path.');
  requireValue(!path.posix.isAbsolute(value) && value.split('/').every((part) => part !== '' && part !== '.' && part !== '..'), 'Package paths must stay relative and contained.');
  return value;
}

function portablePath(value) {
  return value.split(path.sep).join('/');
}

function compareVersions(left, right) {
  const a = left.split('.').map(Number);
  const b = right.split('.').map(Number);
  for (let index = 0; index < 3; index += 1) {
    if ((a[index] ?? 0) !== (b[index] ?? 0)) return (a[index] ?? 0) - (b[index] ?? 0);
  }
  return 0;
}

function minimumNode(range = '*') {
  if (range === '*') return '0.0.0';
  const match = /^>=\s*(\d+)(?:\.(\d+))?(?:\.(\d+))?$/.exec(range.trim());
  requireValue(match, `Unsupported Node engine constraint ${JSON.stringify(range)}; review packaging for the changed dependency.`);
  return `${match[1]}.${match[2] ?? 0}.${match[3] ?? 0}`;
}

function checkNode(minimum) {
  requireValue(compareVersions(process.versions.node, minimum) >= 0, `Node.js ${minimum}+ is required; this process is ${process.versions.node}.`);
}

async function exists(file) {
  try { await lstat(file); return true; }
  catch (error) { if (error.code === 'ENOENT') return false; throw error; }
}

async function json(file) {
  const info = await stat(file);
  requireValue(info.isFile() && info.size <= 4 * 1024 * 1024, 'Expected a bounded JSON metadata file.');
  return JSON.parse(await readFile(file, 'utf8'));
}

async function sha256(file) {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(file)) hash.update(chunk);
  return hash.digest('hex');
}

function checksumText(files, manifestHash) {
  return [...files.map((file) => [file.path, file.sha256]), [MANIFEST, manifestHash]]
    .sort(([left], [right]) => left.localeCompare(right, 'en'))
    .map(([name, hash]) => `${hash}  ${name}\n`).join('');
}

function platformMatches(pkg, platform, arch) {
  const permits = (list, value) => !Array.isArray(list)
    || (!list.includes(`!${value}`) && (list.every((entry) => entry.startsWith('!')) || list.includes(value)));
  return permits(pkg.os, platform) && permits(pkg.cpu, arch);
}

function packageName(name) {
  requireValue(/^(?:@[a-zA-Z0-9._-]+\/)?[a-zA-Z0-9._-]+$/.test(name) && !name.split('/').some((part) => part === '..' || part === '.'), 'Invalid runtime dependency name.');
  return name;
}

async function findDependency(helper, requiringDir, name) {
  packageName(name);
  let directory = requiringDir;
  while (inside(helper, directory)) {
    const candidate = path.join(directory, 'node_modules', name);
    if (path.basename(directory) !== 'node_modules' && await exists(path.join(candidate, 'package.json'))) {
      const resolved = await realpath(candidate);
      requireValue(inside(path.join(helper, 'node_modules'), resolved), 'Runtime dependency symlink escapes the helper node_modules tree.');
      requireValue((await lstat(path.join(candidate, 'package.json'))).isFile(), 'Runtime package metadata must be a regular file.');
      return candidate;
    }
    if (directory === helper) break;
    directory = path.dirname(directory);
  }
  return undefined;
}

async function runtimePackages(helper, { platform, arch, lock }) {
  const root = await json(path.join(helper, 'package.json'));
  const visited = new Map();
  const omittedOptional = [];
  let requiredNode = MIN_NODE;
  async function visit(directory, isRoot = false) {
    const location = portablePath(path.relative(helper, directory));
    if (visited.has(location)) return;
    const pkg = isRoot ? root : await json(path.join(directory, 'package.json'));
    requireValue(typeof pkg.name === 'string' && typeof pkg.version === 'string', 'Runtime packages must have names and versions.');
    requireValue(platformMatches(pkg, platform, arch), `Runtime package ${pkg.name} is incompatible with ${platform}-${arch}.`);
    const node = minimumNode(pkg.engines?.node);
    if (compareVersions(node, requiredNode) > 0) requiredNode = node;
    if (!isRoot) {
      const locked = lock?.packages?.[location];
      if (lock) requireValue(locked?.version === pkg.version && !locked.link, `Installed runtime dependency ${pkg.name} disagrees with package-lock.json.`);
      visited.set(location, { location, name: pkg.name, version: pkg.version,
        ...(pkg.engines?.node ? { nodeEngine: pkg.engines.node } : {}),
        ...(locked?.integrity ? { lockIntegrity: locked.integrity } : {}), directory });
    }
    const dependencies = { ...pkg.dependencies, ...pkg.optionalDependencies };
    for (const name of Object.keys(dependencies).sort()) {
      const dependency = await findDependency(helper, directory, name);
      if (!dependency) {
        requireValue(Object.hasOwn(pkg.optionalDependencies ?? {}, name), `Missing runtime dependency ${name} required by ${pkg.name}.`);
        omittedOptional.push({ from: location || '.', name });
        continue;
      }
      const metadata = await json(path.join(dependency, 'package.json'));
      requireValue(metadata.name === name, `Runtime dependency ${name} has unexpected package metadata.`);
      if (!platformMatches(metadata, platform, arch) && Object.hasOwn(pkg.optionalDependencies ?? {}, name)) {
        omittedOptional.push({ from: location || '.', name });
        continue;
      }
      await visit(dependency);
    }
  }
  await visit(helper, true);
  const sdk = [...visited.values()].find((pkg) => pkg.name === '@cursor/sdk');
  const platformPackage = `@cursor/sdk-${platform}-${arch}`;
  const native = [...visited.values()].find((pkg) => pkg.name === platformPackage);
  requireValue(sdk && native && sdk.version === native.version, `The matching ${platformPackage} runtime package is required beside @cursor/sdk.`);
  for (const file of ['bin/rg', 'bin/cursorsandbox', 'vendor/tree-sitter/binding.node', 'vendor/tree-sitter-bash/binding.node']) {
    const item = path.join(native.directory, file);
    const info = await stat(item).catch(() => undefined);
    requireValue(info?.isFile() && info.size > 0, `Cursor platform runtime is missing ${file}.`);
    if (file.startsWith('bin/')) requireValue((info.mode & 0o111) !== 0, `Cursor platform runtime ${file} must be executable.`);
  }
  checkNode(requiredNode);
  return { root, packages: [...visited.values()].sort((a, b) => a.location.localeCompare(b.location, 'en')),
    omittedOptional: omittedOptional.sort((a, b) => `${a.from}/${a.name}`.localeCompare(`${b.from}/${b.name}`, 'en')),
    requiredNode, platformPackage };
}

async function copyRegular(source, destination) {
  const before = await lstat(source);
  requireValue(before.isFile(), 'Candidate payload must contain regular files.');
  requireValue((before.mode & 0o6000) === 0, 'Setuid/setgid files cannot enter a development candidate.');
  await mkdir(path.dirname(destination), { recursive: true, mode: 0o700 });
  await copyFile(source, destination, constants.COPYFILE_EXCL | constants.COPYFILE_FICLONE);
  await chmod(destination, (before.mode & 0o111) ? 0o700 : 0o600);
  const [sourceHash, destinationHash, after] = await Promise.all([sha256(source), sha256(destination), lstat(source)]);
  requireValue(before.size === after.size && before.mtimeMs === after.mtimeMs && sourceHash === destinationHash, 'Source changed while packaging; no candidate was published.');
}

async function executableFormat(file, arch) {
  const handle = await open(file, 'r');
  try {
    const header = Buffer.alloc(64);
    const { bytesRead } = await handle.read(header, 0, header.length, 0);
    if (bytesRead >= 2 && header.subarray(0, 2).toString() === '#!') return 'script';
    requireValue(bytesRead >= 20 && header.subarray(0, 4).equals(Buffer.from([0x7f, 0x45, 0x4c, 0x46]))
      && header[4] === 2 && header[5] === 1 && [2, 3].includes(header.readUInt16LE(16))
      && header.readUInt16LE(18) === (arch === 'x64' ? 62 : 183), 'Executable is not a compatible Linux 64-bit ELF or explicit script fixture.');
    return 'elf';
  } finally { await handle.close(); }
}

async function copyTree(source, destination, boundary, ancestors = new Set()) {
  const resolved = await realpath(source);
  requireValue(inside(boundary, resolved), 'Source symlink escapes its runtime package.');
  const info = await stat(resolved);
  if (info.isDirectory()) {
    requireValue(!ancestors.has(resolved), 'Source symlink cycle in runtime package.');
    const next = new Set([...ancestors, resolved]);
    await mkdir(destination, { recursive: true, mode: 0o700 });
    for (const entry of (await readdir(resolved)).sort()) {
      relativePath(entry);
      if (entry === 'node_modules' || OMIT_DIRECTORIES.has(entry) || OMIT_FILES.test(entry)) continue;
      await copyTree(path.join(resolved, entry), path.join(destination, entry), boundary, next);
    }
  } else {
    requireValue(info.isFile(), 'Special files cannot enter a development candidate.');
    // Materialize contained symlinks. The finished bundle never depends on source links.
    await copyRegular(resolved, destination);
  }
}

async function inventory(root, directory = root) {
  const files = [];
  const info = await lstat(directory);
  requireValue(info.isDirectory() && (info.mode & 0o077) === 0, 'Candidate directories must be private real directories.');
  for (const name of (await readdir(directory)).sort()) {
    relativePath(name);
    const item = path.join(directory, name);
    const itemInfo = await lstat(item);
    requireValue(!itemInfo.isSymbolicLink(), 'Candidate payload must not contain symlinks.');
    if (itemInfo.isDirectory()) files.push(...await inventory(root, item));
    else {
      requireValue(itemInfo.isFile(), 'Candidate payload contains a special file.');
      const relative = portablePath(path.relative(root, item));
      if (relative === MANIFEST || relative === CHECKSUMS) continue;
      requireValue([0o600, 0o700].includes(itemInfo.mode & 0o7777), `Unexpected payload permissions: ${relative}.`);
      files.push({ path: relative, bytes: itemInfo.size, mode: (itemInfo.mode & 0o7777).toString(8).padStart(4, '0'), sha256: await sha256(item) });
    }
  }
  return files.sort((a, b) => a.path.localeCompare(b.path, 'en'));
}

async function canonicalOutput(output) {
  let parent = path.dirname(path.resolve(output));
  const missing = [];
  while (!await exists(parent)) {
    missing.unshift(path.basename(parent));
    const next = path.dirname(parent);
    requireValue(next !== parent, 'Output parent cannot be resolved.');
    parent = next;
  }
  const resolved = await realpath(parent);
  requireValue(parent === resolved, 'Output path must not traverse directory symlinks.');
  return path.join(resolved, ...missing, path.basename(path.resolve(output)));
}

function launcher() {
  return `#!/usr/bin/env node
import { realpathSync } from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { pathToFileURL } from 'node:url';

const root = path.dirname(realpathSync(process.argv[1]));
try {
  const { verifyCandidate } = await import(pathToFileURL(path.join(root, 'runtime/candidate.mjs')));
  await verifyCandidate(root, { checksums: false });
  const args = process.argv.slice(2);
  const boundary = args.indexOf('--');
  const options = boundary < 0 ? args : args.slice(0, boundary);
  const cliArgs = options.includes('--no-daemon') ? args : ['--no-daemon', ...args];
  const child = spawn(path.join(root, 'bin/codex'), cliArgs, {
    stdio: 'inherit', shell: false,
    env: { ...process.env, OVERMIND_CURSOR_HELPER_DIR: path.join(root, 'cursor-helper'), OVERMIND_NODE: process.execPath },
  });
  for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) process.on(signal, () => child.kill(signal));
  child.on('error', () => { console.error('Development candidate CLI could not start.'); process.exit(1); });
  child.on('exit', (code, signal) => {
    if (signal) { process.removeAllListeners(signal); process.kill(process.pid, signal); }
    else process.exit(code ?? 1);
  });
} catch (error) {
  console.error('Development candidate preflight failed: ' + error.message);
  process.exitCode = 1;
}
`;
}

/** Package already-built, already-tested bytes. Source metadata is explicit caller attestation. */
export async function packageCandidate(options) {
  checkNode(MIN_NODE);
  requireValue(process.platform === 'linux' && ['x64', 'arm64'].includes(process.arch), 'This local candidate workflow supports Linux x64/arm64 hosts only.');
  requireValue(/^[A-Za-z0-9][A-Za-z0-9._-]{0,79}$/.test(options.version ?? '') && !options.version.includes('..'), 'Provide a safe versioned candidate identifier.');
  requireValue(/^[a-f0-9]{40}$/.test(options.sourceCommit ?? ''), 'Provide the exact 40-character source commit.');
  requireValue(['clean', 'dirty'].includes(options.sourceSnapshot), 'Explicitly mark --source-snapshot clean or dirty; commit alone cannot identify modified source.');
  requireValue((options.buildProfile ?? 'debug') === 'debug', 'Only debug development candidates are supported; no optimized release claim is made.');
  const sourceRoot = await realpath(options.sourceRoot ?? SOURCE_ROOT);
  const binDir = await realpath(options.binDir ?? path.join(sourceRoot, 'codex-rs/target/debug'));
  const helper = await realpath(options.helperDir ?? path.join(sourceRoot, 'codex-rs/overmind-cursor/helper'));
  const output = await canonicalOutput(options.output ?? '');
  requireValue(typeof options.output === 'string' && options.output.trim().length > 0, 'Provide an explicit new --output directory.');
  const activeHome = await realpath(process.env.CODEX_HOME || path.join(os.homedir(), '.codex')).catch(() => path.resolve(process.env.CODEX_HOME || path.join(os.homedir(), '.codex')));
  for (const forbidden of [sourceRoot, binDir, helper, activeHome]) {
    requireValue(!inside(forbidden, output) && !inside(output, forbidden), 'Output must be disjoint from source/build/helper trees and the active CODEX_HOME.');
  }
  requireValue(!await exists(output), 'Refusing to overwrite an existing candidate output.');
  const executables = [];
  for (const name of BINARIES) {
    const info = await lstat(path.join(binDir, name)).catch(() => undefined);
    requireValue(info?.isFile() && !info.isSymbolicLink() && info.size > 0 && (info.mode & 0o111) !== 0, `Missing nonempty executable companion ${name}.`);
    executables.push({ path: `bin/${name}`, format: await executableFormat(path.join(binDir, name), process.arch) });
  }
  for (const file of ['overmind-entry.mjs', 'dist/index.js', 'package.json', 'package-lock.json', 'LICENSE', 'NOTICE.md']) {
    const info = await lstat(path.join(helper, file)).catch(() => undefined);
    requireValue(info?.isFile() && !info.isSymbolicLink(), `Missing built helper payload ${file}.`);
  }
  const validationRecord = relativePath(options.validationRecord ?? '');
  const validationPath = await realpath(path.join(sourceRoot, validationRecord));
  requireValue(inside(sourceRoot, validationPath), 'Validation record must stay inside the source root.');
  const validationHash = await sha256(validationPath);
  const lock = await json(path.join(helper, 'package-lock.json'));
  requireValue([2, 3].includes(lock.lockfileVersion) && lock.packages, 'A package-lock v2/v3 installed dependency inventory is required.');
  const runtime = await runtimePackages(helper, { platform: process.platform, arch: process.arch, lock });
  await mkdir(path.dirname(output), { recursive: true, mode: 0o700 });
  const claim = `${output}.publish-lock`;
  await mkdir(claim, { mode: 0o700 }).catch(() => { throw new Error('Candidate output is already being packaged; publication lock exists.'); });
  let staging;
  try {
    requireValue(!await exists(output), 'Refusing to overwrite an existing candidate output.');
    staging = await mkdtemp(path.join(path.dirname(output), `.${path.basename(output)}.staging-`));
    await chmod(staging, 0o700);
    for (const name of BINARIES) await copyRegular(path.join(binDir, name), path.join(staging, 'bin', name));
    const bundledHelper = path.join(staging, 'cursor-helper');
    for (const file of ['overmind-entry.mjs', 'LICENSE', 'NOTICE.md']) await copyRegular(path.join(helper, file), path.join(bundledHelper, file));
    await copyTree(path.join(helper, 'dist'), path.join(bundledHelper, 'dist'), await realpath(path.join(helper, 'dist')));
    const { name, version, type, engines, license, dependencies, optionalDependencies, main, exports: exportsField } = runtime.root;
    await writeFile(path.join(bundledHelper, 'package.json'), `${JSON.stringify({ name, version, private: true, type, engines, license, dependencies, optionalDependencies, main, exports: exportsField }, null, 2)}\n`, { mode: 0o600 });
    for (const pkg of runtime.packages) {
      await copyTree(pkg.directory, path.join(bundledHelper, pkg.location), await realpath(pkg.directory));
    }
    await mkdir(path.join(staging, 'runtime'), { mode: 0o700 });
    await copyRegular(SCRIPT, path.join(staging, 'runtime/candidate.mjs'));
    await writeFile(path.join(staging, 'overmind.mjs'), launcher(), { mode: 0o700 });
    const files = await inventory(staging);
    const manifest = {
      schemaVersion: 1, kind: 'overmind-development-candidate', candidateVersion: options.version,
      source: { commit: options.sourceCommit, snapshot: options.sourceSnapshot, attestation: 'caller-supplied; package checksums identify the actual bundled bytes' },
      build: { profile: 'debug', platform: process.platform, arch: process.arch, executables, cliVersion: 'codex-cli 0.0.0', cliVersionEvidence: 'development build identification; packaging does not execute the CLI',
        v8ArchiveAuthentication: 'not independently verified', optimizedRelease: false },
      validation: { record: validationRecord, sha256: validationHash, scope: 'caller-supplied validation record; packager does not run application tests' },
      runtime: { node: { minimum: runtime.requiredNode, bundled: false, packagingVersion: process.versions.node },
        helper: { name: runtime.root.name, version: runtime.root.version, platformPackage: runtime.platformPackage,
          packages: runtime.packages.map(({ directory: _directory, ...pkg }) => pkg), omittedOptional: runtime.omittedOptional } },
      files,
    };
    await writeFile(path.join(staging, MANIFEST), `${JSON.stringify(manifest, null, 2)}\n`, { mode: 0o600 });
    await writeFile(path.join(staging, CHECKSUMS), checksumText(files, await sha256(path.join(staging, MANIFEST))), { mode: 0o600 });
    await verifyCandidate(staging);
    requireValue(!await exists(output), 'Refusing to overwrite an existing candidate output.');
    await rename(staging, output);
    staging = undefined;
    return { output, launcher: path.join(output, 'overmind.mjs'), fileCount: files.length, manifest };
  } finally {
    if (staging) await rm(staging, { recursive: true, force: true });
    await rm(claim, { recursive: true, force: true });
  }
}

/** Full verification hashes every payload file; launch preflight checks structure/sizes/modes. */
export async function verifyCandidate(candidate, { checksums = true } = {}) {
  const root = await realpath(candidate);
  requireValue((await lstat(candidate)).isDirectory(), 'Candidate root must be a real directory, not a symlink.');
  for (const name of [MANIFEST, CHECKSUMS]) {
    const info = await lstat(path.join(root, name));
    requireValue(info.isFile() && (info.mode & 0o7777) === 0o600, 'Candidate metadata must be private regular files.');
  }
  const manifest = await json(path.join(root, MANIFEST));
  requireValue(manifest.schemaVersion === 1 && manifest.kind === 'overmind-development-candidate'
    && manifest.build?.profile === 'debug' && manifest.build?.optimizedRelease === false
    && manifest.build?.platform === process.platform && manifest.build?.arch === process.arch
    && /^[a-f0-9]{40}$/.test(manifest.source?.commit ?? '') && ['clean', 'dirty'].includes(manifest.source?.snapshot), 'Incompatible or invalid development candidate manifest.');
  requireValue(Array.isArray(manifest.files) && manifest.files.length > 0, 'Manifest payload inventory is missing.');
  checkNode(manifest.runtime?.node?.minimum ?? MIN_NODE);
  const expectedPaths = new Set();
  for (const item of manifest.files) {
    relativePath(item.path);
    requireValue(!expectedPaths.has(item.path) && /^[a-f0-9]{64}$/.test(item.sha256)
      && Number.isSafeInteger(item.bytes) && item.bytes >= 0 && ['0600', '0700'].includes(item.mode), 'Invalid or duplicate manifest payload entry.');
    expectedPaths.add(item.path);
  }
  for (const name of [...BINARIES.map((binary) => `bin/${binary}`), 'overmind.mjs', 'runtime/candidate.mjs', 'cursor-helper/overmind-entry.mjs', 'cursor-helper/dist/index.js', 'cursor-helper/package.json']) {
    requireValue(expectedPaths.has(name), `Incomplete candidate companion payload: ${name}.`);
  }
  const actual = checksums ? await inventory(root) : await inventoryMetadata(root);
  const actualPaths = new Set(actual.map((item) => item.path));
  for (const item of manifest.files) requireValue(actualPaths.has(item.path), `Candidate payload missing: ${item.path}.`);
  for (const item of actual) requireValue(expectedPaths.has(item.path), `Candidate has unexpected payload files: ${item.path}.`);
  const expected = new Map(manifest.files.map((item) => [item.path, item]));
  for (const item of actual) {
    const recorded = expected.get(item.path);
    requireValue(recorded && recorded.bytes === item.bytes && recorded.mode === item.mode
      && (!checksums || recorded.sha256 === item.sha256), `Candidate payload verification failed: ${item.path}.`);
  }
  for (const name of BINARIES) {
    requireValue(expected.get(`bin/${name}`).mode === '0700', `Companion must be executable: ${name}.`);
    const format = await executableFormat(path.join(root, 'bin', name), process.arch);
    requireValue(manifest.build.executables?.some((item) => item.path === `bin/${name}` && item.format === format), 'Executable platform metadata disagrees with the payload.');
  }
  requireValue(await readFile(path.join(root, CHECKSUMS), 'utf8') === checksumText(manifest.files, await sha256(path.join(root, MANIFEST))), 'Checksum index disagrees with the manifest.');
  const runtime = await runtimePackages(path.join(root, 'cursor-helper'), { platform: process.platform, arch: process.arch });
  requireValue(runtime.requiredNode === manifest.runtime.node.minimum
    && runtime.platformPackage === manifest.runtime.helper.platformPackage
    && runtime.packages.length === manifest.runtime.helper.packages.length, 'Bundled runtime dependency inventory is incomplete.');
  for (const pkg of runtime.packages) {
    requireValue(manifest.runtime.helper.packages.some((recorded) => recorded.location === pkg.location && recorded.name === pkg.name && recorded.version === pkg.version), 'Bundled runtime dependency metadata disagrees with the manifest.');
  }
  return manifest;
}

async function inventoryMetadata(root, directory = root) {
  const files = [];
  const info = await lstat(directory);
  requireValue(info.isDirectory() && (info.mode & 0o077) === 0, 'Candidate directories must be private real directories.');
  for (const name of (await readdir(directory)).sort()) {
    relativePath(name);
    const item = path.join(directory, name);
    const itemInfo = await lstat(item);
    requireValue(!itemInfo.isSymbolicLink(), 'Candidate payload must not contain symlinks.');
    if (itemInfo.isDirectory()) files.push(...await inventoryMetadata(root, item));
    else {
      const relative = portablePath(path.relative(root, item));
      requireValue(itemInfo.isFile(), 'Candidate payload contains a special file.');
      if (relative === MANIFEST || relative === CHECKSUMS) continue;
      files.push({ path: relative, bytes: itemInfo.size, mode: (itemInfo.mode & 0o7777).toString(8).padStart(4, '0') });
    }
  }
  return files;
}

const HELP = `Package a private local Overmind DEVELOPMENT candidate; never installs or activates it.

Usage:
  node scripts/overmind/package-candidate.mjs pack --version ID --source-commit FULL_SHA \\
    --source-snapshot clean|dirty --validation-record RELATIVE_FILE --output NEW_DIRECTORY \\
    [--source-root DIRECTORY] [--bin-dir DIRECTORY] [--helper-dir DIRECTORY] [--build-profile debug]
  node scripts/overmind/package-candidate.mjs verify CANDIDATE_DIRECTORY
  node scripts/overmind/package-candidate.mjs --help

Default inputs: this checkout, codex-rs/target/debug, codex-rs/overmind-cursor/helper.
Output must be new and disjoint from all source inputs and the active CODEX_HOME.
Requires existing built/tested CLI + code-mode host + Linux sandbox, helper dist,
installed production dependencies, matching Cursor SDK platform runtime and Node ${MIN_NODE}+.
No installs, downloads, credential reads, config changes, Git commands or activation occur.
Full verify hashes all files; the relocatable launcher performs a lightweight structural
preflight and pins the packaged Cursor helper/current Node, adding --no-daemon for isolation.
Commit/snapshot/validation metadata is caller attestation, not authenticated build provenance.
`;

async function main(args) {
  if (args.length === 0 || args.includes('--help') || args.includes('-h')) { process.stdout.write(HELP); return; }
  const [command, ...rest] = args;
  if (command === 'verify') {
    requireValue(rest.length === 1, 'verify requires exactly one candidate directory.');
    const manifest = await verifyCandidate(rest[0]);
    console.log(JSON.stringify({ verified: true, candidateVersion: manifest.candidateVersion, files: manifest.files.length, sourceSnapshot: manifest.source.snapshot }));
    return;
  }
  requireValue(command === 'pack', 'Unknown command; use --help.');
  const names = { '--version': 'version', '--source-commit': 'sourceCommit', '--source-snapshot': 'sourceSnapshot', '--validation-record': 'validationRecord',
    '--output': 'output', '--source-root': 'sourceRoot', '--bin-dir': 'binDir', '--helper-dir': 'helperDir', '--build-profile': 'buildProfile' };
  const options = {};
  for (let index = 0; index < rest.length; index += 2) {
    const name = names[rest[index]];
    requireValue(name && !Object.hasOwn(options, name) && rest[index + 1] && !rest[index + 1].startsWith('--'), 'Unknown, duplicate or missing packaging argument; use --help.');
    options[name] = rest[index + 1];
  }
  const result = await packageCandidate(options);
  console.log(JSON.stringify({ packaged: true, output: result.output, launcher: result.launcher, files: result.fileCount, sourceSnapshot: result.manifest.source.snapshot }));
}

if (process.argv[1] && await realpath(process.argv[1]).catch(() => undefined) === await realpath(SCRIPT)) {
  main(process.argv.slice(2)).catch((error) => { console.error(`Candidate packaging failed: ${error.message}`); process.exitCode = 1; });
}
