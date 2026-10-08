//! Explicit Coding Plan transport. Not a provider, entitlement check, or agent route.
use crate::claudex_chat_request::ClaudexChatRequest;
use crate::common::ResponseStream;
use crate::error::ApiError;
use crate::sse::claudex_chat::ClaudexChatDecoder;
use crate::sse::claudex_chat_stream::ChatStreamLimits;
use crate::sse::claudex_chat_stream::chat_response_stream;
use codex_http_client::ClientRouteClass;
use codex_http_client::HttpClientBuilder;
use codex_http_client::HttpClientFactory;
use codex_http_client::HttpTransport;
use codex_http_client::ProtocolRetryPolicy;
use codex_http_client::Request;
use codex_http_client::ReqwestTransport;
use codex_http_client::TransportError;
use http::HeaderValue;
use http::Method;
use http::StatusCode;
use http::header::ACCEPT;
use http::header::AUTHORIZATION;
use http::header::CONTENT_TYPE;
use serde::Deserialize;
use tokio::time::Instant;
use tokio::time::timeout_at;

const CODING_DESTINATION: &str = "https://api.z.ai/api/coding/paas/v4/chat/completions";

/// Explicit supplier transport; constructing it does not authorize runtime routing.
/// Fields remain private. No Debug, serialization or parent AuthProvider is supported.
pub struct ClaudexChatClient {
    transport: ReqwestTransport,
    destination: String,
    authorization: HeaderValue,
}

impl ClaudexChatClient {
    /// Uses only the dedicated supplier credential and effective host HTTP factory.
    /// The destination must exactly match the Coding Plan endpoint. Never pass parent auth.
    pub fn new(
        factory: &HttpClientFactory,
        destination: &str,
        credential: &str,
    ) -> Result<Self, ApiError> {
        if destination != CODING_DESTINATION {
            return Err(invalid_request());
        }
        if credential.is_empty()
            || credential.len() > 4096
            || !credential.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(invalid_request());
        }
        let mut authorization = HeaderValue::from_str(&format!("Bearer {credential}"))
            .map_err(|_| invalid_request())?;
        authorization.set_sensitive(true);
        let client = HttpClientBuilder::new()
            .without_redirects()
            .protocol_retry_policy(ProtocolRetryPolicy::Never)
            .without_request_logging()
            .build_respecting_outbound_proxy_policy(factory, destination, ClientRouteClass::Other)
            .map_err(|_| invalid_request())?;
        Ok(Self {
            transport: ReqwestTransport::from_http_client(client),
            destination: destination.to_owned(),
            authorization,
        })
    }

    /// Sends exactly the DTO's bounded bytes once, with no retry, fallback, parent headers,
    /// compression or implicit auth discovery. Dropping this future cancels header acquisition.
    pub async fn stream_request(
        &self,
        request: &ClaudexChatRequest,
        limits: ChatStreamLimits,
    ) -> Result<ResponseStream, ApiError> {
        limits.validate()?;
        let deadline = Instant::now() + limits.deadline;
        let body = request.to_json_bytes().map_err(|_| invalid_request())?;
        // Derive names from the exact immutable body, never a second caller-supplied catalogue.
        #[derive(Deserialize)]
        struct Catalogue<'a> {
            #[serde(default, borrow)]
            tools: Vec<Tool<'a>>,
        }
        #[derive(Deserialize)]
        struct Tool<'a> {
            #[serde(borrow)]
            function: Function<'a>,
        }
        #[derive(Deserialize)]
        struct Function<'a> {
            name: &'a str,
        }
        let catalogue: Catalogue<'_> =
            serde_json::from_slice(&body).map_err(|_| invalid_request())?;
        let decoder = ClaudexChatDecoder::new(
            catalogue
                .tools
                .into_iter()
                .map(|tool| tool.function.name.to_owned())
                .collect(),
        )?;
        let mut outbound = Request::new(Method::POST, self.destination.clone()).with_raw_body(body);
        outbound
            .headers
            .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        outbound
            .headers
            .insert(ACCEPT, HeaderValue::from_static("text/event-stream"));
        outbound
            .headers
            .insert(AUTHORIZATION, self.authorization.clone());
        outbound.timeout = Some(limits.deadline);
        outbound.response_body_limit_bytes = Some(limits.raw_bytes);
        let response = timeout_at(deadline, self.transport.stream(outbound))
            .await
            .map_err(|_| ApiError::Stream("GLM request deadline exceeded".into()))?
            .map_err(|error| match error {
                TransportError::Http { status, .. } => ApiError::Api {
                    status,
                    message: "GLM HTTP request refused".into(),
                },
                _ => ApiError::Stream("GLM HTTP transport failed".into()),
            })?;
        if response.status != StatusCode::OK {
            return Err(ApiError::Api {
                status: response.status,
                message: "GLM HTTP request refused".into(),
            });
        }
        let content_type = response
            .headers
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok());
        if !content_type.is_some_and(|value| {
            value
                .split(';')
                .next()
                .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("text/event-stream"))
        }) {
            return Err(ApiError::Stream("GLM response is not SSE".into()));
        }
        Ok(chat_response_stream(
            response.bytes,
            decoder,
            limits,
            deadline,
        ))
    }
}

fn invalid_request() -> ApiError {
    ApiError::InvalidRequest {
        message: "invalid private GLM transport request".into(),
    }
}

#[cfg(test)]
#[path = "claudex_chat_tests.rs"]
mod tests;
