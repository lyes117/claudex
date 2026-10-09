import test from 'node:test';
import assert from 'node:assert/strict';
import { startGateway } from '../server.mjs';
import { translateRequest, translateUsage, codexToAnthropic, StreamConverter } from '../translation.mjs';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { imageMime } from '../images.mjs';
import { sseEvents } from '../stream.mjs';
import { resolveRoute, aliases, openaiContextWindow } from '../settings.mjs';
import { permissionArgs } from '../launcher.mjs';

const token = 'a'.repeat(64);
const route = { provider: 'openai', model: 'test-model', effort: 'high' };
const settings = { routes: { opus: route, sonnet: route, haiku: route }, catalog: [] };
const tool = { name: 'Read', input_schema: { type: 'object', properties: { path: { type: 'string' }, optional: { type: 'string' } }, required: ['path'] } };
const call = { type: 'function_call', name: 'Read', call_id: 'call-1', arguments: '{"path":"fixture.txt"}' };
const complete = { type: 'response.completed', response: { id: 'r1', output: [call], usage: { input_tokens: 10, output_tokens: 3 } } };
const sse = events => events.map(event => `data: ${JSON.stringify(event)}\n\n`).join('');

test('autonomous launch uses native bypass while explicit permission modes remain authoritative', () => {
  assert.deepEqual(permissionArgs(['--print']), ['--permission-mode', 'bypassPermissions', '--print']);
  for (const args of [['--permission-mode', 'manual'], ['--permission-mode=dontAsk'], ['--restricted'], ['--dangerously-skip-permissions']]) assert.deepEqual(permissionArgs(args), args);
});

test('large-context custom aliases preserve legacy workflow routing and GLM budgets', () => {
  for (const tier of Object.keys(aliases)) {
    assert.ok(!aliases[tier].startsWith('claude-'));
    assert.equal(resolveRoute(aliases[tier], settings).tier, tier);
    assert.equal(resolveRoute(`claude-claudex-${tier}`, settings).tier, tier);
    assert.equal(resolveRoute(tier, settings).tier, tier);
  }
  const catalog = [{ slug: 'test-model', max_context_window: 872000 }];
  assert.equal(openaiContextWindow, 800000);
  assert.equal(catalog[0].max_context_window >= openaiContextWindow, true);
});

test('oversized contexts cannot cross into GLM and context errors remain recognizable without private text', async t => {
  let calls = 0;
  const server = await startGateway({ token, settings: { ...settings, zaiFallback: 'GLM-5.3' }, credentials: { openai: async () => ({}), zai: async () => ({}) }, fetchImpl: async () => {
    calls++;
    if (calls === 3) return new Response(sse([{ type: 'error', code: 'context_length_exceeded', message: 'PRIVATE_UPSTREAM_TEXT' }]));
    return new Response(JSON.stringify({ error: { code: calls === 1 ? 'context_length_exceeded' : 'rate_limit_exceeded', message: 'PRIVATE_UPSTREAM_TEXT' } }), { status: calls === 1 ? 400 : 429 });
  } });
  t.after(() => server.close());
  const post = body => fetch(`http://127.0.0.1:${server.address().port}/v1/messages`, { method: 'POST', headers: { authorization: `Bearer ${token}` }, body: JSON.stringify(body) });
  const body = { model: 'opus', messages: [{ role: 'user', content: 'x'.repeat(210000) }] };
  const rejected = await post(body);
  assert.equal(rejected.status, 400);
  const text = await rejected.text();
  assert.match(text, /Prompt is too long/); assert.doesNotMatch(text, /PRIVATE_UPSTREAM_TEXT/);
  assert.equal(calls, 1);
  const quota = await post(body);
  assert.equal(quota.status, 429);
  assert.equal(calls, 2); // No second HTTP call to GLM after the OpenAI 429.
  const stream = await post({ model: 'openai/test-sse', messages: [], stream: true });
  const frame = await stream.text();
  assert.match(frame, /Prompt is too long/); assert.doesNotMatch(frame, /PRIVATE_UPSTREAM_TEXT/);
  assert.equal(calls, 3);
});

