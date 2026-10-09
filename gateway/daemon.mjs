import { spawn } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { mkdir, open } from 'node:fs/promises';
import { setTimeout as delay } from 'node:timers/promises';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { stateHome, readJson } from './settings.mjs';
export const runtimeFile = join(stateHome, 'runtime.json');
export const launchSettingsFile = join(stateHome, 'claude-settings.json');
export const mcpConfigFile = join(stateHome, 'mcp.json');

export async function gatewayHealth(runtime) {
  if (!runtime || !Number.isInteger(runtime.port) || runtime.port < 1 || runtime.port > 65535 || typeof runtime.token !== 'string' || runtime.token.length !== 64) return null;
  try {
    const response = await fetch(`http://127.0.0.1:${runtime.port}/health`, { headers: { Authorization: `Bearer ${runtime.token}` }, signal: AbortSignal.timeout(1500), redirect: 'error' });
    if (!response.ok) return null;
    const health = await response.json(); return health.gateway === 'claudex-local' ? health : null;
  } catch { return null; }
}

export async function ensureGateway() {
  await mkdir(stateHome, { recursive: true });
  let runtime;
  for (let attempt = 0; attempt < 20; attempt++) {
    try { runtime = await readJson(runtimeFile, null); break; }
    catch (error) { if (attempt === 19) throw error; await delay(50); }
  }
  if (!runtime) {
    const initial = { port: 18082, token: randomBytes(32).toString('hex') };
    try {
      const file = await open(runtimeFile, 'wx', 0o600);
      try { await file.writeFile(JSON.stringify(initial)); } finally { await file.close(); }
      runtime = initial;
    } catch (error) {
      if (error.code !== 'EEXIST') throw error;
      // Another launcher may still be writing the first, very small credential file.
      for (let attempt = 0; attempt < 20 && !runtime; attempt++) {
        await delay(50); try { runtime = await readJson(runtimeFile, null); } catch { /* Finish creation first. */ }
      }
    }
  }
  if (!runtime || !Number.isInteger(runtime.port) || runtime.port < 1 || runtime.port > 65535 || typeof runtime.token !== 'string' || !/^[a-f0-9]{64}$/.test(runtime.token)) throw new Error('Invalid gateway runtime configuration');
  if (await gatewayHealth(runtime)) return runtime;
  // Let the OS arbitrate one listener. No stale filesystem lock after a crash.
  const child = spawn(process.execPath, [join(dirname(fileURLToPath(import.meta.url)), 'server.mjs')], { detached: true, windowsHide: true, stdio: 'ignore', env: { ...process.env, CLAUDEX_GATEWAY_CHILD: '1', CLAUDEX_GATEWAY_TOKEN: runtime.token, CLAUDEX_GATEWAY_PORT: String(runtime.port) } });
  child.on('error', () => {}); child.unref();
  for (let attempt = 0; attempt < 75; attempt++) { await delay(200); if (await gatewayHealth(runtime)) return runtime; }
  child.kill(); throw new Error('Gateway startup failed; its saved local port may be occupied.');
}

export async function stopGateway() {
  const runtime = await readJson(runtimeFile, null);
  if (await gatewayHealth(runtime)) await fetch(`http://127.0.0.1:${runtime.port}/admin/stop`, { method: 'POST', headers: { Authorization: `Bearer ${runtime.token}` }, signal: AbortSignal.timeout(5000), redirect: 'error' });
}
