//! HTTP surface of the loopback protocol gateway.
//!
//! | Endpoint | Behaviour |
//! |----------|-----------|
//! | `POST /v1/responses` | Main entry: translate (chat upstream) or pass through (responses upstream) |
//! | `POST /v1/responses/compact` | Same translation path; Codex's compaction request is a normal Responses body |
//! | `GET /v1/models` | Upstream model list, used by the UI's connectivity probe |
//! | `GET /health` | Readiness probe |
//!
//! Security posture: the listener already binds `127.0.0.1`, but every handler re-checks
//! the peer address so a mis-bound socket can never serve a remote caller. Neither
//! request nor response bodies are logged or persisted — only method, path, status and
//! error text.
//!
//! # Header forwarding
//!
//! Rewriting the wire protocol MUST NOT be confused with rewriting the HTTP envelope.
//! Codex talks to the gateway as if it were the provider, so any header Codex emitted
//! for the *provider* still has to arrive there. A concrete failure this caused in the
//! field: OpenCode Go rejects requests without `x-opencode-session`
//! (`MissingSessionID`) because the header drives upstream routing and prompt caching,
//! and its documentation explicitly asks proxies to preserve it.
//!
//! The forwarding policy is therefore:
//!
//! - **Forward** every inbound header except the ones below, so provider-specific
//!   session, affinity, organisation and beta headers survive untouched.
//! - **Never forward** hop-by-hop headers (they describe *this* connection, not the
//!   request), nor credentials and body metadata that the gateway sets itself.
//! - **Synthesise** a session id only when the upstream needs one and Codex sent none,
//!   so the gateway still works with Codex builds that omit it.
//! - **Never leak** the HTTP client's own identity upstream: `reqwest`'s default
//!   `User-Agent` is replaced by Codex's real one, or by a `codexq/<version>` marker.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::header::{AUTHORIZATION, CACHE_CONTROL, CONTENT_TYPE, USER_AGENT};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{future, StreamExt};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::route::{self, UpstreamRoute};
use super::stream::ChatSseToResponsesConverter;
use super::translate;
use super::{new_response_id, upstream_client};

/// `User-Agent` used when Codex did not supply a usable one.
const GATEWAY_USER_AGENT: &str = concat!("codexq/", env!("CARGO_PKG_VERSION"));

/// OpenCode Go's conversation session header.
///
/// Documented as the header that lets Go route a turn to a warm backend and reuse its
/// prompt cache; requests without it are rejected outright.
const OPENCODE_SESSION_HEADER: &str = "x-opencode-session";

/// Host fragment identifying upstreams that require the session header.
const OPENCODE_HOST: &str = "opencode.ai";

/// Client session header names checked in priority order.
///
/// Header map iteration order is not insertion order, so a bare name-shape scan could pick
/// a different value on different runs when a client sends several session-ish headers.
/// These well-known names are therefore resolved first, deterministically.
const SESSION_HEADER_PREFERENCE: &[&str] = &[
    OPENCODE_SESSION_HEADER,
    "session_id",
    "session-id",
    "x-session-id",
    "conversation_id",
    "conversation-id",
];

/// Inbound headers that must never be copied onto the upstream request.
///
/// Two groups:
///
/// - **Hop-by-hop** (`host` … `accept-encoding`): they describe the Codex↔gateway
///   connection. `host`/`content-length` would additionally be wrong, and reusing the
///   client's `accept-encoding` would ask the upstream for a compressed stream the
///   gateway would have to decode before it could parse SSE.
/// - **Gateway-controlled** (`authorization`, `content-type`, `user-agent`): the gateway
///   authenticates with the provider key from the sandbox and re-serialises the body, so
///   the inbound values are either useless or actively dangerous.
const SKIPPED_REQUEST_HEADERS: &[&str] = &[
    "host",
    "content-length",
    "connection",
    "keep-alive",
    "transfer-encoding",
    "upgrade",
    "te",
    "trailer",
    "proxy-authorization",
    "proxy-connection",
    "accept-encoding",
    "authorization",
    "content-type",
    "user-agent",
];

