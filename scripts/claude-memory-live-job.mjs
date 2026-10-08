// Owned fixture supervisor. This API is not exposed to scripts, models or arbitrary RPC peers.
import { spawn } from 'node:child_process';
import { writeFileSync, realpathSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { randomUUID } from 'node:crypto';
import { assertRunDirectory } from './claude-memory-live-fixture.mjs';

const timer = (milliseconds, reject) => setTimeout(() => reject(new Error('owned_control_timeout')), milliseconds);
export class OwnedLiveJob {
  constructor(child, lifetimeMs) {
    this.child = child; this.lifetimeMs = lifetimeMs; this.deadline = Date.now() + lifetimeMs + 12000; this.records = []; this.waiter = null;
    this.closed = false; this.provenStopped = false; let line = '';
    this.exit = new Promise(resolve => child.once('close', (code, signal) => { this.closed = true; this.rejectPending(); resolve({ code, signal }); }));
    child.once('error', () => this.rejectPending()); child.stderr.resume();
    child.stdout.on('data', bytes => {
      line += bytes.toString('utf8');
      if (Buffer.byteLength(line) > 4096) { this.rejectPending(); child.stdin.end(); return; }
      while (line.includes('\n')) {
        const split = line.indexOf('\n'), text = line.slice(0, split); line = line.slice(split + 1);
        let record; try { record = JSON.parse(text); } catch { this.rejectPending(); child.stdin.end(); return; }
        const keys = Object.keys(record).sort().join(',');
        const valid = (['ready', 'reset', 'stopped', 'failed'].includes(keys) && record[keys] === true)
          || (keys === 'process,started' && record.started === true && Number.isSafeInteger(record.process) && record.process >= 0 && record.process < 16)
          || (keys === 'portOwned' && typeof record.portOwned === 'boolean')
          || (keys === 'running,success' && typeof record.running === 'boolean' && typeof record.success === 'boolean');
        if (!valid || this.records.length >= 4) { this.rejectPending(); child.stdin.end(); return; }
        if (record.stopped) this.provenStopped = true;
        if (this.waiter) { const resolve = this.waiter.resolve; this.waiter = null; resolve(record); }
        else this.records.push(record);
      }
    });
    child.stdin.on('error', () => this.rejectPending());
  }
  rejectPending() { if (this.waiter) { this.waiter.reject(new Error('owned_control_failed')); this.waiter = null; } }
  async next(timeoutMs) {
    if (this.records.length) return this.records.shift();
    if (this.closed || this.waiter) throw new Error('owned_control_unavailable');
    let deadline;
    try { return await new Promise((resolve, reject) => {
      deadline = timer(Math.max(1, Math.min(timeoutMs, this.deadline - Date.now())), reject); this.waiter = { resolve, reject };
    }); } finally { clearTimeout(deadline); this.waiter = null; }
  }
  async command(record, timeoutMs = this.lifetimeMs + 11000) {
    const bytes = JSON.stringify(record) + '\n';
    if (Buffer.byteLength(bytes) > 65536 || this.closed) throw new Error('owned_command_limit');
    this.child.stdin.write(bytes); const result = await this.next(timeoutMs);
    if (result.failed) throw new Error('owned_native_failed'); return result;
  }
  async start({ binary, args, env, cwd, input = null }) {
    const result = await this.command({ action: 'start', binary, args, env, cwd, input });
    if (!result.started) throw new Error('owned_start_failed'); return result.process;
  }
  async wait(process) { const result = await this.command({ action: 'wait', process }); if (result.running || !result.success) throw new Error('owned_child_failed'); }
  async running(process) { return (await this.command({ action: 'status', process })).running === true; }
  async portOwned(port) { return (await this.command({ action: 'port', port })).portOwned === true; }
  async reset() { const result = await this.command({ action: 'reset' }); if (!result.reset) throw new Error('owned_reset_failed'); }
  async close() {
    // EOF is also the parent-crash protocol: native cleanup kills/awaits Job then
    // unlinks owned auth aliases while holding the guard, and releases it last.
    if (!this.closed) this.child.stdin.end();
    let deadline;
    try {
      const result = await Promise.race([this.exit, new Promise((_, reject) => { deadline = timer(Math.max(1, this.deadline - Date.now()), reject); })]);
      if (!this.provenStopped || result.code !== 0 || result.signal) throw new Error('owned_cleanup_unproven');
    } finally { clearTimeout(deadline); }
  }
}

export async function startOwnedLiveJob(fixture, { authSource, supervisor, lifetimeMs = 240000 }) {
  if (process.platform !== 'win32' || !Number.isSafeInteger(lifetimeMs) || lifetimeMs < 1 || lifetimeMs > 240000) throw new Error('unsupported_owned_job');
  assertRunDirectory(fixture.run);
  const sidecar = join(fixture.run, `supervisor-${randomUUID()}.json`);
  writeFileSync(sidecar, JSON.stringify({ root: fixture.run, authSource: realpathSync(authSource),
    authLink: join(fixture.codexHome, 'auth.json'), lifetimeMs, ownerPid: process.pid }), { flag: 'wx', mode: 0o600 });
  const child = spawn(realpathSync(supervisor), [sidecar], { windowsHide: true, shell: false, detached: true, cwd: fixture.run,
    env: { PATH: process.env.PATH, SystemRoot: process.env.SystemRoot, WINDIR: process.env.WINDIR }, stdio: ['pipe', 'pipe', 'pipe'] });
  const job = new OwnedLiveJob(child, lifetimeMs);
  try { if (!(await job.next(5000)).ready) throw new Error('owned_not_ready'); return job; }
  catch { await job.close().catch(() => {}); throw new Error('owned_start_failed'); }
}

export function ownedRuntimeEnvironment(fixture) {
  const temporary = join(fixture.run, 'temp'), appdata = join(fixture.userHome, 'AppData');
  for (const directory of [temporary, appdata]) mkdirSync(directory, { recursive: true });
  return { PATH: process.env.PATH, PATHEXT: process.env.PATHEXT, SystemRoot: process.env.SystemRoot, WINDIR: process.env.WINDIR,
    HOME: fixture.userHome, USERPROFILE: fixture.userHome, APPDATA: appdata, LOCALAPPDATA: appdata,
    TEMP: temporary, TMP: temporary, TMPDIR: temporary };
}
