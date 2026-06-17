use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{header, Method, StatusCode},
    middleware,
    response::{IntoResponse, Json, Response, Sse},
    routing::{get, post},
    Router,
};
use futures::StreamExt;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tower_http::cors::{Any, CorsLayer};

use crate::remote::auth::require_bearer_token;
use crate::remote::types::{
    CommandEnvelope, CreateThreadBody, RemoteCommand, RemoteResponse, SendMessageBody,
    SetModelBody, SetWorkspaceBody,
};

const BODY_LIMIT_BYTES: usize = 1024 * 1024; // 1 MiB
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

pub struct RemoteServerHandle {
    shutdown_tx: Option<oneshot::Sender<()>>,
    runtime: Option<tokio::runtime::Runtime>,
}

impl Drop for RemoteServerHandle {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        // Gracefully shut down the Tokio runtime from a background thread so the
        // GPUI main thread is not blocked. Active SSE connections get up to two
        // seconds to close before the runtime is forcefully terminated.
        if let Some(runtime) = self.runtime.take() {
            std::thread::spawn(move || {
                runtime.shutdown_timeout(Duration::from_secs(2));
            });
        }
    }
}

pub struct AppState {
    pub bearer_token: Option<String>,
    pub command_sender: mpsc::UnboundedSender<CommandEnvelope>,
}

pub fn start(
    port: u16,
    bearer_token: Option<String>,
    command_sender: mpsc::UnboundedSender<CommandEnvelope>,
) -> Result<(RemoteServerHandle, u16), String> {
    let runtime = tokio::runtime::Runtime::new()
        .map_err(|e| format!("failed to create tokio runtime: {}", e))?;

    let state = Arc::new(AppState {
        bearer_token,
        command_sender,
    });

    let addr: SocketAddr = format!("127.0.0.1:{}", port)
        .parse()
        .map_err(|e| format!("invalid bind address: {}", e))?;
    let listener = runtime
        .block_on(tokio::net::TcpListener::bind(addr))
        .map_err(|e| format!("failed to bind {}: {}", addr, e))?;
    let actual_port = listener
        .local_addr()
        .map_err(|e| format!("failed to get local address: {}", e))?
        .port();

    let app = build_router(state);

    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    runtime.spawn(async move {
        if let Err(e) = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
        {
            eprintln!("[remote] axum server error: {}", e);
        }
    });

    Ok((
        RemoteServerHandle {
            shutdown_tx: Some(shutdown_tx),
            runtime: Some(runtime),
        },
        actual_port,
    ))
}

fn build_router(state: Arc<AppState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]);

    Router::new()
        .route("/status", get(status_handler))
        .route("/threads", get(list_threads).post(create_thread))
        .route("/threads/:id/open", post(open_thread))
        .route("/threads/:id/messages", get(get_messages))
        .route("/threads/:id/stream", get(stream_handler))
        .route("/threads/:id/message", post(send_message))
        .route("/threads/:id/abort", post(abort))
        .route("/threads/:id/model", post(set_model))
        .route("/threads/:id/workspace", post(set_workspace))
        .layer(DefaultBodyLimit::max(BODY_LIMIT_BYTES))
        .layer(cors)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_bearer_token,
        ))
        .with_state(state)
}

async fn status_handler(State(state): State<Arc<AppState>>) -> Response {
    let value = send_command(RemoteCommand::Status, &state.command_sender).await;
    json_response(StatusCode::OK, value)
}

async fn list_threads(State(state): State<Arc<AppState>>) -> Response {
    let value = send_command(RemoteCommand::ListThreads, &state.command_sender).await;
    json_response(http_status_for(&value, 200), value)
}

async fn create_thread(State(state): State<Arc<AppState>>, body: Bytes) -> Response {
    let payload: CreateThreadBody = match parse_json_body(&body) {
        Ok(p) => p,
        Err(response) => return response.into(),
    };
    let value = send_command(
        RemoteCommand::CreateThread {
            workspace_id: payload.workspace_id,
            model_id: payload.model_id,
        },
        &state.command_sender,
    )
    .await;
    json_response(http_status_for(&value, 201), value)
}

async fn open_thread(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    let thread_id = match parse_thread_id(&id) {
        Ok(id) => id,
        Err(response) => return response.into(),
    };
    let value = send_command(
        RemoteCommand::OpenThread { thread_id },
        &state.command_sender,
    )
    .await;
    json_response(http_status_for(&value, 200), value)
}

