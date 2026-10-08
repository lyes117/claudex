import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync, writeFileSync, copyFileSync, readdirSync, mkdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn } from 'node:child_process';
import { readSidecar, claimAuditFile } from './claude-memory-observer-config.mjs';
import { ObserverAudit, LIMITS, lineObserver, auditWriter } from './claude-memory-observer-proxy.mjs';

if (process.argv.includes('--echo-fixture')) {
  process.stderr.write('SYNTHETIC_STDERR_SECRET');
  process.stdin.pipe(process.stdout);
} else if (process.argv.includes('--argv-fixture')) {
  process.stdout.write(`${JSON.stringify(process.argv.slice(process.argv.indexOf('--argv-fixture') + 1))}\n`);
} else if (process.argv.includes('--hanging-fixture')) {
  const descendant = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { stdio: 'ignore', windowsHide: true });
  descendant.once('spawn', () => process.stdout.write(`${JSON.stringify({ fixturePid: process.pid, descendantPid: descendant.pid })}\n`));
  setInterval(() => {}, 1000);
} else {
  const privateId = 'SYNTHETIC_PRIVATE_THREAD_ID';
  function fixture(limits = LIMITS) {
    const records = [];
    const audit = new ObserverAudit(value => records.push(value), limits);
    const request = (id, method, params) => audit.frame('input', Buffer.from(JSON.stringify({ id, method, params })));
    const response = (id, result) => audit.frame('output', Buffer.from(JSON.stringify({ id, result })));
    const start = () => {
      request(1, 'thread/start', { config: { 'tools.enabled': false, 'hooks.enabled': false }, secret: 'SYNTHETIC_PROMPT_SECRET' });
      response(1, { thread: { id: privateId }, instructionSources: [], result: 'SYNTHETIC_RESULT_SECRET' });
    };
    const mcp = (id = 2, params = {}, result = { data: [{ serverInfo: null, tools: {}, toolsError: null }], nextCursor: null }) => {
      request(id, 'mcpServerStatus/list', { threadId: privateId, ...params }); response(id, result);
    };
    const complete = (turnId = 'SYNTHETIC_TURN_ID', items = []) => audit.frame('output', Buffer.from(JSON.stringify({
      method: 'turn/completed', params: { threadId: privateId, turn: { id: turnId, status: 'completed', items } },
    })));
    return { records, audit, request, response, start, mcp, complete };
  }
  test('correlates exact thread-scoped attestations before the turn without retaining wire content', () => {
    const f = fixture(); f.start(); f.mcp();
    f.request(3, 'turn/start', { threadId: privateId, input: [{ text: 'SYNTHETIC_PROMPT_SECRET' }] });
    f.response(3, { turn: { id: 'SYNTHETIC_TURN_ID' } }); f.complete(); f.audit.finish();
    assert.deepEqual(f.records[2], { event: 'turn_start', thread: 't1', instructionSourcesEmpty: true,
      mcpInventoryEmpty: true, afterAttestations: true, toolsDisabledRequested: true, hooksDisabledRequested: true });
    assert.equal(f.records.at(-1).noToolItemsObserved, true);
    assert.doesNotMatch(JSON.stringify(f.records), /SYNTHETIC_|threadId|input|result/);
  });
  test('turn before attestations remains visibly unattested', () => {
    const f = fixture(); f.start(); f.request(3, 'turn/start', { threadId: privateId }); f.mcp();
    assert.equal(f.records.find(record => record.event === 'turn_start').afterAttestations, false);
  });
  test('missing instructionSources never equals an explicitly empty list', () => {
    const f = fixture(); f.request(1, 'thread/start', {}); f.response(1, { thread: { id: privateId } });
    assert.equal(f.records[0].instructionSourcesEmpty, false);
  });
  test('toolsAndAuthOnly attests serverInfo and tools but does not attest resources', () => {
    const f = fixture(); f.start(); f.mcp(2, { detail: 'toolsAndAuthOnly' });
    assert.equal(f.records.at(-1).attested, true);
    assert.equal(f.records.at(-1).resourcesAttested, false);
    assert.equal(f.records.at(-1).fullDetail, false);
  });
  test('MCP unknown detail, global scope, filtered inventory, tools errors and missing null are not attestations', () => {
    for (const [params, data] of [
      [{ detail: 'futureDetail' }, { serverInfo: null, tools: {}, toolsError: null }],
      [{ threadId: null }, { serverInfo: null, tools: {}, toolsError: null }],
      [{ serverName: 'SYNTHETIC_SERVER' }, { serverInfo: null, tools: {}, toolsError: null }],
      [{}, { serverInfo: null, tools: {}, toolsError: 'SYNTHETIC_ERROR_SECRET' }],
      [{}, { tools: {}, toolsError: null }],
      [{}, { serverInfo: {}, tools: {}, toolsError: null }],
      [{}, { serverInfo: null, tools: { 'SYNTHETIC_TOOL': {} }, toolsError: null }],
    ]) {
      const f = fixture(); f.start(); f.mcp(2, params, { data: [data], nextCursor: null });
      assert.equal(f.records.at(-1).attested, false);
      assert.doesNotMatch(JSON.stringify(f.records), /SYNTHETIC_/);
    }
  });
  test('pagination attests only a consecutive fully empty chain', () => {
    const f = fixture(); f.start(); f.mcp(2, {}, { data: [], nextCursor: 'opaque-page' });
    assert.equal(f.records.at(-1).attested, false);
    f.mcp(3, { cursor: 'wrong-page' }, { data: [], nextCursor: null });
    assert.equal(f.records.at(-1).attested, false);
    f.mcp(4, {}, { data: [], nextCursor: 'opaque-page' });
    f.mcp(5, { cursor: 'opaque-page' }, { data: [], nextCursor: null });
    assert.equal(f.records.at(-1).attested, true);
  });
  test('late stale MCP response cannot re-attest a newer request', () => {
    const f = fixture(); f.start();
    f.request(2, 'mcpServerStatus/list', { threadId: privateId });
    f.request(3, 'mcpServerStatus/list', { threadId: privateId });
    f.response(2, { data: [], nextCursor: null });
    assert.equal(f.records.at(-1).attested, false);
    f.request(4, 'turn/start', { threadId: privateId });
    assert.equal(f.records.at(-1).afterAttestations, false);
  });
  test('unbounded or structured cursor is rejected before retaining a request', () => {
    const f = fixture(); f.start();
    f.request(2, 'mcpServerStatus/list', { threadId: privateId, cursor: { secret: 'SYNTHETIC_SECRET'.repeat(10000) } });
    assert.equal(f.audit.pending.size, 0);
    assert.deepEqual(f.records.at(-1), { event: 'diagnostic', code: 'invalid_cursor' });
  });
  test('tool and unknown item types invalidate a no-tools observation while retaining no payload', () => {
    const f = fixture(); f.start();
    for (const type of ['agentMessage', 'dynamicToolCall', 'futureItem']) {
      f.audit.frame('output', Buffer.from(JSON.stringify({ method: 'item/completed', params: {
        threadId: privateId, item: { type, text: 'SYNTHETIC_RESULT_SECRET', arguments: 'SYNTHETIC_PROMPT_SECRET' },
      } })));
    }
    f.audit.finish();
    assert.equal(f.records.at(-1).noToolItemsObserved, false);
    assert.equal(f.records.at(-1).toolItemCount, 1);
    assert.equal(f.records.at(-1).unknownItemCount, 1);
    assert.doesNotMatch(JSON.stringify(f.records), /SYNTHETIC_/);
  });
  test('line buffers, pending maps and thread maps have hard bounds and fail their audit closed', () => {
    const f = fixture({ ...LIMITS, line: 32, pending: 1, threads: 1 });
    const lines = lineObserver('input', f.audit);
    for (let index = 0; index < 1000; index++) lines.write(Buffer.from('S'));
    lines.write(Buffer.from('\n{"secret":"SYNTHETIC_SECRET"}\n'));
    f.start(); f.request(2, 'turn/start', { threadId: privateId });
    f.request(3, 'turn/start', { threadId: privateId });
    f.response(2, {}); f.request(4, 'thread/start', {}); f.response(4, { thread: { id: 'another' }, instructionSources: [] });
    f.audit.finish();
    assert.equal(f.audit.pending.size, 0); assert.equal(f.audit.threads.size, 1);
    assert.equal(f.records.at(-1).auditComplete, false);
    assert.doesNotMatch(JSON.stringify(f.records), /SYNTHETIC_/);
  });
  test('malformed JSON, duplicate requests and partial final lines are fixed diagnostics only', () => {
    const f = fixture(); const input = lineObserver('input', f.audit);
    input.write(Buffer.from('SYNTHETIC_SECRET\n')); input.write(Buffer.from('{"secret":')); input.end();
    f.request(1, 'thread/start', {}); f.request(1, 'thread/start', {}); f.audit.finish();
    assert.deepEqual(f.records.slice(0, 3).map(record => record.code), ['invalid_json', 'unterminated_line', 'duplicate_request']);
    assert.equal(f.records.at(-1).auditComplete, false);
  });
  test('audit file cap has an explicit terminal marker and exclusive ownership', t => {
    const directory = mkdtempSync(join(tmpdir(), 'observer-proxy-'));
    t.after(() => rmSync(directory, { recursive: true, force: true }));
    const file = join(directory, 'audit.ndjson'); const writer = auditWriter(file, 180);
    for (let index = 0; index < 100; index++) writer.emit({ event: 'summary', auditComplete: true });
    writer.close(); const bytes = readFileSync(file);
    assert.ok(bytes.length <= 180); assert.match(bytes.toString(), /"log_limit"/);
    assert.throws(() => auditWriter(file));
  });
  test('real child-process relay preserves exact bytes and discards child stderr', { timeout: 10000 }, async t => {
    const directory = mkdtempSync(join(tmpdir(), 'observer-proxy-relay-'));
    t.after(() => rmSync(directory, { recursive: true, force: true }));
    const sidecar = writeSidecar(directory);
    const child = spawn(process.execPath, [fileURLToPath(new URL('./claude-memory-observer-proxy.mjs', import.meta.url)),
      '--observer-sidecar', sidecar, '--', fileURLToPath(import.meta.url), '--echo-fixture'], { env: { ...process.env }, windowsHide: true });
    t.after(() => child.kill());
    const output = [], stderr = [];
    child.stdout.on('data', bytes => output.push(bytes)); child.stderr.on('data', bytes => stderr.push(bytes));
    const bytes = Buffer.from('{"id":1,"method":"initialize","params":{"prompt":"SYNTHETIC_PROMPT_SECRET"}}\r\nSYNTHETIC_INVALID_SECRET\n');
    child.stdin.end(bytes);
    const code = await new Promise((resolve, reject) => { child.once('error', reject); child.once('close', resolve); });
    assert.equal(code, 0); assert.deepEqual(Buffer.concat(output), bytes);
    assert.equal(Buffer.concat(stderr).length, 0);
    const auditDirectory = join(directory, 'audit');
    const file = join(auditDirectory, readdirSync(auditDirectory).find(name => name.endsWith('.ndjson')));
    assert.doesNotMatch(readFileSync(file, 'utf8'), /SYNTHETIC_|prompt|params/);
  });
  function writeSidecar(directory, file = join(directory, 'sidecar.json'), override = {}) {
    const auditDirectory = join(directory, 'audit'); mkdirSync(auditDirectory, { recursive: true });
    writeFileSync(file, JSON.stringify({ version: 1, node: process.execPath,
      script: fileURLToPath(new URL('./claude-memory-observer-proxy.mjs', import.meta.url)),
      binary: process.execPath, auditDirectory, ...override }));
    return file;
  }
  test('checkpoint waits for response and completion in either order with no global summary', () => {
    for (const terminalFirst of [false, true]) {
      const f = fixture(); f.start(); f.mcp(); f.request(3, 'turn/start', { threadId: privateId });
      if (terminalFirst) f.complete();
      assert.equal(f.records.filter(row => row.event === 'turn_checkpoint').length, 0);
      f.response(3, { turn: { id: 'SYNTHETIC_TURN_ID', items: [] } });
      if (!terminalFirst) f.complete();
      assert.deepEqual(f.records.at(-1), { event: 'turn_checkpoint', thread: 't1', turn: 'v1',
        completed: true, afterAttestations: true, toolsDisabledRequested: true, hooksDisabledRequested: true,
        pendingCount: 0, toolItemCount: 0, unknownItemCount: 0, unknownRequestCount: 0,
        auditCompleteThroughTurn: true, noToolItemsObservedThroughTurn: true });
      assert.equal(f.records.some(row => row.event === 'summary'), false);
      assert.equal(f.audit.threads.get(privateId).activeTurn, null);
      assert.doesNotMatch(JSON.stringify(f.records), /SYNTHETIC_/);
    }
  });
  test('terminal tool/unknown kinds, mismatched identity, orphan and duplicate completions fail closed', () => {
    for (const variant of ['tool', 'unknown', 'mismatch', 'missingItems', 'orphan', 'duplicate']) {
      const f = fixture(); f.start(); f.mcp();
      if (variant === 'orphan') f.complete();
      f.request(3, 'turn/start', { threadId: privateId });
      f.response(3, { turn: { id: 'SYNTHETIC_TURN_ID' } });
      f.complete(variant === 'mismatch' ? 'different' : 'SYNTHETIC_TURN_ID',
        variant === 'tool' ? [{ type: 'dynamicToolCall', secret: 'SYNTHETIC_SECRET' }]
          : variant === 'unknown' ? [{ type: 'futureItem' }] : variant === 'missingItems' ? null : []);
      if (variant === 'duplicate') f.complete();
      f.audit.finish(); assert.equal(f.records.at(-1).noToolItemsObserved, false);
      if (variant !== 'duplicate') assert.equal(f.records.find(row => row.event === 'turn_checkpoint').noToolItemsObservedThroughTurn, false);
      assert.doesNotMatch(JSON.stringify(f.records), /SYNTHETIC_/);
    }
  });
  test('all RPC IDs are tracked; unknown requests and known-request ID collisions cannot attest', () => {
    for (const collision of [false, true]) {
      const f = fixture(); f.start(); f.mcp();
      f.request(3, collision ? 'config/read' : 'futureRequest', { private: 'SYNTHETIC_SECRET' });
      f.request(collision ? 3 : 4, 'turn/start', { threadId: privateId });
      f.audit.finish(); assert.equal(f.records.at(-1).auditComplete, false);
      assert.doesNotMatch(JSON.stringify(f.records), /SYNTHETIC_|futureRequest/);
    }
    const f = fixture(); f.audit.frame('input', Buffer.from('{"method":"initialized","params":{}}'));
    f.request(1, 'initialize', {}); f.response(1, {}); f.audit.finish();
    assert.equal(f.records.at(-1).auditComplete, true);
  });
  test('strict owned sidecar rejects duplicates, unknowns, raw oversize and invalid paths', t => {
    const directory = mkdtempSync(join(tmpdir(), 'observer-config-'));
    t.after(() => rmSync(directory, { recursive: true, force: true }));
    const file = writeSidecar(directory), valid = readFileSync(file, 'utf8');
    assert.equal(readSidecar(file).binary, process.execPath);
    for (const text of [valid.replace('"version":1', '"version":1,"version":1'),
      valid.replace('"version":1', '"version":1,"future":true'), ' '.repeat(8193),
      valid.replace('"version":1', '"version":2')]) {
      writeFileSync(file, text); assert.throws(() => readSidecar(file));
    }
    writeSidecar(directory, file, { binary: 'relative-path' }); assert.throws(() => readSidecar(file));
    writeFileSync(join(directory, 'auth.json'), 'SYNTHETIC_NEVER_READ');
    writeSidecar(directory, file, { binary: join(directory, 'auth.json') }); assert.throws(() => readSidecar(file));
  });
  test('audit directory uses nonce filenames with persistent exclusive claims and a hard launch limit', t => {
    const directory = mkdtempSync(join(tmpdir(), 'observer-claims-'));
    t.after(() => rmSync(directory, { recursive: true, force: true }));
    const paths = new Set();
    for (let index = 0; index < 64; index++) {
      const file = claimAuditFile(directory); paths.add(file);
      const writer = auditWriter(file); writer.emit({ event: 'summary', auditComplete: false }); writer.close();
    }
    assert.equal(paths.size, 64); assert.equal(readdirSync(directory).length, 128);
    assert.throws(() => claimAuditFile(directory), /audit_directory_limit/);
    assert.ok([...paths].every(file => /audit-\d\d-[0-9a-f-]{36}\.ndjson$/.test(file)));
  });
  test('a preexisting duplicate-slot or unowned audit log cannot expand the directory budget', t => {
    const directory = mkdtempSync(join(tmpdir(), 'observer-invalid-claims-'));
    t.after(() => rmSync(directory, { recursive: true, force: true }));
    const first = claimAuditFile(directory), writer = auditWriter(first); writer.close();
    const duplicate = join(directory, 'audit-00-11111111-1111-4111-8111-111111111111.ndjson');
    writeFileSync(duplicate, ''); assert.throws(() => claimAuditFile(directory), /duplicate_audit_slot/);
    rmSync(duplicate);
    const unowned = join(directory, 'audit-01-11111111-1111-4111-8111-111111111111.ndjson');
    writeFileSync(unowned, ''); assert.throws(() => claimAuditFile(directory));
  });
  const launcher = process.env.CX_OBSERVER_TEST_LAUNCHER;
  const nativeOptions = { timeout: 10000, skip: process.platform !== 'win32' || !launcher };
  function nativeChild(t, args) {
    const directory = mkdtempSync(join(tmpdir(), 'observer-native-'));
    t.after(() => rmSync(directory, { recursive: true, force: true }));
    const executable = join(directory, 'observer-proxy.exe'); copyFileSync(launcher, executable);
    writeSidecar(directory, executable + '.json');
    const child = spawn(executable, [fileURLToPath(import.meta.url), ...args], { windowsHide: true,
      env: { PATH: process.env.PATH, SystemRoot: process.env.SystemRoot, WINDIR: process.env.WINDIR } });
    t.after(() => child.kill());
    return child;
  }
  test('native launcher preserves empty, quoted, Unicode and trailing slash arguments', nativeOptions, async t => {
    const expected = ['', 'with spaces', 'quote"inside', 'C:\\trailing space\\', 'é漢字'];
    const child = nativeChild(t, ['--argv-fixture', ...expected]);
    const output = [], stderr = [];
    child.stdout.on('data', bytes => output.push(bytes)); child.stderr.on('data', bytes => stderr.push(bytes));
    child.stdin.end();
    const code = await new Promise((resolve, reject) => { child.once('error', reject); child.once('close', resolve); });
    assert.equal(code, 0); assert.equal(Buffer.concat(stderr).length, 0);
    assert.deepEqual(JSON.parse(Buffer.concat(output).toString()), expected);
  });
  test('native sidecar rejects malformed/unknown/duplicate configurations before any child output', nativeOptions, async t => {
    const directory = mkdtempSync(join(tmpdir(), 'observer-native-invalid-'));
    t.after(() => rmSync(directory, { recursive: true, force: true }));
    const executable = join(directory, 'observer-proxy.exe'); copyFileSync(launcher, executable);
    const sidecar = writeSidecar(directory, executable + '.json'), valid = readFileSync(sidecar, 'utf8');
    for (const text of [valid.replace('"version":1', '"version":1,"version":1'),
      valid.replace('"version":1', '"version":1,"future":"SYNTHETIC_SECRET"'), ' '.repeat(8193),
      valid.replace('"version":1', '"version":null')]) {
      writeFileSync(sidecar, text);
      const child = spawn(executable, [fileURLToPath(import.meta.url), '--argv-fixture', 'SYNTHETIC_SECRET'], { windowsHide: true });
      t.after(() => child.kill()); const output = [], error = [];
      child.stdout.on('data', bytes => output.push(bytes)); child.stderr.on('data', bytes => error.push(bytes)); child.stdin.end();
      const code = await new Promise((resolve, reject) => { child.once('error', reject); child.once('close', resolve); });
      assert.equal(code, 1); assert.equal(Buffer.concat(output).length, 0);
      assert.equal(Buffer.concat(error).toString().trim(), 'Observer proxy launcher failed; details suppressed.');
    }
  });
  test('killing native launcher terminates the owned proxy and synthetic descendant tree', nativeOptions, async t => {
    const child = nativeChild(t, ['--hanging-fixture']);
    const pids = await new Promise((resolve, reject) => {
      let text = '';
      child.once('error', reject);
      child.stdout.on('data', bytes => { text += bytes.toString(); if (text.includes('\n')) resolve(JSON.parse(text.split('\n')[0])); });
      child.once('close', () => reject(new Error('fixture closed before readiness')));
    });
    child.kill();
    const deadline = Date.now() + 5000;
    const running = pid => { try { process.kill(pid, 0); return true; } catch { return false; } };
    while ((running(pids.fixturePid) || running(pids.descendantPid)) && Date.now() < deadline) {
      await new Promise(resolve => setTimeout(resolve, 20));
    }
    assert.equal(running(pids.fixturePid), false); assert.equal(running(pids.descendantPid), false);
  });
}
