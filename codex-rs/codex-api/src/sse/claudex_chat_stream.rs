//! One owned reader. Consumer drop, interrupt, backpressure and all waits share a deadline.
use super::claudex_chat::ClaudexChatDecoder;
use super::claudex_chat_framing::ChatSseFramer;
use crate::common::ResponseEvent;
use crate::common::ResponseStream;
use crate::error::ApiError;
use codex_http_client::ByteStream;
use futures::StreamExt;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::time::Instant;
use tokio::time::timeout_at;

/// Host-supplied transport bounds, revalidated before sending each request.
/// These limits do not attest a remote token quota or Coding Plan entitlement.
#[derive(Clone, Copy)]
pub struct ChatStreamLimits {
    /// Maximum raw response bytes, including comments, fields and delimiters.
    pub raw_bytes: usize,
    /// Maximum raw bytes of a single SSE frame.
    pub frame_bytes: usize,
    /// Maximum interval without observed raw byte progress.
    pub idle: Duration,
    /// Whole request deadline, including HTTP, reading and event backpressure.
    pub deadline: Duration,
}

impl ChatStreamLimits {
    pub(crate) fn validate(self) -> Result<(), ApiError> {
        if self.raw_bytes == 0
            || self.raw_bytes > 512 * 1024
            || self.frame_bytes == 0
            || self.frame_bytes > 65_536
            || self.frame_bytes > self.raw_bytes
            || self.idle.is_zero()
            || self.deadline.is_zero()
            || self.idle > self.deadline
            || self.deadline > Duration::from_secs(600)
        {
            return Err(ApiError::InvalidRequest {
                message: "invalid GLM stream limits".into(),
            });
        }
        Ok(())
    }
}

pub(crate) fn chat_response_stream(
    bytes: ByteStream,
    decoder: ClaudexChatDecoder,
    limits: ChatStreamLimits,
    deadline: Instant,
) -> ResponseStream {
    let (tx, rx_event) = mpsc::channel(32);
    let (interrupt, mut interruption) = oneshot::channel();
    tokio::spawn(async move {
        // Keep a terminal-error slot: a deadline must never leave an error send blocked.
        let Ok(error_slot) = tx.clone().reserve_owned().await else {
            return;
        };
        let result = tokio::select! {
            biased;
            _ = tx.closed() => return,
            Ok(()) = &mut interruption => Err(ApiError::Stream("GLM stream interrupted".into())),
            result = timeout_at(deadline, forward(bytes, decoder, limits, &tx)) => {
                result.unwrap_or_else(|_| Err(ApiError::Stream("GLM stream deadline exceeded".into())))
            }
        };
        if let Err(error) = result {
            error_slot.send(Err(error));
        }
    });
    ResponseStream {
        rx_event,
        upstream_request_id: None,
        interrupt: Some(interrupt),
    }
}

async fn forward(
    mut bytes: ByteStream,
    mut decoder: ClaudexChatDecoder,
    limits: ChatStreamLimits,
    tx: &mpsc::Sender<Result<ResponseEvent, ApiError>>,
) -> Result<(), ApiError> {
    let mut framing = ChatSseFramer::new(limits.raw_bytes, limits.frame_bytes);
    let mut terminal = Vec::new();
    let mut idle_deadline = Instant::now() + limits.idle;
    loop {
        // Even an immediately-ready stream of empty/keepalive chunks must remain cancellable.
        tokio::task::yield_now().await;
        if Instant::now() >= idle_deadline {
            return Err(ApiError::Stream("GLM stream idle timeout".into()));
        }
        let chunk = timeout_at(idle_deadline, bytes.next())
            .await
            .map_err(|_| ApiError::Stream("GLM stream idle timeout".into()))?;
        let Some(chunk) = chunk else {
            break;
        };
        let chunk = chunk.map_err(|_| ApiError::Stream("GLM stream transport failed".into()))?;
        if !chunk.is_empty() {
            idle_deadline = Instant::now() + limits.idle;
        }
        for data in framing.push(&chunk)? {
            let events = decoder.push_data(&data)?;
            if data == "[DONE]" {
                // Withhold tool dispatch/Completed until EOF proves no trailing data/fault.
                terminal = events;
            } else {
                for event in events {
                    tx.send(Ok(event))
                        .await
                        .map_err(|_| ApiError::Stream("GLM consumer closed".into()))?;
                }
            }
        }
    }
    framing.finish_eof()?;
    decoder.finish_eof()?;
    for event in terminal {
        tx.send(Ok(event))
            .await
            .map_err(|_| ApiError::Stream("GLM consumer closed".into()))?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "claudex_chat_stream_tests.rs"]
mod tests;
