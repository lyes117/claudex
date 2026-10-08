import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, existsSync, rmSync, symlinkSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { createServer } from 'node:net';
import { patchObserverSource } from './claude-memory-build.mjs';
import { prepareInstallation, activateInstallation, assertOwnedRoot, assertPortFree, controlWorker } from './claude-memory.mjs';
import { VERSION, REVISION, MANIFEST, IDENTITY_SCHEME, sha256, verifyBundle, loadInstallation, runtimeEnvironment, probeOwnedWorker, spawnRuntime } from './claude-memory-runtime.mjs';

function fixture(t) {
  const home = mkdtempSync(join(tmpdir(), 'claudex-memory-test-'));
  t.after(() => rmSync(home, { recursive: true, force: true }));
  const packageRoot = join(home, 'package');
  const plugin = join(packageRoot, 'plugin');
  for (const folder of ['scripts', '.codex-plugin', 'hooks']) mkdirSync(join(plugin, folder), { recursive: true });
  for (const file of ['worker-service.cjs', 'mcp-server.cjs']) writeFileSync(join(plugin, 'scripts', file), '// synthetic test fixture\n');
  writeFileSync(join(plugin, 'scripts', 'claude-memory-project-identity.mjs'), readFileSync(new URL('./claude-memory-project-identity.mjs', import.meta.url)));
  writeFileSync(join(plugin, '.codex-plugin', 'plugin.json'), JSON.stringify({ name: 'upstream', version: '0' }));
  writeFileSync(join(packageRoot, 'LICENSE'), 'Synthetic fixture; contains no upstream runtime');
  const hashes = Object.fromEntries(['worker-service.cjs', 'mcp-server.cjs', 'claude-memory-project-identity.mjs'].map(file => [`plugin/scripts/${file}`, sha256(join(plugin, 'scripts', file))]));
  writeFileSync(join(packageRoot, 'claudex-observer-build.json'), JSON.stringify({ revision: REVISION, version: VERSION, toolsEnabled: false, privateUserHome: true, portReclaimDisabled: true, projectIdentity: { scheme: IDENTITY_SCHEME }, sha256: hashes }));
  const root = join(home, 'owned');
  return { home, root, packageRoot, binary: process.execPath, bun: process.execPath, codexHome: join(home, 'native-auth'), checkPort: false };
}

test('pre-cx1 bundle is rejected before creating an installation root', async t => {
  const f = fixture(t);
  const receiptPath = join(f.packageRoot, 'claudex-observer-build.json');
  const receipt = JSON.parse(readFileSync(receiptPath, 'utf8'));
  delete receipt.projectIdentity;
  writeFileSync(receiptPath, JSON.stringify(receipt));
  await assert.rejects(prepareInstallation(f), /unsupported/);
  assert.equal(existsSync(f.root), false);
});

test('bundle without the no-reclaim policy is refused before preparation', async t => {
  const f = fixture(t);
  const path = join(f.packageRoot, 'claudex-observer-build.json');
  const receipt = JSON.parse(readFileSync(path, 'utf8'));
  delete receipt.portReclaimDisabled;
  writeFileSync(path, JSON.stringify(receipt));
  await assert.rejects(prepareInstallation(f), /unsupported/);
  assert.equal(existsSync(f.root), false);
});

test('installation refuses a missing identity attestation or altered resolver', async t => {
  const f = fixture(t);
  const receipt = await prepareInstallation(f);
  const manifestPath = join(f.root, MANIFEST);
  const oldReceipt = { ...receipt };
  delete oldReceipt.projectIdentity;
  writeFileSync(manifestPath, JSON.stringify(oldReceipt));
  assert.throws(() => loadInstallation(f.root), /unsupported/);
  writeFileSync(manifestPath, JSON.stringify(receipt));
  writeFileSync(join(receipt.pluginRoot, 'scripts', 'claude-memory-project-identity.mjs'), '// altered resolver');
  assert.throws(() => loadInstallation(f.root), /integrity mismatch/);
});