/// Builds the gateway router.
#[must_use]
pub fn router() -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/models", get(models))
        .route("/v1/responses", post(responses))
        .route("/v1/responses/compact", post(responses))
}

/// Readiness probe. Reports the active route without leaking the API key.
async fn health(ConnectInfo(peer): ConnectInfo<SocketAddr>) -> Response {
    if let Err(response) = ensure_loopback(peer) {
        return response;
    }
    let route = route::resolve_active_route()
        .map(|route| {
            json!({
                "provider_id": route.provider_id,
                "protocol": route.protocol,
            })
        })
        .unwrap_or(Value::Null);
    Json(json!({
        "status": "ok",
        "gateway": "codexq",
        "version": env!("CARGO_PKG_VERSION"),
        "route": route,
    }))
    .into_response()
}

/// Forwards the upstream model list so the UI can probe connectivity through the gateway.
async fn models(ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap) -> Response {
    if let Err(response) = ensure_loopback(peer) {
        return response;
    }
    let route = match route::resolve_active_route() {
        Ok(route) => route,
        Err(message) => return error_response(StatusCode::BAD_GATEWAY, "no_active_route", &message),
    };
    if let Err(response) = ensure_active_provider_key(&headers, &route) {
        return response;
    }
    let client = match upstream_client() {
        Ok(client) => client,
        Err(message) => {
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "client_init_failed", &message)
        }
    };

    let builder = apply_forwarded_headers(
        client.get(route.models_url()),
        &headers,
        &route.base_url,
        None,
    )
    .header(AUTHORIZATION, format!("Bearer {}", route.api_key));

    match builder.send().await {
        Ok(upstream) if upstream.status().is_success() => match upstream.json::<Value>().await {
            Ok(value) => Json(value).into_response(),
            Err(err) => error_response(
                StatusCode::BAD_GATEWAY,
                "invalid_upstream_body",
                &format!("Upstream returned a non-JSON model list: {err}"),
            ),
        },
        Ok(upstream) => {
            let status = upstream.status();
            let body = upstream.text().await.unwrap_or_default();
            error_response(
                map_upstream_status(status),
                "upstream_error",
                &format!("Upstream model list returned {status}: {}", truncate(&body, 400)),
            )
        }
        Err(err) => error_response(
            StatusCode::BAD_GATEWAY,
            "upstream_unreachable",
            &format!("Could not reach the upstream: {err}"),
        ),
    }
}

