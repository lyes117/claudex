import { test } from 'node:test';
import assert from 'node:assert/strict';
import { captureOwned, parseJsonLines, stopOwned } from './live-process.mjs';

test('malformed JSON errors never expose captured text', () => {
  assert.throws(() => parseJsonLines('FAKE_SECRET_CANARY_NOT_JSON'), error =>
    error.message === 'Invalid fixture JSON; raw output is not printed');
});

test('captured owned child returns valid stdout', async () => {
  const text = await captureOwned(process.execPath, ['-e', 'process.stdout.write(JSON.stringify({ok:true}))'], { cwd: process.cwd(), timeoutMs: 10000 });
  assert.deepEqual(parseJsonLines(text), [{ ok: true }]);
});

test('owned timeout and output overflow finish with generic errors', { skip: process.platform !== 'win32', timeout: 30000 }, async () => {
  await assert.rejects(captureOwned(process.execPath, ['-e', 'setInterval(()=>{},1000)'], { cwd: process.cwd(), timeoutMs: 250 }), /Owned fixture timed out/);
  await assert.rejects(captureOwned(process.execPath, ['-e', 'process.stdout.write("FAKE_SECRET_CANARY".repeat(100));setInterval(()=>{},1000)'], { cwd: process.cwd(), timeoutMs: 10000, maxBytes: 32 }), /Owned fixture output exceeded its limit/);
});

test('unconfirmable exit is bounded without touching an unverified PID', { skip: process.platform !== 'win32', timeout: 1000 }, async () => {
  const child = { exitCode: null, signalCode: null, pid: undefined };
  await assert.rejects(stopOwned(child, new Promise(() => {}), { graceMs: 1, exitMs: 1 }), /Owned fixture exit could not be confirmed/);
});
