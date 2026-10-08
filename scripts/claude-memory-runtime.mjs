import { existsSync, readFileSync, realpathSync } from 'node:fs';
import { join } from 'node:path';
import { homedir } from 'node:os';
import { createHash } from 'node:crypto';
import { spawn } from 'node:child_process';

export const VERSION = '13.29.0-dev+a1951f2.cx2';
export const IDENTITY_SCHEME = 'cx1-sha256-canonical-root';
export const REVISION = 'a1951f2ad247330b2b5d58a1e0c7efeef4a03be5';
export const DEFAULT_ROOT = join(homedir(), '.claudex', 'memory');
export const DEFAULT_PORT = 37778;
export const MANIFEST = 'claudex-memory-install.json';

export function memorySettings({ dataDir, port, binary }) {
  return {
    CLAUDE_MEM_DATA_DIR: dataDir,
    CLAUDE_MEM_WORKER_PORT: String(port), CLAUDE_MEM_WORKER_HOST: '127.0.0.1',
    CLAUDE_MEM_PROVIDER: 'codex', CLAUDE_MEM_CODEX_PATH: binary,
    CLAUDE_MEM_CODEX_REASONING_EFFORT: 'low', CLAUDE_MEM_CODEX_MAX_CONCURRENT_AGENTS: '1',
    CLAUDE_MEM_QUOTA_FALLBACK_PROVIDER: '', CLAUDE_MEM_CLOUD_SYNC_HUB_URL: '',
    CLAUDE_MEM_CLOUD_SYNC_HUB_TOKEN: '', CLAUDE_MEM_RUNTIME: 'worker',
    CLAUDE_MEM_CHROMA_ENABLED: 'false',
    CLAUDE_MEM_TELEMETRY: '0', DO_NOT_TRACK: '1',
    CLAUDE_MEM_FOLDER_CLAUDEMD_ENABLED: 'false', CLAUDE_MEM_TRANSCRIPTS_ENABLED: 'false',
    CLAUDE_MEM_CODEX_TRANSCRIPT_INGESTION: 'false', CLAUDE_MEM_SKIP_SUBAGENT_OBSERVATIONS: 'false',
    CLAUDE_MEM_REDACT_ENABLED: 'true', CLAUDE_MEM_REDACT_LOG_MATCHES: 'false',
    CLAUDE_MEM_SERVER_URL: '', CLAUDE_MEM_SERVER_API_KEY: '', CLAUDE_MEM_SERVER_PROJECT_ID: '',
    CLAUDE_MEM_SERVER_BETA_URL: '', CLAUDE_MEM_SERVER_BETA_API_KEY: '',
  };
}

export function validatePort(port) {
  if (!Number.isSafeInteger(port) || port < 1024 || port > 65535 || port === 37777) {
    throw new Error('Choose a dedicated port from 1024 to 65535; legacy port 37777 is reserved');
  }
  return port;
}

export function sha256(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex');
}

export function verifyBundle(packageRoot) {
  const receiptPath = join(packageRoot, 'claudex-observer-build.json');
  if (!existsSync(receiptPath)) throw new Error('Package lacks a verified Claudex observer build receipt; published 13.28.0 has no Codex provider');
  const receipt = JSON.parse(readFileSync(receiptPath, 'utf8'));
  if (receipt.revision !== REVISION || receipt.version !== VERSION
      || receipt.toolsEnabled !== false || receipt.privateUserHome !== true
      || receipt.portReclaimDisabled !== true
      || receipt.projectIdentity?.scheme !== IDENTITY_SCHEME) {
    throw new Error('Observer source revision or isolation receipt is unsupported');
  }
  for (const file of ['plugin/scripts/worker-service.cjs', 'plugin/scripts/mcp-server.cjs', 'plugin/scripts/claude-memory-project-identity.mjs']) {
    if (receipt.sha256?.[file] !== sha256(join(packageRoot, file))) throw new Error('Observer bundle integrity mismatch');
  }
  return receipt;
}