test('routing, optional arguments, tool images, choices and structured outputs survive translation', () => {
  assert.equal(resolveRoute('claude-opus-4-6', settings).model, 'test-model');
  assert.throws(() => resolveRoute('unknown', settings));
  const request = { model: 'opus', system: [{ type: 'text', text: 'original system' }], messages: [{ role: 'user', content: [{ type: 'tool_result', tool_use_id: 'call-0', content: [{ type: 'text', text: 'image' }, { type: 'image', source: { type: 'base64', media_type: 'image/png', data: 'fixture' } }] }] }], tools: [tool], tool_choice: { type: 'tool', name: 'Read', disable_parallel_tool_use: true }, output_config: { format: { type: 'json_schema', schema: { type: 'object' } } } };
  const body = translateRequest(request, route);
  assert.equal(body.instructions, 'original system');
  assert.deepEqual(body.tools[0].parameters.required, ['path']);
  assert.equal(body.tools[0].strict, false);
  assert.equal(body.input[1].content[0].type, 'input_image');
  assert.equal(body.tool_choice.name, 'Read');
  assert.equal(body.parallel_tool_calls, false);
  assert.equal(body.text.format.type, 'json_schema');
  assert.equal(body.store, false); assert.equal(body.stream, true);
  assert.deepEqual(translateUsage({ input_tokens: 100, input_tokens_details: { cached_tokens: 90 }, output_tokens: 4 }), { input_tokens: 10, cache_read_input_tokens: 90, output_tokens: 4 });
  assert.throws(() => translateRequest({ model: 'opus', messages: [{ role: 'user', content: [{ type: 'tool_result', tool_use_id: 'call-0', content: [{ type: 'document', source: { type: 'base64' } }] }] }] }, route), /Unsupported content/);
  const reasoning = { type: 'reasoning', id: 'reasoning-1', encrypted_content: 'opaque-provider-state', summary: [{ text: 'public summary' }] };
  const answer = codexToAnthropic({ output: [reasoning, call], usage: {} }, 'opus');
  const next = translateRequest({ model: 'opus', messages: [{ role: 'assistant', content: answer.content }] }, route);
  assert.equal(next.input[0].type, 'reasoning'); assert.equal(next.input[0].encrypted_content, reasoning.encrypted_content);
  assert.equal(next.input[1].type, 'function_call');
  const search = translateRequest({ model: 'opus', messages: [{ role: 'user', content: 'search fixture' }], tools: [{ type: 'web_search_20250305', name: 'web_search' }], tool_choice: { type: 'tool', name: 'web_search' } }, route);
  assert.deepEqual(search.tools, [{ type: 'web_search' }]);
  assert.deepEqual(search.tool_choice, { type: 'web_search' });
  const restricted = translateRequest({ model: 'opus', messages: [], tools: [{ type: 'web_search_20250305', name: 'web_search', blocked_domains: ['example.com'], max_uses: 1 }] }, route);
  assert.deepEqual(restricted.tools[0].filters.blocked_domains, ['example.com']);
  assert.match(restricted.instructions, /at most 1 times/);
  assert.equal(restricted.max_tool_calls, undefined);
  assert.equal(resolveRoute('openai/test-model@high', settings).tier, 'explicit');
  const searchCall = { type: 'web_search_call', id: 'search-1', action: { query: 'fixture' } };
  const searchOutput = [searchCall, { type: 'message', content: [{ type: 'output_text', text: 'fixture', annotations: [{ type: 'url_citation', url: 'https://nodejs.org/api/fs.html', title: 'Node.js' }] }] }];
  const searchAnswer = codexToAnthropic({ output: searchOutput, usage: {} }, 'opus');
  assert.equal(searchAnswer.content[0].type, 'server_tool_use');
  assert.equal(searchAnswer.content[1].content[0].url, 'https://nodejs.org/api/fs.html');
  const searchStream = new StreamConverter('opus');
  assert.match(searchStream.processEvent('response.output_item.done', { item: searchCall }).join(''), /server_tool_use/);
  assert.match(searchStream.processEvent('response.completed', { response: { output: searchOutput, usage: {} } }).join(''), /web_search_tool_result/);
});

