use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;

use codex_extension_api::FunctionCallError;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolCallOutcome;
use codex_extension_api::ToolResultDisposition;
use tokio::sync::oneshot;
use tokio::sync::watch;

use crate::FileTool;
use crate::display::Completion;
use crate::display::envelope;
use crate::display::started_item;
use crate::error;

type Key = (String, String);
const MAX_PENDING: usize = 256;

#[derive(Default)]
pub(crate) struct TurnCalls(Mutex<Admission>);

#[derive(Default)]
struct Admission {
    closed: bool,
    keys: BTreeSet<Key>,
}

#[cfg(test)]
#[path = "publication_tests.rs"]
mod tests;

#[derive(Default)]
pub(crate) struct Publications(Mutex<BTreeMap<Key, Arc<Entry>>>);

struct Entry {
    admission: Arc<TurnCalls>,
    state: Mutex<EntryState>,
}

#[derive(Default)]
struct EntryState {
    publisher: Option<Publisher>,
    result: Option<Completion>,
    terminal: bool,
}

struct Publisher {
    decision: Option<oneshot::Sender<Completion>>,
    progress: watch::Receiver<Stage>,
}

struct PublicationOwner {
    registry: Arc<Publications>,
    key: Key,
    entry: Arc<Entry>,
}

impl Drop for PublicationOwner {
    fn drop(&mut self) {
        // Synchronous cleanup also runs if an emitter panics or the runtime aborts this task.
        self.registry.remove(&self.key, &self.entry);
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Starting,
    Started,
    Done,
}

pub(crate) struct ExecutionGuard {
    pub(crate) registry: Option<Arc<Publications>>,
    pub(crate) turn_id: String,
    pub(crate) call_id: String,
}

impl Drop for ExecutionGuard {
    fn drop(&mut self) {
        if let Some(registry) = &self.registry {
            let outcome = if std::thread::panicking() {
                ToolCallOutcome::Failed {
                    handler_executed: true,
                }
            } else {
                ToolCallOutcome::Aborted
            };
            registry.decide(
                &self.turn_id,
                &self.call_id,
                outcome,
                ToolResultDisposition::Unchanged,
            );
        }
    }
}

impl Publications {
    pub(crate) fn admit(&self, turn: &Arc<TurnCalls>, turn_id: &str, call_id: &str) {
        let mut admission = turn.0.lock().unwrap();
        let mut entries = self.0.lock().unwrap();
        let key = (turn_id.to_owned(), call_id.to_owned());
        if admission.closed || entries.len() >= MAX_PENDING || entries.contains_key(&key) {
            return;
        }
        admission.keys.insert(key.clone());
        entries.insert(
            key,
            Arc::new(Entry {
                admission: Arc::clone(turn),
                state: Mutex::new(EntryState::default()),
            }),
        );
    }

    pub(crate) async fn begin(
        self: &Arc<Self>,
        call: &ToolCall<'_>,
        tool: FileTool,
        arguments: &str,
    ) -> Result<(), FunctionCallError> {
        let key = (call.turn_id.clone(), call.call_id.clone());
        let entry = self.0.lock().unwrap().get(&key).cloned().ok_or_else(|| {
            error("File tool was cancelled or its pending-call limit was reached")
        })?;
        let (mut progress, receiver, signal) = {
            let admission = entry.admission.0.lock().unwrap();
            let mut state = entry.state.lock().unwrap();
            if admission.closed || state.terminal || state.publisher.is_some() {
                return Err(error("File tool was cancelled or already started"));
            }
            let (decision, receiver) = oneshot::channel();
            let (signal, progress) = watch::channel(Stage::Starting);
            state.publisher = Some(Publisher {
                decision: Some(decision),
                progress: progress.clone(),
            });
            (progress, receiver, signal)
        };
        let owner = PublicationOwner {
            registry: Arc::clone(self),
            key,
            entry: Arc::clone(&entry),
        };
        let emitter = Arc::clone(&call.turn_item_emitter);
        let item = started_item(&call.call_id, tool, arguments);
        let started = Instant::now();
        // Spawn only after releasing all guards: a closed runtime may synchronously
        // destroy the future, invoking the owner's cleanup on this same thread.
        tokio::spawn(async move {
            emitter.emit_started(envelope(item.clone())).await;
            signal.send_replace(Stage::Started);
            if let Ok(completion) = receiver.await {
                let duration = i64::try_from(started.elapsed().as_millis()).ok();
                emitter
                    .emit_completed(envelope(completion.apply(item, duration)))
                    .await;
            }
            drop(owner);
            signal.send_replace(Stage::Done);
        });
        while *progress.borrow_and_update() == Stage::Starting {
            if progress.changed().await.is_err() {
                return Err(error("File tool publication stopped before execution"));
            }
        }
        let admission = entry.admission.0.lock().unwrap();
        if admission.closed
            || *progress.borrow() == Stage::Done
            || entry.state.lock().unwrap().terminal
        {
            return Err(error("File tool was cancelled before execution"));
        }
        Ok(())
    }

