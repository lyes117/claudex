import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadInstallation, probeOwnedWorker, spawnRuntime, waitRuntime } from './claude-memory-runtime.mjs';

const pointer = JSON.parse(readFileSync(join(dirname(fileURLToPath(import.meta.url)), 'claudex-memory-pointer.json'), 'utf8'));
const receipt = loadInstallation(pointer.root);
// Never intentionally submit memory payloads to an existing foreign worker.
await probeOwnedWorker(receipt);
const [command, ...arguments_] = process.argv.slice(2);
if (command === 'mcp') {
  process.exitCode = await waitRuntime(spawnRuntime(receipt, arguments_, { mcp: true }));
} else if (['context', 'session-init', 'file-context', 'observation', 'summarize'].includes(command)) {
  process.exitCode = await waitRuntime(spawnRuntime(receipt, ['hook', 'codex', command]));
} else {
  throw new Error('Unknown native memory hook');
}