test('observer patch is exact, local to app-server, and rejects unknown source', () => {
  const input = "    project_doc_max_bytes: 0,\n  if (scopedCodexHome) result.CODEX_HOME = scopedCodexHome;\nthrow codexSetupError(`Cannot read Codex ChatGPT auth: ${error instanceof Error ? error.message : String(error)}`);";
  const output = patchObserverSource(input);
  assert.match(output, /'tools.enabled': false/);
  assert.match(output, /'hooks.enabled': false/);
  assert.match(output, /result.USERPROFILE = privateUserHome/);
  assert.doesNotMatch(output, /process.env.HOME\s*=/);
  assert.doesNotMatch(output, /error.message/);
  assert.throws(() => patchObserverSource('unknown source'), /exactly once/);
  assert.throws(() => patchObserverSource(input + input), /exactly once/);
});

test('bundle verification refuses unpatched or modified distribution', t => {
  const f = fixture(t);
  assert.equal(verifyBundle(f.packageRoot).revision, REVISION);
  writeFileSync(join(f.packageRoot, 'plugin/scripts/worker-service.cjs'), 'modified');
  assert.throws(() => verifyBundle(f.packageRoot), /integrity/);
  assert.throws(() => verifyBundle(f.home), /13.28.0/);
});

test('owned preparation creates isolated native manifests without activating or rewriting a project', async t => {
  const f = fixture(t);
  const receipt = await prepareInstallation(f);
  assert.equal(receipt.registered, false);
  assert.equal(existsSync(join(f.root, 'native-active.json')), false);
  const settings = JSON.parse(readFileSync(join(f.root, 'data/settings.json'), 'utf8'));
  assert.equal(settings.CLAUDE_MEM_PROVIDER, 'codex');
  assert.equal(settings.CLAUDE_MEM_TRANSCRIPTS_ENABLED, 'false');
  assert.equal(settings.CLAUDE_MEM_QUOTA_FALLBACK_PROVIDER, '');
  assert.equal(settings.CLAUDE_MEM_CLOUD_SYNC_HUB_TOKEN, '');
  const hooks = JSON.parse(readFileSync(join(receipt.pluginRoot, 'hooks/codex-hooks.json'), 'utf8'));
  assert.equal(hooks.hooks.PostToolUse[0].matcher, '.*');
  assert.match(hooks.hooks.SessionStart[0].hooks[0].commandWindows, /claude-memory-hook.mjs/);
  const mcp = JSON.parse(readFileSync(join(receipt.pluginRoot, '.mcp.json'), 'utf8'));
  assert.deepEqual(Object.keys(mcp.mcpServers), ['claudex-memory']);
  assert.equal(mcp.mcpServers['claudex-memory'].command, process.execPath);
  assert.deepEqual(loadInstallation(f.root), receipt);
  assert.equal(existsSync(join(f.home, 'CLAUDE.md')), false);
  assert.deepEqual(await prepareInstallation(f), receipt);
});

test('preparation preserves an existing unowned root and its settings', async t => {
  const f = fixture(t);
  mkdirSync(join(f.root, 'data'), { recursive: true });
  const settings = join(f.root, 'data/settings.json');
  const canary = '{"unowned":"preserve-byte-for-byte"}\n';
  writeFileSync(settings, canary);
  await assert.rejects(prepareInstallation(f), /existing unowned/i);
  assert.equal(readFileSync(settings, 'utf8'), canary);
  assert.equal(existsSync(join(f.root, MANIFEST)), false);
  assert.equal(existsSync(join(f.root, 'marketplace')), false);
});

test('preparation refuses an existing empty root without claiming its ownership', async t => {
  const f = fixture(t);
  mkdirSync(f.root);
  await assert.rejects(prepareInstallation(f), /existing unowned/i);
  assert.equal(existsSync(join(f.root, MANIFEST)), false);
  assert.equal(existsSync(join(f.root, 'data')), false);
});

