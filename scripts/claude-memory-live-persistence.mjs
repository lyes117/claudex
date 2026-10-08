// Run only as an owned Bun child after the memory worker has terminated.
// Fixed SQL returns observation IDs/counts; no generated body or auth data escapes.
import { Database } from 'bun:sqlite';
import { lstatSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
const specification = process.argv[2];
try {
  if (lstatSync(specification).size > 8192) throw new Error('bound');
  const bytes = readFileSync(specification); if (bytes.length > 8192) throw new Error('bound');
  const { dataDir, project, session, canary, output } = JSON.parse(bytes.toString('utf8'));
  const db = new Database(join(dataDir, 'claude-mem.db'), { readonly: true, create: false });
  try {
    const total = db.query('SELECT COUNT(*) AS count FROM observations').get().count;
    const provenance = db.query(`SELECT COUNT(*) AS count FROM observations o
      LEFT JOIN sdk_sessions s ON o.memory_session_id = s.memory_session_id
      WHERE o.project = ? AND s.content_session_id = ? AND COALESCE(s.platform_source, 'claude') = 'codex'`).get(project, session).count;
    const rows = db.query(`SELECT o.id FROM observations o
      LEFT JOIN sdk_sessions s ON o.memory_session_id = s.memory_session_id
      WHERE o.project = ? AND s.content_session_id = ? AND COALESCE(s.platform_source, 'claude') = 'codex'
      AND (instr(COALESCE(o.title, ''), ?) > 0 OR instr(COALESCE(o.subtitle, ''), ?) > 0
        OR instr(COALESCE(o.narrative, ''), ?) > 0 OR instr(COALESCE(o.text, ''), ?) > 0 OR instr(COALESCE(o.facts, ''), ?) > 0)
      ORDER BY o.id LIMIT 101`).all(project, session, canary, canary, canary, canary, canary);
    if (!Number.isSafeInteger(total) || total < 1 || total > 100 || total !== provenance
        || !rows.length || rows.length > 100 || rows.some(row => !Number.isSafeInteger(row.id) || row.id <= 0)) throw new Error('invalid');
    writeFileSync(output, JSON.stringify({ persisted: true, observationIds: rows.map(row => row.id), total }), { flag: 'wx', mode: 0o600 });
  } finally { db.close(); }
} catch { process.exitCode = 1; } // No raw error, path, query result or body is emitted.