#[derive(Deserialize)]
struct MessagesQuery {
    since_id: Option<String>,
}

async fn get_messages(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<MessagesQuery>,
) -> Response {
    let thread_id = match parse_thread_id(&id) {
        Ok(id) => id,
        Err(response) => return response.into(),
    };
    let value = send_command(
        RemoteCommand::GetMessages {
            thread_id,
            since_id: query.since_id,
        },
        &state.command_sender,
    )
    .await;
    json_response(http_status_for(&value, 200), value)
}

async fn send_message(State(state): State<Arc<AppState>>, Path(id): Path<String>, body: Bytes) -> Response {
    let thread_id = match parse_thread_id(&id) {
        Ok(id) => id,
        Err(response) => return response.into(),
    };
    let payload: SendMessageBody = match parse_json_body(&body) {
        Ok(p) => p,
        Err(response) => return response.into(),
    };
    let value = send_command(
        RemoteCommand::SendMessage {
            thread_id,
            message: payload.message,
        },
        &state.command_sender,
    )
    .await;
    json_response(http_status_for(&value, 202), value)
}

async fn abort(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    let thread_id = match parse_thread_id(&id) {
        Ok(id) => id,
        Err(response) => return response.into(),
    };
    let value = send_command(RemoteCommand::Abort { thread_id }, &state.command_sender).await;
    json_response(http_status_for(&value, 200), value)
}

async fn set_model(State(state): State<Arc<AppState>>, Path(id): Path<String>, body: Bytes) -> Response {
    let thread_id = match parse_thread_id(&id) {
        Ok(id) => id,
        Err(response) => return response.into(),
    };
    let payload: SetModelBody = match parse_json_body(&body) {
        Ok(p) => p,
        Err(response) => return response.into(),
    };
    let value = send_command(
        RemoteCommand::SetModel {
            thread_id,
            model_id: payload.model_id,
        },
        &state.command_sender,
    )
    .await;
    json_response(http_status_for(&value, 200), value)
}

async fn set_workspace(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let thread_id = match parse_thread_id(&id) {
        Ok(id) => id,
        Err(response) => return response.into(),
    };
    let payload: SetWorkspaceBody = match parse_json_body(&body) {
        Ok(p) => p,
        Err(response) => return response.into(),
    };
    let value = send_command(
        RemoteCommand::SetWorkspace {
            thread_id,
            workspace_id: payload.workspace_id,
        },
        &state.command_sender,
    )
    .await;
    json_response(http_status_for(&value, 200), value)
}

async fn stream_handler(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    let thread_id = match parse_thread_id(&id) {
        Ok(id) => id,
        Err(response) => return response.into(),
    };

    let (sse_tx, sse_rx) = mpsc::unbounded_channel();
    let value = send_command(
        RemoteCommand::AddSseSubscriber {
            thread_id,
            sender: sse_tx,
        },
        &state.command_sender,
    )
    .await;

    if value.get("error").is_some() {
        return json_response(StatusCode::SERVICE_UNAVAILABLE, value);
    }

    let stream =
        UnboundedReceiverStream::new(sse_rx).map(|event| Ok::<_, std::convert::Infallible>(event.to_axum_event()));
    Sse::new(stream)
        .keep_alive(
            axum::response::sse::KeepAlive::new()
                .interval(HEARTBEAT_INTERVAL)
                .text("ping"),
        )
        .into_response()
}

async fn send_command(
    command: RemoteCommand,
    command_sender: &mpsc::UnboundedSender<CommandEnvelope>,
) -> RemoteResponse {
    let (tx, rx) = oneshot::channel();
    if command_sender
        .send(CommandEnvelope {
            command,
            respond_to: tx,
        })
        .is_err()
    {
        return json!({ "error": "remote controller unavailable" });
    }
    match rx.await {
        Ok(value) => value,
        Err(_) => json!({ "error": "remote controller dropped" }),
    }
}

enum ParseError {
    BadThreadId,
    InvalidBody(String),
}

impl From<ParseError> for Response {
    fn from(err: ParseError) -> Self {
        match err {
            ParseError::BadThreadId => {
                json_response(StatusCode::BAD_REQUEST, json!({ "error": "invalid thread id" }))
            }
            ParseError::InvalidBody(msg) => {
                json_response(StatusCode::BAD_REQUEST, json!({ "error": msg }))
            }
        }
    }
}

