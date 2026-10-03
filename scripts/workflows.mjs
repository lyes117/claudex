// Original implementation of the documented Claude workflow primitives.
// Trusted scripts only: node:vm is an execution context, not a security sandbox.
import { createHash, randomUUID } from 'node:crypto';
import { spawn } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync, renameSync, existsSync, openSync, closeSync, unlinkSync } from 'node:fs';
import { resolve, join } from 'node:path';
import { homedir } from 'node:os';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';
import { isDeepStrictEqual } from 'node:util';
import { atomic, cleanLabel, createRunController, listRuns, readRun, sendControl, validRunId, WorkflowStopped } from './workflow-control.mjs';

const hash = value => createHash('sha256').update(JSON.stringify(value)).digest('hex');

export function validate(value, schema, path = '$') {
  if (!schema || typeof schema !== 'object') throw new Error('Invalid output schema');
  if (schema.enum && !schema.enum.some(item => isDeepStrictEqual(item, value))) throw new Error(`${path}: enum mismatch`);
  const types = Array.isArray(schema.type) ? schema.type : [schema.type];
  const actual = value === null ? 'null' : Array.isArray(value) ? 'array' : typeof value;
  if (schema.type && !types.includes(actual) && !(actual === 'number' && Number.isInteger(value) && types.includes('integer'))) throw new Error(`${path}: expected ${types.join('|')}`);
  if (actual === 'object') {
    for (const key of schema.required || []) if (!Object.hasOwn(value, key)) throw new Error(`${path}.${key}: required`);
    for (const [key, item] of Object.entries(value)) {
      if (schema.additionalProperties === false && !Object.hasOwn(schema.properties || {}, key)) throw new Error(`${path}.${key}: unexpected property`);
      if (schema.properties?.[key]) validate(item, schema.properties[key], `${path}.${key}`);
    }
  }
  if (actual === 'array') {
    if (schema.minItems !== undefined && value.length < schema.minItems) throw new Error(`${path}: minItems`);
    if (schema.maxItems !== undefined && value.length > schema.maxItems) throw new Error(`${path}: maxItems`);
    if (schema.items) value.forEach((item, index) => validate(item, schema.items, `${path}[${index}]`));
  }
  if (actual === 'number' && ((schema.minimum !== undefined && value < schema.minimum) || (schema.maximum !== undefined && value > schema.maximum))) throw new Error(`${path}: number outside bounds`);
  for (const key of ['oneOf', 'anyOf', 'allOf', '$ref', 'not', 'if', 'patternProperties']) if (schema[key] !== undefined) throw new Error(`Unsupported schema keyword ${key}; no agent launched`);
}

function validateSchema(schema) {
  if (schema === undefined) return;
  if (!schema || typeof schema !== 'object' || Array.isArray(schema)) throw new Error('Invalid output schema');
  const supported = new Set(['type', 'enum', 'required', 'properties', 'additionalProperties', 'items', 'minItems', 'maxItems', 'minimum', 'maximum', 'description', 'title', '$schema']);
  for (const key of Object.keys(schema)) if (!supported.has(key)) throw new Error(`Unsupported schema keyword ${key}; no agent launched`);
  if (schema.additionalProperties !== undefined && typeof schema.additionalProperties !== 'boolean') throw new Error('Schema-valued additionalProperties is unsupported');
  for (const key of ['oneOf', 'anyOf', 'allOf', '$ref', 'not', 'if', 'patternProperties']) if (schema[key] !== undefined) throw new Error(`Unsupported schema keyword ${key}; no agent launched`);
  if (schema.additionalProperties === false && (schema.required || []).some(key => !Object.hasOwn(schema.properties || {}, key))) throw new Error('Contradictory required/additionalProperties schema');
  for (const child of Object.values(schema.properties || {})) validateSchema(child);
  if (schema.items) validateSchema(schema.items);
}

