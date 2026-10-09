import { createInterface } from 'node:readline';
import { generateImage } from './images.mjs';
const send = value => process.stdout.write(JSON.stringify(value) + '\n');
const active = new Map();
const tool = {
  name: 'imagegen', description: 'Generate or edit images with the current ChatGPT subscription. Returns an image and its saved local file. For edits, inspect reference images first. No paid API fallback.',
  inputSchema: { type: 'object', properties: {
    prompt: { type: 'string' }, referenced_image_paths: { type: 'array', items: { type: 'string' }, maxItems: 5 },
    transparent_background: { type: 'boolean' }, output_dir: { type: 'string', description: 'Absolute output directory; defaults to current working directory' },
    quality: { type: 'string', enum: ['auto', 'low', 'medium', 'high'] }, size: { type: 'string', enum: ['auto', '1024x1024', '1536x1024', '1024x1536'] },
  }, required: ['prompt'], additionalProperties: false },
};
createInterface({ input: process.stdin }).on('line', async line => {
  let request;
  try { if (line.length > 1024 * 1024) throw new Error(); request = JSON.parse(line); }
  catch { return send({ jsonrpc: '2.0', id: null, error: { code: -32700, message: 'Invalid JSON RPC message' } }); }
  if (!request || typeof request !== 'object' || Array.isArray(request) || request.jsonrpc !== '2.0' || typeof request.method !== 'string') {
    return send({ jsonrpc: '2.0', id: null, error: { code: -32600, message: 'Invalid JSON RPC request' } });
  }
  if (request.method === 'notifications/cancelled') { active.get(request.params?.requestId)?.abort(); return; }
  if (request.id === undefined) return;
  const respond = result => send({ jsonrpc: '2.0', id: request.id, result });
  if (request.method === 'initialize') return respond({ protocolVersion: request.params?.protocolVersion || '2024-11-05', capabilities: { tools: {} }, serverInfo: { name: 'claudex-images', version: '0.1.0' } });
  if (request.method === 'ping') return respond({});
  if (request.method === 'tools/list') return respond({ tools: [tool] });
  if (request.method !== 'tools/call' || request.params?.name !== tool.name) return send({ jsonrpc: '2.0', id: request.id, error: { code: -32601, message: 'Unknown method or tool' } });
  const abort = new AbortController(); active.set(request.id, abort);
  try {
    const result = await generateImage(request.params.arguments || {}, abort.signal);
    respond({ content: [{ type: 'text', text: `Image saved: ${result.path}` }, { type: 'image', mimeType: result.mime, data: result.bytes.toString('base64') }] });
  } catch {
    respond({ isError: true, content: [{ type: 'text', text: 'ChatGPT image generation or editing failed. No paid API fallback was used.' }] });
  } finally { active.delete(request.id); }
});
process.stdin.on('end', () => { for (const abort of active.values()) abort.abort(); });
