// Opt-in ChatGPT test: synthetic files only, official auth retained in place.
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { createInterface } from 'node:readline';
import { captureOwned, parseJsonLines, stopOwned } from './live-process.mjs';

const binary = process.env.CLAUDEX_LIVE_BIN;
assert.ok(binary, 'Set CLAUDEX_LIVE_BIN to the compiled or installed fork');
assert.equal(process.platform, 'win32', 'This live process-tree fixture targets Windows');
const artifacts = resolve('.build-tools/live-file-tools');
mkdirSync(artifacts, { recursive: true });
const root = mkdtempSync(join(artifacts, 'run-'));
writeFileSync(join(root, 'sample.txt'), 'SYNTHETIC_FILE_TOOL_MARKER été\n');
mkdirSync(join(root, '.claude'));
writeFileSync(join(root, '.claude/settings.json'), '{}');
assert.equal(spawnSync('git', ['init', '--quiet', root], { windowsHide: true }).status, 0);

async function readThread(threadId) {
  const child = spawn(binary, ['app-server', '--listen', 'stdio://'], {
    cwd: root, windowsHide: true, stdio: ['pipe', 'pipe', 'ignore'],
  });
  const closed = new Promise(resolve => child.once('close', resolve));
  const pending = new Map();
  let nextId = 0;
  const lines = createInterface({ input: child.stdout });
  const failPending = error => {
    for (const { reject, timer } of pending.values()) { clearTimeout(timer); reject(error); }
    pending.clear();
  };
  child.once('error', () => failPending(new Error('History reader failed to launch')));
  child.once('close', () => failPending(new Error('History reader closed')));
  child.stdin.on('error', () => failPending(new Error('History reader input closed')));
  lines.on('line', line => {
    let message;
    try { message = JSON.parse(line); } catch { failPending(new Error('Invalid history protocol')); return; }
    if (!message || typeof message !== 'object') { failPending(new Error('Invalid history message')); return; }
    if (message.method) {
      if (message.id !== undefined) failPending(new Error('Unexpected history server request'));
      return;
    }
    if (!Object.hasOwn(message, 'result') && !Object.hasOwn(message, 'error')) return;
    const request = pending.get(message.id);
    if (!request) return;
    pending.delete(message.id);
    clearTimeout(request.timer);
    if (message.error) request.reject(new Error('History request rejected; no server details exposed'));
    else request.resolve(message.result);
  });
  const request = (method, params) => new Promise((resolve, reject) => {
    const id = nextId++;
    const timer = setTimeout(() => { pending.delete(id); reject(new Error('History request timed out')); }, 30000);
    pending.set(id, { resolve, reject, timer });
    child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', id, method, params })}\n`);
  });
  try {
    await request('initialize', { clientInfo: { name: 'claudex-file-tool-test', version: '1' }, capabilities: { experimentalApi: true } });
    child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', method: 'initialized' })}\n`);
    const thread = (await request('thread/read', { threadId, includeTurns: true })).thread;
    assert.ok(thread?.id === threadId, 'Restored thread identity mismatch');
    assert.ok(typeof thread.cwd === 'string' && resolve(thread.cwd).toLowerCase() === root.toLowerCase(), 'Restored fixture directory mismatch');
    return thread;
  } finally {
    failPending(new Error('History reader stopping'));
    child.stdin.end();
    try { await stopOwned(child, closed); } finally { lines.close(); }
  }
}

const results = [];
for (const codeMode of [false, true]) {
  const prompt = 'Read sample.txt using the Read tool, list *.txt using Glob, and find SYNTHETIC_FILE_TOOL_MARKER using Grep output_mode content. Call each tool once. Use these actual tools, without shell commands, file writes, extra agents or other tools. Then answer FILE_TOOLS_OK.';
  const args = ['exec', '--ignore-user-config', '--skip-git-repo-check', '-s', 'read-only',
    '-c', `projects={${JSON.stringify(root)}={trust_level="trusted"}}`,
    '-c', 'forced_login_method="chatgpt"', '-c', 'model_reasoning_effort="low"',
    '-c', `features.code_mode=${codeMode}`, '-c', `features.code_mode_only=${codeMode}`, '-m', 'gpt-6.1-sol', '-C', root, '--json', prompt];
  const events = parseJsonLines(await captureOwned(binary, args, { cwd: root }));
  const threadId = events.find(event => event.type === 'thread.started')?.thread_id;
  assert.ok(typeof threadId === 'string' && /^[0-9a-f-]{36}$/i.test(threadId), 'Missing valid fixture thread identifier');
  assert.ok(events.some(event => event.type === 'turn.completed'));
  assert.ok(!events.some(event => ['command_execution', 'collab_tool_call'].includes(event.item?.type)), 'The fixture must not execute shell commands or extra agents');
  const thread = await readThread(threadId);
  const cards = thread.turns.flatMap(turn => turn.items).filter(item => item.type === 'dynamicToolCall' && ['Read', 'Glob', 'Grep'].includes(item.tool));
  assert.equal(cards.length, 3, 'Expected exactly the three native tool cards');
  assert.equal(new Set(cards.map(card => card.id)).size, 3, 'No duplicate calls');
  for (const tool of ['Read', 'Glob', 'Grep']) {
    const card = cards.find(card => card.tool === tool);
    assert.ok(card, `Missing native ${tool} call`);
    assert.ok(card.status === 'completed', 'Native fixture call did not complete');
    assert.ok(card.success === true, 'Native fixture call was not successful');
    assert.equal(/^exec-[0-9a-f-]{36}$/i.test(card.id), codeMode, 'The native nested-call identifier must agree with the requested mode');
    const text = card.contentItems.filter(item => item.type === 'inputText').map(item => item.text).join('\n');
    assert.ok(Buffer.byteLength(text) <= 8192);
    assert.ok(text.includes(tool === 'Glob' ? 'sample.txt' : 'SYNTHETIC_FILE_TOOL_MARKER'));
  }
  results.push({ codeMode, threadId, tools: cards.map(card => card.tool), restored: true });
}
writeFileSync(join(root, 'verified.json'), JSON.stringify({ results }, null, 2));
console.log('PASS real ChatGPT inference: Read/Grep/Glob, direct and CodeMode, restored native cards');
