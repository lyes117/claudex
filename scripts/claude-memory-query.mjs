import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { probeOwnedWorker } from './claude-memory-runtime.mjs';

const MAX_RESPONSE_BYTES = 256 * 1024;

export async function boundedResponseText(response) {
  if (!response.ok || !response.body) throw new Error('Owned memory worker request failed');
  const reader = response.body.getReader();
  const chunks = [];
  let bytes = 0;
  try {
    while (true) {
      const chunk = await reader.read();
      if (chunk.done) break;
      bytes += chunk.value.byteLength;
      if (bytes > MAX_RESPONSE_BYTES) throw new Error('Memory response exceeds 262144 bytes');
      chunks.push(Buffer.from(chunk.value));
    }
    return Buffer.concat(chunks).toString('utf8');
  } finally {
    await reader.cancel().catch(() => {});
    reader.releaseLock();
  }
}

export async function queryMemory(receipt, { command, project, query, cwd = process.cwd() }, {
  probe = probeOwnedWorker, request = fetch,
} = {}) {
  if (!await probe(receipt)) throw new Error('Owned memory worker is offline');
  let context;
  if (project !== undefined) {
    if (typeof project !== 'string' || !project.trim() || project.includes(',') || Buffer.byteLength(project) > 256) {
      throw new Error('Invalid explicit memory project');
    }
    context = { primary: project, allProjects: [project] };
  } else {
    const resolver = await import(pathToFileURL(join(receipt.pluginRoot, 'scripts', 'claude-memory-project-identity.mjs')).href);
    context = resolver.resolveProjectIdentity(cwd);
  }
  const params = new URLSearchParams({ project: context.primary, projects: context.allProjects.join(','), platformSource: 'codex' });
  let endpoint;
  if (command === 'search') {
    if (typeof query !== 'string' || !query.trim() || Buffer.byteLength(query) > 4096) throw new Error('Invalid memory query');
    params.set('query', query); params.set('limit', '20');
    endpoint = '/api/search';
  } else if (command === 'context') {
    params.set('platform', 'codex');
    endpoint = '/api/context/inject';
  } else throw new Error('Unsupported memory query command');
  const response = await request(`http://127.0.0.1:${receipt.port}${endpoint}?${params}`, {
    signal: AbortSignal.timeout(5000), redirect: 'error',
  });
  const text = await boundedResponseText(response);
  if (command === 'context') return { project: context.primary, additionalContext: text };
  let result;
  try { result = JSON.parse(text); } catch { throw new Error('Memory search returned invalid JSON'); }
  if (!result || typeof result !== 'object' || !Array.isArray(result.content) || result.isError === true) {
    throw new Error('Memory search returned an invalid result');
  }
  return { ...result, project: context.primary };
}
