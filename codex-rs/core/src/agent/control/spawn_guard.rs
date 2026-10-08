//! Owns a spawned child until its initial input is accepted.

use super::LocalAgentControl;
use crate::codex_thread::CodexThread;
use crate::thread_manager::ThreadManagerState;
use codex_agent_graph_store::ThreadSpawnEdgeStatus;
use std::sync::Arc;
use tokio::task::JoinHandle;
use tracing::warn;

pub(super) struct PendingSpawn {
    state: Arc<ThreadManagerState>,
    child: Option<Arc<CodexThread>>,
    edge_write: Option<JoinHandle<()>>,
    control: LocalAgentControl,
    #[cfg(test)]
    completion: Option<tokio::sync::oneshot::Sender<()>>,
}

impl PendingSpawn {
    pub(super) fn new(
        state: Arc<ThreadManagerState>,
        child: Arc<CodexThread>,
        control: LocalAgentControl,
    ) -> Self {
        Self {
            state,
            child: Some(child),
            edge_write: None,
            control,
            #[cfg(test)]
            completion: None,
        }
    }

    pub(super) fn set_edge_write(&mut self, edge_write: JoinHandle<()>) {
        self.edge_write = Some(edge_write);
    }

    pub(super) async fn wait_for_edge(&mut self) {
        if let Some(edge_write) = self.edge_write.as_mut() {
            assert!(
                edge_write.await.is_ok(),
                "spawn edge write task should complete"
            );
        }
        self.edge_write = None;
    }

    pub(super) fn disarm(mut self) {
        self.child = None;
    }

    #[cfg(test)]
    pub(super) fn notify_when_cleaned(&mut self) -> tokio::sync::oneshot::Receiver<()> {
        let (completion, receiver) = tokio::sync::oneshot::channel();
        self.completion = Some(completion);
        receiver
    }
}

impl Drop for PendingSpawn {
    fn drop(&mut self) {
        let Some(child) = self.child.take() else {
            return;
        };
        let state = Arc::clone(&self.state);
        let edge_write = self.edge_write.take();
        let control = self.control.clone();
        #[cfg(test)]
        let completion = self.completion.take();
        drop(tokio::spawn(async move {
            let id = child.session.thread_id;
            if let Err(error) = child.shutdown_and_wait().await {
                warn!("failed to stop cancelled child spawn: {error}");
            }
            child.wait_until_terminated().await;
            // Native shutdown already retires the local writer. A second discard by
            // stable ID after shutdown could instead destroy a newly resumed writer.
            // Failed persistence shutdown keeps its error/retry semantics; never
            // discard an unidentified writer as a fallback.
            // Finish the original Open write before locking publication and writing Closed.
            if let Some(edge_write) = edge_write {
                let _ = edge_write.await;
            }
            let mut threads = state.threads.write().await;
            let removed = threads
                .get(&id)
                .is_some_and(|current| Arc::ptr_eq(current, &child));
            if removed || !threads.contains_key(&id) {
                // Native spawn/resume publishes its runtime before writing its Open edge.
                // Keeping publication locked here orders Closed before a replacement Open.
                if let Some(store) = state.agent_graph_store()
                    && let Err(error) = store
                        .set_thread_spawn_edge_status(id, ThreadSpawnEdgeStatus::Closed)
                        .await
                {
                    warn!("failed to close cancelled child spawn edge: {error}");
                }
                if removed {
                    threads.remove(&id);
                    control.forget_v2_residency(id);
                    control.runtime.registry.release_spawned_thread(id);
                }
            }
            #[cfg(test)]
            if let Some(completion) = completion {
                let _ = completion.send(());
            }
        }));
    }
}
