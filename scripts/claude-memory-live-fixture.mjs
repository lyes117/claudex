// Preparation/verification only. No auth contents, credentials or wire payloads are logged.
import { cpSync, existsSync, lstatSync, mkdirSync, readdirSync, readFileSync, realpathSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { randomUUID } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { memorySettings, sha256, verifyBundle, validatePort } from './claude-memory-runtime.mjs';
import { projectKeyForRoot } from './claude-memory-project-identity.mjs';

export const REPOSITORY = resolve(dirname(fileURLToPath(import.meta.url)), '..');
export const LIVE_ROOT = join(REPOSITORY, '.build-tools', 'live-memory');
export const HTTP_LIMIT = 1024 * 1024;

export function assertRunDirectory(directory) {
  const root = realpathSync(REPOSITORY);
  const tail = relative(join(root, '.build-tools', 'live-memory'), resolve(directory));
  if (tail.startsWith('..') || tail.includes(sep) || !/^run-[a-f0-9-]{36}$/.test(tail)) throw new Error('invalid_owned_fixture');
  for (let path = resolve(directory); path !== root; path = dirname(path)) {
    if (existsSync(path) && lstatSync(path).isSymbolicLink()) throw new Error('fixture_symlink');
  }
  return resolve(directory);
}

export function prepareLiveFixture({ packageRoot, binary, bun, proxy, guard, port, runRoot = LIVE_ROOT }) {
  if (resolve(runRoot) !== LIVE_ROOT) throw new Error('invalid_fixture_root');
  validatePort(port);
  const build = verifyBundle(packageRoot);
  const executables = Object.fromEntries(Object.entries({ binary, bun, proxy, guard }).map(([key, value]) => {
    const path = realpathSync(value);
    if (!statSync(path).isFile()) throw new Error('missing_fixture_executable');
    return [key, path];
  }));
  const run = assertRunDirectory(join(runRoot, `run-${randomUUID()}`));
  mkdirSync(dirname(run), { recursive: true }); mkdirSync(run, { mode: 0o700 });
  const pluginRoot = join(run, 'plugin');
  cpSync(join(packageRoot, 'plugin'), pluginRoot, { recursive: true, errorOnExist: true, force: false });
  const dataDir = join(run, 'data'), codexHome = join(run, 'observer', '.codex'), userHome = join(run, 'observer', 'user-home');
  for (const path of [dataDir, codexHome, userHome, join(run, 'audit')]) mkdirSync(path, { recursive: true, mode: 0o700 });
  // Native defaults remain ChatGPT. No local API key or user config is inherited.
  writeFileSync(join(codexHome, 'config.toml'), 'forced_login_method = "chatgpt"\n', { flag: 'wx', mode: 0o600 });
  const projects = ['a', 'b'].map(label => {
    const cwd = join(run, 'projects', label, 'same-project');
    mkdirSync(cwd, { recursive: true, mode: 0o700 });
    const git = spawnSync('git', ['init', '--quiet', cwd], { windowsHide: true, shell: false, stdio: 'ignore', timeout: 5000,
      env: { PATH: process.env.PATH, SystemRoot: process.env.SystemRoot, WINDIR: process.env.WINDIR,
        HOME: userHome, USERPROFILE: userHome, GIT_CONFIG_NOSYSTEM: '1',
        GIT_CONFIG_GLOBAL: process.platform === 'win32' ? 'NUL' : '/dev/null' } });
    if (git.error || git.status !== 0) throw new Error('fixture_git_failed');
    const canary = `CX_MEMORY_${label.toUpperCase()}_${randomUUID().replaceAll('-', '')}`;
    writeFileSync(join(cwd, 'fixture-contract.txt'), `${canary}: This synthetic project uses a separate event ledger and retains its integration decision.\n`, { flag: 'wx', mode: 0o600 });
    // These are fresh ordinary Git roots, never worktrees/submodules. Use their
    // canonical root keys directly, without reading personal Git configuration.
    return { cwd, canary, project: projectKeyForRoot(cwd), session: randomUUID() };
  });
  if (projects[0].project === projects[1].project) throw new Error('fixture_project_collision');
  const settings = { ...memorySettings({ dataDir, port, binary: executables.proxy }), CLAUDE_MEM_LOG_LEVEL: 'ERROR' };
  writeFileSync(join(dataDir, 'settings.json'), JSON.stringify(settings), { flag: 'wx', mode: 0o600 });
  const fixture = { run, pluginRoot, dataDir, codexHome, userHome, projects, port, ...executables,
    revision: build.revision, workerHash: sha256(join(pluginRoot, 'scripts', 'worker-service.cjs')) };
  // Local synthetic fixture metadata, never auth.json or live request/response data.
  writeFileSync(join(run, 'fixture.json'), JSON.stringify(fixture), { flag: 'wx', mode: 0o600 });
  return fixture;
}

export async function readBoundedHttp(url, options = {}) {
  const response = await fetch(url, { ...options, redirect: 'error', signal: AbortSignal.timeout(5000) });
  const reader = response.body?.getReader();
  if (!reader) throw new Error('missing_http_body');
  const chunks = []; let size = 0;
  try {
    for (;;) {
      const { done, value } = await reader.read(); if (done) break;
      size += value.byteLength;
      if (size > HTTP_LIMIT) throw new Error('http_body_limit');
      chunks.push(Buffer.from(value));
    }
  } finally { await reader.cancel(); }
  if (!response.ok) throw new Error('http_request_failed');
  try { return JSON.parse(Buffer.concat(chunks, size).toString('utf8')); }
  catch { throw new Error('http_json_invalid'); }
}

export async function assertOwnedHealth(fixture) {
  const health = await readBoundedHttp(`http://127.0.0.1:${fixture.port}/api/health`);
  const expected = realpathSync(join(fixture.pluginRoot, 'scripts', 'worker-service.cjs'));
  if (typeof health.workerPath !== 'string' || realpathSync(health.workerPath) !== expected) throw new Error('foreign_worker');
  return true;
}

export function checkPersistedObservations(payload, project) {
  if (!Array.isArray(payload?.items) || payload.items.length > 100 || payload.hasMore !== false) throw new Error('invalid_observation_page');
  const matching = payload.items.filter(row => row.project === project.project && row.content_session_id === project.session
    && row.platform_source === 'codex' && Number.isSafeInteger(row.id) && row.id > 0);
  const captured = matching.some(row => [row.title, row.subtitle, row.narrative, row.text, row.facts]
    .some(value => typeof value === 'string' && value.includes(project.canary)));
  return { captured, observationCount: matching.length };
}

// A terminal checkpoint is scoped to observed turns, never a clean global close.
export function checkAuditCheckpoints(records) {
  if (!Array.isArray(records) || records.length > 10000) return { attested: false, turnCount: 0 };
  const checkpoints = records.filter(record => record.event === 'turn_checkpoint');
  const good = checkpoints.length > 0 && checkpoints.every(record => record.completed === true
    && record.afterAttestations === true && record.toolsDisabledRequested === true && record.hooksDisabledRequested === true
    && record.auditCompleteThroughTurn === true && record.noToolItemsObservedThroughTurn === true
    && record.pendingCount === 0 && record.toolItemCount === 0 && record.unknownItemCount === 0 && record.unknownRequestCount === 0);
  return { attested: good && !records.some(record => record.event === 'diagnostic'), turnCount: checkpoints.length };
}

export function readAuditFiles(directory) {
  const entries = readdirSync(directory, { withFileTypes: true });
  if (entries.length < 1 || entries.length > 128) throw new Error('audit_file_count');
  const records = []; let totalBytes = 0, lines = 0, fileCount = 0;
  const events = new Set(['diagnostic', 'thread_started', 'mcp_inventory', 'turn_start', 'turn_response',
    'item_started', 'item_completed', 'summary', 'turn_checkpoint']);
  for (const entry of entries.sort((left, right) => left.name.localeCompare(right.name))) {
    const path = join(directory, entry.name), metadata = lstatSync(path);
    if (/^\.slot-(?:[0-5][0-9]|6[0-3])$/.test(entry.name)) {
      if (!metadata.isFile() || metadata.size !== 0) throw new Error('invalid_audit_claim');
      continue;
    }
    if (!entry.isFile() || !metadata.isFile() || !entry.name.endsWith('.ndjson') || ++fileCount > 64) throw new Error('invalid_audit_entry');
    const auditFile = `a${fileCount}`;
    totalBytes += metadata.size;
    if (!metadata.isFile() || metadata.size > HTTP_LIMIT || totalBytes > 4 * HTTP_LIMIT) throw new Error('audit_byte_limit');
    const bytes = readFileSync(path);
    if (bytes.length !== metadata.size || bytes.at(-1) !== 10) throw new Error('partial_audit');
    for (const line of bytes.toString('utf8').split('\n').slice(0, -1)) {
      if (++lines > 10000 || Buffer.byteLength(line) > 2048) throw new Error('audit_record_limit');
      let record; try { record = JSON.parse(line); } catch { throw new Error('audit_json_invalid'); }
      if (!events.has(record?.event)) throw new Error('audit_event_invalid');
      if (record.event === 'diagnostic') records.push({ event: 'diagnostic', auditFile });
      else if (record.event === 'turn_checkpoint') {
        const booleans = ['completed', 'afterAttestations', 'toolsDisabledRequested', 'hooksDisabledRequested',
          'auditCompleteThroughTurn', 'noToolItemsObservedThroughTurn'];
        const counters = ['pendingCount', 'toolItemCount', 'unknownItemCount', 'unknownRequestCount'];
        if (!/^t[1-9][0-9]{0,3}$/.test(record.thread) || !/^v[1-9][0-9]{0,6}$/.test(record.turn)
            || booleans.some(key => typeof record[key] !== 'boolean')
            || counters.some(key => !Number.isSafeInteger(record[key]) || record[key] < 0)) throw new Error('audit_checkpoint_invalid');
        records.push({ auditFile, ...Object.fromEntries(['event', 'thread', 'turn', ...booleans, ...counters].map(key => [key, record[key]])) });
      }
    }
  }
  if (!fileCount) throw new Error('audit_files_missing');
  return records;
}