    pub(crate) fn stage(&self, turn_id: &str, call_id: &str, result: Completion) {
        if let Some(entry) = self
            .0
            .lock()
            .unwrap()
            .get(&(turn_id.to_owned(), call_id.to_owned()))
            .cloned()
        {
            let mut state = entry.state.lock().unwrap();
            if !state.terminal {
                state.result = Some(result);
            }
        }
    }

    pub(crate) async fn finish(
        &self,
        turn_id: &str,
        call_id: &str,
        outcome: ToolCallOutcome,
        disposition: ToolResultDisposition<'_>,
    ) {
        if let Some(mut progress) = self.decide(turn_id, call_id, outcome, disposition) {
            while *progress.borrow_and_update() != Stage::Done {
                if progress.changed().await.is_err() {
                    break;
                }
            }
        }
    }

    pub(crate) fn abandon(&self, turn_id: &str, call_id: &str) {
        let _ = self.decide(
            turn_id,
            call_id,
            ToolCallOutcome::Aborted,
            ToolResultDisposition::Unchanged,
        );
    }

    fn decide(
        &self,
        turn_id: &str,
        call_id: &str,
        outcome: ToolCallOutcome,
        disposition: ToolResultDisposition<'_>,
    ) -> Option<watch::Receiver<Stage>> {
        let key = (turn_id.to_owned(), call_id.to_owned());
        let entry = self.0.lock().unwrap().get(&key).cloned();
        let entry = entry?;
        let progress = {
            let mut state = entry.state.lock().unwrap();
            if !state.terminal {
                state.terminal = true;
                let completion = match outcome {
                    ToolCallOutcome::Aborted => {
                        Completion::new(/*success*/ false, "File tool interrupted")
                    }
                    ToolCallOutcome::Blocked => {
                        Completion::new(
                            /*success*/ false,
                            "File tool blocked before execution",
                        )
                    }
                    ToolCallOutcome::Completed { .. } | ToolCallOutcome::Failed { .. } => {
                        match disposition {
                            ToolResultDisposition::Rejected(reason) => {
                                Completion::new(/*success*/ false, reason)
                            }
                            ToolResultDisposition::Feedback(feedback) => {
                                Completion::new(/*success*/ true, feedback)
                            }
                            ToolResultDisposition::Unchanged => {
                                state.result.take().unwrap_or_else(|| {
                                    Completion::new(
                                        /*success*/ false,
                                        "File tool produced no result",
                                    )
                                })
                            }
                        }
                    }
                };
                if let Some(publisher) = &mut state.publisher
                    && let Some(decision) = publisher.decision.take()
                {
                    let _ = decision.send(completion);
                }
                state.result = None;
            }
            state
                .publisher
                .as_ref()
                .map(|publisher| publisher.progress.clone())
        };
        if progress.is_none() {
            self.remove(&key, &entry);
        }
        progress
    }

    pub(crate) async fn close(&self, turn: &TurnCalls) {
        let keys = {
            let mut admission = turn.0.lock().unwrap();
            admission.closed = true;
            admission.keys.iter().cloned().collect::<Vec<_>>()
        };
        futures::future::join_all(keys.iter().map(|(turn_id, call_id)| {
            self.finish(
                turn_id,
                call_id,
                ToolCallOutcome::Aborted,
                ToolResultDisposition::Unchanged,
            )
        }))
        .await;
    }

    fn remove(&self, key: &Key, entry: &Arc<Entry>) {
        let mut admission = entry.admission.0.lock().unwrap();
        let mut entries = self.0.lock().unwrap();
        if entries
            .get(key)
            .is_some_and(|current| Arc::ptr_eq(current, entry))
        {
            entries.remove(key);
            admission.keys.remove(key);
        }
    }
}