/// Main request handler for `POST /v1/responses` and `/v1/responses/compact`.
async fn responses(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if let Err(response) = ensure_loopback(peer) {
        return response;
    }

    let request: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(err) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request_body",
                &format!("Request body is not valid JSON: {err}"),
            )
        }
    };

    // Protocol is a per-model property, so it is resolved from the request body. One
    // upstream can serve some models natively on `/v1/responses` while others only exist
    // on `/v1/chat/completions`. Resolving here rather than when `config.toml` was written
    // is what makes Codex's in-session model switching work: CodexQ only rewrites the
    // config during an explicit slot switch, but Codex can change models at any time.
    let model = request.get("model").and_then(Value::as_str).unwrap_or_default();

    let route = match route::resolve_active_route_for_model(model) {
        Ok(route) => route,
        Err(message) => return error_response(StatusCode::BAD_GATEWAY, "no_active_route", &message),
    };
    if let Err(response) = ensure_active_provider_key(&headers, &route) {
        return response;
    }

    let wants_stream = request
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    if !route.is_chat() {
        return passthrough_responses(&route, &request, &headers, &body, wants_stream).await;
    }

    let chat_request =
        match translate::responses_to_chat_completions(&request, &route.base_url, wants_stream) {
            Ok(value) => value,
            Err(message) => {
                return error_response(StatusCode::BAD_REQUEST, "translation_failed", &message)
            }
        };

    let client = match upstream_client() {
        Ok(client) => client,
        Err(message) => {
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "client_init_failed", &message)
        }
    };

    let target = route.chat_completions_url();
    let builder = apply_forwarded_headers(
        client.post(&target),
        &headers,
        &route.base_url,
        Some(&request),
    )
    .header(AUTHORIZATION, format!("Bearer {}", route.api_key))
    .header(CONTENT_TYPE, "application/json")
    .json(&chat_request);

    let upstream = match builder.send().await {
        Ok(response) => response,
        Err(err) => {
            return error_response(
                StatusCode::BAD_GATEWAY,
                "upstream_unreachable",
                &format!("Could not reach provider '{}': {err}", route.provider_name),
            )
        }
    };

    let status = upstream.status();
    if !status.is_success() {
        let body = upstream.text().await.unwrap_or_default();
        return error_response(
            map_upstream_status(status),
            "upstream_error",
            &format!(
                "Provider '{}' returned {status} for {target} (model '{model}'): {}",
                route.provider_name,
                truncate(&body, 600)
            ),
        );
    }

    if wants_stream {
        stream_translated_response(upstream, &request)
    } else {
        match upstream.json::<Value>().await {
            Ok(value) => match translate::chat_completion_to_response(&value, &request) {
                Ok(response) => Json(response).into_response(),
                Err(message) => {
                    error_response(StatusCode::BAD_GATEWAY, "translation_failed", &message)
                }
            },
            Err(err) => error_response(
                StatusCode::BAD_GATEWAY,
                "invalid_upstream_body",
                &format!("Upstream returned a non-JSON response: {err}"),
            ),
        }
    }
}

/// Streams a Chat Completions SSE turn back to Codex as Responses SSE events.
fn stream_translated_response(upstream: reqwest::Response, request: &Value) -> Response {
    let model = request
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let converter = ChatSseToResponsesConverter::new(
        new_response_id(),
        model,
        chrono::Utc::now().timestamp(),
    );
    let converter = Arc::new(Mutex::new(converter));
    let opening = lock_converter(&converter).begin();

    let scan_state = Arc::clone(&converter);
    let scan = upstream.bytes_stream().scan(scan_state, |state, item| {
            let mut guard = state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let frames = match item {
                Ok(bytes) => guard.feed_bytes(&bytes),
                Err(err) => {
                    log::warn!("protocol gateway: upstream stream ended abnormally: {err}");
                    guard.fail(&err.to_string())
                }
            };
            drop(guard);
            future::ready(Some(frames))
        });

    // The upstream may close without sending `[DONE]`; the tail guarantees Codex always
    // receives exactly one terminal event.
    let tail_converter = Arc::clone(&converter);
    let tail = futures_util::stream::once(async move {
        lock_converter(&tail_converter).finish()
    });

    // Every stage yields a batch of frames so the stages can be chained before the
    // batches are flattened into individual SSE frames.
    let stream = futures_util::stream::iter(vec![opening])
        .chain(scan)
        .chain(tail)
        .flat_map(|frames| futures_util::stream::iter(frames))
        .map(Ok::<String, std::io::Error>);

    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "text/event-stream")
        .header(CACHE_CONTROL, "no-cache")
        .header("X-Accel-Buffering", "no")
        .body(Body::from_stream(stream))
        .unwrap_or_else(|err| {
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "stream_build_failed",
                &format!("Failed to build the SSE response: {err}"),
            )
        })
}

