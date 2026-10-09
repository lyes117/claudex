import { readFile, stat, mkdir, writeFile } from 'node:fs/promises';
import { randomUUID } from 'node:crypto';
import { resolve, join } from 'node:path';
import { codexBase, codexHeaders } from './providers.mjs';

export function imageMime(bytes) {
  if (bytes.subarray(0, 8).equals(Buffer.from([137,80,78,71,13,10,26,10]))) return 'image/png';
  if (bytes[0] === 255 && bytes[1] === 216 && bytes[2] === 255) return 'image/jpeg';
  if (bytes.subarray(0, 4).toString() === 'RIFF' && bytes.subarray(8, 12).toString() === 'WEBP') return 'image/webp';
  throw new Error('Only genuine PNG, JPEG or WebP images are accepted');
}

export async function generateImage({ prompt, referenced_image_paths = [], transparent_background = false, output_dir = process.cwd(), quality = 'auto', size = 'auto' }, signal, fetchImpl = fetch) {
  if (typeof prompt !== 'string' || !prompt.trim() || prompt.length > 32000) throw new Error('A nonempty prompt of at most 32000 characters is required');
  if (!Array.isArray(referenced_image_paths) || referenced_image_paths.length > 5 || referenced_image_paths.some(path => typeof path !== 'string')) throw new Error('At most five image paths are accepted');
  if (!['auto', 'low', 'medium', 'high'].includes(quality) || !['auto', '1024x1024', '1536x1024', '1024x1536'].includes(size) || typeof transparent_background !== 'boolean' || typeof output_dir !== 'string') throw new Error('Invalid image options');
  const images = [];
  for (const path of referenced_image_paths) {
    if ((await stat(path)).size > 32 * 1024 * 1024) throw new Error('Reference image exceeds 32 MiB');
    const bytes = await readFile(path); const mime = imageMime(bytes);
    images.push({ image_url: `data:${mime};base64,${bytes.toString('base64')}` });
  }
  const body = { model: 'gpt-image-2', prompt, background: transparent_background ? 'transparent' : 'opaque', quality, size, ...(images.length ? { images } : {}) };
  const response = await fetchImpl(`${codexBase}/images/${images.length ? 'edits' : 'generations'}`, { method: 'POST', headers: { ...await codexHeaders(), 'x-codex-image-turn-id': randomUUID() }, body: JSON.stringify(body), signal: AbortSignal.any([signal || new AbortController().signal, AbortSignal.timeout(10 * 60 * 1000)]), redirect: 'error' });
  if (!response.ok) { await response.body?.cancel(); throw new Error(`ChatGPT image generation refused (HTTP ${response.status}); no paid fallback.`); }
  const chunks = []; let length = 0;
  for await (const chunk of response.body) { length += chunk.length; if (length > 64 * 1024 * 1024) throw new Error('Image response exceeds 64 MiB'); chunks.push(chunk); }
  const data = JSON.parse(Buffer.concat(chunks).toString('utf8'));
  if (!data.data?.[0]?.b64_json) throw new Error('Image response did not contain an image');
  const bytes = Buffer.from(data.data[0].b64_json, 'base64'); const mime = imageMime(bytes);
  const directory = resolve(output_dir); await mkdir(directory, { recursive: true });
  const path = join(directory, `claudex-${randomUUID()}.${mime.split('/')[1]}`);
  await writeFile(path, bytes, { flag: 'wx' });
  return { path, mime, bytes };
}
