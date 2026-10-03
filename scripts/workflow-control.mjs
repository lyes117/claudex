// File-based control plane for trusted local workflow runs. No prompts or credentials in status.
import { randomUUID } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, readdirSync, renameSync, unlinkSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';

export const atomic = (path, value) => {
  writeFileSync(`${path}.part`, JSON.stringify(value, null, 2));
  renameSync(`${path}.part`, path);
};
const actions = new Set(['pause', 'resume', 'stop']);
export const validRunId = id => typeof id === 'string' && /^[a-zA-Z0-9_-]{1,128}$/.test(id);
export const cleanLabel = text => String(text).replace(/[\x00-\x1f\x7f\x9b]/g, '').slice(0, 160);

export function readRun(runsRoot, runId) {
  if (!validRunId(runId)) throw new Error('Invalid run ID');
  return JSON.parse(readFileSync(join(resolve(runsRoot), runId, 'status.json'), 'utf8'));
}

export function listRuns(runsRoot) {
  if (!existsSync(runsRoot)) return [];
  return readdirSync(runsRoot, { withFileTypes: true }).filter(item => item.isDirectory() && validRunId(item.name)).flatMap(item => {
    try { return [readRun(runsRoot, item.name)]; } catch { return []; }
  }).sort((a, b) => b.startedAt - a.startedAt).slice(0, 1000);
}

export function sendControl(runsRoot, runId, action) {
  if (!actions.has(action)) throw new Error('Unknown workflow control');
  const state = readRun(runsRoot, runId);
  if (!['running', 'pausing', 'paused'].includes(state.status)) throw new Error('Workflow is not active');
  const directory = join(resolve(runsRoot), runId);
  const lock = JSON.parse(readFileSync(join(directory, 'active.lock'), 'utf8'));
  if (!Number.isInteger(lock.pid) || lock.pid <= 0) throw new Error('Malformed workflow lock');
  try { process.kill(lock.pid, 0); } catch { throw new Error('Workflow process is no longer active; resume its checkpoint'); }
  const commands = join(directory, 'commands');
  mkdirSync(commands, { recursive: true });
  const id = randomUUID();
  atomic(join(commands, `${id}.json`), { id, action, token: state.token, issuedAt: process.hrtime.bigint().toString() });
  return { id, action, runId, accepted: 'queued' };
}

export class WorkflowStopped extends Error {
  constructor() { super('Workflow stopped; checkpoint retained'); }
}

export function createRunController(directory, initial, pollMs = 100) {
  const commands = join(directory, 'commands');
  mkdirSync(commands, { recursive: true });
  const state = { ...initial, version: 1, token: randomUUID(), startedAt: Date.now(), updatedAt: Date.now(), status: 'running', phase: null, agents: [], lastControl: null };
  const abort = new AbortController();
  let failure;
  let resolveStop;
  const stopped = new Promise(resolveStopPromise => { resolveStop = resolveStopPromise; });
  const save = () => { state.updatedAt = Date.now(); atomic(join(directory, 'status.json'), state); };
  const stop = () => { state.status = 'stopped'; abort.abort(); resolveStop(); save(); };
  const apply = () => {
    try {
      const pending = readdirSync(commands).filter(name => name.endsWith('.json')).map(name => {
        const request = JSON.parse(readFileSync(join(commands, name), 'utf8'));
        if (typeof request.issuedAt !== 'string' || !/^\d{1,30}$/.test(request.issuedAt)) throw new Error('Malformed workflow control');
        return { name, request };
      }).sort((a, b) => BigInt(a.request.issuedAt) < BigInt(b.request.issuedAt) ? -1 : 1);
      for (const { name, request } of pending) {
        if (request.token === state.token && actions.has(request.action)) {
          if (request.action === 'stop') stop();
          else if (state.status !== 'stopped') {
            state.status = request.action === 'resume' ? 'running' : state.agents.some(agent => agent.status === 'running') ? 'pausing' : 'paused';
          }
          state.lastControl = request.id;
          save();
        }
        unlinkSync(join(commands, name));
      }
      if (state.status === 'pausing' && !state.agents.some(agent => agent.status === 'running')) { state.status = 'paused'; save(); }
    } catch (error) { failure = error; abort.abort(); resolveStop(); }
  };
  save();
  const timer = setInterval(apply, pollMs);
  const interrupt = () => { try { stop(); } catch (error) { failure = error; abort.abort(); resolveStop(); } };
  process.once('SIGINT', interrupt);
  const assertActive = () => { if (failure) throw failure; if (abort.signal.aborted) throw new WorkflowStopped(); };
  const stopRequested = stopped.then(() => { assertActive(); });
  stopRequested.catch(() => {});
  return {
    state, signal: abort.signal,
    stopRequested,
    fail(error) { failure = error; abort.abort(); resolveStop(); },
    async gate() {
      apply(); assertActive();
      while (state.status === 'paused' || state.status === 'pausing') {
        await new Promise(resolveWait => setTimeout(resolveWait, pollMs));
        assertActive();
      }
    },
    phase(title) { state.phase = cleanLabel(title); save(); },
    agent(index, patch) { state.agents[index] = { ...state.agents[index], ...patch }; save(); },
    finish(status) {
      clearInterval(timer); process.removeListener('SIGINT', interrupt);
      state.status = failure ? 'failed' : abort.signal.aborted ? 'stopped' : status;
      state.endedAt = Date.now(); save();
    },
  };
}