test('image MCP rejects null JSON requests and remains usable', async () => {
  const child = spawn(process.execPath, [fileURLToPath(new URL('../images-mcp.mjs', import.meta.url))], { windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
  let output = ''; child.stdout.on('data', bytes => { output += bytes; });
  child.stdin.end('null\n{"jsonrpc":"2.0","id":1,"method":"tools/list"}\n');
  const code = await new Promise((resolve, reject) => { child.once('error', reject); child.once('exit', resolve); });
  assert.equal(code, 0);
  const replies = output.trim().split('\n').map(JSON.parse);
  assert.equal(replies[0].error.code, -32600);
  assert.equal(replies[1].result.tools[0].name, 'imagegen');
});

test('reasoning summaries and tool arguments stream before completion without duplicate blocks', () => {
  const stream = new StreamConverter('opus');
  const frames = [];
  frames.push(...stream.processEvent('response.reasoning_summary_text.delta', { item_id: 'reason-1', delta: 'Checking the fixture' }));
  assert.match(frames.join(''), /thinking_delta/);
  const reasoning = { id: 'reason-1', type: 'reasoning', encrypted_content: 'opaque', summary: [{ text: 'Checking the fixture' }] };
  frames.push(...stream.processEvent('response.output_item.done', { item: reasoning }));
  frames.push(...stream.processEvent('response.output_item.added', { item: { ...call, id: 'tool-1', arguments: '' } }));
  const partial = stream.processEvent('response.function_call_arguments.delta', { item_id: 'tool-1', delta: '{"path":' });
  assert.match(partial.join(''), /input_json_delta/);
  frames.push(...partial, ...stream.processEvent('response.function_call_arguments.delta', { item_id: 'tool-1', delta: '"fixture.txt"}' }));
  frames.push(...stream.processEvent('response.output_item.done', { item: { ...call, id: 'tool-1' } }));
  frames.push(...stream.processEvent('response.completed', complete));
  const events = frames.map(frame => JSON.parse(frame.slice(frame.indexOf('data: ') + 6)));
  const starts = events.filter(event => event.type === 'content_block_start');
  assert.deepEqual(starts.map(event => event.content_block.type), ['thinking', 'tool_use']);
  assert.equal(new Set(starts.map(event => event.index)).size, 2);
  assert.equal(events.filter(event => event.delta?.thinking).map(event => event.delta.thinking).join(''), 'Checking the fixture');
  assert.equal(events.filter(event => event.delta?.partial_json).map(event => event.delta.partial_json).join(''), call.arguments);
  assert.equal(events.filter(event => event.type === 'content_block_stop').length, 2);
  assert.ok(events.some(event => event.delta?.signature?.startsWith('claudex-openai:')));
  for (const type of ['response.completed', 'response.incomplete']) {
    const truncated = new StreamConverter('opus');
    truncated.processEvent('response.output_item.added', { item: { ...call, id: 'pending' } });
    assert.throws(() => truncated.processEvent(type, complete), /without a completed item/);
    const pendingReasoning = new StreamConverter('opus');
    pendingReasoning.processEvent('response.reasoning_summary_text.delta', { item_id: 'pending', delta: 'Summary' });
    assert.throws(() => pendingReasoning.processEvent(type, complete), /without a completed item/);
  }
  const unsigned = new StreamConverter('opus');
  unsigned.processEvent('response.reasoning_summary_text.delta', { item_id: 'unsigned', delta: 'Summary' });
  assert.throws(() => unsigned.processEvent('response.output_item.done', { item: { type: 'reasoning', id: 'unsigned' } }), /without its signature/);
});

test('SSE decoder handles split UTF-8 and rejects incomplete frames and fake images', async () => {
  const bytes = Buffer.from('data: {"text":"été"}\r\n\r\n');
  const stream = new ReadableStream({ start(controller) { for (const byte of bytes) controller.enqueue(Uint8Array.of(byte)); controller.close(); } });
  const values = []; for await (const event of sseEvents(stream)) values.push(event);
  assert.deepEqual(values, [{ text: 'été' }]);
  await assert.rejects(async () => { for await (const unused of sseEvents(new Response('data: {').body)) void unused; });
  await assert.rejects(async () => { for await (const unused of sseEvents(new Response(': ping\ndata: {').body)) void unused; });
  assert.throws(() => imageMime(Buffer.from('secret pretending to be an image')));
});

test('real HTTP gateway emits tools once, rejects invalid tools/truncated streams, authenticates and cancels', async () => {
  let events = [{ type: 'response.created' }, { type: 'response.output_item.done', item: call }, complete];
  let cancellations = 0, requestBodies = [], clientStream = true, requestedTools = [tool];
  const server = await startGateway({ token, settings, credentials: { openai: async () => ({}) }, fetchImpl: async (url, options) => {
    requestBodies.push(JSON.parse(options.body));
    options.signal.addEventListener('abort', () => cancellations++);
    if (events === 'wait') return new Response(new ReadableStream({ start(controller) { controller.enqueue(Buffer.from(sse([{ type: 'response.created' }]))); options.signal.addEventListener('abort', () => controller.error(new Error('cancelled'))); } }));
    return new Response(sse(events));
  } });
  const base = `http://127.0.0.1:${server.address().port}`;
  const send = () => fetch(base + '/v1/messages', { method: 'POST', headers: { Authorization: `Bearer ${token}` }, body: JSON.stringify({ model: 'opus', messages: [{ role: 'user', content: 'fixture' }], tools: requestedTools, stream: clientStream }) });
  try {
    assert.equal((await fetch(base + '/health')).status, 401);
    let output = await (await send()).text();
    assert.equal((output.match(/event: message_stop/g) || []).length, 1);
    assert.match(output, /"input":\{\}/); assert.match(output, /input_json_delta/); assert.match(output, /tool_use/);
    events = [{ type: 'response.output_item.done', item: { ...call, arguments: '{bad' } }, complete];
    output = await (await send()).text(); assert.match(output, /event: error/); assert.doesNotMatch(output, /message_stop/);
    events = [{ type: 'response.created' }];
    output = await (await send()).text(); assert.match(output, /event: error/); assert.doesNotMatch(output, /message_stop/);
    events = [complete, { type: 'error' }];
    output = await (await send()).text(); assert.match(output, /event: error/); assert.doesNotMatch(output, /message_stop/);
    events = 'wait'; const response = await send(); await response.body.cancel();
    await new Promise(resolve => setTimeout(resolve, 50)); assert.ok(cancellations > 0);
    assert.equal(requestBodies[0].model, 'test-model');
    const searchCall = { type: 'web_search_call', id: 'search-1', action: { query: 'fixture' } };
    const searchMessage = { type: 'message', id: 'message-1', content: [{ type: 'output_text', text: 'verified', annotations: [{ type: 'url_citation', url: 'https://nodejs.org/api/fs.html', title: 'Node.js' }] }] };
    events = [searchCall, searchMessage].map(item => ({ type: 'response.output_item.done', item }));
    events.push({ ...complete, response: { ...complete.response, output: [] } });
    clientStream = false;
    const restored = await (await send()).json();
    assert.equal(restored.content[0].type, 'server_tool_use');
    assert.equal(restored.content[1].content[0].url, 'https://nodejs.org/api/fs.html');
    assert.equal(restored.content[2].text, 'verified');
    requestedTools = [{ type: 'web_search_20250305', name: 'web_search', max_uses: 1 }];
    events = [searchCall, { ...searchCall, id: 'search-2' }].map(item => ({ type: 'response.output_item.added', item }));
    const limited = await send(); assert.equal(limited.status, 502);
    assert.match(await limited.text(), /exceeded max_uses/);
  } finally { server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); }
});

test('quota fallback uses the Coding Plan only and fails visibly when unavailable', async () => {
  let attempts = [];
  const server = await startGateway({ token, settings: { ...settings, zaiFallback: 'glm-5.3' }, credentials: { openai: async () => ({}), zai: async () => ({}) }, fetchImpl: async (url, options) => {
    attempts.push({ url, model: JSON.parse(options.body).model });
    return url.includes('chatgpt.com') ? new Response('', { status: 429 }) : Response.json({ type: 'message', content: [{ type: 'text', text: 'fallback' }], model: 'glm-5.3' });
  } });
  try {
    const response = await fetch(`http://127.0.0.1:${server.address().port}/v1/messages`, { method: 'POST', headers: { Authorization: `Bearer ${token}` }, body: JSON.stringify({ model: 'sonnet', messages: [{ role: 'user', content: 'fixture' }] }) });
    assert.equal((await response.json()).content[0].text, 'fallback');
    assert.ok(attempts[1].url.startsWith('https://api.z.ai/api/anthropic/'));
    assert.equal(attempts[1].model, 'glm-5.3');
    await (await fetch(`http://127.0.0.1:${server.address().port}/v1/messages`, { method: 'POST', headers: { Authorization: `Bearer ${token}` }, body: JSON.stringify({ model: 'sonnet', messages: [{ role: 'user', content: 'fixture' }] }) })).json();
    assert.deepEqual(attempts.map(x => x.model), ['test-model', 'glm-5.3', 'glm-5.3']);
  } finally { server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); }
});