async function executeAgent(prompt, options, context) {
  const executable = process.env.CLAUDEX_BIN;
  if (!executable || !existsSync(executable)) throw new Error('CLAUDEX_BIN must point to the compiled fork');
  const output = join(context.directory, `${context.index}.output.txt`);
  const args = ['exec', '--skip-git-repo-check', '--output-last-message', output, '-C', context.cwd];
  if (options.schema) {
    const schema = join(context.directory, `${context.index}.schema.json`);
    atomic(schema, options.schema);
    args.push('--output-schema', schema);
  }
  args.push('-');
  return new Promise((fulfill, reject) => {
    const child = spawn(executable, args, { cwd: context.cwd, windowsHide: true, stdio: ['pipe', 'ignore', 'ignore'] });
    const terminate = () => { if (process.platform === 'win32') spawn('taskkill', ['/pid', String(child.pid), '/t', '/f'], { windowsHide: true, stdio: 'ignore' }); else child.kill('SIGTERM'); };
    let persistenceError;
    child.stdin.on('error', error => { persistenceError ||= error; terminate(); });
    const timeout = setTimeout(terminate, 20 * 60 * 1000);
    context.signal.addEventListener('abort', terminate, { once: true });
    process.once('SIGINT', terminate);
    child.once('error', error => { clearTimeout(timeout); context.signal.removeEventListener('abort', terminate); process.removeListener('SIGINT', terminate); reject(error); });
    child.once('close', code => {
      try { context.unregisterChild(child.pid); } catch (error) { persistenceError ||= error; }
      clearTimeout(timeout); process.removeListener('SIGINT', terminate);
      context.signal.removeEventListener('abort', terminate);
      if (persistenceError) return reject(persistenceError);
      if (code !== 0) return reject(new Error(`Codex agent failed (${code}); run checkpoint retained`));
      try { const result = readFileSync(output, 'utf8').trim(); fulfill(options.schema ? JSON.parse(result) : result); } catch (error) { reject(error); }
    });
    try { if (child.pid) context.registerChild(child.pid); } catch (error) { persistenceError = error; terminate(); }
    child.stdin.end(prompt);
  });
}

export async function runWorkflow({ scriptPath, args, cwd = process.cwd(), runId = randomUUID(), runsRoot = join(homedir(), '.claudex', 'workflow-runs'), execute = executeAgent, nativeLockHeld = false, log = message => process.stderr.write(`${message}\n`) }) {
  if (!validRunId(runId)) throw new Error('Invalid run ID');
  const script = readFileSync(resolve(scriptPath), 'utf8');
  const directory = join(resolve(runsRoot), runId);
  mkdirSync(directory, { recursive: true });
  const checkpoint = join(directory, 'checkpoint.json');
  const lock = join(directory, 'active.lock');
  const isAlive = pid => { try { process.kill(pid, 0); return true; } catch (error) { if (error.code === 'ESRCH') return false; throw error; } };
  if (existsSync(lock)) {
    const previous = JSON.parse(readFileSync(lock, 'utf8'));
    if (!Number.isInteger(previous.pid) || previous.pid <= 0 || !Array.isArray(previous.children)) throw new Error('Malformed workflow lock; inspect it before recovery');
    if ([previous.pid, ...previous.children].some(isAlive)) throw new Error('Workflow or child agent is still active');
    if (!nativeLockHeld) throw new Error('Recover a dead lock through claudex workflow; an exclusive OS lock is required');
    unlinkSync(lock);
  }
  const fd = openSync(lock, 'wx');
  const children = new Set();
  const updateLock = () => atomic(lock, { pid: process.pid, children: [...children] });
  writeFileSync(fd, JSON.stringify({ pid: process.pid, children: [] }));
  closeSync(fd);
  let queue = Promise.resolve();
  let fatalError;
  let controller;
  let finalStatus = 'failed';
  try {
    const fingerprint = hash({ script, args, cwd: resolve(cwd) });
    const identity = hash({ args, cwd: resolve(cwd) });
    const state = existsSync(checkpoint) ? JSON.parse(readFileSync(checkpoint, 'utf8')) : { fingerprint, identity, results: [] };
    if (state.identity ? state.identity !== identity : state.fingerprint !== fingerprint) throw new Error('Arguments or cwd changed, or legacy checkpoint cannot replay edited script; use a new run ID');
    if (!Array.isArray(state.results) || state.results.length > 1000) throw new Error('Malformed workflow checkpoint');
    controller = createRunController(directory, { runId, scriptPath: resolve(scriptPath), cwd: resolve(cwd) });
    let replayInvalidated = false;
    let index = 0;
    const agent = (prompt, options = {}) => {
      if (typeof prompt !== 'string' || !prompt.trim()) return Promise.reject(new Error('Agent prompt must be nonempty'));
      options = structuredClone(options);
      for (const key of Object.keys(options)) if (!['label', 'phase', 'schema'].includes(key)) throw new Error(`Unsupported agent option ${key}; no agent launched`);
      validateSchema(options.schema);
      const position = index++;
      if (position >= 1000) throw new Error('1000-agent run limit');
      const key = hash({ prompt, options });
      const phase = cleanLabel(options.phase || controller.state.phase || '');
      try {
        controller.agent(position, { index: position, label: cleanLabel(options.label || `Agent ${position + 1}`), phase, status: 'pending' });
      } catch (error) {
        fatalError ||= error;
        throw error;
      }
      const task = queue.then(async () => {
        await controller.gate();
        if (!replayInvalidated && state.results[position]?.key === key && state.results[position].status !== 'failed') {
          controller.agent(position, { status: 'cached' });
          return structuredClone(state.results[position].result);
        }
        if (!replayInvalidated) { replayInvalidated = true; state.results.splice(position); }
        log(`[${phase || 'Agent'}] ${options.label || position + 1}`);
        controller.agent(position, { status: 'running', startedAt: Date.now() });
        let result;
        let failed = false;
        try {
          result = await execute(prompt, options, { index: position, directory, cwd: resolve(cwd), signal: controller.signal, registerChild: pid => { children.add(pid); updateLock(); }, unregisterChild: pid => { children.delete(pid); updateLock(); } });
          if (options.schema) validate(result, options.schema);
        } catch {
          result = null; failed = true;
          log(`Agent ${position + 1} did not complete; checkpoint retained`);
        }
        controller.agent(position, { status: failed ? controller.signal.aborted ? 'stopped' : 'failed' : 'completed', endedAt: Date.now() });
        state.results[position] = { key, result: structuredClone(result), status: failed ? 'failed' : 'completed' };
        state.fingerprint = fingerprint; state.identity = identity;
        atomic(checkpoint, state);
        return result;
      });
      queue = task.catch(error => { fatalError ||= error; });
      task.catch(() => {});
      return task;
    };
    // ponytail: serial scheduling preserves ordering; add concurrency only with isolated workspaces.
    const parallel = async tasks => {
      if (!Array.isArray(tasks) || tasks.length > 4096) throw new Error('parallel expects at most 4096 tasks');
      const results = [];
      for (const task of tasks) results.push(await (typeof task === 'function' ? task() : task));
      return results;
    };
    const pipeline = (items, task) => parallel(items.map((item, index) => () => task(item, index)));
    const context = vm.createContext({ args: structuredClone(args), agent, parallel, pipeline, phase: title => { controller.phase(title); log(`[Phase] ${cleanLabel(title)}`); }, log,
      Workflow: () => { throw new Error('Nested Workflow calls are unsupported'); }, workflow: () => { throw new Error('Nested workflows are unsupported'); } }, { codeGeneration: { strings: false, wasm: false } });
    vm.runInContext('Math.random = () => { throw new Error("Pass randomness through args") }; const NativeDate = Date; Date = class extends NativeDate { constructor(...args) { if (!args.length) throw new Error("Pass timestamps through args"); super(...args); } static now() { throw new Error("Pass timestamps through args") } };', context);
    const body = script.replace(/\bexport\s+const\s+meta\s*=/, 'const meta =');
    const result = await Promise.race([new vm.Script(`(async () => {\n${body}\n})()`, { filename: resolve(scriptPath) }).runInContext(context, { timeout: 10000 }), controller.stopRequested]);
    await queue;
    if (fatalError) throw fatalError;
    if (controller.signal.aborted) throw new WorkflowStopped();
    if (state.fingerprint !== fingerprint || state.results.length > index) {
      state.fingerprint = fingerprint; state.results.length = index; atomic(checkpoint, state);
    }
    atomic(join(directory, 'result.json'), { result: result ?? null });
    finalStatus = 'completed';
    log(`Run ${runId} complete: ${directory}`);
    return result;
  } catch (error) {
    if (!(error instanceof WorkflowStopped)) controller?.fail(error);
    throw error;
  } finally {
    await queue.catch(() => {});
    try { controller?.finish(finalStatus); } finally { unlinkSync(lock); }
  }
}

