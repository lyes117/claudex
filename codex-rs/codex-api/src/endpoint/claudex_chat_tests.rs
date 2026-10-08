use super::*;
use crate::ResponsesApiRequest;
use crate::common::ResponseEvent;
use codex_http_client::OutboundProxyPolicy;
use futures::StreamExt;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::time::Duration;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

fn request() -> ClaudexChatRequest {
    ClaudexChatRequest::from_responses(
        &ResponsesApiRequest {
            model: "glm-5.3".into(),
            instructions: "exact é\n".into(),
            input: vec![
                serde_json::from_value(json!({"type":"message","role":"user",
            "content":[{"type":"input_text","text":"🦀 text"}]}))
                .unwrap(),
            ],
            tools: None,
            tool_choice: "auto".into(),
            parallel_tool_calls: false,
            reasoning: None,
            store: false,
            stream: true,
            stream_options: None,
            include: Vec::new(),
            service_tier: None,
            prompt_cache_key: None,
            text: None,
            client_metadata: None,
            access_programs: None,
        },
        /*max_output_tokens*/ 4096,
    )
    .unwrap()
}

fn limits() -> ChatStreamLimits {
    ChatStreamLimits {
        raw_bytes: 8192,
        frame_bytes: 4096,
        idle: Duration::from_millis(200),
        deadline: Duration::from_secs(2),
    }
}

// The only HTTP exception is in this test module via private fields; production validator
// accepts exactly the Coding Plan endpoint and cannot select loopback/Pay As You Go.
fn loopback(server: &MockServer) -> ClaudexChatClient {
    let factory = HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault)
        .with_chatgpt_cookies([HeaderValue::from_static("parent-cookie=fixture")]);
    let mut client =
        ClaudexChatClient::new(&factory, CODING_DESTINATION, "dedicated-fixture").unwrap();
    client.destination = format!("{}/api/coding/paas/v4/chat/completions", server.uri());
    client
}

fn text_response() -> String {
    format!(
        "data: {}\n\ndata: [DONE]\n\n",
        json!({"id":"loopback", "model":"glm-5.3",
        "choices":[{"index":0,"delta":{"role":"assistant","content":"é🦀"},"finish_reason":"stop"}]})
    )
}

#[tokio::test]
async fn real_http_posts_exact_bounded_body_and_only_supplier_headers() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/coding/paas/v4/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            text_response().into_bytes(),
            "text/event-stream; charset=utf-8",
        ))
        .expect(1)
        .mount(&server)
        .await;
    let request = request();
    let mut response = loopback(&server)
        .stream_request(&request, limits())
        .await
        .unwrap();
    let mut completed = 0;
    while let Some(event) = response.next().await {
        if matches!(event.unwrap(), ResponseEvent::Completed { .. }) {
            completed += 1;
        }
    }
    assert_eq!(completed, 1);
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].body, request.to_json_bytes().unwrap());
    let mut headers = requests[0].headers.clone();
    headers.remove("host");
    headers.remove("content-length");
    assert_eq!(
        headers,
        http::HeaderMap::from_iter([
            (CONTENT_TYPE, HeaderValue::from_static("application/json")),
            (ACCEPT, HeaderValue::from_static("text/event-stream")),
            (
                AUTHORIZATION,
                HeaderValue::from_static("Bearer dedicated-fixture")
            ),
        ])
    );
}

#[tokio::test]
async fn redirects_transient_errors_wrong_mime_and_oversized_error_bodies_never_retry() {
    let target = MockServer::start().await;
    for template in [
        ResponseTemplate::new(307).insert_header("location", target.uri()),
        ResponseTemplate::new(429).set_body_string("provider-secret-fixture"),
        ResponseTemplate::new(500).set_body_string("provider-secret-fixture"),
        ResponseTemplate::new(200).set_body_raw(b"{}".to_vec(), "application/json"),
        ResponseTemplate::new(500).set_body_bytes(vec![b'x'; 8193]),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(template)
            .expect(1)
            .mount(&server)
            .await;
        let result = loopback(&server).stream_request(&request(), limits()).await;
        let Err(error) = result else {
            panic!("response must fail before SSE");
        };
        assert!(!error.to_string().contains("provider-secret"));
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }
    assert!(target.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn invalid_limits_and_header_deadline_fail_before_stream_spawn() {
    let server = MockServer::start().await;
    let mut invalid = limits();
    invalid.raw_bytes = 0;
    assert!(
        loopback(&server)
            .stream_request(&request(), invalid)
            .await
            .is_err()
    );
    assert!(server.received_requests().await.unwrap().is_empty());
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(500)))
        .expect(1)
        .mount(&server)
        .await;
    let limits = ChatStreamLimits {
        idle: Duration::from_millis(20),
        deadline: Duration::from_millis(40),
        ..limits()
    };
    assert!(
        loopback(&server)
            .stream_request(&request(), limits)
            .await
            .is_err()
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[test]
fn destination_and_auth_validation_refuse_alternate_hosts_paid_paths_and_header_injection() {
    let factory = HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault);
    for destination in [
        "https://api.z.ai/api/paas/v4/chat/completions",
        "https://api.z.ai/api/coding/paas/v4/responses",
        "https://api.z.ai/api/coding/paas/v4/chat/completions?x=y",
        "https://api.z.ai.evil.invalid/api/coding/paas/v4/chat/completions",
        "http://127.0.0.1:1234/api/coding/paas/v4/chat/completions",
        "https://fixture@api.z.ai/api/coding/paas/v4/chat/completions",
    ] {
        assert!(ClaudexChatClient::new(&factory, destination, "fixture").is_err());
    }
    for credential in ["", "space fixture", "fixture\r\ninjected: value", "é"] {
        assert!(ClaudexChatClient::new(&factory, CODING_DESTINATION, credential).is_err());
    }
}
