import test from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { randomUUID } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { createServer, connect } from 'node:net';
import { startOwnedLiveJob, ownedRuntimeEnvironment } from './claude-memory-live-job.mjs';
import { LIVE_ROOT, assertRunDirectory } from './claude-memory-live-fixture.mjs';

const self = fileURLToPath(import.meta.url);
if (process.argv.includes('--owned-child')) {
  const index = process.argv.indexOf('--owned-child'), marker = process.argv[index + 1], port = Number(process.argv[index + 2]);
  const descendant = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { stdio: 'ignore', windowsHide: true });
  descendant.once('spawn', () => createServer(socket => socket.end()).listen(port, '127.0.0.1', () => {
    writeFileSync(marker, JSON.stringify({ child: process.pid, descendant: descendant.pid }));
  }));
} else if (process.argv.includes('--owned-parent')) {
  const index = process.argv.indexOf('--owned-parent'), fixture = JSON.parse(readFileSync(process.argv[index + 1], 'utf8'));
  const job = await startOwnedLiveJob(fixture, { supervisor: fixture.supervisor, authSource: fixture.source, lifetimeMs: 10000 });
  await job.start({ binary: process.execPath, args: [self, '--owned-child', fixture.marker, String(fixture.port)],
    env: ownedRuntimeEnvironment(fixture), cwd: fixture.codexHome });
  process.stdout.write('{"ready":true}\n'); setInterval(() => {}, 1000);
} else {
  const supervisor = process.env.CX_LIVE_SUPERVISOR_TEST;
  const options = { timeout: 20000, skip: process.platform !== 'win32' || !supervisor };
  const alive = pid => { try { process.kill(pid, 0); return true; } catch { return false; } };
  async function waitFor(predicate, timeout = 5000) {
    const deadline = Date.now() + timeout;
    while (!await predicate()) { if (Date.now() >= deadline) throw new Error('synthetic_wait_timeout'); await new Promise(resolve => setTimeout(resolve, 15)); }
  }
  async function portFree(port) {
    return await new Promise(resolve => {
      const socket = connect({ port, host: '127.0.0.1' });
      socket.once('connect', () => { socket.destroy(); resolve(false); });
      socket.once('error', error => resolve(error.code === 'ECONNREFUSED'));
    });
  }
  async function fixture(t) {
    const outside = mkdtempSync(join(tmpdir(), 'live-job-original-')), source = join(outside, 'synthetic-auth.json');
    writeFileSync(source, 'synthetic-original');
    const run = assertRunDirectory(join(LIVE_ROOT, `run-${randomUUID()}`)); mkdirSync(run, { recursive: true });
    const codexHome = join(run, 'observer', '.codex'), userHome = join(run, 'observer', 'user-home');
    mkdirSync(codexHome, { recursive: true }); mkdirSync(userHome, { recursive: true });
    const server = createServer(); await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
    const port = server.address().port; await new Promise(resolve => server.close(resolve));
    t.after(() => { rmSync(assertRunDirectory(run), { recursive: true, force: true }); rmSync(outside, { recursive: true, force: true }); });
    return { run, source, codexHome, userHome, marker: join(run, 'synthetic-processes.json'), port, supervisor };
  }
  async function child(job, f) {
    const index = await job.start({ binary: process.execPath, args: [self, '--owned-child', f.marker, String(f.port)],
      env: ownedRuntimeEnvironment(f), cwd: f.codexHome });
    await waitFor(() => existsSync(f.marker)); return { index, pids: JSON.parse(readFileSync(f.marker, 'utf8')) };
  }
  test('native owned reset/stop await descendants, free the port and clean links before releasing guard', options, async t => {
    const f = await fixture(t), job = await startOwnedLiveJob(f, { supervisor, authSource: f.source, lifetimeMs: 10000 });
    t.after(() => job.close().catch(() => {}));
    assert.throws(() => writeFileSync(f.source, 'must-not-write'));
    assert.throws(() => writeFileSync(join(f.codexHome, 'auth.json'), 'must-not-write'));
    const first = await child(job, f); assert.equal(await job.running(first.index), true); assert.equal(await job.portOwned(f.port), true);
    await job.reset();
    assert.equal(alive(first.pids.child), false); assert.equal(alive(first.pids.descendant), false); assert.equal(await portFree(f.port), true);
    assert.equal(await job.portOwned(f.port), false);
    assert.equal(existsSync(join(f.codexHome, 'auth.json')), true); assert.throws(() => writeFileSync(f.source, 'still-protected'));
    rmSync(f.marker);
    const second = await child(job, f); await job.close();
    assert.equal(alive(second.pids.child), false); assert.equal(alive(second.pids.descendant), false); assert.equal(await portFree(f.port), true);
    assert.equal(existsSync(join(f.codexHome, 'auth.json')), false); assert.equal(readFileSync(f.source, 'utf8'), 'synthetic-original');
    writeFileSync(f.source, 'write-after-cleanup');
  });
  test('a foreign listener is refused and is never stopped by owned cleanup', options, async t => {
    const f = await fixture(t), server = createServer(socket => socket.end());
    await new Promise(resolve => server.listen(f.port, '127.0.0.1', resolve));
    t.after(() => new Promise(resolve => server.close(resolve)));
    const job = await startOwnedLiveJob(f, { supervisor, authSource: f.source, lifetimeMs: 10000 });
    t.after(() => job.close().catch(() => {}));
    assert.equal(await job.portOwned(f.port), false); await job.close();
    assert.equal(await portFree(f.port), false); assert.equal(server.listening, true);
  });
  test('native global deadline kills owned descendants without requiring a stop command', options, async t => {
    const f = await fixture(t), job = await startOwnedLiveJob(f, { supervisor, authSource: f.source, lifetimeMs: 700 });
    t.after(() => job.close().catch(() => {})); const running = await child(job, f);
    await job.exit; assert.equal(alive(running.pids.child), false); assert.equal(alive(running.pids.descendant), false);
    assert.equal(await portFree(f.port), true); assert.equal(existsSync(join(f.codexHome, 'auth.json')), false);
    assert.equal(readFileSync(f.source, 'utf8'), 'synthetic-original');
    assert.equal(job.provenStopped, false); // Error timeout is not a successful capture or a clean terminal proof.
  });
  test('crashing the JS parent triggers native EOF cleanup of worker tree before guard release', options, async t => {
    const f = await fixture(t), specification = join(f.run, 'parent-fixture.json'); writeFileSync(specification, JSON.stringify(f));
    const parent = spawn(process.execPath, [self, '--owned-parent', specification], { windowsHide: true, stdio: ['ignore', 'pipe', 'ignore'] });
    t.after(() => parent.kill());
    await new Promise((resolve, reject) => { parent.once('error', reject); parent.stdout.once('data', bytes => {
      try { assert.equal(JSON.parse(bytes.toString()).ready, true); resolve(); } catch (error) { reject(error); }
    }); });
    await waitFor(() => existsSync(f.marker)); const pids = JSON.parse(readFileSync(f.marker, 'utf8'));
    parent.kill();
    try { await waitFor(() => !alive(pids.child) && !alive(pids.descendant) && !existsSync(join(f.codexHome, 'auth.json'))); }
    catch { assert.fail(JSON.stringify({ parentAlive: alive(parent.pid), childAlive: alive(pids.child), descendantAlive: alive(pids.descendant), aliasPresent: existsSync(join(f.codexHome, 'auth.json')) })); }
    assert.equal(await portFree(f.port), true); assert.equal(readFileSync(f.source, 'utf8'), 'synthetic-original');
    writeFileSync(f.source, 'write-after-parent-crash');
  });
}
