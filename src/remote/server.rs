use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use futures::channel::{mpsc::UnboundedSender, oneshot};
use futures::executor::block_on;
use serde_json::json;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

use crate::remote::auth::require_bearer_token;
use crate::remote::types::{
    CommandEnvelope, CreateThreadBody, RemoteCommand, RemoteResponse, SendMessageBody,
    SetModelBody, SetWorkspaceBody, SseEvent,
};

const BODY_LIMIT_BYTES: usize = 1024 * 1024; // 1 MiB
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
const WORKER_COUNT: usize = 16;

pub struct RemoteServerHandle {
    shutdown: Arc<AtomicBool>,
    server: Arc<Server>,
    request_sender: Sender<Option<Request>>,
    thread: Option<JoinHandle<()>>,
    workers: Vec<JoinHandle<()>>,
}

impl Drop for RemoteServerHandle {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        self.server.unblock();
        // Send sentinel values to wake all workers.
        for _ in 0..WORKER_COUNT {
            let _ = self.request_sender.send(None);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

pub fn start(
    port: u16,
    bearer_token: Option<String>,
    command_sender: UnboundedSender<CommandEnvelope>,
) -> Result<(RemoteServerHandle, u16), String> {
    let addr = format!("127.0.0.1:{}", port);
    let server = Server::http(&addr).map_err(|e| format!("failed to bind {}: {}", addr, e))?;
    let server = Arc::new(server);
    let actual_port = match server.server_addr() {
        tiny_http::ListenAddr::IP(addr) => addr.port(),
        #[cfg(unix)]
        tiny_http::ListenAddr::Unix(_) => {
            return Err("unix sockets are not supported".to_string())
        }
    };
    let shutdown = Arc::new(AtomicBool::new(false));

    let (request_sender, request_receiver): (Sender<Option<Request>>, Receiver<Option<Request>>) =
        mpsc::channel();
    let request_receiver = Arc::new(std::sync::Mutex::new(request_receiver));

    // Spawn a fixed-size worker pool for short-lived requests.
    // Long-lived SSE connections are moved to dedicated threads so they cannot
    // exhaust the worker pool when a client disconnects.
    let mut workers = Vec::with_capacity(WORKER_COUNT);
    for _ in 0..WORKER_COUNT {
        let rx = request_receiver.clone();
        let token = bearer_token.clone();
        let cmd_tx = command_sender.clone();
        workers.push(thread::spawn(move || worker_loop(rx, token, cmd_tx)));
    }

    let server_for_thread = server.clone();
    let shutdown_for_thread = shutdown.clone();
    let request_sender_for_thread = request_sender.clone();
    let thread = thread::spawn(move || {
        while !shutdown_for_thread.load(Ordering::SeqCst) {
            match server_for_thread.recv() {
                Ok(request) => {
                    if request_sender_for_thread.send(Some(request)).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    Ok((
        RemoteServerHandle {
            shutdown,
            server,
            request_sender,
            thread: Some(thread),
            workers,
        },
        actual_port,
    ))
}

fn worker_loop(
    rx: Arc<std::sync::Mutex<Receiver<Option<Request>>>>,
    bearer_token: Option<String>,
    command_sender: UnboundedSender<CommandEnvelope>,
) {
    while let Ok(Some(request)) = rx.lock().unwrap().recv() {
        handle_request(request, bearer_token.clone(), command_sender.clone());
    }
}

fn handle_request(
    mut request: Request,
    bearer_token: Option<String>,
    command_sender: UnboundedSender<CommandEnvelope>,
) {
    let method = request.method().clone();
    let full_url = request.url().to_string();
    let (path, query) = match full_url.split_once('?') {
        Some((p, q)) => (p.to_string(), Some(q.to_string())),
        None => (full_url, None),
    };
    let trimmed_path = path.trim_start_matches('/');
    let segments: Vec<&str> = trimmed_path.split('/').filter(|s| !s.is_empty()).collect();

    // SSE streams are handled on dedicated threads so the worker pool is not blocked.
    if method == Method::Get
        && segments.len() == 3
        && segments[0] == "threads"
        && segments[2] == "stream"
    {
        if let Ok(thread_id) = segments[1].parse::<i64>() {
            spawn_stream_handler(request, thread_id, bearer_token, command_sender);
            return;
        }
    }

    if let Err(response) = require_bearer_token(&request, &bearer_token) {
        let _ = request.respond(cors(response.boxed(), &method));
        return;
    }

    let result = match method {
        Method::Get => handle_get(&segments, query.as_deref(), &command_sender),
        Method::Post => handle_post(&segments, &mut request, &command_sender),
        Method::Options => Response::empty(StatusCode(204)).boxed(),
        _ => method_not_allowed(&["GET", "POST", "OPTIONS"]),
    };

    let _ = request.respond(cors(result, &method));
}

fn handle_get(
    segments: &[&str],
    query: Option<&str>,
    command_sender: &UnboundedSender<CommandEnvelope>,
) -> ResponseBox {
    match segments {
        ["status"] => {
            let value = send_command_value(RemoteCommand::Status, command_sender);
            json_response(200, value)
        }
        ["threads"] => {
            let value = send_command_value(RemoteCommand::ListThreads, command_sender);
            json_response(http_status_for(&value, 200), value)
        }
        ["threads", id, "messages"] => match parse_thread_id(id) {
            Ok(thread_id) => {
                let since_id = query.and_then(parse_since_id);
                let value = send_command_value(
                    RemoteCommand::GetMessages {
                        thread_id,
                        since_id,
                    },
                    command_sender,
                );
                json_response(http_status_for(&value, 200), value)
            }
            Err(response) => response,
        },
        ["threads", id, "stream"] => match parse_thread_id(id) {
            Ok(_) => json_response(500, json!({ "error": "stream routed to worker" })),
            Err(response) => response,
        },
        _ => json_response(404, json!({ "error": "not found" })),
    }
}

fn handle_post(
    segments: &[&str],
    request: &mut Request,
    command_sender: &UnboundedSender<CommandEnvelope>,
) -> ResponseBox {
    let body = match read_limited_body(request, BODY_LIMIT_BYTES) {
        Ok(b) => b,
        Err(e) => return json_response(400, json!({ "error": e })),
    };

    match segments {
        ["threads"] => {
            let payload: CreateThreadBody = match serde_json::from_str(&body) {
                Ok(p) => p,
                Err(e) => return json_response(400, json!({ "error": format!("invalid body: {}", e) })),
            };
            let value = send_command_value(
                RemoteCommand::CreateThread {
                    workspace_id: payload.workspace_id,
                    model_id: payload.model_id,
                },
                command_sender,
            );
            json_response(http_status_for(&value, 201), value)
        }
        ["threads", id, "open"] => match parse_thread_id(id) {
            Ok(thread_id) => {
                let value = send_command_value(RemoteCommand::OpenThread { thread_id }, command_sender);
                json_response(http_status_for(&value, 200), value)
            }
            Err(response) => response,
        },
        ["threads", id, "message"] => match parse_thread_id(id) {
            Ok(thread_id) => {
                let payload: SendMessageBody = match serde_json::from_str(&body) {
                    Ok(p) => p,
                    Err(e) => {
                        return json_response(400, json!({ "error": format!("invalid body: {}", e) }))
                    }
                };
                let value = send_command_value(
                    RemoteCommand::SendMessage {
                        thread_id,
                        message: payload.message,
                    },
                    command_sender,
                );
                json_response(http_status_for(&value, 202), value)
            }
            Err(response) => response,
        },
        ["threads", id, "abort"] => match parse_thread_id(id) {
            Ok(thread_id) => {
                let value = send_command_value(RemoteCommand::Abort { thread_id }, command_sender);
                json_response(http_status_for(&value, 200), value)
            }
            Err(response) => response,
        },
        ["threads", id, "model"] => match parse_thread_id(id) {
            Ok(thread_id) => {
                let payload: SetModelBody = match serde_json::from_str(&body) {
                    Ok(p) => p,
                    Err(e) => {
                        return json_response(400, json!({ "error": format!("invalid body: {}", e) }))
                    }
                };
                let value = send_command_value(
                    RemoteCommand::SetModel {
                        thread_id,
                        model_id: payload.model_id,
                    },
                    command_sender,
                );
                json_response(http_status_for(&value, 200), value)
            }
            Err(response) => response,
        },
        ["threads", id, "workspace"] => match parse_thread_id(id) {
            Ok(thread_id) => {
                let payload: SetWorkspaceBody = match serde_json::from_str(&body) {
                    Ok(p) => p,
                    Err(e) => {
                        return json_response(400, json!({ "error": format!("invalid body: {}", e) }))
                    }
                };
                let value = send_command_value(
                    RemoteCommand::SetWorkspace {
                        thread_id,
                        workspace_id: payload.workspace_id,
                    },
                    command_sender,
                );
                json_response(http_status_for(&value, 200), value)
            }
            Err(response) => response,
        },
        _ => json_response(404, json!({ "error": "not found" })),
    }
}

fn parse_thread_id(id: &str) -> Result<i64, ResponseBox> {
    id.parse::<i64>()
        .map_err(|_| json_response(400, json!({ "error": "invalid thread id" })))
}

fn parse_since_id(query: &str) -> Option<String> {
    for pair in query.split('&') {
        if let Some((key, value)) = pair.split_once('=') {
            if key == "since_id" || key == "since" {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn read_limited_body(request: &mut Request, limit: usize) -> Result<String, String> {
    let reader = request.as_reader();
    let mut total = 0usize;
    let mut buf = [0u8; 4096];
    let mut body = Vec::new();
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("bad body: {}", e))?;
        if n == 0 {
            break;
        }
        total += n;
        if total > limit {
            return Err("request body too large".to_string());
        }
        body.extend_from_slice(&buf[..n]);
    }
    String::from_utf8(body).map_err(|_| "invalid UTF-8 body".to_string())
}

fn send_command_value(
    command: RemoteCommand,
    command_sender: &UnboundedSender<CommandEnvelope>,
) -> RemoteResponse {
    let (tx, rx) = oneshot::channel();
    if command_sender
        .unbounded_send(CommandEnvelope {
            command,
            respond_to: tx,
        })
        .is_err()
    {
        return json!({ "error": "remote controller unavailable" });
    }
    match block_on(rx) {
        Ok(value) => value,
        Err(_) => json!({ "error": "remote controller dropped" }),
    }
}

fn json_response(status: u16, value: RemoteResponse) -> ResponseBox {
    Response::from_string(value.to_string())
        .with_header(
            Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
        )
        .with_status_code(StatusCode(status))
        .boxed()
}

fn method_not_allowed(allow: &[&str]) -> ResponseBox {
    Response::from_string(json!({ "error": "method not allowed" }).to_string())
        .with_header(
            Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
        )
        .with_header(
            Header::from_bytes(&b"Allow"[..], allow.join(", ").as_bytes()).unwrap(),
        )
        .with_status_code(StatusCode(405))
        .boxed()
}

/// Map a command result to an HTTP status code. Errors that clearly indicate a
/// client mistake (unknown model, bad id, missing workspace) become 4xx;
/// everything else is treated as a server-side failure.
fn http_status_for(value: &RemoteResponse, ok_status: u16) -> u16 {
    if value.get("error").is_none() {
        return ok_status;
    }
    let error = value.get("error").and_then(|e| e.as_str()).unwrap_or("");
    if error == "thread not found"
        || (error.starts_with("workspace ") && error.contains("not found"))
    {
        404
    } else if error.starts_with("unknown model_id") || error == "invalid thread id" {
        400
    } else {
        500
    }
}

fn cors_headers(method: &Method) -> Vec<Header> {
    let mut headers = vec![
        Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap(),
    ];
    if *method == Method::Options {
        headers.push(
            Header::from_bytes(&b"Access-Control-Allow-Methods"[..], &b"GET, POST, OPTIONS"[..]).unwrap(),
        );
        headers.push(
            Header::from_bytes(&b"Access-Control-Allow-Headers"[..], &b"Authorization, Content-Type"[..]).unwrap(),
        );
    }
    headers
}

fn cors(response: ResponseBox, method: &Method) -> ResponseBox {
    let mut response = response;
    for header in cors_headers(method) {
        response = response.with_header(header);
    }
    response
}

fn build_sse_response_headers(http_version: tiny_http::HTTPVersion, method: &Method) -> String {
    let mut headers = String::new();
    headers.push_str(&format!(
        "HTTP/{}.{} 200 OK\r\n",
        http_version.0, http_version.1
    ));
    headers.push_str("Content-Type: text/event-stream\r\n");
    headers.push_str("Cache-Control: no-cache\r\n");
    headers.push_str("Connection: keep-alive\r\n");
    for header in cors_headers(method) {
        headers.push_str(&format!("{}\r\n", header));
    }
    headers.push_str("\r\n");
    headers
}

fn spawn_stream_handler(
    request: Request,
    thread_id: i64,
    bearer_token: Option<String>,
    command_sender: UnboundedSender<CommandEnvelope>,
) {
    let method = request.method().clone();

    // SSE streams are public endpoints behind the Cloudflare tunnel edge; the
    // optional local bearer_token is still validated before spawning a thread.
    if let Err(response) = require_bearer_token(&request, &bearer_token) {
        let _ = request.respond(cors(response.boxed(), &method));
        return;
    }

    thread::spawn(move || {
        let (sse_tx, sse_rx) = std::sync::mpsc::channel::<SseEvent>();

        let (tx, rx) = oneshot::channel();
        if command_sender
            .unbounded_send(CommandEnvelope {
                command: RemoteCommand::AddSseSubscriber {
                    thread_id,
                    sender: sse_tx,
                },
                respond_to: tx,
            })
            .is_err()
        {
            let _ = request.respond(cors(
                json_response(503, json!({ "error": "remote controller unavailable" })),
                &method,
            ));
            return;
        }
        if block_on(rx).is_err() {
            let _ = request.respond(cors(
                json_response(503, json!({ "error": "remote controller dropped" })),
                &method,
            ));
            return;
        }

        // tiny_http's normal respond path uses chunked transfer encoding and
        // buffers small bodies, which would prevent heartbeats from reaching the
        // client. Grab the raw response writer and stream the SSE frames ourselves.
        let http_version = request.http_version().clone();
        let mut writer = request.into_writer();
        if writer.write_all(build_sse_response_headers(http_version, &method).as_bytes()).is_err() {
            return;
        }
        if writer.flush().is_err() {
            return;
        }

        loop {
            let event = match sse_rx.recv_timeout(HEARTBEAT_INTERVAL) {
                Ok(event) => event,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if writer.write_all(&SseEvent::heartbeat_bytes()).is_err() {
                        break;
                    }
                    if writer.flush().is_err() {
                        break;
                    }
                    continue;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            };
            if writer.write_all(&event.to_bytes()).is_err() {
                break;
            }
            if writer.flush().is_err() {
                break;
            }
        }
    });
}

// ResponseBox is a type alias in tiny_http; ensure we can use it.
type ResponseBox = tiny_http::ResponseBox;

#[cfg(test)]
mod tests {
    use super::*;
    use futures::channel::mpsc::UnboundedReceiver;
    use futures::StreamExt;
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

    fn spawn_dummy_controller(mut rx: UnboundedReceiver<CommandEnvelope>) {
        thread::spawn(move || {
            let mut sse_senders: Vec<Sender<SseEvent>> = Vec::new();
            while let Some(envelope) = block_on(rx.next()) {
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
                        // Keep the sender alive so the SSE thread can send heartbeats
                        // even when no further events are produced.
                        sse_senders.push(sender.clone());
                        let _ = sender.send(SseEvent::new("update", json!({
                            "state": "idle",
                            "messages": make_dummy_messages(),
                        })));
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
        let (tx, rx) = futures::channel::mpsc::unbounded();
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
        let (tx, rx) = futures::channel::mpsc::unbounded();
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
        let (tx, rx) = futures::channel::mpsc::unbounded();
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
        let (tx, rx) = futures::channel::mpsc::unbounded();
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
        let (tx, rx) = futures::channel::mpsc::unbounded();
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
        let (tx, rx) = futures::channel::mpsc::unbounded();
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
            if line.starts_with(":ping") {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "timed out waiting for heartbeat"
            );
        }
    }
}
