import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, readFileSync, rmSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import { runWorkflow, validate } from './workflows.mjs';
import { childExecutionProfile } from './workflow-execution-profile.mjs';

const root = mkdtempSync(join(tmpdir(), 'claudex-workflow-'));
try {
  const scriptPath = join(root, 'fixture.js');
  writeFileSync(scriptPath, `export const meta = { name: 'fixture', description: 'Local control-flow check' };
phase('Test');
const first = await agent('one', { schema: { type: 'object', required: ['ok'], properties: { ok: { type: 'boolean' } }, additionalProperties: false } });
const rest = await parallel([() => agent('two'), () => agent('three')]);
return { first, rest, input: args.input };`);
  let calls = 0;
  const execute = async (prompt, options) => { calls++; return options.schema ? { ok: true } : prompt; };
  const options = { scriptPath, cwd: root, runsRoot: root, runId: 'fixture', args: { input: 42 }, execute, log: () => {} };
  const expected = { first: { ok: true }, rest: ['two', 'three'], input: 42 };
  assert.deepEqual(JSON.parse(JSON.stringify(await runWorkflow(options))), expected);
  const checkpointPath = join(root, 'fixture', 'checkpoint.json');
  const inherited = JSON.parse(readFileSync(checkpointPath, 'utf8'));
  const inheritedBytes = readFileSync(checkpointPath, 'utf8');
  const hash = value => createHash('sha256').update(JSON.stringify(value)).digest('hex');
  assert.equal(inherited.identity, hash({ args: options.args, cwd: root }));
  assert.equal(inherited.fingerprint, hash({ script: readFileSync(scriptPath, 'utf8'), args: options.args, cwd: root }));
  assert.equal(inherited.results[0].key, hash({ prompt: 'one', options: { schema: { type: 'object', required: ['ok'], properties: { ok: { type: 'boolean' } }, additionalProperties: false } } }));
  await runWorkflow({ ...options, executionProfile: 'inherit' });
  for (const executionProfile of ['unknown', '', null, 1, {}, false]) {
    const runsRoot = join(root, `invalid-${typeof executionProfile}-${String(executionProfile)}`);
    await assert.rejects(() => runWorkflow({ ...options, executionProfile, runsRoot }), /execution profile/);
    assert.equal(existsSync(runsRoot), false, 'invalid profile rejected before artifacts');
  }
  await assert.rejects(() => runWorkflow({ ...options, executionProfile: 'text-only' }), /execution profile changed/);
  assert.equal(readFileSync(checkpointPath, 'utf8'), inheritedBytes);
  assert.equal(calls, 3, 'profile mismatch did not launch an agent');
  const textOnly = { ...options, runId: 'text-only', executionProfile: 'text-only' };
  await runWorkflow(textOnly);
  await runWorkflow(textOnly);
  assert.equal(calls, 6, 'same text-only profile reuses its own cache');
  const textCheckpoint = readFileSync(join(root, 'text-only', 'checkpoint.json'), 'utf8');
  await assert.rejects(() => runWorkflow({ ...textOnly, executionProfile: 'inherit' }), /execution profile changed/);
  assert.equal(readFileSync(join(root, 'text-only', 'checkpoint.json'), 'utf8'), textCheckpoint);
  assert.equal(calls, 6, 'reverse mismatch did not launch an agent');
  assert.deepEqual(JSON.parse(JSON.stringify(await runWorkflow(options))), expected);
  assert.equal(calls, 6, 'replay does not execute completed agents again');
  writeFileSync(join(root, 'fixture', 'active.lock'), JSON.stringify({ pid: 2147483647, children: [] }));
  await assert.rejects(() => runWorkflow(options), /exclusive OS lock/);
  await runWorkflow({ ...options, nativeLockHeld: true });
  assert.equal(calls, 6, 'dead lock recovery replays checkpoint');
  await assert.rejects(() => runWorkflow({ ...options, args: { input: 43 } }), /changed/);
  assert.throws(() => validate({ ok: 'yes' }, { type: 'object', properties: { ok: { type: 'boolean' } } }), /expected/);
  assert.equal(JSON.parse(readFileSync(join(root, 'fixture', 'result.json'), 'utf8')).result.input, 42);
  writeFileSync(scriptPath, `export const meta = { name: 'error' }; agent('pending'); throw new Error('script error');`);
  let finished = false;
  await assert.rejects(() => runWorkflow({ ...options, runId: 'error', execute: async () => { await new Promise(resolve => setTimeout(resolve, 20)); finished = true; return 'done'; } }), /script error/);
  assert.equal(finished, true, 'pending agent is joined before lock release');
  assert.equal(existsSync(join(root, 'error', 'active.lock')), false);
  writeFileSync(scriptPath, `const first = await agent('first'); first.count++; await agent(String(first.count)); return first;`);
  const mutation = { ...options, runId: 'mutation', execute: async prompt => prompt === 'first' ? { count: 0 } : prompt };
  assert.deepEqual(JSON.parse(JSON.stringify(await runWorkflow(mutation))), { count: 1 });
  assert.deepEqual(JSON.parse(JSON.stringify(await runWorkflow(mutation))), { count: 1 });
  const environment = { OPENAI_API_KEY: 'synthetic-openai', codex_api_key: 'synthetic-codex', CODEX_HOME: 'preserved-auth-home', OTHER: 'unchanged' };
  const baseline = { ...environment };
  const childProfile = childExecutionProfile('text-only', environment);
  assert.deepEqual(childProfile.spawnOptions.env, { CODEX_HOME: 'preserved-auth-home', OTHER: 'unchanged' });
  assert.deepEqual(environment, baseline, 'child env filtering never mutates parent');
  assert.deepEqual(childExecutionProfile('inherit', environment), { args: [], spawnOptions: {} });
  // Exercise production executeAgent's real OS spawn against a Node process double.
  // This validates argv/env plumbing, not Codex inference or native policy behavior.
  writeFileSync(join(root, 'exec'), `const fs = require('node:fs'); const args = process.argv.slice(2); let input=''; process.stdin.on('data', chunk=>input+=chunk); process.stdin.on('end',()=>{ fs.writeFileSync('spawn-receipt.json', JSON.stringify({args,input,apiEnvKeys:Object.keys(process.env).filter(key=>['OPENAI_API_KEY','CODEX_API_KEY'].includes(key.toUpperCase()))})); fs.writeFileSync(args[args.indexOf('--output-last-message')+1], JSON.stringify({ok:true})); });`);
  writeFileSync(scriptPath, `const result = await agent('synthetic prompt', {schema:{type:'object',required:['ok'],properties:{ok:{type:'boolean'}},additionalProperties:false}}); if (!result) throw new Error('agent failed'); return result;`);
  const previousBinary = process.env.CLAUDEX_BIN;
  try {
    process.env.CLAUDEX_BIN = process.execPath;
    assert.deepEqual(JSON.parse(JSON.stringify(await runWorkflow({ scriptPath, cwd: root, runsRoot: root, runId: 'real-spawn-text-only', executionProfile: 'text-only', log: () => {} }))), { ok: true });
    const receipt = JSON.parse(readFileSync(join(root, 'spawn-receipt.json'), 'utf8'));
    assert.equal(receipt.input, 'synthetic prompt');
    assert.deepEqual(receipt.apiEnvKeys, []);
    assert.deepEqual(receipt.args, ['--skip-git-repo-check', '--output-last-message', join(root, 'real-spawn-text-only', '0.output.txt'), '-C', root, ...childProfile.args, '--output-schema', join(root, 'real-spawn-text-only', '0.schema.json'), '-']);
    await runWorkflow({ scriptPath, cwd: root, runsRoot: root, runId: 'real-spawn-inherit', log: () => {} });
    const inheritReceipt = JSON.parse(readFileSync(join(root, 'spawn-receipt.json'), 'utf8'));
    assert.deepEqual(inheritReceipt.args, ['--skip-git-repo-check', '--output-last-message', join(root, 'real-spawn-inherit', '0.output.txt'), '-C', root, '--output-schema', join(root, 'real-spawn-inherit', '0.schema.json'), '-']);
  } finally {
    if (previousBinary === undefined) delete process.env.CLAUDEX_BIN;
    else process.env.CLAUDEX_BIN = previousBinary;
  }
  console.log('PASS workflow control flow, structured output validation and checkpoint replay (fixture, no inference)');
} finally { rmSync(root, { recursive: true, force: true }); }
