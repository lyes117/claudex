import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
const bun = process.env.CX_LIVE_BUN_TEST;
const module = fileURLToPath(new URL('./claude-memory-live-persistence.mjs', import.meta.url));
test('real Bun read-only reopen requires exact SQLite provenance and canary, emits IDs only', { skip: !bun }, t => {
  const root = mkdtempSync(join(tmpdir(), 'live-persistence-')); t.after(() => rmSync(root, { recursive: true, force: true }));
  const schema = `import { Database } from 'bun:sqlite'; const d=new Database(process.argv.at(-1));
    d.run('CREATE TABLE sdk_sessions(memory_session_id TEXT,content_session_id TEXT,platform_source TEXT)');
    d.run('CREATE TABLE observations(id INTEGER,memory_session_id TEXT,project TEXT,title TEXT,subtitle TEXT,narrative TEXT,text TEXT,facts TEXT)');
    d.run('INSERT INTO sdk_sessions VALUES(?,?,?)',['memory-a','session-a','codex']);
    d.run('INSERT INTO observations VALUES(?,?,?,?,?,?,?,?)',[7,'memory-a','cx1-a',null,null,'SYNTHETIC_CANARY',null,null]); d.close();`;
  assert.equal(spawnSync(bun, ['-e', schema, join(root, 'claude-mem.db')], { windowsHide: true, stdio: 'ignore' }).status, 0);
  const input = join(root, 'input.json'), output = join(root, 'output.json');
  const specification = { dataDir: root, project: 'cx1-a', session: 'session-a', canary: 'SYNTHETIC_CANARY', output };
  writeFileSync(input, JSON.stringify(specification));
  const result = spawnSync(bun, [module, input], { windowsHide: true, encoding: 'utf8' });
  assert.equal(result.status, 0); assert.equal(result.stdout, ''); assert.equal(result.stderr, '');
  assert.deepEqual(JSON.parse(readFileSync(output, 'utf8')), { persisted: true, observationIds: [7], total: 1 });
  rmSync(output);
  for (const changed of [{ project: 'cx1-b' }, { session: 'session-b' }, { canary: "' OR 1=1 --" }]) {
    writeFileSync(input, JSON.stringify({ ...specification, ...changed }));
    const negative = spawnSync(bun, [module, input], { windowsHide: true, encoding: 'utf8' });
    assert.equal(negative.status, 1); assert.equal(negative.stdout, ''); assert.equal(negative.stderr, ''); assert.equal(existsSync(output), false);
  }
});
