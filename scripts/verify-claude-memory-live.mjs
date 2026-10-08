// First tranche: prepare synthetic fixtures and define capture verification.
// CLI activation remains blocked until root reviews the immutable gates below.
import { pathToFileURL } from 'node:url';
import { startOwnedLiveJob } from './claude-memory-live-job.mjs';
import { prepareCaptureInvocation, captureOneInvocation, assertPortFree } from './claude-memory-live-capture.mjs';
import { prepareLiveFixture, assertOwnedHealth, readBoundedHttp, checkPersistedObservations,
  checkAuditCheckpoints } from './claude-memory-live-fixture.mjs';

export const LIVE_GATES = Object.freeze({ authGuardReviewed: false, proxyDirectoryReviewed: false,
  workerForeignPortFailClosedReviewed: false, ownerCrashCleanupReviewed: false });

export function parseLiveArguments(args) {
  const options = {}; const names = new Set(['package-root', 'binary', 'bun', 'proxy', 'guard', 'port', 'supervisor', 'auth-source']);
  for (let index = 0; index < args.length; index++) {
    if (!args[index].startsWith('--')) throw new Error('invalid_option');
    const name = args[index].replace(/^--/, '');
    if (['prepare', 'execute-reviewed'].includes(name)) {
      if (options[name] !== undefined) throw new Error('duplicate_option'); options[name] = true;
    } else {
      if (!args[index].startsWith('--') || !names.has(name) || options[name] !== undefined
          || !args[index + 1] || args[index + 1].startsWith('--')) throw new Error('invalid_option');
      options[name] = args[++index];
    }
  }
  return options;
}

export async function verifyCaptureCompression(fixture, hooks, auditRecords, { deadlineMs = 120000 } = {}) {
  if (!Number.isSafeInteger(deadlineMs) || deadlineMs < 1 || deadlineMs > 120000) throw new Error('invalid_capture_budget');
  // The supplied hook runner must be the reviewed, bounded, job-owned foreground runner.
  // Hooks can return no-op success on failure: their exit codes are never sufficient evidence.
  await assertOwnedHealth(fixture);
  for (const project of fixture.projects) {
    await hooks('session-init', { hook_event_name: 'UserPromptSubmit', session_id: project.session, cwd: project.cwd,
      prompt: 'Record the synthetic integration decision from the fixture tool result.' });
    await hooks('observation', { hook_event_name: 'PostToolUse', session_id: project.session, cwd: project.cwd,
      tool_name: 'Read', tool_use_id: project.session + '-read', tool_input: { file_path: project.cwd + '/fixture-contract.txt' },
      tool_response: `${project.canary}: The independent event ledger is a durable integration decision. Preserve this exact synthetic identifier.` });
  }
  const deadline = Date.now() + deadlineMs;
  let captures = [];
  while (Date.now() < deadline) {
    await assertOwnedHealth(fixture);
    captures = await Promise.all(fixture.projects.map(async project => {
      const query = new URLSearchParams({ project: project.project, contentSessionId: project.session, platformSource: 'codex', limit: '100' });
      const payload = await readBoundedHttp(`http://127.0.0.1:${fixture.port}/api/observations?${query}`);
      return checkPersistedObservations(payload, project);
    }));
    if (captures.every(result => result.captured)) break;
    await new Promise(resolve => setTimeout(resolve, 250));
  }
  const audit = checkAuditCheckpoints(await auditRecords());
  const persisted = captures.length === 2 && captures.every(result => result.captured);
  // Preparation cannot associate an observer invocation/audit-file namespace with
  // each persisted capture yet. Independent true booleans never prove that binding.
  return { passed: false, captureAuditAssociationProven: false, persisted, observerAttested: audit.attested,
    projectCount: fixture.projects.length, observationCount: captures.reduce((sum, value) => sum + value.observationCount, 0),
    attestedTurnCount: audit.turnCount, nativeHookDispatchProven: false, mcpRetrievalProven: false, sessionStartIsolationProven: false };
}

// Root may invoke this private fixture stage only after the independent gate review.
// Each capture has its own fresh worker data and audit namespace, not a counter-based binding.
export async function executePreparedCaptures(fixture, { authSource, supervisor, budgetMs = 240000 }) {
  if (!Number.isSafeInteger(budgetMs) || budgetMs < 1000 || budgetMs > 240000) throw new Error('invalid_live_budget');
  const deadline = Date.now() + budgetMs;
  const phases = fixture.projects.map(project => prepareCaptureInvocation(fixture, project));
  const remaining = deadline - Date.now(); if (remaining < 1) throw new Error('global_live_timeout');
  const job = await startOwnedLiveJob(fixture, { authSource, supervisor, lifetimeMs: remaining });
  const proofs = [];
  try {
    for (const phase of phases) {
      if (Date.now() >= deadline) throw new Error('global_live_timeout');
      proofs.push(await captureOneInvocation(job, phase, deadline));
    }
  } finally { await job.close(); await assertPortFree(fixture.port); }
  const bound = proofs.length === 2 && new Set(proofs.map(proof => proof.invocation)).size === 2;
  return { passed: bound, executed: true, captureAuditAssociationProven: bound, observerAttested: bound,
    projectCount: proofs.length, observationCount: proofs.reduce((count, proof) => count + proof.observationIds.length, 0),
    attestedTurnCount: proofs.reduce((count, proof) => count + proof.checkpoints.length, 0),
    nativeHookDispatchProven: false, mcpRetrievalProven: false, sessionStartIsolationProven: false,
    sharedDatabaseIsolationProven: false };
}

export async function main(args) {
  const options = parseLiveArguments(args);
  if (options['execute-reviewed']) {
    // Fail before fixture creation, auth linking or any process launch. These gates
    // require source/negative-fixture review, not a user-supplied attestation file.
    if (!Object.values(LIVE_GATES).every(Boolean)) return { executed: false, prepared: false, passed: false, liveBlocked: true };
    const fixture = prepareLiveFixture({ packageRoot: options['package-root'], binary: options.binary, bun: options.bun,
      proxy: options.proxy, guard: options.guard, port: Number(options.port) });
    return await executePreparedCaptures(fixture, { authSource: options['auth-source'], supervisor: options.supervisor });
  }
  if (!options.prepare) return { executed: false, prepared: false, passed: false };
  prepareLiveFixture({ packageRoot: options['package-root'], binary: options.binary, bun: options.bun,
    proxy: options.proxy, guard: options.guard, port: Number(options.port) });
  return { executed: false, prepared: true, passed: false, projectCount: 2 };
}

if (process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url) {
  try { const verdict = await main(process.argv.slice(2)); process.stdout.write(`${JSON.stringify(verdict)}\n`); process.exitCode = verdict.liveBlocked ? 1 : 0; }
  catch { process.stdout.write('{"executed":false,"prepared":false,"passed":false,"failed":true}\n'); process.exitCode = 1; }
}
