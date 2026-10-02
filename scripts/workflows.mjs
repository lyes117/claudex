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

const hash = value => createHash('sha256').update(JSON.stringify(value)).digest('hex');
const atomic = (path, value) => { writeFileSync(`${path}.part`, JSON.stringify(value, null, 2)); renameSync(`${path}.part`, path); };

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
    process.once('SIGINT', terminate);
    child.once('error', error => { clearTimeout(timeout); process.removeListener('SIGINT', terminate); reject(error); });
    child.once('close', code => {
      try { context.unregisterChild(child.pid); } catch (error) { persistenceError ||= error; }
      clearTimeout(timeout); process.removeListener('SIGINT', terminate);
      if (persistenceError) return reject(persistenceError);
      if (code !== 0) return reject(new Error(`Codex agent failed (${code}); run checkpoint retained`));
      try { const result = readFileSync(output, 'utf8').trim(); fulfill(options.schema ? JSON.parse(result) : result); } catch (error) { reject(error); }
    });
    try { if (child.pid) context.registerChild(child.pid); } catch (error) { persistenceError = error; terminate(); }
    child.stdin.end(prompt);
  });
}

export async function runWorkflow({ scriptPath, args, cwd = process.cwd(), runId = randomUUID(), runsRoot = join(homedir(), '.claudex', 'workflow-runs'), execute = executeAgent, nativeLockHeld = false, log = message => process.stderr.write(`${message}\n`) }) {
  if (!/^[a-zA-Z0-9_-]+$/.test(runId)) throw new Error('Invalid run ID');
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
  try {
    const fingerprint = hash({ script, args, cwd: resolve(cwd) });
    const state = existsSync(checkpoint) ? JSON.parse(readFileSync(checkpoint, 'utf8')) : { fingerprint, results: [] };
    if (state.fingerprint !== fingerprint) throw new Error('Script, arguments or cwd changed; use a new run ID');
    let index = 0;
    const agent = (prompt, options = {}) => {
      if (typeof prompt !== 'string' || !prompt.trim()) return Promise.reject(new Error('Agent prompt must be nonempty'));
      options = structuredClone(options);
      for (const key of Object.keys(options)) if (!['label', 'phase', 'schema'].includes(key)) throw new Error(`Unsupported agent option ${key}; no agent launched`);
      validateSchema(options.schema);
      const position = index++;
      if (position >= 1000) throw new Error('1000-agent run limit');
      const key = hash({ prompt, options });
      const task = queue.then(async () => {
        if (state.results[position]) {
          if (state.results[position].key !== key) throw new Error('Agent sequence changed; use a new run ID');
          return structuredClone(state.results[position].result);
        }
        log(`[${options.phase || 'Agent'}] ${options.label || position + 1}`);
        const result = await execute(prompt, options, { index: position, directory, cwd: resolve(cwd), registerChild: pid => { children.add(pid); updateLock(); }, unregisterChild: pid => { children.delete(pid); updateLock(); } });
        if (options.schema) validate(result, options.schema);
        state.results[position] = { key, result: structuredClone(result) };
        atomic(checkpoint, state);
        return result;
      });
      queue = task;
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
    const context = vm.createContext({ args, agent, parallel, pipeline, phase: title => log(`[Phase] ${title}`), log,
      Workflow: () => { throw new Error('Nested Workflow calls are unsupported'); }, workflow: () => { throw new Error('Nested workflows are unsupported'); } }, { codeGeneration: { strings: false, wasm: false } });
    vm.runInContext('Math.random = () => { throw new Error("Pass randomness through args") }; Date.now = () => { throw new Error("Pass timestamps through args") };', context);
    const body = script.replace(/\bexport\s+const\s+meta\s*=/, 'const meta =');
    const result = await new vm.Script(`(async () => {\n${body}\n})()`, { filename: resolve(scriptPath) }).runInContext(context, { timeout: 10000 });
    await queue;
    atomic(join(directory, 'result.json'), { result: result ?? null });
    log(`Run ${runId} complete: ${directory}`);
    return result;
  } finally { await queue.catch(() => {}); unlinkSync(lock); }
}

async function main() {
  const [scriptPath, ...rest] = process.argv.slice(2);
  if (!scriptPath || scriptPath === '--help') { console.log('claudex workflow <script.js> [--args JSON|@file] [--run-id ID] [--cwd PATH]\nTrusted scripts; agents run sequentially through the compiled Codex fork.'); return; }
  const options = { scriptPath, nativeLockHeld: process.env.CLAUDEX_WORKFLOW_LOCK_HELD === '1' };
  console.error(`Executing trusted workflow script: ${resolve(scriptPath)} (Node runtime, not a security sandbox)`);
  for (let index = 0; index < rest.length; index += 2) {
    const [key, value] = rest.slice(index, index + 2);
    if (!value) throw new Error(`Missing value for ${key}`);
    if (key === '--args') options.args = JSON.parse(value.startsWith('@') ? readFileSync(value.slice(1), 'utf8') : value);
    else if (key === '--run-id') options.runId = value;
    else if (key === '--cwd') options.cwd = resolve(value);
    else throw new Error(`Unknown option ${key}`);
  }
  console.log(JSON.stringify(await runWorkflow(options)));
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main().catch(error => { console.error(error.message); process.exitCode = 1; });
