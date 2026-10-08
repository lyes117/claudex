use super::*;
use bytes::Bytes;
use codex_http_client::TransportError;
use codex_protocol::models::ResponseItem;
use futures::Stream;
use futures::stream;
use serde_json::json;
use std::collections::BTreeSet;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::task::Context;
use std::task::Poll;

fn limits() -> ChatStreamLimits {
    ChatStreamLimits {
        raw_bytes: 512 * 1024,
        frame_bytes: 65_536,
        idle: Duration::from_secs(2),
        deadline: Duration::from_secs(5),
    }
}

fn wire(delta: serde_json::Value, finish: &str) -> String {
    format!(
        "data: {}\n\ndata: [DONE]\n\n",
        json!({"id":"fixture", "model":"glm-5.3",
        "choices":[{"index":0,"delta":delta,"finish_reason":finish}]})
    )
}

fn response(bytes: ByteStream) -> ResponseStream {
    chat_response_stream(
        bytes,
        ClaudexChatDecoder::new(BTreeSet::from(["Read".into()])).unwrap(),
        limits(),
        Instant::now() + limits().deadline,
    )
}

async fn outcome(mut response: ResponseStream) -> (usize, usize, Vec<String>) {
    let mut completed = 0;
    let mut functions = 0;
    let mut errors = Vec::new();
    while let Some(event) = response.next().await {
        match event {
            Ok(ResponseEvent::Completed { .. }) => completed += 1,
            Ok(ResponseEvent::OutputItemDone(ResponseItem::FunctionCall { .. })) => functions += 1,
            Err(error) => errors.push(error.to_string()),
            Ok(_) => {}
        }
    }
    (completed, functions, errors)
}

#[tokio::test]
async fn bytewise_unicode_and_tools_complete_only_on_valid_eof() {
    for delta in [
        json!({"role":"assistant","content":"é🦀"}),
        json!({"tool_calls":[{
            "index":0,"id":"call-1","type":"function","function":{"name":"Read","arguments":"{}"}
        }]}),
    ] {
        let tools = delta.get("tool_calls").is_some();
        let wire = wire(delta, if tools { "tool_calls" } else { "stop" });
        let chunks = wire
            .bytes()
            .map(|byte| Ok(Bytes::from(vec![byte])))
            .collect::<Vec<_>>();
        let result = outcome(response(Box::pin(stream::iter(chunks)))).await;
        assert_eq!(result, (1, usize::from(tools), Vec::new()));
    }
}

#[tokio::test]
async fn done_followed_by_data_or_transport_fault_never_dispatches_tools() {
    let wire = wire(
        json!({"tool_calls":[{"index":0,"id":"c1","type":"function",
        "function":{"name":"Read","arguments":"{}"}}]}),
        "tool_calls",
    );
    for tail in [
        Ok(Bytes::from_static(b"data: [DONE]\n\n")),
        Err(TransportError::Network(
            "fixture-provider-body-not-for-diagnostics".into(),
        )),
    ] {
        let result = outcome(response(Box::pin(stream::iter([
            Ok(Bytes::from(wire.clone())),
            tail,
        ]))))
        .await;
        assert_eq!((result.0, result.1, result.2.len()), (0, 0, 1));
        assert!(!result.2[0].contains("fixture-provider-body"));
    }
    let truncated = wire.split("data: [DONE]").next().unwrap().to_owned();
    let result = outcome(response(Box::pin(stream::iter([Ok(Bytes::from(
        truncated,
    ))]))))
    .await;
    assert_eq!((result.0, result.1, result.2.len()), (0, 0, 1));
}

struct PendingReader(Arc<AtomicBool>);
impl Stream for PendingReader {
    type Item = Result<Bytes, TransportError>;
    fn poll_next(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Pending
    }
}
impl Drop for PendingReader {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[tokio::test(start_paused = true)]
async fn idle_timeout_drops_pending_reader_and_returns_one_fixed_error() {
    let dropped = Arc::new(AtomicBool::new(false));
    let result = outcome(response(Box::pin(PendingReader(dropped.clone())))).await;
    assert_eq!(
        result,
        (0, 0, vec!["stream error: GLM stream idle timeout".into()])
    );
    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test(start_paused = true)]
async fn consumer_drop_and_interrupt_cancel_a_pending_reader() {
    for interrupted in [false, true] {
        let dropped = Arc::new(AtomicBool::new(false));
        let mut response = response(Box::pin(PendingReader(dropped.clone())));
        tokio::task::yield_now().await;
        if interrupted {
            response.interrupt.take().unwrap().send(()).unwrap();
            let result = outcome(response).await;
            assert_eq!(
                result,
                (0, 0, vec!["stream error: GLM stream interrupted".into()])
            );
        } else {
            drop(response);
            tokio::task::yield_now().await;
        }
        assert!(dropped.load(Ordering::SeqCst));
    }
}

#[tokio::test(start_paused = true, flavor = "current_thread")]
async fn deadline_during_backpressure_drops_reader_without_blocking_error_delivery() {
    let dropped = Arc::new(AtomicBool::new(false));
    let frames: String = (0..100)
        .map(|_| {
            format!(
                "data: {}\n\n",
                json!({"id":"fixture",
        "model":"glm-5.3","choices":[{"index":0,"delta":{"content":"x"}}]})
            )
        })
        .collect();
    // One chunk: no per-chunk yield or idle check can run between these event sends.
    // On this current-thread runtime, a full channel then attests a pending writer,
    // rather than a yield just after the last available slot was filled.
    let bytes = stream::iter([Ok(Bytes::from(frames))]).chain(PendingReader(dropped.clone()));
    let mut response = response(Box::pin(bytes));
    while response.rx_event.capacity() > 0 {
        tokio::task::yield_now().await;
    }
    tokio::time::advance(Duration::from_secs(6)).await;
    tokio::task::yield_now().await;
    assert!(dropped.load(Ordering::SeqCst));
    let mut errors = Vec::new();
    while let Some(event) = response.next().await {
        if let Err(error) = event {
            errors.push(error.to_string());
        }
    }
    assert_eq!(errors, ["stream error: GLM stream deadline exceeded"]);
}
