// Explicit subscription check; synthetic padding only, never run by npm test.
import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import { loadSettings } from '../settings.mjs';
import { codexBase, codexHeaders } from '../providers.mjs';
import { translateRequest } from '../translation.mjs';
import { sseEvents } from '../stream.mjs';

const settings = await loadSettings();
const route = { ...settings.routes.opus, effort: 'low' };
assert.equal(route.provider, 'openai');
const body = translateRequest({ model: 'opus', system: 'Ignore the synthetic padding. Reply only OK. Do not use tools.', messages: [{ role: 'user', content: ' x'.repeat(805000) + '\nReply OK.' }] }, route);
const started = Date.now();
const response = await fetch(`${codexBase}/responses`, { method: 'POST', headers: await codexHeaders(), body: JSON.stringify(body), signal: AbortSignal.timeout(240000), redirect: 'error' });
const receipt = { model: route.model, status: response.status, completed: false };
if (!response.ok) {
  const error = await response.json().catch(() => ({}));
  receipt.contextRejected = /context|too.long|maximum.*token/i.test(String(error.error?.code || '') + ' ' + String(error.error?.message || ''));
} else {
  for await (const event of sseEvents(response.body)) {
    if (event.type === 'response.completed') { receipt.completed = true; receipt.usage = { input_tokens: event.response?.usage?.input_tokens, output_tokens: event.response?.usage?.output_tokens }; }
    if (['response.failed', 'error'].includes(event.type)) receipt.contextRejected = /context|too.long|maximum.*token/i.test(String(event.error?.code || event.response?.error?.code || '') + ' ' + String(event.error?.message || event.response?.error?.message || ''));
  }
}
receipt.milliseconds = Date.now() - started;
await mkdir(new URL('../artifacts/', import.meta.url), { recursive: true });
await writeFile(new URL('../artifacts/context-receipt.json', import.meta.url), JSON.stringify(receipt, null, 2));
console.log(JSON.stringify(receipt));
assert.ok(receipt.completed && receipt.usage?.input_tokens >= 800000, 'Subscription did not validate an 800k input');