fn parse_thread_id(id: &str) -> Result<i64, ParseError> {
    id.parse::<i64>().map_err(|_| ParseError::BadThreadId)
}

fn parse_json_body<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, ParseError> {
    let body = String::from_utf8(body.to_vec())
        .map_err(|_| ParseError::InvalidBody("invalid UTF-8 body".to_string()))?;
    serde_json::from_str(&body)
        .map_err(|e| ParseError::InvalidBody(format!("invalid body: {}", e)))
}

fn json_response(status: StatusCode, value: RemoteResponse) -> Response {
    (status, Json(value)).into_response()
}

/// Map a command result to an HTTP status code. Errors that clearly indicate a
/// client mistake become 4xx; everything else is treated as a server-side failure.
fn http_status_for(value: &RemoteResponse, ok_status: u16) -> StatusCode {
    if value.get("error").is_none() {
        return StatusCode::from_u16(ok_status).unwrap_or(StatusCode::OK);
    }
    let error = value.get("error").and_then(|e| e.as_str()).unwrap_or("");
    if error == "thread not found"
        || (error.starts_with("workspace ") && error.contains("not found"))
    {
        StatusCode::NOT_FOUND
    } else if error.starts_with("unknown model_id") || error == "invalid thread id" {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;
    use std::io::{BufRead, BufReader, Write};
    use std::time::{Duration, Instant};

    fn make_dummy_messages() -> Vec<serde_json::Value> {
        vec![
            json!({
                "id": "msg1",
                "entry_id": "entry1",
                "role": "user",
                "parts": [{"type": "text", "text": "hello", "state": null}],
            }),
            json!({
                "id": "msg2",
                "entry_id": "entry2",
                "role": "assistant",
                "parts": [{"type": "text", "text": "hi", "state": null}],
            }),
            json!({
                "id": "msg3",
                "entry_id": "entry3",
                "role": "user",
                "parts": [{"type": "text", "text": "again", "state": null}],
            }),
        ]
    }

    fn spawn_dummy_controller(mut rx: tokio::sync::mpsc::UnboundedReceiver<CommandEnvelope>) {
        std::thread::spawn(move || {
            let mut sse_senders: Vec<tokio::sync::mpsc::UnboundedSender<crate::remote::types::SseEvent>> = Vec::new();
            while let Some(envelope) = block_on(rx.recv()) {
                let value = match envelope.command {
                    RemoteCommand::Status => {
                        json!({ "enabled": true, "status": "running", "tunnel_url": "https://example.com" })
                    }
                    RemoteCommand::GetMessages { since_id, .. } => {
                        let messages = make_dummy_messages();
                        let start_idx = since_id
                            .as_ref()
                            .and_then(|sid| {
                                messages
                                    .iter()
                                    .position(|m| m["id"].as_str() == Some(sid.as_str()))
                            })
                            .map(|i| i + 1)
                            .unwrap_or(0);
                        json!(messages.into_iter().skip(start_idx).collect::<Vec<_>>())
                    }
                    RemoteCommand::AddSseSubscriber { sender, .. } => {
                        // Keep the sender alive so the SSE stream can send heartbeats
                        // even when no further events are produced.
                        sse_senders.push(sender.clone());
                        let _ = sender.send(crate::remote::types::SseEvent::new(
                            "update",
                            json!({
                                "state": "idle",
                                "messages": make_dummy_messages(),
                            }),
                        ));
                        json!(null)
                    }
                    _ => json!({ "error": "unexpected command" }),
                };
                let _ = envelope.respond_to.send(value);
            }
        });
    }

    fn connect_and_send_request(port: u16, path: &str, token: Option<&str>) -> std::net::TcpStream {
        let mut stream = std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        let auth = token
            .map(|t| format!("Authorization: Bearer {}\r\n", t))
            .unwrap_or_default();
        stream
            .write_all(
                format!(
                    "GET {} HTTP/1.1\r\nHost: 127.0.0.1\r\n{}\r\n",
                    path, auth
                )
                .as_bytes(),
            )
            .unwrap();
        stream
    }

    fn read_headers<R: BufRead>(reader: &mut R) -> Vec<String> {
        let mut headers = Vec::new();
        let mut line = String::new();
        loop {
            line.clear();
            let n = reader.read_line(&mut line).expect("failed to read header");
            if n == 0 {
                panic!("connection closed before headers completed");
            }
            if line == "\r\n" || line == "\n" {
                break;
            }
            headers.push(line.clone());
        }
        headers
    }

    #[test]
    fn server_responds_to_status() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        spawn_dummy_controller(rx);
        let (_handle, port) = start(0, None, tx).expect("server should start");

        let client = reqwest::blocking::Client::new();
        let response = client
            .get(&format!("http://127.0.0.1:{}/status", port))
            .send()
            .expect("request should succeed");
        assert_eq!(response.status(), 200);
        let body: serde_json::Value = response.json().expect("response should be JSON");
        assert_eq!(body["status"], "running");
    }

    #[test]
    fn server_rejects_bearer_token() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        spawn_dummy_controller(rx);
        let (_handle, port) = start(0, Some("secret".to_string()), tx).expect("server should start");

        let client = reqwest::blocking::Client::new();
        let response = client
            .get(&format!("http://127.0.0.1:{}/status", port))
            .send()
            .expect("request should succeed");
        assert_eq!(response.status(), 401);
    }

    #[test]
    fn server_allows_with_bearer_token() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        spawn_dummy_controller(rx);
        let (_handle, port) = start(0, Some("secret".to_string()), tx).expect("server should start");

        let client = reqwest::blocking::Client::new();
        let response = client
            .get(&format!("http://127.0.0.1:{}/status", port))
            .bearer_auth("secret")
            .send()
            .expect("request should succeed");
        assert_eq!(response.status(), 200);
    }

    #[test]
    fn messages_since_id_returns_only_newer() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        spawn_dummy_controller(rx);
        let (_handle, port) = start(0, None, tx).expect("server should start");

        let client = reqwest::blocking::Client::new();
        let response = client
            .get(&format!(
                "http://127.0.0.1:{}/threads/1/messages?since_id=msg1",
                port
            ))
            .send()
            .expect("request should succeed");
        assert_eq!(response.status(), 200);
        let body: serde_json::Value = response.json().expect("response should be JSON");
        let ids: Vec<&str> = body
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["msg2", "msg3"]);
    }

    #[test]
    fn sse_response_includes_cors_headers() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        spawn_dummy_controller(rx);
        let (_handle, port) = start(0, None, tx).expect("server should start");

        let stream = connect_and_send_request(port, "/threads/1/stream", None);
        let mut reader = BufReader::new(stream);
        let headers = read_headers(&mut reader);

        let found = headers.iter().any(|h| {
            h.to_ascii_lowercase()
                .starts_with("access-control-allow-origin:")
        });
        assert!(found, "missing CORS header in SSE response: {:?}", headers);
    }

    #[test]
    fn sse_stream_sends_heartbeat() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        spawn_dummy_controller(rx);
        let (_handle, port) = start(0, None, tx).expect("server should start");

        let stream = connect_and_send_request(port, "/threads/1/stream", None);
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        read_headers(&mut reader);

        let start = Instant::now();
        let mut line = String::new();
        loop {
            line.clear();
            reader.read_line(&mut line).unwrap();
            if line.starts_with(": ping") {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "timed out waiting for heartbeat"
            );
        }
    }

    #[test]
    fn sse_rejects_missing_query_token() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        spawn_dummy_controller(rx);
        let (_handle, port) = start(0, Some("secret".to_string()), tx).expect("server should start");

        let mut stream =
            std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        stream
            .write_all(
                "GET /threads/1/stream HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n".as_bytes(),
            )
            .unwrap();
        let mut reader = BufReader::new(stream);
        let headers = read_headers(&mut reader);
        let status = headers
            .first()
            .expect("missing status line")
            .split_whitespace()
            .nth(1)
            .expect("missing status code");
        assert_eq!(status, "401", "expected 401 without token, got: {:?}", headers);
    }

    #[test]
    fn sse_accepts_query_token() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        spawn_dummy_controller(rx);
        let (_handle, port) = start(0, Some("secret".to_string()), tx).expect("server should start");

        let mut stream =
            std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        stream
            .write_all(
                "GET /threads/1/stream?access_token=secret HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"
                    .as_bytes(),
            )
            .unwrap();
        let mut reader = BufReader::new(stream);
        let headers = read_headers(&mut reader);
        let status = headers
            .first()
            .expect("missing status line")
            .split_whitespace()
            .nth(1)
            .expect("missing status code");
        assert_eq!(status, "200", "expected 200 with query token, got: {:?}", headers);
    }
}
