import { createServer } from 'node:http';
import { timingSafeEqual } from 'node:crypto';
import { aliases, loadSettings, resolveRoute } from './settings.mjs';
import { codexBase, zaiBase, codexHeaders, zaiHeaders, refreshCodex } from './providers.mjs';
import { translateRequest, translateUsage, StreamConverter, codexToAnthropic, estimateRequestTokens, fixStreamFrame } from './translation.mjs';
import { sseEvents, writeEvent, validateToolItem } from './stream.mjs';

const errorBody = message => ({ type: 'error', error: { type: 'api_error', message } });
const json = (res, status, body) => { res.writeHead(status, { 'Content-Type': 'application/json' }); res.end(JSON.stringify(body)); };
const hasImage = messages => messages.some(message => Array.isArray(message.content) && message.content.some(block => block.type === 'image' || block.type === 'tool_result' && Array.isArray(block.content) && block.content.some(part => part.type === 'image')));
const contextError = error => /^(context_length_exceeded|prompt_too_long|input_too_long)$/.test(error?.code || '') || /^(prompt is too long|your input exceeds the context window|maximum context length)/i.test(error?.message || '')
  ? Object.assign(new Error('Prompt is too long: the selected provider context window was exceeded.'), { status: 400 }) : null;

export async function startGateway({ token, settings, port = 0, fetchImpl = fetch, credentials = { openai: codexHeaders, zai: zaiHeaders }, renew = refreshCodex } = {}) {
  settings ??= await loadSettings();
  if (!token || token.length < 32) throw new Error('A strong local gateway credential is required');
  const receipts = [];
  const cooldowns = new Map();
  const routeKey = route => `${route.provider}/${route.model}`;
  const ready = route => (cooldowns.get(routeKey(route)) || 0) <= Date.now();
  const observe = (route, response) => {
    if (response.ok) cooldowns.delete(routeKey(route));
    else if ([429, 503].includes(response.status)) {
      const retry = response.headers.get('retry-after');
      const seconds = retry && /^\d+$/.test(retry) ? Number(retry) : retry && Number.isFinite(Date.parse(retry)) ? (Date.parse(retry) - Date.now()) / 1000 : 30;
      cooldowns.set(routeKey(route), Date.now() + Math.min(900, Math.max(5, seconds)) * 1000);
    }
  };
  // ponytail: observed availability, not a learned estimate of answer quality.
  const server = createServer(async (req, res) => {
    const supplied = (req.headers.authorization || '').replace(/^Bearer /i, '') || req.headers['x-api-key'] || '';
    const expected = Buffer.from(token), actual = Buffer.from(supplied);
    if (expected.length !== actual.length || !timingSafeEqual(expected, actual)) return json(res, 401, errorBody('Local gateway authentication required'));
    let url; try { url = new URL(req.url, 'http://127.0.0.1'); } catch { return json(res, 400, errorBody('Malformed request URL')); }
    if (url.pathname === '/health') return json(res, 200, { gateway: 'claudex-local', receipts });
    if (req.method === 'POST' && url.pathname === '/admin/stop') {
      json(res, 200, { stopped: true }); server.close(); server.closeAllConnections(); return;
    }
    if (req.method === 'HEAD' && url.pathname === '/api/hello') { res.writeHead(200); return res.end(); }
    if (req.method === 'GET' && url.pathname === '/v1/models') {
      const models = [...Object.entries(aliases).map(([tier, id]) => ({ id, display_name: `${tier}: ${settings.routes[tier].model}` })), ...settings.catalog.map(model => ({ id: `openai/${model.slug}`, display_name: model.slug })), ...['GLM-5.3', 'GLM-5.3-Flash'].map(model => ({ id: `zai/${model}`, display_name: model }))];
      return json(res, 200, { data: models.map(model => ({ ...model, type: 'model', created_at: '2026-10-07T00:00:00Z' })), has_more: false });
    }
    if (req.method !== 'POST' || !['/v1/messages', '/v1/messages/count_tokens'].includes(url.pathname)) return json(res, 404, errorBody('Unknown gateway endpoint'));
    const abort = new AbortController();
    const timeout = setTimeout(() => abort.abort(), 10 * 60 * 1000); timeout.unref();
    res.once('close', () => { if (!res.writableEnded) abort.abort(); });
    let ping, route, usage, firstEventMilliseconds, routingReason = 'role', outcome = 'error', hostedSearchCalls = 0;
    const requestClass = ['main', 'subagent', 'workflow', 'compaction', 'auxiliary'].includes(req.headers['x-claude-code-request-class']) ? req.headers['x-claude-code-request-class'] : 'unknown';
    const started = Date.now();
    try {
      let size = 0, chunks = [];
      for await (const chunk of req) {
        size += chunk.length;
        if (size > 32 * 1024 * 1024) throw Object.assign(new Error('Request too large'), { status: 413 });
        chunks.push(chunk);
      }
      let request; try { request = JSON.parse(Buffer.concat(chunks).toString('utf8')); } catch { throw Object.assign(new Error('Malformed request JSON'), { status: 400 }); }
      if (!Array.isArray(request.messages) || typeof request.model !== 'string') throw Object.assign(new Error('model and messages are required'), { status: 400 });
      if (url.pathname.endsWith('/count_tokens')) { outcome = 'estimated'; return json(res, 200, { input_tokens: estimateRequestTokens(request) }); }
      route = resolveRoute(request.model, settings);
      if (route.tier === 'explicit') routingReason = 'explicit';
      const vision = hasImage(request.messages);
      const hostedSearch = request.tools?.some(tool => /^web_search_/.test(tool.type || ''));
      if (route.provider === 'zai' && vision) {
        if (route.tier === 'explicit') throw Object.assign(new Error('The GLM Coding Plan route is text-only; select an OpenAI model for images.'), { status: 400 });
        route = { provider: 'openai', model: settings.openaiFast || settings.routes.opus.model, effort: 'low', tier: route.tier };
        routingReason = 'vision';
      }
      if (route.provider === 'zai' && hostedSearch) {
        if (route.tier === 'explicit') throw Object.assign(new Error('Hosted WebSearch requires an OpenAI model.'), { status: 400 });
        route = { provider: 'openai', model: settings.routes.opus.model, effort: 'medium', tier: route.tier }; routingReason = 'web-search';
      }
      const effort = request.output_config?.effort;
      if (route.provider === 'openai' && effort) route.effort = effort === 'max' ? 'xhigh' : ['none', 'minimal', 'low', 'medium', 'high', 'xhigh'].includes(effort) ? effort : route.effort;
      if (route.provider === 'openai' && route.tier !== 'explicit' && requestClass === 'auxiliary' && !effort && !vision && !hostedSearch) {
        route.effort = 'low'; routingReason = 'auxiliary-low-effort';
      }
      // Capability gates and explicit provider selections apply before any fallback.
      // ponytail: UTF-8 bytes plus output allowance is a conservative text budget,
      // not tokenization. Use a tokenizer if larger GLM fallbacks become necessary.
      const fitsZaiFallback = size + Math.max(0, request.max_tokens || 32000) <= 200000;
      const fallback = route.tier === 'explicit' || vision || hostedSearch || requestClass === 'compaction' ? null : route.provider === 'openai'
        ? settings.zaiFallback && fitsZaiFallback ? { provider: 'zai', model: settings.zaiFallback, tier: route.tier } : null
        : { provider: 'openai', model: settings.routes.opus.model, effort: route.tier === 'haiku' ? 'low' : 'medium', tier: route.tier };
      if (!ready(route)) {
        if (fallback && ready(fallback)) { route = fallback; routingReason = 'cooldown-fallback'; }
        else {
          res.setHeader('retry-after', String(Math.ceil((cooldowns.get(routeKey(route)) - Date.now()) / 1000)));
          throw Object.assign(new Error('Selected subscription routes are temporarily unavailable; retry after the cooldown.'), { status: 503 });
        }
      }
      const open = async (selected, renewed = false) => {
        const headers = await credentials[selected.provider]();
        if (selected.provider === 'zai' && req.headers['anthropic-beta']) headers['anthropic-beta'] = req.headers['anthropic-beta'];
        const body = selected.provider === 'openai' ? translateRequest(request, selected) : { ...request, model: selected.model, messages: request.messages.map(message => Array.isArray(message.content) ? { ...message, content: message.content.filter(block => !(block.type === 'thinking' && block.signature?.startsWith('claudex-openai:'))) } : message) };
        const response = await fetchImpl(selected.provider === 'openai' ? `${codexBase}/responses` : `${zaiBase}/v1/messages`, { method: 'POST', headers, body: JSON.stringify(body), signal: abort.signal, redirect: 'error' });
        if (response.status === 401 && selected.provider === 'openai' && !renewed) { await response.body?.cancel(); await renew(); return open(selected, true); }
        return response;
      };
      let upstream = await open(route);
      observe(route, upstream);
      if ([429, 503].includes(upstream.status) && fallback && routeKey(route) !== routeKey(fallback) && ready(fallback)) {
        await upstream.body?.cancel();
        route = fallback; routingReason = 'quota-fallback';
        upstream = await open(route);
        observe(route, upstream);
      }
      if (!upstream.ok) {
        const error = await upstream.json().catch(() => null);
        const tooLong = contextError(error?.error || error);
        if (tooLong) throw tooLong;
        throw Object.assign(new Error(`${route.provider} refused inference (HTTP ${upstream.status}); no paid API fallback.`), { status: upstream.status });
      }
      if (request.stream) {
        res.writeHead(200, { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-cache', Connection: 'keep-alive' });
        res.flushHeaders();
        ping = setInterval(() => { if (!res.destroyed && !res.writableEnded) res.write('event: ping\ndata: {"type":"ping"}\n\n'); }, 10000); ping.unref();
      }
      if (route.provider === 'zai') {
        if (request.stream) {
          let terminal;
          for await (const event of sseEvents(upstream.body)) {
            firstEventMilliseconds ??= Date.now() - started;
            if (event.message?.usage) usage = { ...event.message.usage };
            if (event.usage) usage = { ...usage, ...event.usage };
            if (event.type === 'error') throw contextError(event.error) || new Error('Z.ai inference stream failed');
            if (terminal) throw new Error('Unexpected event after message_stop');
            const frame = `event: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`;
            if (event.type === 'message_stop') terminal = frame; else await writeEvent(res, frame);
          }
          if (!terminal) throw new Error('Z.ai inference stream ended without message_stop');
          await writeEvent(res, terminal); res.end();
        } else {
          const result = await upstream.json();
          if (result.type !== 'message' || !Array.isArray(result.content)) throw new Error('Invalid Z.ai message response');
          result.model = request.model; json(res, 200, result);
          usage = result.usage;
        }
      } else {
        const stream = new StreamConverter(request.model);
        const completedItems = new Map();
        const searchIds = new Set();
        const searchLimit = request.tools?.find(tool => /^web_search_/.test(tool.type || ''))?.max_uses;
        let terminal, ending;
        for await (const event of sseEvents(upstream.body)) {
          firstEventMilliseconds ??= Date.now() - started;
          if (terminal) throw new Error('Unexpected event after response completion');
          if (['response.failed', 'error'].includes(event.type)) throw contextError(event.error || event.response?.error || event) || new Error('OpenAI inference stream failed');
          if (['response.output_item.added', 'response.output_item.done'].includes(event.type) && event.item?.type === 'web_search_call') {
            searchIds.add(event.item.id); hostedSearchCalls = searchIds.size;
            if (searchLimit && hostedSearchCalls > searchLimit) {
              abort.abort(); throw Object.assign(new Error('WebSearch exceeded max_uses; inference cancelled.'), { status: 502 });
            }
          }
          if (event.type === 'response.output_item.done') {
            validateToolItem(event.item, request.tools);
            completedItems.set(event.item.id || event.item.call_id || JSON.stringify(event.item), event.item);
          }
          if (['response.completed', 'response.incomplete'].includes(event.type)) {
            terminal = event.response;
            if (!terminal || !Array.isArray(terminal.output)) throw new Error('Invalid OpenAI completion');
            // The subscription backend can omit output from its final envelope.
            for (const item of terminal.output) completedItems.set(item.id || item.call_id || JSON.stringify(item), item);
            terminal.output = [...completedItems.values()];
            for (const item of terminal.output) validateToolItem(item, request.tools);
            ending = stream.processEvent(event.type, event);
          } else if (request.stream) {
            for (const frame of stream.processEvent(event.type, event)) await writeEvent(res, fixStreamFrame(frame));
          }
        }
        if (!terminal) throw new Error('OpenAI inference stream ended without completion');
        if (terminal.usage) usage = translateUsage(terminal.usage);
        if (request.stream) {
          for (const frame of ending) await writeEvent(res, fixStreamFrame(frame, terminal.usage));
          res.end();
        } else {
          const result = codexToAnthropic(terminal, request.model); result.usage = translateUsage(terminal.usage);
          json(res, 200, result);
        }
      }
      outcome = 'complete';
    } catch (error) {
      // Do not echo upstream errors, which can contain prompts or credentials.
      const message = error.status ? error.message : abort.signal.aborted ? 'Inference cancelled or timed out' : 'Inference protocol failed; no successful completion was emitted';
      if (!res.destroyed) {
        if (res.headersSent) { res.end(`event: error\ndata: ${JSON.stringify(errorBody(message))}\n\n`); }
        else json(res, error.status >= 400 && error.status < 600 ? error.status : 502, errorBody(message));
      }
    } finally {
      clearTimeout(timeout); clearInterval(ping);
      receipts.push({ provider: route?.provider || 'local', model: route?.model, effort: route?.effort, tier: route?.tier, requestClass, routingReason, outcome, hostedSearchCalls, firstEventMilliseconds, usage: usage ? Object.fromEntries(['input_tokens', 'cache_read_input_tokens', 'output_tokens'].filter(key => typeof usage[key] === 'number').map(key => [key, usage[key]])) : null, milliseconds: Date.now() - started, timestamp: new Date(started).toISOString() });
      if (receipts.length > 100) receipts.shift();
    }
  });
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(port, '127.0.0.1', resolve); });
  return server;
}

if (process.env.CLAUDEX_GATEWAY_CHILD === '1') {
  try {
    const server = await startGateway({ token: process.env.CLAUDEX_GATEWAY_TOKEN, port: Number(process.env.CLAUDEX_GATEWAY_PORT) });
    process.send?.({ port: server.address().port });
    process.on('message', message => { if (message === 'stop') { server.close(); server.closeAllConnections(); } });
  } catch { process.send?.({ error: 'Gateway startup failed; run claudex doctor.' }); process.exitCode = 1; }
}