/// Forwards a Responses request verbatim when the upstream already speaks Responses.
///
/// This keeps the gateway usable as a single stable `base_url` for every provider type
/// without altering traffic for native Responses endpoints. The body is passed through
/// byte for byte; only the HTTP envelope is rebuilt (headers forwarded, credentials
/// swapped) so the upstream still sees the session and affinity headers Codex emitted.
async fn passthrough_responses(
    route: &UpstreamRoute,
    request: &Value,
    headers: &HeaderMap,
    body: &[u8],
    wants_stream: bool,
) -> Response {
    let client = match upstream_client() {
        Ok(client) => client,
        Err(message) => {
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "client_init_failed", &message)
        }
    };

    let builder = apply_forwarded_headers(
        client.post(route.responses_url()),
        headers,
        &route.base_url,
        Some(request),
    )
    .header(AUTHORIZATION, format!("Bearer {}", route.api_key))
    .header(CONTENT_TYPE, "application/json")
    .body(body.to_vec());

    let upstream = match builder.send().await {
        Ok(response) => response,
        Err(err) => {
            return error_response(
                StatusCode::BAD_GATEWAY,
                "upstream_unreachable",
                &format!("Could not reach provider '{}': {err}", route.provider_name),
            )
        }
    };

    let status = upstream.status();
    let content_type = upstream
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/json")
        .to_string();

    if status.is_success() && wants_stream && content_type.contains("text/event-stream") {
        let stream = upstream
            .bytes_stream()
            .map(|item| item.map_err(std::io::Error::other));
        return Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, "text/event-stream")
            .header(CACHE_CONTROL, "no-cache")
            .body(Body::from_stream(stream))
            .unwrap_or_else(|err| {
                error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "stream_build_failed",
                    &format!("Failed to build the SSE response: {err}"),
                )
            });
    }

    let upstream_status = map_upstream_status(status);
    match upstream.bytes().await {
        Ok(bytes) => Response::builder()
            .status(upstream_status)
            .header(CONTENT_TYPE, content_type)
            .body(Body::from(bytes))
            .unwrap_or_else(|err| {
                error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "response_build_failed",
                    &format!("Failed to build the response: {err}"),
                )
            }),
        Err(err) => error_response(
            StatusCode::BAD_GATEWAY,
            "upstream_body_failed",
            &format!("Failed to read the upstream response: {err}"),
        ),
    }
}

/// Rewrites the outbound HTTP envelope for an upstream call.
///
/// Copies Codex's own headers onto the upstream request (see the module docs for the
/// policy), guarantees a real `User-Agent`, and injects a session id for upstreams that
/// require one but received none.
///
/// # Arguments
///
/// * `builder` - Partially built request; the caller appends credentials and body.
/// * `headers` - Inbound headers as received from Codex.
/// * `base_url` - Upstream base URL, used to detect session-header requirements.
/// * `request` - Parsed Responses body, used to derive a stable session id.
fn apply_forwarded_headers(
    mut builder: reqwest::RequestBuilder,
    headers: &HeaderMap,
    base_url: &str,
    request: Option<&Value>,
) -> reqwest::RequestBuilder {
    let forwarded = forwardable_headers(headers, base_url, request);
    for (name, value) in &forwarded {
        builder = builder.header(name.as_str(), value.as_bytes());
    }
    builder
}

/// Builds the header set to place on the upstream request.
///
/// # Arguments
///
/// * `headers` - Inbound headers as received from Codex.
/// * `base_url` - Upstream base URL, used to detect session-header requirements.
/// * `request` - Parsed Responses body, used to derive a stable session id.
#[must_use]
fn forwardable_headers(
    headers: &HeaderMap,
    base_url: &str,
    request: Option<&Value>,
) -> HeaderMap {
    let mut forwarded = HeaderMap::new();
    for (name, value) in headers {
        if SKIPPED_REQUEST_HEADERS.contains(&name.as_str()) {
            continue;
        }
        forwarded.append(name.clone(), value.clone());
    }

    if let Ok(user_agent) = HeaderValue::try_from(upstream_user_agent(headers)) {
        forwarded.insert(USER_AGENT, user_agent);
    }

    if needs_session_header(base_url) && !forwarded.contains_key(OPENCODE_SESSION_HEADER) {
        // Prefer the client's own session header over a derived one. OpenCode states it
        // recognises Codex's native session header, so mirroring that value keeps a single
        // session identity on the wire; minting an independent id alongside it would give
        // the upstream two competing answers to "which conversation is this?".
        let session = inbound_session_id(headers).unwrap_or_else(|| derive_session_id(request));
        if let Ok(value) = HeaderValue::try_from(session.as_str()) {
            forwarded.insert(HeaderName::from_static(OPENCODE_SESSION_HEADER), value);
        }
    }

    forwarded
}

