// Terminal proof is scoped to one admitted turn, not to a future global process close.
const id = value => typeof value === 'string' && value.length > 0 && value.length <= 256;
export class TurnCheckpoints {
  constructor(audit) { this.audit = audit; this.sequence = 0; }
  begin(thread) {
    const audit = this.audit;
    if (!thread || thread.activeTurn || this.sequence >= 9999999) {
      audit.diagnostic('invalid_turn_admission'); return null;
    }
    const turn = { alias: `v${++this.sequence}`, thread, id: null, terminal: null,
      afterAttestations: audit.complete && thread.instructions === true && thread.mcpAttested === true,
      toolsDisabledRequested: thread.toolsDisabledRequested === true,
      hooksDisabledRequested: thread.hooksDisabledRequested === true };
    thread.activeTurn = turn; return turn;
  }
  response(turn, result) {
    if (!turn || turn.thread.activeTurn !== turn || !id(result.turn?.id)
        || result.turn.id === turn.thread.lastTurnId) { this.audit.diagnostic('invalid_turn_response'); return; }
    turn.id = result.turn.id;
    if (result.turn.items !== undefined) this.items(result.turn.items);
    this.publish(turn);
  }
  items(items) {
    if (!Array.isArray(items) || items.length > 4096) { this.audit.diagnostic('invalid_terminal_items'); return; }
    for (const item of items) this.audit.classifyItem(item?.type);
  }
  completed(params) {
    const thread = this.audit.threads.get(params?.threadId), turn = thread?.activeTurn;
    if (!turn || !id(params?.turn?.id) || turn.terminal) { this.audit.diagnostic('orphan_turn_completion'); return; }
    // Inspect item kinds immediately; never retain or serialize terminal contents.
    this.items(params.turn.items);
    turn.terminal = { id: params.turn.id, completed: params.turn.status === 'completed' };
    this.publish(turn);
  }
  publish(turn) {
    if (!turn.id || !turn.terminal) return;
    const audit = this.audit;
    if (turn.id !== turn.terminal.id) audit.diagnostic('turn_identity_mismatch');
    const complete = audit.complete && audit.pending.size === 0;
    audit.emit({ event: 'turn_checkpoint', thread: turn.thread.alias, turn: turn.alias,
      completed: turn.id === turn.terminal.id && turn.terminal.completed,
      afterAttestations: turn.afterAttestations,
      toolsDisabledRequested: turn.toolsDisabledRequested, hooksDisabledRequested: turn.hooksDisabledRequested,
      pendingCount: audit.pending.size, toolItemCount: audit.toolCount, unknownItemCount: audit.unknownItems,
      unknownRequestCount: audit.unknownRequests, auditCompleteThroughTurn: complete,
      noToolItemsObservedThroughTurn: complete && audit.toolCount === 0 && audit.unknownItems === 0 && audit.unknownRequests === 0 });
    turn.thread.lastTurnId = turn.id; turn.thread.activeTurn = null;
  }
}
