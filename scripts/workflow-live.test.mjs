// Explicit live check: uses the installed fork and its existing ChatGPT authentication.
// Run only with CLAUDEX_LIVE_BIN set; no credentials are copied or printed.
import { spawn, spawnSync } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const binary = process.env.CLAUDEX_LIVE_BIN;
if (process.platform !== 'win32') throw new Error('This live process-tree check targets Windows');
if (!binary || !existsSync(binary)) throw new Error('Set CLAUDEX_LIVE_BIN to the installed fork for this authorized live check');
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../.build-tools/live-workflow-checks', randomUUID());
mkdirSync(root, { recursive: true });
const runs = join(homedir(), '.claudex/workflow-runs');
const read = path => JSON.parse(readFileSync(path, 'utf8'));
const status = id => read(join(runs, id, 'status.json'));
const lock = id => read(join(runs, id, 'active.lock'));
const check = (condition, label) => { if (!condition) throw new Error(label); };
const alive = pid => { try { process.kill(pid, 0); return true; } catch { return false; } };
const fixtures = [];
async function until(predicate, label, timeout = 180_000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    try { if (predicate()) return; } catch {}
    await new Promise(resolveWait => setTimeout(resolveWait, 50));
  }
  throw new Error(`Timed out: ${label}`);
}
function launch(script, id) {
  const child = spawn(binary, ['workflow', script, '--run-id', id, '--args', '{}', '--cwd', root], {
    windowsHide: true, stdio: ['ignore', 'ignore', 'ignore'],
  });
  const run = { child, id, exited: false };
  run.finished = new Promise((fulfill, reject) => {
    child.once('error', error => { run.exited = true; reject(error); });
    child.once('close', code => { run.exited = true; fulfill(code); });
  });
  run.finished.catch(() => {});
  fixtures.push(run);
  return run;
}
async function exit(run, timeout = 30_000) {
  await until(() => run.exited, 'fixture process exit', timeout);
  return run.finished;
}
async function cleanup() {
  for (const run of fixtures) {
    const activeLock = join(runs, run.id, 'active.lock');
    // A crashed CLI may leave its Node controller alive. The controller's stop
    // channel remains usable independently of the original CLI process.
    if (run.exited && !existsSync(activeLock)) continue;
    try {
      control(run.id, 'stop');
      await until(() => !existsSync(activeLock), 'fixture controller shutdown', 10_000);
      await exit(run, 10_000);
    } catch {}
    if (!run.exited && run.child.pid) {
      // Only the still-owned fixture CLI and its descendants may be terminated.
      spawnSync('taskkill', ['/pid', String(run.child.pid), '/t', '/f'], { windowsHide: true, stdio: 'ignore', timeout: 15_000 });
      await exit(run, 10_000);
    }
    if (existsSync(activeLock)) {
      throw new Error(`Fixture cleanup could not confirm descendant shutdown; inspect ${activeLock}. Unverified PIDs were not terminated.`);
    }
  }
}
function control(id, action) {
  const result = spawnSync(binary, ['workflow', action, id], { windowsHide: true, encoding: 'utf8', timeout: 15_000 });
  check(result.status === 0, `${action} command failed`);
  check(JSON.parse(result.stdout).accepted === 'queued', `${action} was not queued`);
}
const schema = marker => ({ type: 'object', properties: { marker: { type: 'string', enum: [marker] } }, required: ['marker'], additionalProperties: false });
const prompt = marker => `Return exactly the object {"marker":"${marker}"}. This is a Claudex verification fixture; use no tools and change no files.`;
const script = join(root, 'pause.workflow.js');
writeFileSync(script, `
phase('First');
const first = await agent(${JSON.stringify(prompt('ONE'))}, {label:'First real agent', schema:${JSON.stringify(schema('ONE'))}});
phase('Second');
const second = await agent(${JSON.stringify(prompt('TWO'))}, {label:'Second real agent', schema:${JSON.stringify(schema('TWO'))}});
return [first, second];
`);
const pauseId = `live-pause-${randomUUID()}`;
const active = launch(script, pauseId);
try {
  await until(() => status(pauseId).agents[0]?.status === 'running' && lock(pauseId).children.length === 1, 'first real child');
  control(pauseId, 'pause');
  const duplicate = spawnSync(binary, ['workflow', script, '--run-id', pauseId, '--run-id', 'different-id'], { windowsHide: true, encoding: 'utf8', timeout: 15_000 });
  check(duplicate.status !== 0 && duplicate.stderr.includes('Duplicate workflow option'), 'duplicate run ID must fail before execution');
  await until(() => status(pauseId).status === 'paused', 'pause after current agent');
  const paused = status(pauseId);
  check(paused.agents[0].status === 'completed', 'pause must drain the current agent');
  check(!paused.agents.slice(1).some(agent => agent.status === 'running' || agent.status === 'completed'), 'pause launched another agent');
  control(pauseId, 'resume');
  await until(() => status(pauseId).status === 'completed', 'resumed workflow completion');
  check(await exit(active) === 0, 'workflow did not exit successfully');
  const result = read(join(runs, pauseId, 'result.json')).result;
  check(result?.[0]?.marker === 'ONE' && result?.[1]?.marker === 'TWO', 'real inference results did not match the structured fixture');
  const replay = launch(script, pauseId);
  check(await exit(replay) === 0, 'checkpoint replay failed');
  const cached = status(pauseId).agents;
  check(cached.length === 2 && cached.every(agent => agent.status === 'cached'), 'replay did not reuse both results');
  const replayed = read(join(runs, pauseId, 'result.json')).result;
  check(replayed?.[0]?.marker === 'ONE' && replayed?.[1]?.marker === 'TWO', 'replayed results did not match the fixture');
  check(!existsSync(join(runs, pauseId, 'active.lock')), 'finished workflow retained its active lock');
  console.log('PASS: real sequential agents, pause, resume, duplicate ID rejection and cached replay');
} finally { await cleanup(); }

const stopScript = join(root, 'stop.workflow.js');
writeFileSync(stopScript, `await agent('Run only a PowerShell Start-Sleep -Seconds 30 command, then reply STOP. Do not change files.', {label:'Child to cancel'}); await agent(${JSON.stringify(prompt('NEVER'))}, {label:'Must not start'});`);
const stopId = `live-stop-${randomUUID()}`;
const stopped = launch(stopScript, stopId);
try {
  await until(() => lock(stopId).children.length === 1 && status(stopId).agents[0]?.status === 'running', 'real child before stop');
  const pid = lock(stopId).children[0];
  control(stopId, 'stop');
  await until(() => status(stopId).status === 'stopped' && !alive(pid), 'stop and child exit', 30_000);
  check(await exit(stopped) !== 0, 'stopped workflow must report interruption');
  check(status(stopId).agents[0].status === 'stopped', 'the observed child completed naturally instead of being cancelled');
  check(!status(stopId).agents.slice(1).some(agent => agent.status === 'running' || agent.status === 'completed'), 'stop launched a queued child');
  check(!existsSync(join(runs, stopId, 'active.lock')), 'stop did not release its active lock');
  console.log('PASS: actual child cancellation and queued-agent suppression');
} finally { await cleanup(); }
console.log(`Verification artifacts: ${root}`);
