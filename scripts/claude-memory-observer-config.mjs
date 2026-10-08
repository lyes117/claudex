import { closeSync, lstatSync, openSync, readFileSync, readdirSync, realpathSync, statSync } from 'node:fs';
import { basename, dirname, extname, isAbsolute, join, resolve } from 'node:path';
import { randomUUID } from 'node:crypto';

// Owned operator configuration, never a model-provided path or inherited environment option.
// Exact ordered JSON grammar rejects duplicate/unknown keys before JSON.parse.
const jsonString = String.raw`"(?:[^"\\\x00-\x1f]|\\(?:["\\/bfnrt]|u[0-9a-fA-F]{4}))*"`;
const shape = new RegExp(String.raw`^\s*\{\s*"version"\s*:\s*1\s*,\s*"node"\s*:\s*${jsonString}\s*,\s*"script"\s*:\s*${jsonString}\s*,\s*"binary"\s*:\s*${jsonString}\s*,\s*"auditDirectory"\s*:\s*${jsonString}\s*\}\s*$`);

export function checkedPath(value, directory = false) {
  if (typeof value !== 'string' || !isAbsolute(value) || value.length > 2048 || /[\x00-\x1f]/.test(value)) throw new Error('invalid_owned_path');
  let current = resolve(value);
  for (;;) {
    const metadata = lstatSync(current);
    if (metadata.isSymbolicLink()) throw new Error('redirected_owned_path');
    const parent = dirname(current); if (parent === current) break; current = parent;
  }
  const canonical = realpathSync(value), metadata = statSync(canonical);
  if (directory ? !metadata.isDirectory() : !metadata.isFile()) throw new Error('invalid_owned_path_kind');
  return canonical;
}

export function readSidecar(file) {
  checkedPath(file);
  const metadata = lstatSync(file);
  if (metadata.size < 1 || metadata.size > 8192) throw new Error('sidecar_byte_limit');
  const bytes = readFileSync(file);
  if (bytes.length !== metadata.size || bytes.length > 8192) throw new Error('sidecar_changed');
  const text = bytes.toString('utf8');
  if (!shape.test(text)) throw new Error('invalid_sidecar');
  const config = JSON.parse(text);
  if (basename(config.script) !== 'claude-memory-observer-proxy.mjs'
      || (process.platform === 'win32' && [config.node, config.binary].some(file => extname(file).toLowerCase() !== '.exe')))
    throw new Error('invalid_sidecar_program');
  return { node: checkedPath(config.node), script: checkedPath(config.script), binary: checkedPath(config.binary),
    auditDirectory: checkedPath(config.auditDirectory, true) };
}

export function claimAuditFile(directory) {
  directory = checkedPath(directory, true);
  const entries = readdirSync(directory);
  if (entries.length > 128) throw new Error('audit_directory_limit');
  const logSlots = new Set();
  for (const name of entries) {
    const claim = /^\.slot-(?:[0-5][0-9]|6[0-3])$/.test(name);
    const log = /^audit-([0-5][0-9]|6[0-3])-[0-9a-f-]{36}\.ndjson$/.exec(name);
    const metadata = lstatSync(join(directory, name));
    if ((!claim && !log) || !metadata.isFile() || metadata.isSymbolicLink()
        || (claim ? metadata.size !== 0 : metadata.size > 64 * 1024)) throw new Error('invalid_audit_directory');
    if (log) {
      if (logSlots.has(log[1])) throw new Error('duplicate_audit_slot');
      logSlots.add(log[1]);
      const owner = lstatSync(join(directory, `.slot-${log[1]}`));
      if (!owner.isFile() || owner.size !== 0 || owner.isSymbolicLink()) throw new Error('unowned_audit_file');
    }
  }
  // Persistent claims deliberately survive a crash. At most 64 processes can ever
  // launch into one fresh owned directory; no stale claim is reclaimed automatically.
  for (let slot = 0; slot < 64; slot++) {
    let fd;
    try { fd = openSync(join(directory, `.slot-${String(slot).padStart(2, '0')}`), 'wx', 0o600); }
    catch (error) { if (error.code === 'EEXIST') continue; throw new Error('audit_claim_failed'); }
    closeSync(fd);
    return join(directory, `audit-${String(slot).padStart(2, '0')}-${randomUUID()}.ndjson`);
  }
  throw new Error('audit_directory_limit');
}