test('routing preserves capability gates, explicit selections and observed Coding Plan fallback', async () => {
  const attempts = [];
  const config = { ...settings, routes: { ...settings.routes, haiku: { provider: 'zai', model: 'glm-fast' } }, zaiFallback: 'glm-main' };
  const server = await startGateway({ token, settings: config, credentials: { openai: async () => ({}), zai: async () => ({}) }, fetchImpl: async (url, options) => {
    const body = JSON.parse(options.body); attempts.push(body);
    return url.includes('z.ai') ? new Response('', { status: 429, headers: { 'retry-after': new Date(Date.now() + 60000).toUTCString() } }) : new Response(sse([{ ...complete, response: { ...complete.response, output: [{ type: 'message', content: [{ type: 'output_text', text: 'fixture' }] }] } }]));
  } });
  const send = (model, extra = {}, requestClass) => fetch(`http://127.0.0.1:${server.address().port}/v1/messages`, { method: 'POST', headers: { Authorization: `Bearer ${token}`, ...(requestClass ? { 'x-claude-code-request-class': requestClass } : {}) }, body: JSON.stringify({ model, messages: [{ role: 'user', content: 'fixture' }], ...extra }) });
  try {
    assert.equal((await send('haiku')).status, 200);
    assert.deepEqual(attempts.slice(-2).map(x => x.model), ['glm-fast', 'test-model']);
    assert.equal((await send('haiku')).status, 200);
    assert.equal(attempts.at(-1).model, 'test-model');
    const before = attempts.length;
    assert.equal((await send('zai/glm-explicit', { tools: [{ name: 'web_search', type: 'web_search_20250305' }] })).status, 400);
    assert.equal(attempts.length, before);
    assert.equal((await send('opus', {}, 'auxiliary')).status, 200);
    assert.equal(attempts.at(-1).reasoning.effort, 'low');
    await send('opus', { output_config: { effort: 'high' } }, 'auxiliary');
    assert.equal(attempts.at(-1).reasoning.effort, 'high');
    assert.equal((await send('zai/glm-explicit')).status, 429);
    assert.equal(attempts.at(-1).model, 'glm-explicit');
    const previous = attempts.length;
    const cooling = await send('zai/glm-explicit');
    assert.equal(cooling.status, 503); assert.equal(attempts.length, previous);
    assert.ok(Number(cooling.headers.get('retry-after')) > 30);
    await send('openai/test-model@high', {}, 'auxiliary');
    assert.equal(attempts.at(-1).reasoning.effort, 'high');
    const health = await (await fetch(`http://127.0.0.1:${server.address().port}/health`, { headers: { Authorization: `Bearer ${token}` } })).json();
    assert.ok(health.receipts.some(x => x.routingReason === 'cooldown-fallback'));
    assert.ok(health.receipts.some(x => x.usage?.output_tokens === 3));
  } finally { server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); }
});

