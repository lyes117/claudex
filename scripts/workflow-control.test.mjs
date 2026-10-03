import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdtempSync, writeFileSync, readFileSync, rmSync, mkdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { runWorkflow } from './workflows.mjs';
import { readRun, sendControl } from './workflow-control.mjs';

const until = async predicate => {
  const deadline = Date.now() + 5000;
  while (!predicate()) {
    if (Date.now() > deadline) throw new Error('Timed out waiting for workflow state');
    await new Promise(resolve => setTimeout(resolve, 10));
  }
};

test('edited workflow replays the unchanged prefix and reruns the changed suffix', async () => {
  const root = mkdtempSync(join(tmpdir(), 'claudex-replay-'));
  try {
    const scriptPath = join(root, 'fixture.js');
    const options = { scriptPath, cwd: root, runsRoot: root, runId: 'replay', log: () => {} };
    const calls = [];
    const execute = async prompt => { calls.push(prompt); return prompt; };
    writeFileSync(scriptPath, "await agent('one'); await agent('two'); return await agent('three');");
    assert.equal(await runWorkflow({ ...options, execute }), 'three');
    writeFileSync(scriptPath, "await agent('one'); await agent('new two'); return await agent('three');");
    assert.equal(await runWorkflow({ ...options, execute }), 'three');
    assert.deepEqual(calls, ['one', 'two', 'three', 'new two', 'three']);
    const status = JSON.parse(readFileSync(join(root, 'replay', 'status.json'), 'utf8'));
    assert.equal(status.status, 'completed');
    assert.deepEqual(status.agents.map(agent => agent.status), ['cached', 'completed', 'completed']);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('pause drains the running agent, blocks the next one, and resume continues', async () => {
  const root = mkdtempSync(join(tmpdir(), 'claudex-pause-'));
  try {
    const scriptPath = join(root, 'fixture.js');
    writeFileSync(scriptPath, "await agent('one'); return await agent('two');");
    let release;
    const pending = new Promise(resolve => { release = resolve; });
    const calls = [];
    const running = runWorkflow({ scriptPath, cwd: root, runsRoot: root, runId: 'pause', log: () => {}, execute: async prompt => { calls.push(prompt); if (prompt === 'one') await pending; return prompt; } });
    await until(() => calls.length === 1);
    const paused = sendControl(root, 'pause', 'pause');
    await until(() => readRun(root, 'pause').lastControl === paused.id);
    assert.equal(readRun(root, 'pause').status, 'pausing');
    release();
    await until(() => readRun(root, 'pause').status === 'paused');
    assert.deepEqual(calls, ['one']);
    sendControl(root, 'pause', 'resume');
    assert.equal(await running, 'two');
    assert.equal(readRun(root, 'pause').status, 'completed');
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('stop aborts the running executor and never starts the queued agent', async () => {
  const root = mkdtempSync(join(tmpdir(), 'claudex-stop-'));
  try {
    const scriptPath = join(root, 'fixture.js');
    writeFileSync(scriptPath, "await agent('one'); return await agent('two');");
    const calls = [];
    const running = runWorkflow({ scriptPath, cwd: root, runsRoot: root, runId: 'stop', log: () => {}, execute: async (prompt, options, context) => {
      calls.push(prompt);
      await new Promise((resolve, reject) => context.signal.addEventListener('abort', () => reject(new Error('stopped')), { once: true }));
    } });
    const rejection = assert.rejects(running, /stopped/);
    await until(() => calls.length === 1);
    sendControl(root, 'stop', 'stop');
    await rejection;
    assert.deepEqual(calls, ['one']);
    assert.equal(readRun(root, 'stop').status, 'stopped');
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('implicit timestamps fail before launching any agents', async () => {
  const root = mkdtempSync(join(tmpdir(), 'claudex-date-'));
  try {
    const scriptPath = join(root, 'fixture.js');
    writeFileSync(scriptPath, "new Date(); await agent('must not run');");
    let calls = 0;
    await assert.rejects(runWorkflow({ scriptPath, cwd: root, runsRoot: root, execute: async () => { calls++; }, log: () => {} }), /timestamps/);
    assert.equal(calls, 0);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('failed agent returns null, remains visibly failed, and invalidates its replay suffix', async () => {
  const root = mkdtempSync(join(tmpdir(), 'claudex-failure-'));
  try {
    const scriptPath = join(root, 'fixture.js');
    writeFileSync(scriptPath, "const result = await agent('one'); await agent('two'); return result;");
    const calls = [];
    const options = { scriptPath, cwd: root, runsRoot: root, runId: 'failure', log: () => {}, execute: async prompt => { calls.push(prompt); if (prompt === 'one') throw new Error('fixture'); return prompt; } };
    assert.equal(await runWorkflow(options), null);
    assert.equal(readRun(root, 'failure').agents[0].status, 'failed');
    assert.equal(await runWorkflow(options), null);
    assert.deepEqual(calls, ['one', 'two', 'one', 'two']);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('a removed suffix cannot reappear from the cache after another script edit', async () => {
  const root = mkdtempSync(join(tmpdir(), 'claudex-truncate-'));
  try {
    const scriptPath = join(root, 'fixture.js');
    const calls = [];
    const options = { scriptPath, cwd: root, runsRoot: root, runId: 'truncate', log: () => {}, execute: async prompt => { calls.push(prompt); return prompt; } };
    const original = "await agent('one'); await agent('two'); return await agent('three');";
    writeFileSync(scriptPath, original);
    await runWorkflow(options);
    writeFileSync(scriptPath, "return await agent('one');");
    await runWorkflow(options);
    writeFileSync(scriptPath, original);
    await runWorkflow(options);
    assert.deepEqual(calls, ['one', 'two', 'three', 'two', 'three']);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('fire-and-forget agents propagate checkpoint failures before reporting success', async () => {
  const root = mkdtempSync(join(tmpdir(), 'claudex-storage-'));
  try {
    const scriptPath = join(root, 'fixture.js');
    writeFileSync(scriptPath, "agent('one'); return 'ok';");
    mkdirSync(join(root, 'storage', 'checkpoint.json.part'), { recursive: true });
    await assert.rejects(runWorkflow({ scriptPath, cwd: root, runsRoot: root, runId: 'storage', log: () => {}, execute: async () => 'done' }));
    assert.equal(readRun(root, 'storage').status, 'failed');
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('script failure drains a paused queue without leaving the run hung', async () => {
  const root = mkdtempSync(join(tmpdir(), 'claudex-paused-error-'));
  try {
    const scriptPath = join(root, 'fixture.js');
    writeFileSync(scriptPath, "await agent('one'); agent('two'); throw new Error('script failed');");
    let release;
    const pending = new Promise(resolve => { release = resolve; });
    let started = false;
    const running = runWorkflow({ scriptPath, cwd: root, runsRoot: root, runId: 'paused-error', log: () => {}, execute: async prompt => {
      started = true; if (prompt === 'one') await pending; return prompt;
    } });
    const rejection = assert.rejects(running, /script failed/);
    await until(() => started);
    const control = sendControl(root, 'paused-error', 'pause');
    await until(() => readRun(root, 'paused-error').lastControl === control.id);
    release();
    await rejection;
    assert.equal(readRun(root, 'paused-error').status, 'failed');
  } finally { rmSync(root, { recursive: true, force: true }); }
});
