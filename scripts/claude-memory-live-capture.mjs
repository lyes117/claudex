// First live stage: one official worker/client invocation per synthetic capture.
// Independent databases intentionally exclude backlog/recovery ambiguity. This
// does not prove cross-project MCP retrieval or SessionStart injection isolation.
import { copyFileSync, mkdirSync, readdirSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { randomUUID } from 'node:crypto';
import { createServer } from 'node:net';
import { memorySettings, runtimeEnvironment, sha256 } from './claude-memory-runtime.mjs';
import { assertRunDirectory, assertOwnedHealth, checkAuditCheckpoints, readAuditFiles, readBoundedHttp } from './claude-memory-live-fixture.mjs';
import { ownedRuntimeEnvironment } from './claude-memory-live-job.mjs';

const scripts = dirname(fileURLToPath(import.meta.url));
const pause = () => new Promise(resolve => setTimeout(resolve, 100));
export async function assertPortFree(port) {
  const server = createServer();
  try { await new Promise((resolve, reject) => { server.once('error', () => reject(new Error('foreign_port'))); server.listen({ port, host: '127.0.0.1', exclusive: true }, resolve); }); }
  finally { if (server.listening) await new Promise(resolve => server.close(resolve)); }
}
export function prepareCaptureInvocation(fixture, project) {
  assertRunDirectory(fixture.run);
  const invocation = randomUUID(), directory = join(fixture.run, `capture-${invocation}`);
  mkdirSync(directory, { mode: 0o700 });
  const dataDir = join(directory, 'data'), auditDirectory = join(directory, 'audit'), temporary = join(directory, 'temp');
  for (const path of [dataDir, auditDirectory, temporary]) mkdirSync(path, { mode: 0o700 });
  const proxy = join(directory, 'observer-proxy.exe'); copyFileSync(fixture.proxy, proxy);
  const moduleFiles = ['claude-memory-observer-proxy.mjs', 'claude-memory-observer-config.mjs', 'claude-memory-observer-turns.mjs'];
  writeFileSync(proxy + '.json', JSON.stringify({ version: 1, node: realpathSync(process.execPath),
    script: join(scripts, moduleFiles[0]), binary: fixture.binary, auditDirectory }), { flag: 'wx', mode: 0o600 });
  const settings = { ...memorySettings({ dataDir, port: fixture.port, binary: proxy }),
    CLAUDE_MEM_LOG_LEVEL: 'ERROR', CLAUDE_MEM_LLM_TIMEOUT_MS: '60000' };
  writeFileSync(join(dataDir, 'settings.json'), JSON.stringify(settings), { flag: 'wx', mode: 0o600 });
  return { ...fixture, proxy, directory, dataDir, auditDirectory, temporary, project, invocation,
    provenance: { workerHash: fixture.workerHash, binaryHash: sha256(fixture.binary), proxyHash: sha256(proxy),
      nodeHash: sha256(process.execPath), moduleHashes: Object.fromEntries(moduleFiles.map(file => [file, sha256(join(scripts, file))])) } };
}
function assertProvenance(phase) {
  if (phase.provenance.workerHash !== sha256(join(phase.pluginRoot, 'scripts', 'worker-service.cjs'))
      || phase.provenance.binaryHash !== sha256(phase.binary) || phase.provenance.proxyHash !== sha256(phase.proxy)
      || phase.provenance.nodeHash !== sha256(process.execPath)
      || Object.entries(phase.provenance.moduleHashes).some(([file, hash]) => hash !== sha256(join(scripts, file)))) throw new Error('changed_capture_code');
}
export function bindCaptureProof(phase, rows, records, auditFiles) {
  const project = phase.project;
  // Every row in a fresh single-session invocation must have the exact provenance.
  if (!Array.isArray(rows) || rows.length < 1 || rows.length > 100 || rows.some(row =>
    row.project !== project.project || row.content_session_id !== project.session || row.platform_source !== 'codex'
    || !Number.isSafeInteger(row.id) || row.id <= 0)) throw new Error('capture_row_provenance');
  const matching = rows.filter(row => [row.title, row.subtitle, row.narrative, row.text, row.facts]
    .some(value => typeof value === 'string' && value.includes(project.canary)));
  const audit = checkAuditCheckpoints(records), checkpoints = records.filter(row => row.event === 'turn_checkpoint');
  if (!matching.length || !audit.attested || auditFiles.length !== 1 || !/^audit-00-[a-f0-9-]{36}\.ndjson$/.test(auditFiles[0]) || records.some(record => record.auditFile !== 'a1')
      || checkpoints.some(record => record.thread !== 't1')
      || new Set(checkpoints.map(record => record.turn)).size !== checkpoints.length) throw new Error('capture_audit_binding');
  return { invocation: phase.invocation, project: project.project, session: project.session,
    observationIds: matching.map(row => row.id), auditFile: auditFiles[0],
    checkpoints: checkpoints.map(row => ({ thread: row.thread, turn: row.turn })), provenance: phase.provenance };
}
async function ownedHealth(job, phase, process) {
  if (!await job.running(process) || !await job.portOwned(phase.port)) throw new Error('worker_not_owned');
  return await assertOwnedHealth(phase);
}
async function hook(job, phase, name, payload) {
  const input = join(phase.directory, `hook-${name}.json`);
  writeFileSync(input, JSON.stringify(payload) + '\n', { flag: 'wx', mode: 0o600 });
  const environment = runtimeEnvironment(phase, { ...ownedRuntimeEnvironment(phase), TEMP: phase.temporary, TMP: phase.temporary, TMPDIR: phase.temporary });
  const process = await job.start({ binary: phase.bun, args: [join(phase.pluginRoot, 'scripts', 'worker-service.cjs'), 'hook', 'codex', name],
    env: environment, cwd: phase.project.cwd, input });
  await job.wait(process); // Exit success alone is deliberately not the proof below.
}
export async function captureOneInvocation(job, phase, deadline) {
  assertProvenance(phase); await assertPortFree(phase.port);
  const environment = runtimeEnvironment(phase, { ...ownedRuntimeEnvironment(phase), TEMP: phase.temporary, TMP: phase.temporary, TMPDIR: phase.temporary });
  const process = await job.start({ binary: phase.bun, args: [join(phase.pluginRoot, 'scripts', 'worker-service.cjs'), '--daemon'],
    env: environment, cwd: phase.directory });
  let healthy = false;
  while (Date.now() < deadline) { try { await ownedHealth(job, phase, process); healthy = true; break; } catch { if (!await job.running(process)) throw new Error('worker_boot_failed'); await pause(); } }
  if (!healthy) throw new Error('worker_health_timeout');
  const project = phase.project;
  await hook(job, phase, 'session-init', { hook_event_name: 'UserPromptSubmit', session_id: project.session, cwd: project.cwd,
    prompt: 'Record the synthetic integration decision from the fixture tool result.' });
  await hook(job, phase, 'observation', { hook_event_name: 'PostToolUse', session_id: project.session, cwd: project.cwd,
    tool_name: 'Read', tool_use_id: project.session + '-read', tool_input: { file_path: join(project.cwd, 'fixture-contract.txt') },
    tool_response: `${project.canary}: The independent event ledger is a durable integration decision. Preserve this exact synthetic identifier.` });
  let rows, proof;
  while (Date.now() < deadline) {
    await ownedHealth(job, phase, process);
    const query = new URLSearchParams({ project: project.project, contentSessionId: project.session, platformSource: 'codex', limit: '100' });
    const payload = await readBoundedHttp(`http://127.0.0.1:${phase.port}/api/observations?${query}`);
    if (!Array.isArray(payload.items) || payload.hasMore !== false) throw new Error('invalid_capture_page');
    rows = payload.items;
    try { proof = bindCaptureProof(phase, rows, readAuditFiles(phase.auditDirectory), readdirSync(phase.auditDirectory).filter(name => name.endsWith('.ndjson'))); } catch {}
    if (proof) break; await pause();
  }
  if (!proof) throw new Error('capture_compression_timeout');
  await job.reset(); await assertPortFree(phase.port);
  // Capture the terminal scoped proof after all owned writers have stopped.
  proof = bindCaptureProof(phase, rows, readAuditFiles(phase.auditDirectory), readdirSync(phase.auditDirectory).filter(name => name.endsWith('.ndjson')));
  assertProvenance(phase);
  const output = join(phase.directory, 'persisted-observations.json'), specification = join(phase.directory, 'persistence-input.json');
  writeFileSync(specification, JSON.stringify({ dataDir: phase.dataDir, project: project.project, session: project.session,
    canary: project.canary, output }), { flag: 'wx', mode: 0o600 });
  const verifier = await job.start({ binary: phase.bun, args: [join(scripts, 'claude-memory-live-persistence.mjs'), specification],
    env: environment, cwd: phase.directory }); await job.wait(verifier);
  const disk = readFileSync(output); if (disk.length > 8192) throw new Error('persistence_proof_limit');
  const persisted = JSON.parse(disk.toString('utf8'));
  if (persisted.persisted !== true || JSON.stringify(persisted.observationIds) !== JSON.stringify([...proof.observationIds].sort((a, b) => a - b))
      || persisted.total !== rows.length) throw new Error('changed_persisted_capture');
  proof.observationIds.sort((a, b) => a - b);
  await job.reset();
  writeFileSync(join(phase.directory, 'capture-proof.json'), JSON.stringify(proof), { flag: 'wx', mode: 0o600 });
  return proof;
}