/// Extracts a client's native session identifier from the inbound headers.
///
/// Resolves well-known names first (see [`SESSION_HEADER_PREFERENCE`]) so the result is
/// deterministic, then falls back to a name-shape scan: every coding agent names its header
/// differently (`session_id`, `x-session-id`, `conversation_id`, …) and the upstream's
/// documented requirement is simply that the session survive the hop. The first non-empty
/// match wins. Returns `None` when the client sent none, which is the case for the Codex
/// builds that motivated this code.
#[must_use]
fn inbound_session_id(headers: &HeaderMap) -> Option<String> {
    for name in SESSION_HEADER_PREFERENCE {
        if let Some(text) = headers.get(*name).and_then(non_empty_header) {
            return Some(text);
        }
    }

    for (name, value) in headers {
        let name = name.as_str();
        if !name.contains("session") && name != "conversation_id" && name != "conversation-id" {
            continue;
        }
        if let Ok(text) = value.to_str() {
            let text = text.trim();
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
    }
    None
}

/// Returns a trimmed header value, or `None` when it is absent or blank.
fn non_empty_header(value: &HeaderValue) -> Option<String> {
    let text = value.to_str().ok()?.trim();
    if text.is_empty() {
        return None;
    }
    Some(text.to_string())
}

/// Picks the `User-Agent` to present upstream.
///
/// Codex's own identifier is forwarded when available so upstreams that gate on agent
/// identity keep working. An absent or client-default (`reqwest`) value is replaced:
/// leaking the gateway's HTTP library would make the request look like neither Codex nor
/// a browser, and some gateways reject unknown agents.
#[must_use]
fn upstream_user_agent(headers: &HeaderMap) -> &str {
    headers
        .get(USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.to_ascii_lowercase().starts_with("reqwest"))
        .unwrap_or(GATEWAY_USER_AGENT)
}

/// Returns whether the upstream rejects requests that lack a session header.
#[must_use]
fn needs_session_header(base_url: &str) -> bool {
    base_url.to_ascii_lowercase().contains(OPENCODE_HOST)
}

/// Derives a session id for an upstream that received none.
///
/// Preference order, most authoritative first:
///
/// 1. `prompt_cache_key` — Codex's own conversation identifier when it sends one.
/// 2. `conversation.id` — the explicit conversation handle.
/// 3. A digest of `model` + `instructions`.
///
/// The digest deliberately excludes `input`: that array grows with every turn of a
/// conversation, so hashing it would mint a fresh session id per request and defeat the
/// routing and prompt-cache reuse the header exists for. `model` and `instructions` are
/// stable for the life of a conversation, which makes the derived id stable too.
#[must_use]
fn derive_session_id(request: Option<&Value>) -> String {
    let Some(request) = request else {
        return format!("codexq-{}", std::process::id());
    };

    if let Some(key) = request.get("prompt_cache_key").and_then(Value::as_str) {
        let key = key.trim();
        if !key.is_empty() {
            return key.to_string();
        }
    }

    if let Some(id) = request
        .get("conversation")
        .and_then(|conversation| conversation.get("id"))
        .and_then(Value::as_str)
    {
        let id = id.trim();
        if !id.is_empty() {
            return id.to_string();
        }
    }

    let mut hasher = Sha256::new();
    hasher.update(
        request
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .as_bytes(),
    );
    hasher.update(b"\x1f");
    hasher.update(
        request
            .get("instructions")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .as_bytes(),
    );
    let fingerprint: String = hasher
        .finalize()
        .iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("codexq-{fingerprint}")
}

/// Maps an upstream status onto the status Codex should see.
///
/// `401`/`403` become `502`: Codex treats them as *its own* credentials being rejected
/// and escalates to an official-login refresh, which is wrong when the rejection is the
/// third-party provider's and would silently switch the user to another account. All
/// other statuses pass through untouched so `400` diagnostics and `429` backoff behave
/// normally.
fn map_upstream_status(status: reqwest::StatusCode) -> StatusCode {
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return StatusCode::BAD_GATEWAY;
    }
    if status.is_server_error() {
        return StatusCode::BAD_GATEWAY;
    }
    StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY)
}

