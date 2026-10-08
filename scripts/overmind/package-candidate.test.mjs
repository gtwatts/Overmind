import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { chmod, lstat, mkdir, mkdtemp, readFile, readdir, rename, rm, symlink, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';
import test from 'node:test';
import { packageCandidate, verifyCandidate } from './package-candidate.mjs';

const execute = promisify(execFile);
const script = fileURLToPath(new URL('./package-candidate.mjs', import.meta.url));
const sourceCommit = 'a'.repeat(40);
const platformPackage = `@cursor/sdk-${process.platform}-${process.arch}`;

async function put(file, content, mode = 0o600) {
  await mkdir(path.dirname(file), { recursive: true, mode: 0o700 });
  await writeFile(file, content, { mode });
}

async function fixture(t) {
  const root = await mkdtemp(path.join(os.tmpdir(), 'overmind-package-test-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const sourceRoot = path.join(root, 'source with spaces');
  const binDir = path.join(sourceRoot, 'bin');
  const helperDir = path.join(sourceRoot, 'helper');
  const lock = { lockfileVersion: 3, packages: {} };
  async function dependency(location, metadata, files = {}) {
    const directory = path.join(helperDir, location);
    await put(path.join(directory, 'package.json'), JSON.stringify({ type: 'module', exports: './index.js', ...metadata }));
    await put(path.join(directory, 'LICENSE'), 'Synthetic fixture license.\n');
    lock.packages[location] = { version: metadata.version, integrity: 'sha512-fixture-metadata-not-authenticated' };
    for (const [file, contents] of Object.entries(files)) await put(path.join(directory, file), contents);
    return directory;
  }
  await put(path.join(binDir, 'codex'), `#!/bin/sh
exec "$OVERMIND_NODE" --input-type=module -e '
import path from "node:path";
import { pathToFileURL } from "node:url";
const helper = process.env.OVERMIND_CURSOR_HELPER_DIR;
const { value } = await import(pathToFileURL(path.join(helper, "dist/index.js")));
console.log(JSON.stringify({ helper, node: process.env.OVERMIND_NODE, args: process.argv.slice(1), value }));
' -- "$@"
`, 0o700);
  for (const name of ['codex-code-mode-host', 'codex-linux-sandbox']) await put(path.join(binDir, name), '#!/bin/sh\nexit 0\n', 0o700);
  await put(path.join(helperDir, 'package.json'), JSON.stringify({ name: 'fixture-helper', version: '0.4.0', type: 'module',
    engines: { node: '>=22.19.0' }, dependencies: { '@cursor/sdk': '1.0.30', 'proxy-agent': '8.0.2', undici: '8.10.0' }, devDependencies: { vitest: '0.0.1' },
    scripts: { build: 'never execute this development script' } }));
  await put(path.join(helperDir, 'overmind-entry.mjs'), 'export * from "./dist/index.js";\n');
  await put(path.join(helperDir, 'LICENSE'), 'Synthetic helper license.\n');
  await put(path.join(helperDir, 'NOTICE.md'), 'Synthetic helper notice.\n');
  await put(path.join(helperDir, 'dist/index.js'), 'import { sdk } from "@cursor/sdk"; import { proxy } from "proxy-agent"; import { data } from "undici"; export const value = `${sdk}/${proxy}/${data}`;\n');
  await put(path.join(helperDir, 'dist/index.js.map'), JSON.stringify({ sources: ['/private/checkout/source.ts'] }));
  await put(path.join(helperDir, 'dist/index.d.ts'), 'export declare const value: string;\n');
  await put(path.join(helperDir, 'dist/.env.private'), 'SYNTHETIC_SECRET=exclude-this-file\n');
  await put(path.join(helperDir, 'src/developer.ts'), 'throw new Error("not a runtime file");\n');
  await dependency('node_modules/@cursor/sdk', { name: '@cursor/sdk', version: '1.0.30', dependencies: { shared: '2.0.0' },
    optionalDependencies: { [platformPackage]: '1.0.30', '@cursor/sdk-nonexistent-platform': '1.0.30' } }, {
    'index.js': 'import { version } from "shared"; export const sdk = `sdk-${version}`;\n',
    'asset.json': '{"necessary":"data"}\n',
  });
  await dependency('node_modules/@cursor/sdk/node_modules/shared', { name: 'shared', version: '2.0.0' }, { 'index.js': 'export const version = "2";\n' });
  await dependency('node_modules/shared', { name: 'shared', version: '1.0.0' }, { 'index.js': 'export const version = "1";\n' });
  await dependency('node_modules/proxy-agent', { name: 'proxy-agent', version: '8.0.2', dependencies: { shared: '1.0.0' } }, {
    'index.js': 'import { version } from "shared"; export const proxy = `proxy-${version}`;\n',
    'test/irrelevant.js': 'throw new Error("not runtime");\n',
  });
  await dependency('node_modules/undici', { name: 'undici', version: '8.10.0', engines: { node: '>=22.19.0' } }, {
    'index.js': 'import { readFileSync } from "node:fs"; export const data = readFileSync(new URL("./data/runtime.wasm", import.meta.url), "utf8").trim();\n',
    'data/runtime.wasm': 'synthetic-wasm-data\n',
    'data/empty-runtime-file': '',
  });
  const native = await dependency(`node_modules/${platformPackage}`, { name: platformPackage, version: '1.0.30', os: [process.platform], cpu: [process.arch] }, {
    'index.js': 'export {};\n', 'bin/rg': '#!/bin/sh\nexit 0\n', 'bin/cursorsandbox': '#!/bin/sh\nexit 0\n',
    'vendor/tree-sitter/binding.node': 'synthetic-native-binding\n', 'vendor/tree-sitter-bash/binding.node': 'synthetic-bash-binding\n',
  });
  for (const name of ['rg', 'cursorsandbox']) await chmod(path.join(native, 'bin', name), 0o700);
  await dependency('node_modules/vitest', { name: 'vitest', version: '0.0.1' }, { 'index.js': 'throw new Error("development dependency must be excluded");\n' });
  await put(path.join(helperDir, 'package-lock.json'), JSON.stringify(lock));
  await put(path.join(sourceRoot, 'validation.md'), 'Synthetic validation record; no model or credential access.\n');
  return { root, sourceRoot, binDir, helperDir, lock, native,
    options: { version: '2026-10-08-fixture-dev.1', sourceCommit, sourceSnapshot: 'dirty', validationRecord: 'validation.md', sourceRoot, binDir, helperDir, output: path.join(root, 'candidate-1') } };
}

test('packages only the production closure, preserves nested versions/data/licenses and records development provenance', async (t) => {
  const fx = await fixture(t);
  const { manifest, output } = await packageCandidate(fx.options);
  assert.equal(manifest.source.snapshot, 'dirty');
  assert.equal(manifest.build.optimizedRelease, false);
  assert.equal(manifest.build.v8ArchiveAuthentication, 'not independently verified');
  assert.equal(manifest.runtime.node.minimum, '22.19.0');
  assert.equal(manifest.runtime.node.bundled, false);
  assert.equal(manifest.runtime.helper.packages.length, 6);
  const files = manifest.files.map((item) => item.path);
  assert(files.includes('cursor-helper/node_modules/@cursor/sdk/node_modules/shared/index.js'));
  assert(files.includes('cursor-helper/node_modules/undici/data/runtime.wasm'));
  assert(files.includes('cursor-helper/node_modules/undici/data/empty-runtime-file'));
  assert(files.includes(`cursor-helper/node_modules/${platformPackage}/vendor/tree-sitter/binding.node`));
  assert(files.includes('cursor-helper/NOTICE.md'));
  assert(!files.some((file) => /vitest|developer|\.map$|\.d\.ts$|\.env|\/test\//.test(file)));
  const helperMetadata = JSON.parse(await readFile(path.join(output, 'cursor-helper/package.json'), 'utf8'));
  assert.equal(helperMetadata.scripts, undefined);
  assert.equal(helperMetadata.devDependencies, undefined);
  assert(!JSON.stringify(manifest).includes(fx.sourceRoot));
  assert(!await readFile(path.join(output, 'overmind.mjs'), 'utf8').then((text) => text.includes(fx.sourceRoot)));
  assert.equal((await lstat(output)).mode & 0o777, 0o700);
  assert.equal((await lstat(path.join(output, 'bin/codex'))).mode & 0o777, 0o700);
  assert.equal((await verifyCandidate(output)).candidateVersion, fx.options.version);
});

test('launcher works after relocation and source removal, through a symlink, with spaced arguments and pinned helper/Node', async (t) => {
  const fx = await fixture(t);
  await packageCandidate(fx.options);
  const relocated = path.join(fx.root, 'relocated candidate');
  await rename(fx.options.output, relocated);
  await rm(fx.sourceRoot, { recursive: true });
  const alias = path.join(fx.root, 'codex-alias');
  await symlink(path.join(relocated, 'overmind.mjs'), alias);
  const { stdout } = await execute(process.execPath, [alias, '--version', 'argument with spaces'], { cwd: os.tmpdir(),
    env: { PATH: process.env.PATH, NODE_OPTIONS: '--preserve-symlinks-main', OVERMIND_CURSOR_HELPER_DIR: '/wrong/helper', OVERMIND_NODE: '/wrong/node' } });
  const observed = JSON.parse(stdout);
  assert.equal(observed.helper, path.join(relocated, 'cursor-helper'));
  assert.equal(observed.node, process.execPath);
  assert.deepEqual(observed.args, ['--no-daemon', '--version', 'argument with spaces']);
  assert.equal(observed.value, 'sdk-2/proxy-1/synthetic-wasm-data');
  await verifyCandidate(relocated);
});

test('the same source bytes and metadata produce identical manifests/checksum inventories at different output paths', async (t) => {
  const fx = await fixture(t);
  const first = await packageCandidate(fx.options);
  const second = await packageCandidate({ ...fx.options, output: path.join(fx.root, 'candidate-2') });
  assert.deepEqual(first.manifest, second.manifest);
  assert.equal(await readFile(path.join(first.output, 'checksums.sha256'), 'utf8'), await readFile(path.join(second.output, 'checksums.sha256'), 'utf8'));
});

test('launcher adds no-daemon only when absent before the positional boundary', async (t) => {
  const fx = await fixture(t);
  await packageCandidate(fx.options);
  const launcher = path.join(fx.options.output, 'overmind.mjs');
  for (const [supplied, expected] of [
    [['--no-daemon', '--version'], ['--no-daemon', '--version']],
    [['--version', '--no-daemon'], ['--version', '--no-daemon']],
    [['exec', '--', '--no-daemon'], ['--no-daemon', 'exec', '--', '--no-daemon']],
    [['--no-daemon', 'exec', '--', '--no-daemon'], ['--no-daemon', 'exec', '--', '--no-daemon']],
  ]) {
    const { stdout } = await execute(process.execPath, [launcher, ...supplied], { env: { PATH: process.env.PATH } });
    assert.deepEqual(JSON.parse(stdout).args, expected, `supplied ${JSON.stringify(supplied)}`);
  }
});

test('requires explicit source snapshot, valid commit and debug profile before publication', async (t) => {
  const fx = await fixture(t);
  for (const [override, message] of [
    [{ sourceSnapshot: undefined }, /source-snapshot/], [{ sourceCommit: 'short' }, /exact 40/],
    [{ buildProfile: 'release' }, /debug development/], [{ version: '../unsafe' }, /candidate identifier/],
  ]) await assert.rejects(packageCandidate({ ...fx.options, ...override }), message);
  assert(!await lstat(fx.options.output).catch(() => undefined));
});

test('missing companions, built helper, mandatory dependencies and native SDK assets fail preflight', async (t) => {
  for (const [relative, message] of [
    ['bin/codex-code-mode-host', /companion codex-code-mode-host/],
    ['helper/dist/index.js', /built helper payload dist\/index.js/],
    ['helper/node_modules/proxy-agent', /Missing runtime dependency proxy-agent/],
    [`helper/node_modules/${platformPackage}`, /matching .* runtime package/],
    [`helper/node_modules/${platformPackage}/bin/cursorsandbox`, /missing bin\/cursorsandbox/],
  ]) {
    const fx = await fixture(t);
    await rm(path.join(fx.sourceRoot, relative), { recursive: true });
    await assert.rejects(packageCandidate(fx.options), message);
    assert(!await lstat(fx.options.output).catch(() => undefined));
  }
});

test('installed runtime versions must agree with the lock and executable permissions/platform must match', async (t) => {
  const fx = await fixture(t);
  fx.lock.packages['node_modules/proxy-agent'].version = '999.0.0';
  await put(path.join(fx.helperDir, 'package-lock.json'), JSON.stringify(fx.lock));
  await assert.rejects(packageCandidate(fx.options), /disagrees with package-lock/);
  fx.lock.packages['node_modules/proxy-agent'].version = '8.0.2';
  await put(path.join(fx.helperDir, 'package-lock.json'), JSON.stringify(fx.lock));
  await chmod(path.join(fx.binDir, 'codex-linux-sandbox'), 0o600);
  await assert.rejects(packageCandidate(fx.options), /executable companion codex-linux-sandbox/);
  const header = Buffer.alloc(64);
  header.set([0x7f, 0x45, 0x4c, 0x46, 2, 1]);
  header.writeUInt16LE(2, 16);
  header.writeUInt16LE(process.arch === 'x64' ? 183 : 62, 18);
  await put(path.join(fx.binDir, 'codex-linux-sandbox'), header);
  await chmod(path.join(fx.binDir, 'codex-linux-sandbox'), 0o700);
  await assert.rejects(packageCandidate(fx.options), /compatible Linux/);
});

test('refuses output inside source/current CODEX_HOME, symlinked parents and existing output without modifying either', async (t) => {
  const fx = await fixture(t);
  await assert.rejects(packageCandidate({ ...fx.options, output: path.join(fx.sourceRoot, 'candidate') }), /disjoint/);
  const activeHome = path.join(fx.root, 'isolated-active-home');
  await mkdir(activeHome, { mode: 0o700 });
  await put(path.join(activeHome, 'marker'), 'must remain unchanged');
  const original = process.env.CODEX_HOME;
  process.env.CODEX_HOME = activeHome;
  try { await assert.rejects(packageCandidate({ ...fx.options, output: path.join(activeHome, 'candidate') }), /disjoint/); }
  finally { if (original === undefined) delete process.env.CODEX_HOME; else process.env.CODEX_HOME = original; }
  assert.equal(await readFile(path.join(activeHome, 'marker'), 'utf8'), 'must remain unchanged');
  const linked = path.join(fx.root, 'linked-output');
  await symlink(activeHome, linked);
  await assert.rejects(packageCandidate({ ...fx.options, output: path.join(linked, 'candidate') }), /directory symlinks/);
  await packageCandidate(fx.options);
  const before = await readFile(path.join(fx.options.output, 'candidate-manifest.json'), 'utf8');
  await assert.rejects(packageCandidate(fx.options), /overwrite/);
  assert.equal(await readFile(path.join(fx.options.output, 'candidate-manifest.json'), 'utf8'), before);
});

test('contained runtime symlinks are materialized; escaping links abort atomically and clean staging/lock', async (t) => {
  const fx = await fixture(t);
  const sdk = path.join(fx.helperDir, 'node_modules/@cursor/sdk');
  await symlink('asset.json', path.join(sdk, 'asset-alias.json'));
  await packageCandidate(fx.options);
  assert.equal((await lstat(path.join(fx.options.output, 'cursor-helper/node_modules/@cursor/sdk/asset-alias.json'))).isSymbolicLink(), false);
  assert.equal(await readFile(path.join(fx.options.output, 'cursor-helper/node_modules/@cursor/sdk/asset-alias.json'), 'utf8'), '{"necessary":"data"}\n');
  await put(path.join(fx.root, 'outside-private-file'), 'synthetic value must never enter the bundle');
  await symlink(path.join(fx.root, 'outside-private-file'), path.join(sdk, 'escape.txt'));
  const failedOutput = path.join(fx.root, 'failed-candidate');
  await assert.rejects(packageCandidate({ ...fx.options, output: failedOutput }), /symlink escapes/);
  assert(!await lstat(failedOutput).catch(() => undefined));
  assert(!(await readdir(fx.root)).some((name) => name.includes('failed-candidate')));
});

test('full verification catches same-size corruption, missing/extra files, external links and permission changes', async (t) => {
  const fx = await fixture(t);
  await packageCandidate(fx.options);
  const asset = path.join(fx.options.output, 'cursor-helper/node_modules/@cursor/sdk/asset.json');
  const original = await readFile(asset);
  const changed = Buffer.from(original);
  changed[changed.length - 2] ^= 1;
  await writeFile(asset, changed);
  await assert.rejects(verifyCandidate(fx.options.output), /payload verification failed/);
  await writeFile(asset, original);
  await put(path.join(fx.options.output, 'unexpected'), 'extra');
  await assert.rejects(verifyCandidate(fx.options.output), /unexpected payload files/);
  await rm(path.join(fx.options.output, 'unexpected'));
  await rm(asset);
  await symlink(path.join(fx.root, 'missing-external-target'), asset);
  await assert.rejects(verifyCandidate(fx.options.output), /must not contain symlinks/);
  await rm(asset);
  await put(asset, original);
  await chmod(path.join(fx.options.output, 'bin/codex-code-mode-host'), 0o600);
  await assert.rejects(verifyCandidate(fx.options.output), /payload verification failed/);
});

test('launcher rejects an incomplete companion before starting the CLI', async (t) => {
  const fx = await fixture(t);
  await packageCandidate(fx.options);
  await rm(path.join(fx.options.output, 'bin/codex-code-mode-host'));
  await assert.rejects(execute(process.execPath, [path.join(fx.options.output, 'overmind.mjs'), '--version']), (error) => {
    assert.match(error.stderr, /preflight failed/);
    assert.equal(error.stdout, '');
    return true;
  });
});

test('launcher reports missing helper and missing Node without starting an application', async (t) => {
  const fx = await fixture(t);
  await packageCandidate(fx.options);
  const launcher = path.join(fx.options.output, 'overmind.mjs');
  await assert.rejects(execute(launcher, ['--version'], { env: { PATH: path.join(fx.root, 'no-node-on-path') } }), (error) => {
    assert.match(error.stderr, /node.*(?:No such file|not found)/i);
    return true;
  });
  await rm(path.join(fx.options.output, 'cursor-helper/dist/index.js'));
  await assert.rejects(execute(process.execPath, [launcher, '--version']), (error) => {
    assert.match(error.stderr, /preflight failed: Candidate payload missing: cursor-helper\/dist\/index\.js/);
    assert.equal(error.stdout, '');
    return true;
  });
});

test('fake-prefix backup, atomic switch and rollback preserve the original executable', async (t) => {
  const fx = await fixture(t);
  await packageCandidate(fx.options);
  const prefix = path.join(fx.root, 'fake-user-prefix');
  const active = path.join(prefix, 'bin/codex');
  const backup = path.join(prefix, 'activation-backups/original-codex');
  const original = '#!/bin/sh\nprintf "original fixture executable\\n"\n';
  await put(active, original, 0o700);
  await put(backup, await readFile(active), 0o700);
  const next = `${active}.overmind-next`;
  await symlink(path.join(fx.options.output, 'overmind.mjs'), next);
  await rename(next, active);
  const switched = await execute(active, ['--version'], { env: { PATH: process.env.PATH } });
  assert.equal(JSON.parse(switched.stdout).value, 'sdk-2/proxy-1/synthetic-wasm-data');
  assert.equal(await readFile(backup, 'utf8'), original);
  const rollback = `${active}.overmind-rollback`;
  await put(rollback, await readFile(backup), 0o700);
  await rename(rollback, active);
  assert.equal((await lstat(active)).isSymbolicLink(), false);
  assert.equal(await readFile(active, 'utf8'), original);
  assert.equal((await execute(active)).stdout, 'original fixture executable\n');
});

test('parallel publication to one output permits one result and never overwrites it', async (t) => {
  const fx = await fixture(t);
  const results = await Promise.allSettled([packageCandidate(fx.options), packageCandidate(fx.options)]);
  assert.equal(results.filter((result) => result.status === 'fulfilled').length, 1);
  assert.match(results.find((result) => result.status === 'rejected').reason.message, /already being packaged|overwrite/);
  await verifyCandidate(fx.options.output);
  assert(!(await readdir(fx.root)).some((name) => /\.staging-|\.publish-lock$/.test(name)));
});

test('CLI help is explicit and verify works from the bundled runtime after checkout removal', async (t) => {
  const help = await execute(process.execPath, [script, '--help']);
  assert.match(help.stdout, /DEVELOPMENT candidate/);
  assert.match(help.stdout, /source-snapshot clean\|dirty/);
  assert.match(help.stdout, /No installs, downloads, credential reads/);
  const fx = await fixture(t);
  await packageCandidate(fx.options);
  await rm(fx.sourceRoot, { recursive: true });
  const result = await execute(process.execPath, [path.join(fx.options.output, 'runtime/candidate.mjs'), 'verify', fx.options.output], { cwd: os.tmpdir() });
  assert.equal(JSON.parse(result.stdout).verified, true);
});