test('runtime environment strips credentials and pins data and worker before imports', async t => {
  const f = fixture(t);
  const receipt = await prepareInstallation(f);
  const env = runtimeEnvironment(receipt, { PATH: 'testpath', HOME: 'original-home', USERPROFILE: 'original-profile',
    OPENAI_API_KEY: 'synthetic-must-not-pass', ANTHROPIC_API_KEY: 'synthetic-must-not-pass',
    CLAUDE_MEM_DATA_DIR: 'foreign-data', CLAUDE_MEM_WORKER_SCRIPT_PATH: 'foreign-worker', CODEX_HOME: 'foreign-auth' });
  assert.equal(env.OPENAI_API_KEY, undefined);
  assert.equal(env.ANTHROPIC_API_KEY, undefined);
  assert.equal(env.HOME, 'original-home'); // Only the observer app-server changes its home.
  assert.equal(env.USERPROFILE, 'original-profile');
  assert.equal(env.CODEX_HOME, f.codexHome);
  assert.equal(env.CLAUDE_MEM_DATA_DIR, receipt.dataDir);
  assert.equal(env.CLAUDE_MEM_TELEMETRY, '0');
  assert.equal(env.DO_NOT_TRACK, '1');
  assert.equal(env.CLAUDE_MEM_WORKER_SCRIPT_PATH, join(receipt.pluginRoot, 'scripts/worker-service.cjs'));
});

test('native activation marker appears only after enabled inventory verification', async t => {
  const f = fixture(t);
  const receipt = await prepareInstallation(f);
  const calls = [];
  const activated = activateInstallation(receipt, { root: f.root, run: (_binary, args) => {
    assert.equal(existsSync(join(f.root, 'native-active.json')), false);
    calls.push(args);
    return args[1] === 'list' ? JSON.stringify({ installed: [{ pluginId: 'claude-mem@claudex-memory', installed: true, enabled: true }] }) : '{}';
  } });
  assert.equal(activated.registered, true);
  assert.equal(calls.length, 3);
  const marker = JSON.parse(readFileSync(join(f.root, 'native-active.json'), 'utf8'));
  assert.deepEqual(marker, { version: 1, active: true, pluginId: 'claude-mem@claudex-memory' });
  assert.ok(Buffer.byteLength(JSON.stringify(marker)) < 1024);
});

test('failed activation rolls back only the new native plugin and leaves no marker', async t => {
  const f = fixture(t);
  const receipt = await prepareInstallation(f);
  const calls = [];
  assert.throws(() => activateInstallation(receipt, { root: f.root, run: (_binary, args) => {
    calls.push(args);
    return args[1] === 'list' ? '{"installed":[]}' : '{}';
  } }), /not active/);
  assert.deepEqual(calls.at(-1), ['plugin', 'remove', 'claude-mem@claudex-memory', '--json']);
  assert.equal(existsSync(join(f.root, 'native-active.json')), false);
  assert.equal(loadInstallation(f.root).registered, false);
});

test('installation fails closed for escaped data or modified runtime', async t => {
  const f = fixture(t);
  const receipt = await prepareInstallation(f);
  writeFileSync(join(f.root, MANIFEST), JSON.stringify({ ...receipt, dataDir: f.home }));
  assert.throws(() => loadInstallation(f.root), /escapes/);
  writeFileSync(join(f.root, MANIFEST), JSON.stringify(receipt));
  writeFileSync(join(receipt.pluginRoot, 'scripts/worker-service.cjs'), 'modified');
  assert.throws(() => loadInstallation(f.root), /integrity/);
});

test('legacy roots, symlink ancestors and legacy or occupied ports are refused', async t => {
  const f = fixture(t);
  const legacy = join(f.home, 'legacy');
  assert.throws(() => assertOwnedRoot(legacy, legacy), /overlaps/);
  assert.throws(() => assertOwnedRoot(join(legacy, 'nested'), legacy), /overlaps/);
  assert.throws(() => assertOwnedRoot(f.home, legacy), /overlaps/);
  const actual = join(f.home, 'actual'); mkdirSync(actual);
  const alias = join(f.home, 'alias'); symlinkSync(actual, alias, 'junction');
  assert.throws(() => assertOwnedRoot(join(alias, 'nested'), legacy), /symlink/);
  await assert.rejects(assertPortFree(37777), /reserved/);
  const server = createServer();
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(() => new Promise(resolve => server.close(resolve)));
  await assert.rejects(assertPortFree(server.address().port), /occupied/);
});

