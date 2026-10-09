import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { homedir, platform, userInfo } from 'node:os';
import { createHash, createDecipheriv } from 'node:crypto';
import { join } from 'node:path';
import { codexHome, readJson } from './settings.mjs';

export const codexBase = 'https://chatgpt.com/backend-api/codex';
export const zaiBase = 'https://api.z.ai/api/anthropic';
export const codexExe = join(homedir(), 'AppData', 'Local', 'Programs', 'OpenAI', 'Codex', 'bin', 'codex.exe');
let refreshing;

// Keep OAuth renewal and storage owned by the installed Codex; never copy its tokens.
export function refreshCodex() {
  refreshing ??= new Promise((resolve, reject) => {
    const child = spawn(codexExe, ['app-server', '--listen', 'stdio://'], { windowsHide: true, stdio: ['pipe', 'pipe', 'ignore'] });
    const lines = createInterface({ input: child.stdout });
    const timer = setTimeout(() => finish(new Error('Codex OAuth renewal timed out')), 30000);
    const send = value => child.stdin.write(JSON.stringify(value) + '\n');
    let settled = false;
    function finish(error) {
      if (settled) return; settled = true; clearTimeout(timer); lines.close(); child.stdin.end();
      const cleanup = setTimeout(() => child.kill(), 3000); cleanup.unref();
      child.once('exit', () => clearTimeout(cleanup));
      error ? reject(error) : resolve();
    }
    child.on('error', () => finish(new Error('Installed Codex could not be started')));
    child.once('exit', () => { if (!settled) finish(new Error('Codex ended before OAuth renewal')); });
    lines.on('line', line => {
      let value; try { value = JSON.parse(line); } catch { return; }
      if (value.id === 1) {
        if (value.error) return finish(new Error('Codex initialization refused'));
        send({ method: 'initialized' });
        send({ method: 'account/read', id: 2, params: { refreshToken: true } });
      }
      if (value.id === 2) finish(value.error ? new Error('Codex OAuth renewal refused') : undefined);
    });
    send({ method: 'initialize', id: 1, params: { clientInfo: { name: 'claudex_gateway', version: '0.1.0' } } });
  }).finally(() => { refreshing = undefined; });
  return refreshing;
}

export async function codexHeaders() {
  let auth = await readJson(join(codexHome, 'auth.json'), {});
  if (!auth.tokens?.access_token) throw Object.assign(new Error('ChatGPT OAuth unavailable. Use codex login; API keys are never used.'), { status: 401 });
  let expires = 0;
  try { expires = JSON.parse(Buffer.from(auth.tokens.access_token.split('.')[1], 'base64url')).exp || 0; } catch { /* Opaque token: let the provider validate it. */ }
  if (expires && expires * 1000 < Date.now() + 60000) { await refreshCodex(); auth = await readJson(join(codexHome, 'auth.json'), {}); }
  if (!auth.tokens?.access_token) throw Object.assign(new Error('Codex OAuth renewal did not produce a session'), { status: 401 });
  return { 'Content-Type': 'application/json', Authorization: `Bearer ${auth.tokens.access_token}`, originator: 'codex_cli_rs', 'User-Agent': 'claudex-gateway/0.1.0', ...(auth.tokens.account_id ? { 'ChatGPT-Account-Id': auth.tokens.account_id } : {}) };
}

export async function zaiHeaders() {
  let key = process.env.CLAUDEX_ZAI_TOKEN;
  if (!key) {
    const config = await readJson(join(homedir(), '.claude', 'settings.json'), {});
    if (config.env?.ANTHROPIC_BASE_URL?.replace(/\/$/, '') === zaiBase) key = config.env.ANTHROPIC_AUTH_TOKEN;
  }
  if (!key) {
    const record = await readJson(join(process.env.ZCODE_DATA_BASE_DIR || homedir(), '.zcode', 'v2', 'credentials.json'), {});
    const names = Object.keys(record).filter(name => name.startsWith('account-provider:coding-plan:account:zai-individual-coding-plan:account:') && name.endsWith(':api-key'));
    if (names.length === 1) key = decryptZcodeCredential(record[names[0]]);
  }
  if (!key) throw Object.assign(new Error('Z.ai Coding Plan credential unavailable. Set CLAUDEX_ZAI_TOKEN in your local environment.'), { status: 401 });
  return { 'Content-Type': 'application/json', 'x-api-key': key, 'anthropic-version': '2023-06-01' };
}

// Match the installed ZCode credential cipher; decrypt in memory, never persist.
export function decryptZcodeCredential(value) {
  if (typeof value !== 'string') throw new Error('Invalid ZCode credential');
  if (!value.startsWith('enc:v1:')) return value;
  const parts = value.slice(7).split('.');
  if (parts.length !== 3) throw new Error('Invalid encrypted ZCode credential');
  const [iv, tag, data] = parts.map(part => Buffer.from(part, 'base64url'));
  if (iv.length !== 12 || tag.length !== 16) throw new Error('Invalid encrypted ZCode credential');
  let username = 'unknown'; try { username = userInfo().username; } catch { /* Match ZCode fallback. */ }
  const secret = process.env.ZCODE_CREDENTIAL_SECRET?.trim() || `zcode-credential-fallback:${platform()}:${homedir()}:${username}`;
  const cipher = createDecipheriv('aes-256-gcm', createHash('sha256').update(secret).digest(), iv); cipher.setAuthTag(tag);
  return Buffer.concat([cipher.update(data), cipher.final()]).toString('utf8');
}

export async function providerStatus() {
  const auth = await readJson(join(codexHome, 'auth.json'), {});
  let zai = false; try { await zaiHeaders(); zai = true; } catch { /* No credential is a status, not a paid fallback. */ }
  return { openaiChatgpt: Boolean(auth.tokens?.access_token), zaiCodingPlan: zai, paidFallback: false };
}
