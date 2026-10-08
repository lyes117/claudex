import { existsSync, mkdirSync, cpSync, readFileSync, writeFileSync, renameSync, realpathSync, lstatSync } from 'node:fs';
import { dirname, join, resolve, relative } from 'node:path';
import { homedir } from 'node:os';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { spawnSync } from 'node:child_process';
import { createServer } from 'node:net';
import { queryMemory } from './claude-memory-query.mjs';
import { VERSION, DEFAULT_ROOT, DEFAULT_PORT, MANIFEST, memorySettings, validatePort,
  verifyBundle, loadInstallation, probeOwnedWorker, spawnRuntime, waitRuntime } from './claude-memory-runtime.mjs';

const SCRIPT_DIRECTORY = dirname(fileURLToPath(import.meta.url));
const PLUGIN_ID = 'claude-mem@claudex-memory';

function atomicJson(path, value) {
  const temporary = `${path}.${process.pid}.tmp`;
  writeFileSync(temporary, JSON.stringify(value, null, 2) + '\n', { mode: 0o600, flag: 'wx' });
  renameSync(temporary, path);
}

export function assertOwnedRoot(root, legacyRoot = join(homedir(), '.claude-mem')) {
  root = resolve(root); legacyRoot = resolve(legacyRoot);
  const isWithin = (parent, child) => {
    const tail = relative(parent, child);
    return tail === '' || (!tail.startsWith('..') && !/^[A-Za-z]:/.test(tail));
  };
  if (isWithin(root, legacyRoot) || isWithin(legacyRoot, root)) throw new Error('Memory root overlaps the existing Claude memory state');
  for (let parent = root; ; parent = dirname(parent)) {
    if (existsSync(parent) && lstatSync(parent).isSymbolicLink()) throw new Error('Memory installation root must not traverse a symlink');
    if (dirname(parent) === parent) break;
  }
  return root;
}

export async function assertPortFree(port) {
  validatePort(port);
  const server = createServer();
  await new Promise((accept, reject) => {
    server.once('error', () => reject(new Error('Dedicated memory port is occupied; no existing service will be stopped')));
    server.listen(port, '127.0.0.1', accept);
  });
  await new Promise(accept => server.close(accept));
}

