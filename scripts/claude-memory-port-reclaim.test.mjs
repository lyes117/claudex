import test from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, writeFileSync, rmSync, unlinkSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { homedir, tmpdir } from 'node:os';
import { pathToFileURL } from 'node:url';
import { randomUUID } from 'node:crypto';
import { patchPortReclaimSource } from './claude-memory-build.mjs';
import { REVISION } from './claude-memory-runtime.mjs';

const source = () => execFileSync('git', ['-C', '.build-tools/claude-mem-upstream',
  'show', `${REVISION}:src/shared/port-reclaim.ts`], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });

test('port policy refuses changed signatures, declarations and double patches', () => {
  const original = source();
  assert.throws(() => patchPortReclaimSource(original.replace(
    'export async function reclaimGhostListeningPort(\n  port: number,',
    'export async function reclaimGhostListeningPort(\n  port: bigint,')));
  assert.throws(() => patchPortReclaimSource(original.replace("        | 'out-of-budget';", "        | 'unexpected';")));
  assert.throws(() => patchPortReclaimSource(original + '\nexport function future() {}\n'));
  assert.throws(() => patchPortReclaimSource(patchPortReclaimSource(original)));
});

test('actual patched upstream TypeScript never inspects or kills a port owner', t => {
  const directory = mkdtempSync(join(tmpdir(), 'claudex-port-policy-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  // A fresh generated file beside the pinned dependencies preserves its real
  // relative imports. It is never part of a published bundle and is removed.
  const path = resolve('.build-tools/cm-build-a1951f2-cx1-cycle1/source/src/shared',
    `claudex-port-policy-${randomUUID()}.ts`);
  writeFileSync(path, patchPortReclaimSource(source()), { flag: 'wx' });
  t.after(() => unlinkSync(path));
  const script = `import {reclaimGhostListeningPort} from ${JSON.stringify(pathToFileURL(path).href)};
    const deps=new Proxy({}, {get(){throw Error('Unexpected process inspection')}});
    for(const port of [1024,37777,37778,65535]) {
      const result=await reclaimGhostListeningPort(port,deps);
      if(JSON.stringify(result)!==JSON.stringify({reclaimed:false,reason:'claudex-reclaim-disabled',killedPids:[]}))
        throw Error('Unexpected port result');
    }
    console.log('NO_PROCESS_INSPECTION');`;
  const env = Object.fromEntries(['Path', 'PATH', 'SystemRoot', 'SYSTEMROOT', 'TEMP', 'TMP']
    .filter(key => process.env[key] !== undefined).map(key => [key, process.env[key]]));
  Object.assign(env, { HOME: directory, USERPROFILE: directory,
    CLAUDE_MEM_DATA_DIR: join(directory, 'data'), CLAUDE_CONFIG_DIR: join(directory, 'claude'),
    CLAUDE_MEM_TELEMETRY: '0', DO_NOT_TRACK: '1' });
  const output = execFileSync(join(homedir(), '.bun', 'bin', process.platform === 'win32' ? 'bun.exe' : 'bun'),
    ['--eval', script], { env, cwd: directory, encoding: 'utf8', timeout: 15000,
      windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
  assert.equal(output.trim(), 'NO_PROCESS_INSPECTION');
});
