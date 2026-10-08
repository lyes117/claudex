import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { mkdtempSync, mkdirSync, copyFileSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { boundedResponseText, queryMemory } from './claude-memory-query.mjs';
import { resolveProjectIdentity } from './claude-memory-project-identity.mjs';

test('search and context return real HTTP contents scoped to the current checkout', async t => {
  const root = mkdtempSync(join(tmpdir(), 'claudex-query-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const pluginRoot = join(root, 'plugin');
  mkdirSync(join(pluginRoot, 'scripts'), { recursive: true });
  copyFileSync(new URL('./claude-memory-project-identity.mjs', import.meta.url), join(pluginRoot, 'scripts', 'claude-memory-project-identity.mjs'));
  const cwd = join(root, 'project'); mkdirSync(cwd);
  writeFileSync(join(cwd, 'package.json'), '{}');
  const identity = resolveProjectIdentity(cwd);
  const requests = [];
  const server = createServer((request, response) => {
    const url = new URL(request.url, 'http://fixture');
    requests.push(url);
    if (url.pathname === '/api/search') response.end(JSON.stringify({ content: [{ type: 'text', text: 'SYNTHETIC_SEARCH_CANARY' }] }));
    else response.end('SYNTHETIC_CONTEXT_CANARY 研究');
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(() => new Promise(resolve => server.close(resolve)));
  const receipt = { pluginRoot, port: server.address().port };
  const deps = { probe: async () => ({ initialized: true }) };
  assert.deepEqual(await queryMemory(receipt, { command: 'search', query: 'fixture literal', cwd }, deps), {
    content: [{ type: 'text', text: 'SYNTHETIC_SEARCH_CANARY' }], project: identity.primary,
  });
  assert.deepEqual(await queryMemory(receipt, { command: 'context', cwd }, deps), {
    project: identity.primary, additionalContext: 'SYNTHETIC_CONTEXT_CANARY 研究',
  });
  for (const url of requests) {
    assert.equal(url.searchParams.get('project'), identity.primary);
    assert.equal(url.searchParams.get('projects'), identity.allProjects.join(','));
    assert.equal(url.searchParams.get('platformSource'), 'codex');
  }
  assert.equal(requests[0].searchParams.get('query'), 'fixture literal');
  assert.equal(requests[1].searchParams.get('platform'), 'codex');
});

test('query errors never become success or unscoped retrieval', async () => {
  const receipt = { port: 12345 };
  const probe = async () => ({ initialized: true });
  let requests = 0;
  const request = async () => { requests += 1; return new Response('{}'); };
  for (const project of ['', 'a,b', 'X'.repeat(257)]) {
    await assert.rejects(queryMemory(receipt, { command: 'search', project, query: 'fixture' }, { probe, request }), /Invalid explicit/);
  }
  assert.equal(requests, 0);
  await assert.rejects(queryMemory(receipt, { command: 'search', project: 'fixture', query: 'fixture' }, {
    probe: async () => null, request,
  }), /offline/);
  assert.equal(requests, 0);
  for (const response of [new Response('not-json'), new Response('{"content":[],"isError":true}'), new Response('failure', { status: 500 })]) {
    await assert.rejects(queryMemory(receipt, { command: 'search', project: 'fixture', query: 'fixture' }, {
      probe, request: async () => response,
    }));
  }
});

test('response streaming enforces byte budget before JSON parsing', async () => {
  const bytes = Buffer.from('研究'.repeat(40000));
  assert.equal(await boundedResponseText(new Response(bytes)), bytes.toString('utf8'));
  let cancelled = false;
  const stream = new ReadableStream({
    start(controller) {
      controller.enqueue(new Uint8Array(262144));
      controller.enqueue(new Uint8Array(1));
    },
    cancel() { cancelled = true; },
  });
  await assert.rejects(boundedResponseText(new Response(stream)), /exceeds/);
  assert.equal(cancelled, true);
});