export function loadInstallation(root = DEFAULT_ROOT) {
  const receipt = JSON.parse(readFileSync(join(root, MANIFEST), 'utf8'));
  if (receipt.owner !== 'claudex-memory' || receipt.version !== VERSION
      || receipt.portReclaimDisabled !== true
      || receipt.projectIdentity?.scheme !== IDENTITY_SCHEME) throw new Error('Foreign or unsupported memory installation');
  validatePort(receipt.port);
  const pluginRoot = realpathSync(receipt.pluginRoot);
  const allowed = realpathSync(join(root, 'marketplace', 'plugin'));
  if (pluginRoot !== allowed) throw new Error('Plugin root escapes owned installation');
  if (realpathSync(receipt.dataDir) !== realpathSync(join(root, 'data'))) throw new Error('Memory data directory escapes owned installation');
  if (!existsSync(receipt.binary) || !existsSync(receipt.bun)) throw new Error('Installed runtime executable is missing');
  for (const file of ['worker-service.cjs', 'mcp-server.cjs', 'claude-memory-project-identity.mjs']) {
    if (receipt.bundleHashes?.[file] !== sha256(join(pluginRoot, 'scripts', file))) throw new Error('Installed observer bundle integrity mismatch');
  }
  return { ...receipt, pluginRoot };
}

export async function probeOwnedWorker(receipt, request = fetch) {
  let response;
  try {
    response = await request(`http://127.0.0.1:${receipt.port}/api/health`, {
      signal: AbortSignal.timeout(3000), redirect: 'error',
    });
  } catch (error) {
    const cause = error.cause;
    if (cause?.code === 'ECONNREFUSED' || (cause?.errors?.length && cause.errors.every(item => item.code === 'ECONNREFUSED'))) return null;
    throw new Error('Dedicated memory worker identity cannot be verified');
  }
  if (response.status !== 200 && response.status !== 503) throw new Error('Foreign service occupies dedicated memory port');
  const text = await response.text();
  if (Buffer.byteLength(text) > 512 * 1024) throw new Error('Memory health response exceeds diagnostic limit');
  let health;
  try { health = JSON.parse(text); } catch { throw new Error('Foreign service occupies dedicated memory port'); }
  const expected = join(receipt.pluginRoot, 'scripts', 'worker-service.cjs');
  const canonical = value => process.platform === 'win32' ? value.toLowerCase().replaceAll('/', '\\') : value;
  if (typeof health.workerPath !== 'string' || canonical(health.workerPath) !== canonical(expected)) {
    throw new Error('Foreign worker occupies dedicated memory port');
  }
  return health;
}

export function runtimeEnvironment(receipt, environment = process.env) {
  const permitted = new Set(['PATH', 'PATHEXT', 'SYSTEMROOT', 'WINDIR', 'TEMP', 'TMP', 'TMPDIR',
    'HOME', 'USERPROFILE', 'HOMEDRIVE', 'HOMEPATH', 'APPDATA', 'LOCALAPPDATA', 'LANG',
    'NODE_EXTRA_CA_CERTS', 'SSL_CERT_FILE', 'SSL_CERT_DIR']);
  const env = Object.fromEntries(Object.entries(environment).filter(([key]) => permitted.has(key.toUpperCase()) || key.toUpperCase().startsWith('LC_')));
  return { ...env, ...memorySettings(receipt), CODEX_HOME: receipt.codexHome,
    CLAUDE_MEM_WORKER_SCRIPT_PATH: join(receipt.pluginRoot, 'scripts', 'worker-service.cjs'),
    CLAUDE_PLUGIN_ROOT: receipt.pluginRoot, PLUGIN_ROOT: receipt.pluginRoot,
    CLAUDE_MEM_CODEX_HOOK: '1' };
}

export function spawnRuntime(receipt, arguments_, { mcp = false, spawnProcess = spawn } = {}) {
  const target = join(receipt.pluginRoot, 'scripts', mcp ? 'mcp-server.cjs' : 'worker-service.cjs');
  // Bun is required by worker bun:sqlite. MCP uses the same runtime so its lazy
  // imports resolve consistently, without invoking bun-runner auto-installation.
  return spawnProcess(receipt.bun, [target, ...arguments_], {
    stdio: 'inherit', windowsHide: true, cwd: mcp ? process.cwd() : receipt.pluginRoot,
    env: runtimeEnvironment(receipt), shell: false,
  });
}

export async function waitRuntime(child) {
  for (const signal of ['SIGTERM', 'SIGINT']) {
    process.once(signal, () => { try { child.kill(signal); } catch {} });
  }
  return await new Promise((resolveExit, reject) => {
    child.once('error', reject);
    child.once('exit', (code, signal) => resolveExit(signal ? 1 : code ?? 1));
  });
}