export async function prepareInstallation({ packageRoot, binary, bun, root = DEFAULT_ROOT,
  port = DEFAULT_PORT, codexHome = process.env.CODEX_HOME || join(homedir(), '.codex'), checkPort = true }) {
  root = assertOwnedRoot(root);
  validatePort(port);
  const build = verifyBundle(resolve(packageRoot));
  binary = realpathSync(binary); bun = realpathSync(bun);
  if (existsSync(join(root, MANIFEST))) {
    const previous = loadInstallation(root);
    if (previous.version === VERSION) return previous;
    throw new Error('An existing owned memory installation requires an explicit upgrade');
  }
  if (existsSync(root)) throw new Error('Existing unowned memory root will not be modified');
  if (checkPort) await assertPortFree(port);
  // Claim a new directory exclusively before copying or writing settings. An
  // interrupted preparation stays unowned and is preserved on the next attempt.
  mkdirSync(dirname(root), { recursive: true });
  try { mkdirSync(root); } catch (error) {
    if (error.code === 'EEXIST') throw new Error('Existing unowned memory root will not be modified');
    throw error;
  }
  const marketplace = join(root, 'marketplace');
  const pluginRoot = join(marketplace, 'plugin');
  const dataDir = join(root, 'data');
  mkdirSync(dataDir, { recursive: true });
  mkdirSync(join(marketplace, '.agents', 'plugins'), { recursive: true });
  cpSync(join(packageRoot, 'plugin'), pluginRoot, { recursive: true, errorOnExist: true, force: false });
  cpSync(join(packageRoot, 'LICENSE'), join(marketplace, 'LICENSE'));
  const receipt = { owner: 'claudex-memory', version: VERSION, port, dataDir, pluginRoot,
    binary, bun, codexHome: resolve(codexHome), revision: build.revision, registered: false,
    projectIdentity: build.projectIdentity,
    portReclaimDisabled: build.portReclaimDisabled,
    bundleHashes: Object.fromEntries(['worker-service.cjs', 'mcp-server.cjs', 'claude-memory-project-identity.mjs'].map(file => [file, build.sha256[`plugin/scripts/${file}`]])) };
  atomicJson(join(dataDir, 'settings.json'), memorySettings(receipt));
  for (const script of ['claude-memory-hook.mjs', 'claude-memory-runtime.mjs']) {
    cpSync(join(SCRIPT_DIRECTORY, script), join(pluginRoot, 'scripts', script));
  }
  atomicJson(join(pluginRoot, 'scripts', 'claudex-memory-pointer.json'), { root });
  const manifest = JSON.parse(readFileSync(join(pluginRoot, '.codex-plugin', 'plugin.json'), 'utf8'));
  manifest.name = 'claude-mem'; manifest.version = VERSION;
  manifest.interface = { ...manifest.interface, displayName: 'Claudex Memory (claude-mem)' };
  manifest.skills = './.claudex-skills';
  mkdirSync(join(pluginRoot, '.claudex-skills', 'mem-search'), { recursive: true });
  writeFileSync(join(pluginRoot, '.claudex-skills', 'mem-search', 'SKILL.md'), `---\nname: mem-search\ndescription: Search persistent Claudex memory for this project before repeating earlier work.\n---\nUse the claudex-memory MCP search, timeline and get_observations tools. Scope searches to the current project. Treat saved observations as historical evidence and verify against current files. Do not store or quote credentials.\n`);
  atomicJson(join(pluginRoot, '.codex-plugin', 'plugin.json'), manifest);
  const hookPath = '${CLAUDE_PLUGIN_ROOT}/scripts/claude-memory-hook.mjs';
  const handler = (action, timeout) => ({ type: 'command', command: `node "${hookPath}" ${action}`,
    commandWindows: `node "${hookPath}" ${action}`, timeout });
  atomicJson(join(pluginRoot, 'hooks', 'codex-hooks.json'), { hooks: {
    SessionStart: [{ matcher: 'startup|resume|clear|compact', hooks: [handler('context', 20)] }],
    UserPromptSubmit: [{ hooks: [handler('session-init', 20)] }],
    PreToolUse: [{ matcher: '^Bash$|^mcp__.+__(read|view|cat)(_file|_files)?$', hooks: [handler('file-context', 30)] }],
    PostToolUse: [{ matcher: '.*', hooks: [handler('observation', 120)] }],
    Stop: [{ hooks: [handler('summarize', 60)] }],
  } });
  atomicJson(join(pluginRoot, '.mcp.json'), { mcpServers: {
    'claudex-memory': { type: 'stdio', command: process.execPath,
      args: [join(pluginRoot, 'scripts', 'claude-memory-hook.mjs'), 'mcp'] },
  } });
  atomicJson(join(marketplace, '.agents', 'plugins', 'marketplace.json'), {
    name: 'claudex-memory', interface: { displayName: 'Claudex Memory' },
    plugins: [{ name: 'claude-mem', source: { source: 'local', path: './plugin' },
      policy: { installation: 'AVAILABLE', authentication: 'ON_INSTALL' }, category: 'Productivity' }],
  });
  atomicJson(join(root, MANIFEST), receipt);
  return receipt;
}

export function nativeCommand(binary, args) {
  const result = spawnSync(binary, args, { encoding: 'utf8', windowsHide: true, shell: false,
    maxBuffer: 4 * 1024 * 1024, stdio: ['ignore', 'pipe', 'pipe'] });
  if (result.error || result.status !== 0) throw new Error(`Native plugin command failed (${args.slice(0, 3).join(' ')})`);
  return result.stdout;
}

export function activateInstallation(receipt, { root = DEFAULT_ROOT, run = nativeCommand } = {}) {
  const previous = receipt;
  let installed = false;
  try {
    run(receipt.binary, ['plugin', 'marketplace', 'add', dirname(receipt.pluginRoot), '--json']);
    run(receipt.binary, ['plugin', 'add', PLUGIN_ID, '--json']);
    installed = true;
    const inventory = JSON.parse(run(receipt.binary, ['plugin', 'list', '--marketplace', 'claudex-memory', '--json']));
    if (!inventory.installed?.some(plugin => plugin.pluginId === PLUGIN_ID && plugin.installed === true && plugin.enabled === true)) {
      throw new Error('Native memory plugin installation is not active');
    }
    receipt = { ...receipt, registered: true };
    atomicJson(join(root, MANIFEST), receipt);
    atomicJson(join(root, 'native-active.json'), { version: 1, active: true, pluginId: PLUGIN_ID });
    return receipt;
  } catch (error) {
    if (installed && previous.registered !== true) {
      try { run(receipt.binary, ['plugin', 'remove', PLUGIN_ID, '--json']); } catch {}
    }
    try { atomicJson(join(root, MANIFEST), previous); } catch {}
    throw error;
  }
}