test('worker identity accepts owned degraded health but rejects foreign or malformed service', async t => {
  const f = fixture(t);
  const receipt = await prepareInstallation(f);
  const health = { workerPath: join(receipt.pluginRoot, 'scripts/worker-service.cjs'), version: VERSION };
  assert.deepEqual(await probeOwnedWorker(receipt, async () => new Response(JSON.stringify(health), { status: 503 })), health);
  await assert.rejects(probeOwnedWorker(receipt, async () => new Response('{"workerPath":"foreign"}')), /Foreign worker/);
  await assert.rejects(probeOwnedWorker(receipt, async () => new Response('not-json')), /Foreign service/);
  assert.equal(await probeOwnedWorker(receipt, async () => { throw new TypeError('offline', { cause: { code: 'ECONNREFUSED' } }); }), null);
  await assert.rejects(probeOwnedWorker(receipt, async () => { throw new TypeError('timeout'); }), /cannot be verified/);
});

test('native MCP preserves caller checkout while worker uses owned runtime directory', async t => {
  const f = fixture(t);
  const receipt = await prepareInstallation(f);
  const calls = [];
  const spawnProcess = (binary, args, options) => { calls.push({ binary, args, options }); return {}; };
  spawnRuntime(receipt, [], { mcp: true, spawnProcess });
  spawnRuntime(receipt, ['start'], { spawnProcess });
  assert.equal(calls[0].options.cwd, process.cwd());
  assert.equal(calls[1].options.cwd, receipt.pluginRoot);
  assert.equal(calls[0].args[0], join(receipt.pluginRoot, 'scripts/mcp-server.cjs'));
  assert.equal(calls[0].options.env.CLAUDE_MEM_DATA_DIR, receipt.dataDir);
});

test('worker lifecycle verifies the service after a successful child exit', async () => {
  const receipt = { port: 37778 };
  const owned = { workerPath: 'synthetic-owned-worker' };
  for (const command of ['start', 'stop']) {
    let probes = command === 'start' ? [null, owned] : [owned, null];
    const calls = [];
    const dependencies = {
      probe: async () => probes.shift(),
      launch: (_receipt, args) => { calls.push(args); return {}; },
      wait: async () => 0,
      portFree: async port => { calls.push(port); },
    };
    assert.deepEqual(await controlWorker(receipt, command, dependencies), { exitCode: 0, running: command === 'start' });
    assert.equal(probes.length, 0);
    assert.ok(calls.some(call => Array.isArray(call) && call[0] === command));
    assert.ok(calls.includes(receipt.port));
    probes = command === 'start' ? [null, null] : [owned, owned];
    await assert.rejects(controlWorker(receipt, command, dependencies), command === 'start' ? /did not start/ : /remains running/);
  }
});

test('worker lifecycle is idempotent and refuses a foreign post-stop listener', async () => {
  const receipt = { port: 37778 };
  const owned = { workerPath: 'synthetic-owned-worker' };
  const noLaunch = () => { assert.fail('An idempotent control must not spawn a worker'); };
  assert.deepEqual(await controlWorker(receipt, 'start', { probe: async () => owned, launch: noLaunch }), { exitCode: 0, running: true });
  assert.deepEqual(await controlWorker(receipt, 'stop', { probe: async () => null, launch: noLaunch }), { exitCode: 0, running: false });
  const probes = [owned, null];
  await assert.rejects(controlWorker(receipt, 'stop', {
    probe: async () => probes.shift(), launch: () => ({}), wait: async () => 0,
    portFree: async () => { throw new Error('Dedicated memory port is occupied'); },
  }), /occupied/);
});
