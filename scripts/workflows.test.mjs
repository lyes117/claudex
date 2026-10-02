import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, readFileSync, rmSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { runWorkflow, validate } from './workflows.mjs';

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
  assert.deepEqual(JSON.parse(JSON.stringify(await runWorkflow(options))), expected);
  assert.equal(calls, 3, 'replay does not execute completed agents again');
  writeFileSync(join(root, 'fixture', 'active.lock'), JSON.stringify({ pid: 2147483647, children: [] }));
  await assert.rejects(() => runWorkflow(options), /exclusive OS lock/);
  await runWorkflow({ ...options, nativeLockHeld: true });
  assert.equal(calls, 3, 'dead lock recovery replays checkpoint');
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
  console.log('PASS workflow control flow, structured output validation and checkpoint replay (fixture, no inference)');
} finally { rmSync(root, { recursive: true, force: true }); }