/// Refuses stale Codex sessions whose saved key belongs to a previous provider slot.
fn ensure_active_provider_key(headers: &HeaderMap, route: &UpstreamRoute) -> Result<(), Response> {
    let expected = format!("Bearer {}", route.api_key);
    if headers
        .get(AUTHORIZATION)
        .is_some_and(|value| value.as_bytes() == expected.as_bytes())
    {
        return Ok(());
    }
    Err(error_response(
        StatusCode::BAD_GATEWAY,
        "stale_provider_route",
        "This Codex session's provider credentials do not match the active CodexQ provider.",
    ))
}

/// Rejects any request whose peer is not on the loopback interface.
fn ensure_loopback(peer: SocketAddr) -> Result<(), Response> {
    if peer.ip().is_loopback() {
        return Ok(());
    }
    log::warn!("protocol gateway: rejected non-loopback peer");
    Err(error_response(
        StatusCode::FORBIDDEN,
        "loopback_only",
        "The CodexQ protocol gateway only accepts connections from the local machine.",
    ))
}

/// Builds a Responses-style error envelope so Codex surfaces a usable message.
fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(json!({
            "error": {
                "message": message,
                "type": "codexq_gateway_error",
                "code": code,
            }
        })),
    )
        .into_response()
}

/// Locks the shared converter, recovering from poisoning instead of panicking mid-stream.
fn lock_converter(
    converter: &Arc<Mutex<ChatSseToResponsesConverter>>,
) -> std::sync::MutexGuard<'_, ChatSseToResponsesConverter> {
    converter
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Truncates an upstream error body so a stray HTML error page cannot flood the UI.
fn truncate(text: &str, limit: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= limit {
        return trimmed.to_string();
    }
    let clipped: String = trimmed.chars().take(limit).collect();
    format!("{clipped}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::routing::post;

    #[tokio::test]
    async fn streamed_passthrough_preserves_upstream_error_status() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test upstream");
        let address = listener.local_addr().expect("local address");
        let app = Router::new().route(
            "/v1/responses",
            post(|| async {
                (
                    StatusCode::UNAUTHORIZED,
                    [(CONTENT_TYPE, "text/event-stream")],
                    "event: error\ndata: unauthorized\n\n",
                )
            }),
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        let route = UpstreamRoute {
            provider_id: "test".into(),
            provider_name: "Test".into(),
            base_url: format!("http://{address}/v1"),
            protocol: "responses".into(),
            api_key: "test-key".into(),
        };
        let request = json!({"model": "test-model", "stream": true});
        let body = serde_json::to_vec(&request).expect("request body");
        let response = passthrough_responses(&route, &request, &HeaderMap::new(), &body, true).await;
        server.abort();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    }

    #[test]
    fn stale_provider_credentials_cannot_route_to_the_new_active_provider() {
        let route = UpstreamRoute {
            provider_id: "new".into(),
            provider_name: "New".into(),
            base_url: "https://example.com/v1".into(),
            protocol: "chat".into(),
            api_key: "new-key".into(),
        };
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer old-key"));
        assert_eq!(
            ensure_active_provider_key(&headers, &route).unwrap_err().status(),
            StatusCode::BAD_GATEWAY
        );
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer new-key"));
        assert!(ensure_active_provider_key(&headers, &route).is_ok());
    }

    #[test]
    fn non_loopback_peers_are_rejected() {
        let peer: SocketAddr = "10.0.0.7:51234".parse().expect("addr");
        assert!(ensure_loopback(peer).is_err());
    }

    #[test]
    fn loopback_peers_are_accepted() {
        let peer: SocketAddr = "127.0.0.1:51234".parse().expect("addr");
        assert!(ensure_loopback(peer).is_ok());
    }

    #[test]
    fn truncate_keeps_short_text_intact() {
        assert_eq!(truncate("  short  ", 10), "short");
    }

    #[test]
    fn truncate_clips_long_text_on_char_boundaries() {
        let long = "错".repeat(20);
        let clipped = truncate(&long, 5);
        assert_eq!(clipped.chars().count(), 6);
        assert!(clipped.ends_with('…'));
    }

    #[test]
    fn session_header_is_forwarded_and_hop_headers_are_dropped() {
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static(OPENCODE_SESSION_HEADER),
            HeaderValue::from_static("sess-123"),
        );
        headers.insert(
            HeaderName::from_static("x-trace-id"),
            HeaderValue::from_static("trace-1"),
        );
        headers.insert(
            axum::http::header::CONTENT_LENGTH,
            HeaderValue::from_static("42"),
        );
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer inbound"));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let forwarded = forwardable_headers(&headers, "https://example.com/v1", None);

        assert_eq!(
            forwarded
                .get(OPENCODE_SESSION_HEADER)
                .and_then(|value| value.to_str().ok()),
            Some("sess-123"),
            "a provider-specific session header must survive the hop"
        );
        assert_eq!(
            forwarded
                .get("x-trace-id")
                .and_then(|value| value.to_str().ok()),
            Some("trace-1"),
            "unknown headers are forwarded by default"
        );
        assert!(forwarded.get(axum::http::header::CONTENT_LENGTH).is_none());
        assert!(forwarded.get(AUTHORIZATION).is_none());
        assert!(forwarded.get(CONTENT_TYPE).is_none());
    }

    #[test]
    fn session_header_is_synthesised_for_upstreams_that_need_one() {
        let request = json!({ "model": "grok-4.7", "instructions": "be nice" });
        let forwarded =
            forwardable_headers(&HeaderMap::new(), "https://opencode.ai/zen/go/v1", Some(&request));

        let session = forwarded
            .get(OPENCODE_SESSION_HEADER)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        assert!(!session.is_empty());
        assert_eq!(session, derive_session_id(Some(&request)));
    }

    #[test]
    fn client_session_header_is_mirrored_into_the_opencode_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("session_id"),
            HeaderValue::from_static("native-session-7"),
        );
        let request = json!({ "model": "grok-4.7", "instructions": "be nice" });

        let forwarded =
            forwardable_headers(&headers, "https://opencode.ai/zen/go/v1", Some(&request));

        assert_eq!(
            forwarded
                .get(OPENCODE_SESSION_HEADER)
                .and_then(|value| value.to_str().ok()),
            Some("native-session-7"),
            "the client's own session id must win over a derived one"
        );
        assert_eq!(
            forwarded
                .get("session_id")
                .and_then(|value| value.to_str().ok()),
            Some("native-session-7"),
            "the native header is still preserved verbatim"
        );
        assert_ne!(
            forwarded
                .get(OPENCODE_SESSION_HEADER)
                .and_then(|value| value.to_str().ok()),
            Some(derive_session_id(Some(&request)).as_str()),
            "a derived id must not be minted when the client already has one"
        );
    }

    #[test]
    fn well_known_session_header_wins_over_a_shape_match() {
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("x-internal-session-trace"),
            HeaderValue::from_static("trace-should-lose"),
        );
        headers.insert(
            HeaderName::from_static("session_id"),
            HeaderValue::from_static("preferred-session"),
        );

        assert_eq!(
            inbound_session_id(&headers),
            Some("preferred-session".to_string()),
            "iteration order of the header map must not decide the session id"
        );
    }

    #[test]
    fn blank_session_headers_are_ignored() {
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("session_id"),
            HeaderValue::from_static("   "),
        );
        assert_eq!(inbound_session_id(&headers), None);
        assert_eq!(inbound_session_id(&HeaderMap::new()), None);
    }

    #[test]
    fn other_upstreams_get_no_session_header() {
        let request = json!({ "model": "deepseek-v4-flash" });
        let forwarded =
            forwardable_headers(&HeaderMap::new(), "https://api.deepseek.com/v1", Some(&request));
        assert!(forwarded.get(OPENCODE_SESSION_HEADER).is_none());
    }

    #[test]
    fn user_agent_prefers_the_client_and_never_leaks_reqwest() {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static("codex_cli_rs/0.148.0"));
        assert_eq!(upstream_user_agent(&headers), "codex_cli_rs/0.148.0");

        let mut default_agent = HeaderMap::new();
        default_agent.insert(USER_AGENT, HeaderValue::from_static("reqwest/0.12.5"));
        assert_eq!(upstream_user_agent(&default_agent), GATEWAY_USER_AGENT);

        assert_eq!(upstream_user_agent(&HeaderMap::new()), GATEWAY_USER_AGENT);
    }

    #[test]
    fn session_id_prefers_the_conversation_cache_key() {
        let request = json!({
            "model": "grok-4.7",
            "prompt_cache_key": "conv-abc",
            "conversation": { "id": "conv-xyz" },
            "instructions": "be nice",
        });
        assert_eq!(derive_session_id(Some(&request)), "conv-abc");
    }

    #[test]
    fn session_id_falls_back_to_a_stable_digest() {
        let first_turn = json!({
            "model": "grok-4.7",
            "instructions": "be nice",
            "input": [{ "type": "message", "role": "user", "content": "turn one" }],
        });
        let second_turn = json!({
            "model": "grok-4.7",
            "instructions": "be nice",
            "input": [
                { "type": "message", "role": "user", "content": "turn one" },
                { "type": "message", "role": "user", "content": "turn two" },
            ],
        });

        let id = derive_session_id(Some(&first_turn));
        assert!(id.starts_with("codexq-"));
        assert_eq!(id, derive_session_id(Some(&first_turn)));
        assert_eq!(
            id,
            derive_session_id(Some(&second_turn)),
            "a growing input array must not rotate the session id"
        );

        let other_model = json!({ "model": "gpt-5.6-luna", "instructions": "be nice" });
        assert_ne!(id, derive_session_id(Some(&other_model)));
    }

    #[test]
    fn session_id_without_a_request_is_still_non_empty() {
        assert!(!derive_session_id(None).is_empty());
    }

    #[test]
    fn upstream_client_errors_survive_but_auth_errors_do_not() {
        assert_eq!(
            map_upstream_status(reqwest::StatusCode::BAD_REQUEST),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            map_upstream_status(reqwest::StatusCode::TOO_MANY_REQUESTS),
            StatusCode::TOO_MANY_REQUESTS,
            "429 must reach Codex so its backoff can engage"
        );
        assert_eq!(
            map_upstream_status(reqwest::StatusCode::UNAUTHORIZED),
            StatusCode::BAD_GATEWAY,
            "a provider-side 401 must not look like Codex's own credentials expiring"
        );
        assert_eq!(
            map_upstream_status(reqwest::StatusCode::FORBIDDEN),
            StatusCode::BAD_GATEWAY
        );
        assert_eq!(
            map_upstream_status(reqwest::StatusCode::INTERNAL_SERVER_ERROR),
            StatusCode::BAD_GATEWAY
        );
    }
}
