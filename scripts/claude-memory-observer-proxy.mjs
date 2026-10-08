// Test instrumentation, not an enforcement proxy or a claim of model isolation.
// Bytes pass unchanged; only fixed metadata is persisted. Never serialize a wire object.
import { spawn } from 'node:child_process';
import { closeSync, openSync, writeSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
import { readSidecar, claimAuditFile } from './claude-memory-observer-config.mjs';
import { TurnCheckpoints } from './claude-memory-observer-turns.mjs';

export const LIMITS = Object.freeze({ line: 1024 * 1024, pending: 256, threads: 64, log: 64 * 1024 });
const own = (value, key) => Object.hasOwn(value, key);
const object = value => value !== null && typeof value === 'object' && !Array.isArray(value);
const identifier = value => typeof value === 'string' && value.length > 0 && value.length <= 256;
const requestKey = id => typeof id === 'string' && id.length <= 256 ? `s:${id}`
  : Number.isSafeInteger(id) ? `n:${id}` : null;
const nonToolItems = new Set(['userMessage', 'agentMessage', 'reasoning', 'plan', 'contextCompaction']);
const knownRequests = new Set(['initialize', 'config/read', 'thread/unsubscribe', 'turn/interrupt']);
const toolItems = new Set(['commandExecution', 'fileChange', 'mcpToolCall', 'dynamicToolCall',
  'collabAgentToolCall', 'webSearch', 'imageGeneration', 'realtimeToolCall']);

export class ObserverAudit {
  constructor(emit, limits = LIMITS) {
    this.emit = emit;
    this.limits = limits;
    this.pending = new Map();
    this.threads = new Map();
    this.complete = true;
    this.toolCount = 0;
    this.unknownItems = 0;
    this.unknownRequests = 0;
    this.turns = new TurnCheckpoints(this);
  }
  diagnostic(code) {
    this.complete = false;
    this.emit({ event: 'diagnostic', code });
  }
  frame(direction, bytes) {
    let message;
    try { message = JSON.parse(bytes.toString('utf8')); }
    catch { this.diagnostic('invalid_json'); return; }
    if (!object(message)) { this.diagnostic('invalid_envelope'); return; }
    if (direction === 'input' && typeof message.method === 'string') this.request(message);
    if (direction === 'output') {
      if (own(message, 'id') && !own(message, 'method')) this.response(message);
      if (message.method === 'item/started' || message.method === 'item/completed') this.item(message);
      if (message.method === 'turn/completed') this.turns.completed(message.params);
      if (own(message, 'id') && own(message, 'method')) { this.unknownRequests++; this.diagnostic('server_request'); }
    }
  }
  request(message) {
    const { method, params } = message;
    const observed = ['thread/start', 'mcpServerStatus/list', 'turn/start'].includes(method);
    if (!own(message, 'id') && method === 'initialized') return;
    if (!own(message, 'id')) { this.unknownRequests++; this.diagnostic('uncorrelated_request'); return; }
    if (!observed && !knownRequests.has(method)) { this.unknownRequests++; this.diagnostic('unknown_request'); }
    const key = requestKey(message.id);
    if (key === null || (observed && !object(params))) { this.diagnostic('invalid_request'); return; }
    if (this.pending.has(key)) { this.pending.delete(key); this.diagnostic('duplicate_request'); return; }
    if (this.pending.size >= this.limits.pending) { this.diagnostic('pending_limit'); return; }
    const thread = this.threads.get(params?.threadId);
    let record = { method: observed || knownRequests.has(method) ? method : 'unknown' };
    if (!observed) { this.pending.set(key, record); return; }
    if (method === 'thread/start') {
      record.toolsDisabledRequested = params.config?.['tools.enabled'] === false;
      record.hooksDisabledRequested = params.config?.['hooks.enabled'] === false;
    } else {
      record.thread = thread;
      if (method === 'mcpServerStatus/list') {
        record.full = params.detail === undefined || params.detail === null || params.detail === 'full';
        record.detailKnown = record.full || params.detail === 'toolsAndAuthOnly';
        record.unfiltered = params.serverName === undefined || params.serverName === null;
        record.cursor = params.cursor ?? null;
        if (record.cursor !== null && !identifier(record.cursor)) { this.diagnostic('invalid_cursor'); return; }
        if (thread) {
          thread.mcpAttested = false;
          thread.mcpGeneration = (thread.mcpGeneration ?? 0) + 1;
          record.generation = thread.mcpGeneration;
        }
      } else {
        record.turn = this.turns.begin(thread);
        this.emit({ event: 'turn_start', thread: thread?.alias ?? null,
          instructionSourcesEmpty: thread?.instructions === true,
          mcpInventoryEmpty: thread?.mcpAttested === true,
          afterAttestations: this.complete && thread?.instructions === true && thread?.mcpAttested === true,
          toolsDisabledRequested: thread?.toolsDisabledRequested === true,
          hooksDisabledRequested: thread?.hooksDisabledRequested === true });
      }
    }
    this.pending.set(key, record);
  }
  response(message) {
    const key = requestKey(message.id);
    const record = this.pending.get(key);
    if (!record) { this.diagnostic('orphan_response'); return; }
    this.pending.delete(key);
    if (own(message, 'error')) {
      if (record.turn) record.turn.thread.activeTurn = null;
      this.diagnostic('response_failed'); return;
    }
    if (!['thread/start', 'mcpServerStatus/list', 'turn/start'].includes(record.method)) return;
    if (!object(message.result)) { this.diagnostic('invalid_response'); return; }
    const result = message.result;
    if (record.method === 'thread/start') {
      const id = result.thread?.id;
      if (!identifier(id) || this.threads.has(id)) { this.diagnostic('invalid_thread'); return; }
      if (this.threads.size >= this.limits.threads) { this.diagnostic('thread_limit'); return; }
      const instructions = Array.isArray(result.instructionSources) && result.instructionSources.length === 0;
      const thread = { alias: `t${this.threads.size + 1}`, instructions, mcpAttested: false,
        toolsDisabledRequested: record.toolsDisabledRequested, hooksDisabledRequested: record.hooksDisabledRequested };
      this.threads.set(id, thread);
      this.emit({ event: 'thread_started', thread: thread.alias, instructionSourcesEmpty: instructions,
        toolsDisabledRequested: thread.toolsDisabledRequested, hooksDisabledRequested: thread.hooksDisabledRequested });
    } else if (record.method === 'mcpServerStatus/list') {
      const thread = record.thread;
      const data = result.data;
      const valid = thread && record.generation === thread.mcpGeneration && record.detailKnown && record.unfiltered && Array.isArray(data) && data.length <= 1024
        && own(result, 'nextCursor') && (result.nextCursor === null || identifier(result.nextCursor));
      const empty = valid && data.every(server => object(server) && own(server, 'serverInfo')
        && server.serverInfo === null && object(server.tools) && Object.keys(server.tools).length === 0
        && own(server, 'toolsError') && server.toolsError === null);
      const chain = record.cursor === null || (thread?.mcpCursor === record.cursor && thread?.mcpChain === true);
      const resourcesEmpty = valid && record.full && data.every(server => Array.isArray(server.resources)
        && server.resources.length === 0 && Array.isArray(server.resourceTemplates) && server.resourceTemplates.length === 0);
      if (thread && record.generation === thread.mcpGeneration) {
        thread.mcpChain = Boolean(empty && chain);
        thread.resourceChain = Boolean(resourcesEmpty && (record.cursor === null || thread.resourceChain));
        thread.mcpCursor = valid ? result.nextCursor : undefined;
        thread.mcpAttested = thread.mcpChain && result.nextCursor === null;
      }
      this.emit({ event: 'mcp_inventory', thread: thread?.alias ?? null,
        unfilteredThreadScope: Boolean(valid), fullDetail: record.full,
        pageEmpty: Boolean(empty), finalPage: Boolean(valid && result.nextCursor === null),
        attested: Boolean(valid && thread?.mcpAttested),
        resourcesAttested: Boolean(valid && thread?.mcpAttested && thread.resourceChain) });
    } else {
      this.emit({ event: 'turn_response', thread: record.thread?.alias ?? null, success: true });
      this.turns.response(record.turn, result);
    }
  }
  classifyItem(type) {
    if (toolItems.has(type)) this.toolCount = Math.min(this.toolCount + 1, Number.MAX_SAFE_INTEGER);
    else if (!nonToolItems.has(type)) this.unknownItems = Math.min(this.unknownItems + 1, Number.MAX_SAFE_INTEGER);
  }
  item(message) {
    const params = message.params;
    const type = params?.item?.type;
    const thread = this.threads.get(params?.threadId);
    this.classifyItem(type);
    if (!thread) this.diagnostic('orphan_item');
    this.emit({ event: message.method === 'item/started' ? 'item_started' : 'item_completed',
      thread: thread?.alias ?? null, toolItem: toolItems.has(type), knownNonToolItem: nonToolItems.has(type) });
  }
  finish() {
    this.emit({ event: 'summary', auditComplete: this.complete && this.pending.size === 0 && [...this.threads.values()].every(thread => !thread.activeTurn),
      pendingCount: this.pending.size, threadCount: this.threads.size,
      toolItemCount: this.toolCount, unknownItemCount: this.unknownItems, unknownRequestCount: this.unknownRequests,
      noToolItemsObserved: this.complete && this.pending.size === 0 && this.toolCount === 0 && this.unknownItems === 0 && this.unknownRequests === 0
        && [...this.threads.values()].every(thread => !thread.activeTurn) });
  }
}

// Retain at most one bounded line; an oversized line is discarded until its delimiter.
export function lineObserver(direction, audit) {
  const buffer = Buffer.alloc(audit.limits.line);
  let size = 0, dropping = false;
  return {
    write(chunk) {
      let offset = 0;
      while (offset < chunk.length) {
        const delimiter = chunk.indexOf(10, offset);
        const end = delimiter < 0 ? chunk.length : delimiter;
        const piece = chunk.subarray(offset, end);
        if (!dropping) {
          if (piece.length > buffer.length - size) { dropping = true; audit.diagnostic('line_limit'); }
          else { piece.copy(buffer, size); size += piece.length; }
        }
        if (delimiter >= 0) {
          if (!dropping && size) audit.frame(direction, buffer.subarray(0, size));
          size = 0; dropping = false;
        }
        offset = delimiter < 0 ? chunk.length : delimiter + 1;
      }
    },
    end() { if (size || dropping) audit.diagnostic('unterminated_line'); size = 0; },
  };
}

export function auditWriter(file, maxBytes = LIMITS.log) {
  // Exclusive creation keeps separate proxy processes from corrupting one audit.
  const fd = openSync(file, 'wx', 0o600);
  let total = 0, exhausted = false;
  const terminal = Buffer.from('{"event":"diagnostic","code":"log_limit"}\n');
  return {
    emit(record) {
      if (exhausted) return;
      const bytes = Buffer.from(`${JSON.stringify(record)}\n`);
      if (total + bytes.length + terminal.length > maxBytes) {
        exhausted = true;
        // Disk failure must not turn an event callback into an uncaught exception
        // whose runtime diagnostic could expose paths or a surrounding payload.
        if (total + terminal.length <= maxBytes) { try { writeSync(fd, terminal); } catch {} }
        return;
      }
      try {
        let offset = 0;
        while (offset < bytes.length) {
          const written = writeSync(fd, bytes, offset, bytes.length - offset);
          if (!written) throw new Error('audit_write_failed');
          offset += written;
        }
        total += bytes.length;
      }
      catch { exhausted = true; }
    },
    get failed() { return exhausted; },
    close() { closeSync(fd); },
  };
}

export async function runProxy({ binary, args, file, input = process.stdin, output = process.stdout,
  spawnProcess = spawn }) {
  const writer = auditWriter(file);
  const audit = new ObserverAudit(record => writer.emit(record));
  let child;
  try {
    child = spawnProcess(binary, args, { stdio: ['pipe', 'pipe', 'pipe'], shell: false, windowsHide: true });
    const incoming = lineObserver('input', audit), outgoing = lineObserver('output', audit);
    const observeInput = chunk => incoming.write(chunk);
    const inputEnded = () => incoming.end();
    input.on('data', observeInput);
    input.once('end', inputEnded);
    child.stdout.on('data', chunk => outgoing.write(chunk));
    child.stdout.once('end', () => outgoing.end());
    child.stderr.resume(); // No stderr content is retained or forwarded.
    const fail = () => { audit.diagnostic('transport_failure'); child.kill(); };
    input.once('error', fail); output.once('error', fail); child.stdin.once('error', fail);
    child.stdout.once('error', fail);
    input.pipe(child.stdin); child.stdout.pipe(output, { end: false });
    const stop = () => { audit.diagnostic('proxy_interrupted'); child.kill(); };
    process.once('SIGINT', stop); process.once('SIGTERM', stop);
    try {
      return await new Promise(resolve => {
        child.once('error', () => { audit.diagnostic('child_start_failed'); resolve(1); });
        child.once('close', (code, signal) => {
          if (signal || code !== 0) audit.diagnostic('child_failed');
          resolve(signal ? 1 : code ?? 1);
        });
      });
    } finally {
      process.removeListener('SIGINT', stop); process.removeListener('SIGTERM', stop);
      input.unpipe(child.stdin); child.stdout.unpipe(output);
      input.removeListener('data', observeInput); input.removeListener('end', inputEnded);
      input.pause();
    }
  } finally {
    audit.finish(); writer.close();
    if (writer.failed) throw new Error('observer_audit_incomplete');
  }
}

if (process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url) {
  try {
    const args = process.argv.slice(2);
    if (args[0] !== '--observer-sidecar' || args[2] !== '--') throw new Error('sidecar_required');
    const config = readSidecar(args[1]);
    const file = claimAuditFile(config.auditDirectory);
    process.exitCode = await runProxy({ binary: config.binary, file, args: args.slice(3) });
  } catch { process.stderr.write('Observer proxy failed; details suppressed.\n'); process.exitCode = 1; }
}
