//! Bound the complete server-side expansion, including remote filesystem waits.
use codex_app_server_protocol::ClientResponsePayload;
use codex_app_server_protocol::JSONRPCErrorError;
use std::future::Future;
use std::time::Duration;

pub(super) async fn resolve(
    work: impl Future<Output = Result<Option<ClientResponsePayload>, JSONRPCErrorError>>,
) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
    tokio::time::timeout(Duration::from_secs(25), work)
        .await
        .map_err(|_| super::invalid_request("Claude command expansion timed out".to_string()))?
}

#[cfg(test)]
#[path = "claude_command_deadline_tests.rs"]
mod tests;
