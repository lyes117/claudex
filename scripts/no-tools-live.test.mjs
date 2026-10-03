// Opt-in inference smoke. Adversarial dispatch is covered by native SSE tests;
// CLI events alone do not prove the absence of every filesystem operation.
import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { captureOwned, parseJsonLines } from './live-process.mjs';

const binary = process.env.CLAUDEX_LIVE_BIN;
assert.ok(binary, 'Set CLAUDEX_LIVE_BIN to the installed or compiled fork');
assert.equal(process.platform, 'win32', 'This fixture owns a Windows process tree');
const artifacts = resolve('.build-tools/live-no-tools');
mkdirSync(artifacts, { recursive: true });
const root = mkdtempSync(join(artifacts, 'run-'));
mkdirSync(join(root, '.claude'));
writeFileSync(join(root, '.claude/settings.json'), '{}');

// No model catalog override: exercise the official model's current default.
// Ignore user settings only for this synthetic fixture; keep official auth in place.
const args = ['-c', 'tools.enabled=false',
  'exec', '--ignore-user-config', '--skip-git-repo-check', '-s', 'read-only',
  '-c', `projects={${JSON.stringify(root)}={trust_level="trusted"}}`,
  '-c', 'forced_login_method="chatgpt"', '-c', 'features.hooks=false',
  '-c', 'model_reasoning_effort="low"', '-m', 'gpt-6.1-sol', '-C', root,
  '--json', 'Reply exactly NO_TOOLS_OK. This is a text-only smoke test.'];
const events = parseJsonLines(await captureOwned(binary, args, { cwd: root }));
const threadId = events.find(event => event.type === 'thread.started')?.thread_id;
assert.ok(typeof threadId === 'string' && /^[0-9a-f-]{36}$/i.test(threadId), 'Missing fixture thread');
assert.ok(events.some(event => event.type === 'turn.completed'), 'Inference did not complete');
assert.ok(!events.some(event => ['error', 'turn.failed'].includes(event.type)), 'Inference reported failure');
const items = events.filter(event => event.type === 'item.completed').map(event => event.item);
assert.ok(items.every(item => item && ['reasoning', 'agent_message'].includes(item.type)),
  'Unexpected tool or other completed item; raw output is not printed');
assert.ok(items.some(item => item.type === 'agent_message' && item.text?.trim() === 'NO_TOOLS_OK'),
  'Expected real text reply; raw output is not printed');
writeFileSync(join(root, 'verified.json'), JSON.stringify({
  threadId, toolsEnabled: false, model: 'gpt-6.1-sol', catalogOverride: false,
  completed: true, completedItemTypes: items.map(item => item.type),
  boundary: 'CLI inference smoke; native fixture proves hostile dispatch rejection',
}, null, 2));
console.log('PASS real ChatGPT text inference with tools.enabled=false and official model defaults');