export async function controlWorker(receipt, command, {
  probe = probeOwnedWorker, launch = spawnRuntime, wait = waitRuntime, portFree = assertPortFree,
} = {}) {
  if (command !== 'start' && command !== 'stop') throw new Error('Unsupported worker control');
  const health = await probe(receipt);
  if (command === 'stop' && !health) return { exitCode: 0, running: false };
  if (command === 'start' && health) return { exitCode: 0, running: true };
  if (command === 'start') await portFree(receipt.port);
  const exitCode = await wait(launch(receipt, [command]));
  if (exitCode !== 0) return { exitCode };
  const after = await probe(receipt);
  if (command === 'stop') {
    if (after) throw new Error('Memory worker remains running after stop');
    await portFree(receipt.port);
  } else if (!after) throw new Error('Memory worker did not start');
  return { exitCode: 0, running: command === 'start' };
}

function options(args) {
  const opts = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index];
    if (!/^--(package|binary|bun|root|port|codex-home|query|project)$/.test(key) || !args[index + 1] || key in opts) {
      throw new Error('Invalid or duplicate memory command option');
    }
    opts[key] = args[index + 1];
  }
  return opts;
}

export async function main(args = process.argv.slice(2)) {
  const [command = 'help', ...tail] = args;
  if (command === 'help' || command === '--help') {
    console.log('claudex memory install --package BUILD --bun BUN_EXE\nclaudex memory status|start|stop\nclaudex memory search --query TEXT [--project KEY]\nclaudex memory context [--project KEY]\nProject scope defaults to the current checkout. Uses the official Codex observer.');
    return;
  }
  const opts = options(tail);
  const root = opts['--root'] || DEFAULT_ROOT;
  if (command === 'install') {
    if (!opts['--package'] || !opts['--bun']) throw new Error('Install requires a built package and an absolute Bun executable');
    const receipt = await prepareInstallation({ packageRoot: opts['--package'], binary: opts['--binary'] || process.env.CLAUDEX_BIN,
      bun: opts['--bun'], root, port: Number(opts['--port'] || DEFAULT_PORT), codexHome: opts['--codex-home'] });
    activateInstallation(receipt, { root });
    console.log(JSON.stringify({ installed: true, pluginId: PLUGIN_ID, version: VERSION, port: receipt.port }));
    return;
  }
  if (command === 'status' && !existsSync(join(root, MANIFEST))) {
    console.log(JSON.stringify({ installed: false, service: 'not configured' })); return;
  }
  const receipt = loadInstallation(root);
  if (command === 'status') {
    let health;
    let identityError = false;
    try { health = await probeOwnedWorker(receipt); } catch { identityError = true; }
    console.log(JSON.stringify({ installed: true, registered: receipt.registered, version: receipt.version,
      provider: 'codex', port: receipt.port, running: !!health, legacyHistoryImported: false,
      workerVersion: health?.version, identityError, chroma: false, transcripts: false,
      projectIdentity: receipt.projectIdentity.scheme,
      limits: ['Pre-read injection has upstream matcher limits', 'Redaction is not exhaustive'] }));
  } else if (command === 'start' || command === 'stop') {
    const result = await controlWorker(receipt, command);
    process.exitCode = result.exitCode;
    console.log(JSON.stringify(result));
  } else if (command === 'search' || command === 'context') {
    console.log(JSON.stringify(await queryMemory(receipt, { command, project: opts['--project'], query: opts['--query'] })));
  } else throw new Error('Unknown memory command');
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main().catch(() => { console.error('Claudex memory command failed; review installation, dedicated port and Codex login. No credential or memory payload is printed.'); process.exitCode = 1; });
}