async function main() {
  const [scriptPath, ...rest] = process.argv.slice(2);
  if (!scriptPath || scriptPath === '--help') { console.log('claudex workflow <script.js> [--args JSON|@file] [--run-id ID] [--cwd PATH]\nclaudex workflow list|status <ID>|pause <ID>|resume <ID>|stop <ID>\nTrusted scripts; agents run sequentially through the compiled Codex fork.'); return; }
  const runsRoot = join(homedir(), '.claudex', 'workflow-runs');
  if (scriptPath === 'list') { console.log(JSON.stringify(listRuns(runsRoot), null, 2)); return; }
  if (scriptPath === 'status') { console.log(JSON.stringify(readRun(runsRoot, rest[0]), null, 2)); return; }
  if (['pause', 'resume', 'stop'].includes(scriptPath)) { console.log(JSON.stringify(sendControl(runsRoot, rest[0], scriptPath))); return; }
  const options = { scriptPath, nativeLockHeld: process.env.CLAUDEX_WORKFLOW_LOCK_HELD === '1' };
  const seen = new Set();
  console.error(`Executing trusted workflow script: ${resolve(scriptPath)} (Node runtime, not a security sandbox)`);
  for (let index = 0; index < rest.length; index += 2) {
    const [key, value] = rest.slice(index, index + 2);
    if (!value) throw new Error(`Missing value for ${key}`);
    if (seen.has(key)) throw new Error('Duplicate workflow option');
    seen.add(key);
    if (key === '--args') options.args = JSON.parse(value.startsWith('@') ? readFileSync(value.slice(1), 'utf8') : value);
    else if (key === '--run-id') options.runId = value;
    else if (key === '--cwd') options.cwd = resolve(value);
    else throw new Error(`Unknown option ${key}`);
  }
  console.log(JSON.stringify(await runWorkflow(options)));
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main().catch(error => { console.error(error.message); process.exitCode = 1; });
