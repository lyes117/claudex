import test from 'node:test';
import assert from 'node:assert/strict';
import { bindCaptureProof } from './claude-memory-live-capture.mjs';
const checkpoint = { event: 'turn_checkpoint', auditFile: 'a1', thread: 't1', turn: 'v1', completed: true,
  afterAttestations: true, toolsDisabledRequested: true, hooksDisabledRequested: true, pendingCount: 0,
  toolItemCount: 0, unknownItemCount: 0, unknownRequestCount: 0, auditCompleteThroughTurn: true,
  noToolItemsObservedThroughTurn: true };
const auditFile = 'audit-00-11111111-1111-4111-8111-111111111111.ndjson';
const phase = { invocation: 'synthetic-invocation-a', project: { project: 'cx1-a', session: 'session-a', canary: 'CANARY_A' },
  provenance: { workerHash: 'synthetic-worker' } };
const row = { id: 7, project: 'cx1-a', content_session_id: 'session-a', platform_source: 'codex', narrative: 'CANARY_A' };
test('one fresh capture binds exact stored IDs to its own invocation, audit file and terminal checkpoint', () => {
  assert.deepEqual(bindCaptureProof(phase, [row], [checkpoint], [auditFile]), {
    invocation: phase.invocation, project: 'cx1-a', session: 'session-a', observationIds: [7], auditFile,
    checkpoints: [{ thread: 't1', turn: 'v1' }], provenance: phase.provenance });
});
test('matching counts never replace exact capture/project/session/canary provenance', () => {
  for (const changed of [{ project: 'cx1-b' }, { content_session_id: 'session-b' }, { platform_source: 'claude' },
    { narrative: 'CANARY_B' }, { id: 0 }]) {
    assert.throws(() => bindCaptureProof(phase, [{ ...row, ...changed }], [checkpoint], [auditFile]));
  }
  assert.throws(() => bindCaptureProof(phase, [row, { ...row, project: 'cx1-b' }], [checkpoint], [auditFile]));
});
test('wrong namespace, another observer process, duplicate terminal and failed attestation are rejected', () => {
  for (const records of [[{ ...checkpoint, auditFile: 'a2' }], [{ ...checkpoint, thread: 't2' }],
    [checkpoint, checkpoint], [{ ...checkpoint, completed: false }], [checkpoint, { event: 'diagnostic', auditFile: 'a1' }]]) {
    assert.throws(() => bindCaptureProof(phase, [row], records, [auditFile]));
  }
  assert.throws(() => bindCaptureProof(phase, [row], [checkpoint], [auditFile, 'other.ndjson']));
  assert.throws(() => bindCaptureProof(phase, [row], [checkpoint], ['other.ndjson']));
});
