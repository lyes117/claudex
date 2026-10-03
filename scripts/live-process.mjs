// Windows process helpers for opt-in live fixtures. Never expose captured output
// in an exception, and terminate only the still-owned process and its descendants.
import { spawn, spawnSync } from 'node:child_process';

export async function waitClosed(closed, timeoutMs) {
  let timer;
  try {
    return await Promise.race([closed.then(() => true), new Promise(resolve => { timer = setTimeout(() => resolve(false), timeoutMs); })]);
  } finally { clearTimeout(timer); }
}

export async function stopOwned(child, closed, { graceMs = 5000, exitMs = 10000 } = {}) {
  if (await waitClosed(closed, graceMs)) return;
  if (process.platform !== 'win32') throw new Error('Owned-tree cleanup requires Windows');
  if (child.exitCode === null && child.signalCode === null && child.pid) {
    const killed = spawnSync('taskkill', ['/pid', String(child.pid), '/t', '/f'], { windowsHide: true, stdio: 'ignore', timeout: 10000 });
    if (killed.status !== 0 && !(await waitClosed(closed, 250))) throw new Error('Owned fixture tree could not be terminated');
  }
  if (!(await waitClosed(closed, exitMs))) throw new Error('Owned fixture exit could not be confirmed');
}

export async function captureOwned(binary, args, { cwd, timeoutMs = 180000, maxBytes = 8 * 1024 * 1024 }) {
  const child = spawn(binary, args, { cwd, windowsHide: true, stdio: ['ignore', 'pipe', 'ignore'] });
  const closed = new Promise(resolve => child.once('close', resolve));
  let fail;
  const failed = new Promise((_, reject) => { fail = reject; });
  child.once('error', () => fail(new Error('Owned fixture failed to launch')));
  const chunks = [];
  let bytes = 0;
  child.stdout.on('data', chunk => {
    bytes += chunk.length;
    if (bytes > maxBytes) fail(new Error('Owned fixture output exceeded its limit'));
    else chunks.push(chunk);
  });
  const timer = setTimeout(() => fail(new Error('Owned fixture timed out')), timeoutMs);
  try {
    const code = await Promise.race([closed, failed]);
    if (code !== 0) throw new Error('Owned fixture failed; raw server details are not printed');
    return Buffer.concat(chunks).toString('utf8');
  } finally {
    clearTimeout(timer);
    await stopOwned(child, closed, { graceMs: 0 });
  }
}

export function parseJsonLines(text) {
  try { return text.split(/\r?\n/).filter(Boolean).map(line => JSON.parse(line)); }
  catch { throw new Error('Invalid fixture JSON; raw output is not printed'); }
}
