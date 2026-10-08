import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, mkdtempSync, mkdirSync, linkSync, readFileSync, rmSync, statSync, unlinkSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { spawn } from 'node:child_process';
import { main, parseLiveArguments } from './verify-claude-memory-live.mjs';
import { assertRunDirectory, prepareLiveFixture, checkPersistedObservations, checkAuditCheckpoints, readAuditFiles } from './claude-memory-live-fixture.mjs';
import { REVISION, VERSION, IDENTITY_SCHEME, sha256 } from './claude-memory-runtime.mjs';

test('default and explicit execute remain preparation-only until source gates are reviewed', async () => {
  assert.deepEqual(await main([]), { executed: false, prepared: false, passed: false });
  assert.deepEqual(await main(['--execute-reviewed']), { executed: false, prepared: false, passed: false, liveBlocked: true });
  assert.throws(() => parseLiveArguments(['--prepare', '--prepare']));
  assert.throws(() => assertRunDirectory('C:\\foreign\\run-00000000-0000-0000-0000-000000000000'));
});
test('preparation makes two isolated same-basename Git projects without auth or process execution', t => {
  const packageRoot = mkdtempSync(join(tmpdir(), 'live-package-'));
  t.after(() => rmSync(packageRoot, { recursive: true, force: true }));
  const scripts = join(packageRoot, 'plugin', 'scripts'); mkdirSync(scripts, { recursive: true });
  const files = ['worker-service.cjs', 'mcp-server.cjs', 'claude-memory-project-identity.mjs'];
  for (const file of files) writeFileSync(join(scripts, file), '// synthetic; never executed\n');
  writeFileSync(join(packageRoot, 'claudex-observer-build.json'), JSON.stringify({ revision: REVISION, version: VERSION,
    toolsEnabled: false, privateUserHome: true, portReclaimDisabled: true, projectIdentity: { scheme: IDENTITY_SCHEME },
    sha256: Object.fromEntries(files.map(file => [`plugin/scripts/${file}`, sha256(join(scripts, file))])) }));
  const f = prepareLiveFixture({ packageRoot, binary: process.execPath, bun: process.execPath,
    proxy: process.execPath, guard: process.execPath, port: 43187 });
  t.after(() => rmSync(assertRunDirectory(f.run), { recursive: true, force: true }));
  assert.equal(f.projects.length, 2); assert.notEqual(f.projects[0].project, f.projects[1].project);
  assert.equal(existsSync(join(f.codexHome, 'auth.json')), false);
  assert.equal(existsSync(join(f.projects[0].cwd, '.git')), true);
  assert.equal(existsSync(join(f.projects[1].cwd, '.git')), true);
  assert.equal(existsSync(join(f.dataDir, 'claude-mem.db')), false);
});

test('stored matching Codex observation and exact canary are required; queued/bytes never suffice', () => {
  const project = { project: 'cx1-project-a', session: 'synthetic-session-a', canary: 'SYNTHETIC_CANARY_A' };
  const row = { id: 1, project: project.project, content_session_id: project.session, platform_source: 'codex', narrative: project.canary };
  assert.deepEqual(checkPersistedObservations({ items: [row], hasMore: false }, project), { captured: true, observationCount: 1 });
  for (const replacement of [{ project: 'cx1-project-b' }, { content_session_id: 'session-b' }, { platform_source: 'claude' }, { narrative: 'unrelated' }]) {
    assert.equal(checkPersistedObservations({ items: [{ ...row, ...replacement }], hasMore: false }, project).captured, false);
  }
  assert.throws(() => checkPersistedObservations({ status: 'queued' }, project));
});

const checkpoint = { event: 'turn_checkpoint', thread: 't1', turn: 'v1', completed: true, afterAttestations: true,
  toolsDisabledRequested: true, hooksDisabledRequested: true, auditCompleteThroughTurn: true,
  noToolItemsObservedThroughTurn: true, toolItemCount: 0, unknownItemCount: 0, unknownRequestCount: 0, pendingCount: 0 };