test('exhausted subscriptions do not loop, and inverse fallback renews OpenAI OAuth once', async () => {
  let count = 0, renewals = 0, exhausted = true;
  const server = await startGateway({ token, settings: { ...settings, routes: { ...settings.routes, haiku: { provider: 'zai', model: 'glm-fast' } }, zaiFallback: 'glm-main' }, renew: async () => renewals++, credentials: { openai: async () => ({}), zai: async () => ({}) }, fetchImpl: async url => {
    count++;
    if (url.includes('z.ai') || exhausted) return new Response('', { status: 429 });
    if (!renewals) return new Response('', { status: 401 });
    return new Response(sse([{ ...complete, response: { ...complete.response, output: [] } }]));
  } });
  const send = model => fetch(`http://127.0.0.1:${server.address().port}/v1/messages`, { method: 'POST', headers: { Authorization: `Bearer ${token}` }, body: JSON.stringify({ model, messages: [{ role: 'user', content: 'fixture' }] }) });
  try {
    assert.equal((await send('sonnet')).status, 429);
    const previous = count;
    assert.equal((await send('sonnet')).status, 503); assert.equal(count, previous);
    // Fresh gateway for the inverse fallback: the first fixture's cooldown is deliberate.
  } finally { server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); }
  exhausted = false;
  const second = await startGateway({ token, settings: { ...settings, routes: { ...settings.routes, haiku: { provider: 'zai', model: 'glm-other' } } }, renew: async () => renewals++, credentials: { openai: async () => ({}), zai: async () => ({}) }, fetchImpl: async url => {
    if (url.includes('z.ai')) return new Response('', { status: 429 });
    if (!renewals) return new Response('', { status: 401 });
    return new Response(sse([{ ...complete, response: { ...complete.response, output: [] } }]));
  } });
  try {
    const r = await fetch(`http://127.0.0.1:${second.address().port}/v1/messages`, { method: 'POST', headers: { Authorization: `Bearer ${token}` }, body: JSON.stringify({ model: 'haiku', messages: [] }) });
    assert.equal(r.status, 200); assert.equal(renewals, 1);
  } finally { second.closeAllConnections(); await new Promise(resolve => second.close(resolve)); }
});