test('attested checkpoint is required and does not claim a clean global close', () => {
  assert.deepEqual(checkAuditCheckpoints([{ event: 'summary', auditComplete: true }]), { attested: false, turnCount: 0 });
  assert.deepEqual(checkAuditCheckpoints([checkpoint]), { attested: true, turnCount: 1 });
  for (const change of [{ completed: false }, { afterAttestations: false }, { toolsDisabledRequested: false },
    { hooksDisabledRequested: false }, { pendingCount: 1 }, { toolItemCount: 1 }, { unknownItemCount: 1 }, { unknownRequestCount: 1 }]) {
    assert.equal(checkAuditCheckpoints([{ ...checkpoint, ...change }]).attested, false);
  }
  assert.equal(checkAuditCheckpoints([checkpoint, { event: 'diagnostic' }]).attested, false);
});
test('audit reader projects fixed metadata and rejects partial writes and record limits', t => {
  const directory = mkdtempSync(join(tmpdir(), 'live-audit-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const file = join(directory, 'nonce.ndjson');
  writeFileSync(file, JSON.stringify({ ...checkpoint, privatePayload: 'SYNTHETIC_DO_NOT_PROPAGATE' }) + '\n');
  assert.deepEqual(readAuditFiles(directory), [{ auditFile: 'a1', ...checkpoint }]);
  writeFileSync(join(directory, '.slot-00'), '');
  writeFileSync(join(directory, 'second.ndjson'), JSON.stringify(checkpoint) + '\n');
  assert.deepEqual(readAuditFiles(directory).map(record => record.auditFile), ['a1', 'a2']);
  unlinkSync(join(directory, 'second.ndjson'));
  writeFileSync(file, JSON.stringify(checkpoint)); assert.throws(() => readAuditFiles(directory), /partial_audit/);
  writeFileSync(file, JSON.stringify({ ...checkpoint, privatePayload: 'x'.repeat(3000) }) + '\n');
  assert.throws(() => readAuditFiles(directory), /audit_record_limit/);
});

test('Windows guard protects original and shared writes; replacing a private alias does not alter the original',
  { timeout: 10000, skip: process.platform !== 'win32' || !process.env.CX_LIVE_AUTH_GUARD_TEST }, async t => {
    const directory = mkdtempSync(join(tmpdir(), 'live-auth-guard-'));
    t.after(() => rmSync(directory, { recursive: true, force: true }));
    const source = join(directory, 'synthetic-auth.json'), linked = join(directory, 'hardlink.json');
    const content = '{"synthetic":true}\n';
    writeFileSync(source, content);
    const before = statSync(source);
    const child = spawn(process.env.CX_LIVE_AUTH_GUARD_TEST, [source], { windowsHide: true, stdio: ['pipe', 'pipe', 'ignore'] });
    t.after(() => child.kill());
    const closed = new Promise((resolve, reject) => { child.once('error', reject); child.once('close', resolve); });
    let text = '';
    await new Promise((resolve, reject) => {
      child.stdout.on('data', bytes => {
        text += bytes.toString();
        if (text.length > 64) reject(new Error('guard_output_limit'));
        else if (text.includes('\n')) { try { if (JSON.parse(text).ready === true) resolve(); else reject(new Error('guard_not_ready')); } catch { reject(new Error('guard_protocol')); } }
      });
      child.once('close', () => reject(new Error('guard_closed_before_readiness')));
    });
    try {
      // Acquire the original's guard BEFORE creating the first hardlink.
      linkSync(source, linked);
      for (const path of [source, linked]) {
        assert.throws(() => writeFileSync(path, 'changed'), 'write must remain blocked');
        assert.equal(statSync(path).mode, before.mode);
        assert.equal(statSync(path).size, before.size);
        assert.equal(statSync(path).mtimeMs, before.mtimeMs);
      }
      assert.throws(() => unlinkSync(source), 'original delete must remain blocked');
      assert.equal(existsSync(source), true);
      // Probe whether Windows permits link creation while the guard is held.
      const late = join(directory, 'late-hardlink.json');
      linkSync(source, late);
      assert.throws(() => writeFileSync(late, 'changed'));
      // Windows permits removing a different hardlink name. Its replacement is
      // a private inode, so writing that replacement cannot update the original.
      unlinkSync(late); writeFileSync(late, 'private-replacement');
      assert.equal(readFileSync(source, 'utf8') === content, true);
      assert.equal(statSync(source).mode, before.mode);
      assert.equal(statSync(source).size, before.size);
      assert.equal(statSync(source).mtimeMs, before.mtimeMs);
      unlinkSync(late); // Owned private alias cleanup while the guard remains held.
    } finally { child.stdin.end(); await closed; }
    writeFileSync(linked, 'writes-reenabled');
    assert.equal(readFileSync(source, 'utf8'), 'writes-reenabled');
    assert.equal(statSync(source).mode, before.mode);
  });
