use std::collections::HashMap;
use std::io::BufRead;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use futures::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

use crate::utils::format::truncate_str;

// ---------------------------------------------------------------------------
// Bridge Events
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum BridgeEvent {
    AgentStart,
    AgentEnd,
    Disconnected,
    MessageStart {
        message: Option<serde_json::Value>,
    },
    MessageEnd,
    TextStart,
    TextDelta {
        content: String,
    },
    TextEnd {
        content: String,
    },
    ThinkingStart,
    ThinkingDelta {
        content: String,
    },
    ThinkingEnd {
        content: String,
    },
    ToolCallStart {
        name: String,
        call_id: String,
    },
    ToolCallDelta {
        call_id: String,
        delta: String,
    },
    ToolCallEnd {
        call_id: String,
        name: String,
        args: serde_json::Value,
    },
    ToolStart {
        name: String,
        args: Option<serde_json::Value>,
        call_id: String,
    },
    ToolUpdate {
        call_id: String,
        tool_name: String,
        partial_output: String,
    },
    ToolEnd {
        call_id: String,
        tool_name: String,
        output: String,
        is_error: bool,
    },
    TurnStart,
    TurnEnd,
    Error {
        message: String,
    },
    ExtensionUiRequest {
        id: String,
        method: String,
        payload: serde_json::Value,
    },
    ExtensionError {
        extension_path: String,
        event: String,
        error: String,
    },
    Response {
        command: String,
        success: bool,
        data: Option<serde_json::Value>,
        error: Option<String>,
        request_id: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub enum LoadedPart {
    Text { text: String },
    Thinking { text: String },
    ToolCall { name: String, args: String },
    ToolResult { name: String, output: String },
}

#[derive(Debug, Clone)]
pub struct LoadedMessage {
    pub id: Option<String>,
    pub role: String,
    pub parts: Vec<LoadedPart>,
}

// ---------------------------------------------------------------------------
// Image Content
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ImageContent {
    pub data: String,
    pub mime_type: String,
}

// ---------------------------------------------------------------------------
// Macros
// ---------------------------------------------------------------------------

macro_rules! log {
    ($($arg:tt)*) => {
        eprintln!("[pi-rpc] {}", format!($($arg)*))
    };
}

// ---------------------------------------------------------------------------
// PiBridge: shared WebSocket connection to the SDK bridge
// ---------------------------------------------------------------------------

pub struct PiBridge {
    write_tx: tokio::sync::mpsc::UnboundedSender<String>,
    sessions: Arc<Mutex<HashMap<String, futures::channel::mpsc::UnboundedSender<BridgeEvent>>>>,
    child: Arc<Mutex<std::process::Child>>,
    _runtime: tokio::runtime::Runtime,
}

impl Drop for PiBridge {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
        }
    }
}

impl PiBridge {
    pub fn spawn() -> Result<Arc<Self>, PiRpcError> {
        let bridge_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("pi-bridge");
        let (program, args) = find_runtime(&bridge_dir)?;

        let agent_dir = dirs::home_dir()
            .map(|h| h.join(".mini-pi").join("agent"))
            .unwrap_or_else(|| bridge_dir.join("agent"));

        let mut cmd = Command::new(&program);
        cmd.args(&args)
            .current_dir(&bridge_dir)
            .arg("--agent-dir")
            .arg(&agent_dir);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        #[cfg(windows)]
        {
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }

        log!("spawning bridge: {:?} {:?}", cmd, args);
        let mut child = cmd.spawn().map_err(|e| {
            log!("failed to spawn pi bridge: {}", e);
            PiRpcError::Spawn(format!("failed to spawn pi bridge with {}: {}", program, e))
        })?;
        log!("pi bridge spawned, pid={}", child.id());

        let stdout = child.stdout.take().ok_or(PiRpcError::Stdout)?;
        let port = read_bridge_port(stdout)?;
        log!("pi bridge listening on port {}", port);

        // Forward bridge stderr to our stderr so SDK diagnostics are visible.
        if let Some(stderr) = child.stderr.take() {
            std::thread::spawn(move || {
                let reader = std::io::BufReader::new(stderr);
                for line in reader.lines() {
                    if let Ok(text) = line {
                        if !text.is_empty() {
                            eprintln!("[pi-bridge] {}", text);
                        }
                    }
                }
            });
        }

        let child = Arc::new(Mutex::new(child));

        // Monitor the child process and log when it exits.
        let child_for_monitor = Arc::clone(&child);
        std::thread::spawn(move || {
            let status = child_for_monitor.lock().unwrap().wait();
            log!("pi bridge process exited: {:?}", status);
        });

        let url = format!("ws://127.0.0.1:{}/", port);

        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| PiRpcError::Runtime(format!("tokio runtime: {}", e)))?;

        let (ws_stream, _) = runtime
            .block_on(tokio_tungstenite::connect_async(&url))
            .map_err(|e| PiRpcError::WebSocket(format!("connect: {}", e)))?;

        let (write_half, read_half) = ws_stream.split();
        let (write_tx, mut write_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

        let sessions: Arc<
            Mutex<HashMap<String, futures::channel::mpsc::UnboundedSender<BridgeEvent>>>,
        > = Arc::new(Mutex::new(HashMap::new()));
        let sessions_for_reader = Arc::clone(&sessions);

        // Writer task: forward outgoing JSON to the WebSocket.
        runtime.spawn(async move {
            let mut write_half = write_half;
            while let Some(text) = write_rx.recv().await {
                if write_half.send(Message::text(text)).await.is_err() {
                    break;
                }
            }
            log!("bridge writer task exiting");
            let _ = write_half.close().await;
        });

        // Reader task: parse incoming messages and route to the right session.
        runtime.spawn(async move {
            let mut read_half = read_half;
            while let Some(result) = read_half.next().await {
                match result {
                    Ok(Message::Text(text)) => {
                        if let Some((session_id, event)) = parse_bridge_message(&text) {
                            let sessions_guard = sessions_for_reader.lock().unwrap();
                            if let Some(sender) = sessions_guard.get(&session_id) {
                                let _ = sender.unbounded_send(event);
                            } else {
                                log!("no session receiver for {}", session_id);
                            }
                        }
                    }
                    Ok(Message::Close(_)) => {
                        log!("bridge closed websocket");
                        break;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        log!("bridge websocket error: {}", e);
                        break;
                    }
                }
            }
            log!("bridge reader task exiting; notifying sessions");
            let mut sessions_guard = sessions_for_reader.lock().unwrap();
            for (_id, sender) in sessions_guard.drain() {
                let _: Result<(), _> = sender.unbounded_send(BridgeEvent::Disconnected);
            }
        });

        Ok(Arc::new(Self {
            write_tx,
            sessions,
            child,
            _runtime: runtime,
        }))
    }

    pub fn create_session(
        &self,
        session_id: String,
        session_path: Option<PathBuf>,
        cwd: Option<PathBuf>,
        model: Option<String>,
        thinking_level: Option<String>,
    ) -> Result<futures::channel::mpsc::UnboundedReceiver<BridgeEvent>, PiRpcError> {
        let (tx, rx) = futures::channel::mpsc::unbounded();
        {
            let mut sessions = self.sessions.lock().unwrap();
            sessions.insert(session_id.clone(), tx);
        }

        let mut cmd = serde_json::json!({
            "type": "create_session",
            "sessionId": session_id,
        });
        if let Some(path) = session_path {
            cmd["sessionPath"] = serde_json::json!(path.to_string_lossy().to_string());
        }
        if let Some(dir) = cwd {
            cmd["cwd"] = serde_json::json!(dir.to_string_lossy().to_string());
        }
        if let Some(m) = model {
            cmd["model"] = serde_json::json!(m);
        }
        if let Some(level) = thinking_level {
            cmd["thinkingLevel"] = serde_json::json!(level);
        }

        self.send_json(&cmd)?;
        Ok(rx)
    }

    pub fn send(&self, session_id: String, json: &serde_json::Value) -> Result<(), PiRpcError> {
        let mut msg = json.clone();
        msg["sessionId"] = serde_json::json!(session_id);
        self.send_json(&msg)
    }

    fn send_json(&self, json: &serde_json::Value) -> Result<(), PiRpcError> {
        let text = serde_json::to_string(json).map_err(PiRpcError::Serde)?;
        log!(
            "send: type={} sessionId={}",
            json.get("type").and_then(|t| t.as_str()).unwrap_or("?"),
            json.get("sessionId")
                .and_then(|s| s.as_str())
                .unwrap_or("?")
        );
        self.write_tx
            .send(text)
            .map_err(|_| PiRpcError::WebSocket("writer channel closed".to_string()))
    }
}

// ---------------------------------------------------------------------------
// PiRpc: per-session handle that sends commands through PiBridge
// ---------------------------------------------------------------------------

pub struct PiRpc {
    session_id: String,
    bridge: Arc<PiBridge>,
}

impl PiRpc {
    pub fn new(session_id: String, bridge: Arc<PiBridge>) -> Self {
        Self { session_id, bridge }
    }

    fn send(&mut self, json: &serde_json::Value) -> Result<(), PiRpcError> {
        self.bridge.send(self.session_id.clone(), json)
    }

    // ------------------------------------------------------------------
    // Prompting
    // ------------------------------------------------------------------

    pub fn send_prompt(&mut self, message: &str) -> Result<(), PiRpcError> {
        self.send_prompt_ext(message, None, None, None)
    }

    pub fn send_prompt_ext(
        &mut self,
        message: &str,
        images: Option<&[ImageContent]>,
        streaming_behavior: Option<&str>,
        request_id: Option<&str>,
    ) -> Result<(), PiRpcError> {
        let mut cmd = serde_json::json!({
            "type": "prompt",
            "message": message,
        });
        add_request_id(&mut cmd, request_id);
        add_images(&mut cmd, images);
        if let Some(behavior) = streaming_behavior {
            cmd["streamingBehavior"] = serde_json::json!(behavior);
        }
        self.send(&cmd)
    }

    pub fn send_steer(
        &mut self,
        message: &str,
        images: Option<&[ImageContent]>,
        request_id: Option<&str>,
    ) -> Result<(), PiRpcError> {
        let mut cmd = serde_json::json!({
            "type": "steer",
            "message": message,
        });
        add_request_id(&mut cmd, request_id);
        add_images(&mut cmd, images);
        self.send(&cmd)
    }

    pub fn send_follow_up(
        &mut self,
        message: &str,
        images: Option<&[ImageContent]>,
        request_id: Option<&str>,
    ) -> Result<(), PiRpcError> {
        let mut cmd = serde_json::json!({
            "type": "follow_up",
            "message": message,
        });
        add_request_id(&mut cmd, request_id);
        add_images(&mut cmd, images);
        self.send(&cmd)
    }

    pub fn send_abort(&mut self, request_id: Option<&str>) -> Result<(), PiRpcError> {
        let mut cmd = serde_json::json!({ "type": "abort" });
        add_request_id(&mut cmd, request_id);
        self.send(&cmd)
    }

    pub fn send_new_session(
        &mut self,
        parent_session: Option<&str>,
        request_id: Option<&str>,
    ) -> Result<(), PiRpcError> {
        let mut cmd = serde_json::json!({ "type": "new_session" });
        if let Some(parent) = parent_session {
            cmd["parentSession"] = serde_json::json!(parent);
        }
        add_request_id(&mut cmd, request_id);
        self.send(&cmd)
    }

    // ------------------------------------------------------------------
    // State
    // ------------------------------------------------------------------

    pub fn send_get_state(&mut self, request_id: Option<&str>) -> Result<(), PiRpcError> {
        let mut cmd = serde_json::json!({ "type": "get_state" });
        add_request_id(&mut cmd, request_id);
        self.send(&cmd)
    }

    pub fn send_get_messages(&mut self, request_id: Option<&str>) -> Result<(), PiRpcError> {
        let mut cmd = serde_json::json!({ "type": "get_messages" });
        add_request_id(&mut cmd, request_id);
        self.send(&cmd)
    }

    pub fn send_get_commands(&mut self, request_id: Option<&str>) -> Result<(), PiRpcError> {
        let mut cmd = serde_json::json!({ "type": "get_commands" });
        add_request_id(&mut cmd, request_id);
        self.send(&cmd)
    }

    pub fn send_fork(
        &mut self,
        entry_id: &str,
        request_id: Option<&str>,
    ) -> Result<(), PiRpcError> {
        let mut cmd = serde_json::json!({
            "type": "fork",
            "entryId": entry_id,
        });
        add_request_id(&mut cmd, request_id);
        self.send(&cmd)
    }

    pub fn send_clone(&mut self, request_id: Option<&str>) -> Result<(), PiRpcError> {
        let mut cmd = serde_json::json!({ "type": "clone" });
        add_request_id(&mut cmd, request_id);
        self.send(&cmd)
    }

    pub fn send_get_fork_messages(&mut self, request_id: Option<&str>) -> Result<(), PiRpcError> {
        let mut cmd = serde_json::json!({ "type": "get_fork_messages" });
        add_request_id(&mut cmd, request_id);
        self.send(&cmd)
    }

    // ------------------------------------------------------------------
    // Model
    // ------------------------------------------------------------------

    pub fn send_set_model(
        &mut self,
        provider: &str,
        model_id: &str,
        request_id: Option<&str>,
    ) -> Result<(), PiRpcError> {
        let mut cmd = serde_json::json!({
            "type": "set_model",
            "provider": provider,
            "modelId": model_id,
        });
        add_request_id(&mut cmd, request_id);
        self.send(&cmd)
    }

    // ------------------------------------------------------------------
    // Thinking
    // ------------------------------------------------------------------

    pub fn send_set_thinking_level(
        &mut self,
        level: &str,
        request_id: Option<&str>,
    ) -> Result<(), PiRpcError> {
        let mut cmd = serde_json::json!({
            "type": "set_thinking_level",
            "level": level,
        });
        add_request_id(&mut cmd, request_id);
        self.send(&cmd)
    }

    // ------------------------------------------------------------------
    // Bash
    // ------------------------------------------------------------------

    pub fn send_bash(&mut self, command: &str, request_id: Option<&str>) -> Result<(), PiRpcError> {
        let mut cmd = serde_json::json!({
            "type": "bash",
            "command": command,
        });
        add_request_id(&mut cmd, request_id);
        self.send(&cmd)
    }

    // ------------------------------------------------------------------
    // Compaction
    // ------------------------------------------------------------------

    pub fn send_compact(
        &mut self,
        custom_instructions: Option<&str>,
        request_id: Option<&str>,
    ) -> Result<(), PiRpcError> {
        let mut cmd = serde_json::json!({ "type": "compact" });
        if let Some(instructions) = custom_instructions {
            cmd["customInstructions"] = serde_json::json!(instructions);
        }
        add_request_id(&mut cmd, request_id);
        self.send(&cmd)
    }

    // ------------------------------------------------------------------
    // Export
    // ------------------------------------------------------------------

    pub fn send_export_html(
        &mut self,
        output_path: Option<&str>,
        request_id: Option<&str>,
    ) -> Result<(), PiRpcError> {
        let mut cmd = serde_json::json!({ "type": "export_html" });
        if let Some(path) = output_path {
            cmd["outputPath"] = serde_json::json!(path);
        }
        add_request_id(&mut cmd, request_id);
        self.send(&cmd)
    }

    // ------------------------------------------------------------------
    // Extension UI
    // ------------------------------------------------------------------

    pub fn send_extension_ui_response(
        &mut self,
        id: &str,
        response: &serde_json::Value,
    ) -> Result<(), PiRpcError> {
        let mut cmd = response.clone();
        if let Some(obj) = cmd.as_object_mut() {
            obj.insert(
                "type".to_string(),
                serde_json::json!("extension_ui_response"),
            );
            obj.insert("id".to_string(), serde_json::json!(id));
        }
        self.send(&cmd)
    }
}

// ---------------------------------------------------------------------------
// Bridge process helpers
// ---------------------------------------------------------------------------

fn find_runtime(bridge_dir: &PathBuf) -> Result<(String, Vec<String>), PiRpcError> {
    // Prefer bun because it can run TypeScript directly.
    if Command::new("bun").arg("--version").output().is_ok() {
        return Ok((
            "bun".to_string(),
            vec!["run".to_string(), "src/index.ts".to_string()],
        ));
    }

    // Use the local tsx binary if npm install was run.
    #[cfg(windows)]
    let tsx_names: [&str; 2] = ["tsx.cmd", "tsx"];
    #[cfg(not(windows))]
    let tsx_names: [&str; 1] = ["tsx"];

    for name in tsx_names.iter() {
        let tsx = bridge_dir.join("node_modules").join(".bin").join(name);
        if tsx.exists() {
            return Ok((
                tsx.to_string_lossy().to_string(),
                vec!["src/index.ts".to_string()],
            ));
        }
    }

    // Last resort: npx tsx (requires network if tsx is missing).
    if Command::new("npx").arg("--version").output().is_ok() {
        return Ok((
            "npx".to_string(),
            vec!["tsx".to_string(), "src/index.ts".to_string()],
        ));
    }

    Err(PiRpcError::Spawn(
        "no JavaScript runtime found (tried bun, tsx, npx).".to_string(),
    ))
}

fn read_bridge_port(stdout: std::process::ChildStdout) -> Result<u16, PiRpcError> {
    let reader = std::io::BufReader::new(stdout);
    for line in reader.lines() {
        let text = line.map_err(PiRpcError::Io)?;
        if text.trim().is_empty() {
            continue;
        }
        log!("bridge stdout: {}", &text[..text.len().min(200)]);
        if let Some(prefix) = text.strip_prefix("BRIDGE_PORT ") {
            return prefix
                .trim()
                .parse::<u16>()
                .map_err(|e| PiRpcError::Spawn(format!("invalid port: {}", e)));
        }
    }
    Err(PiRpcError::Spawn(
        "bridge exited before reporting port".to_string(),
    ))
}

// ---------------------------------------------------------------------------
// Message helpers
// ---------------------------------------------------------------------------

fn add_request_id(cmd: &mut serde_json::Value, request_id: Option<&str>) {
    if let Some(id) = request_id {
        cmd["id"] = serde_json::json!(id);
    }
}

fn add_images(cmd: &mut serde_json::Value, images: Option<&[ImageContent]>) {
    if let Some(imgs) = images {
        if !imgs.is_empty() {
            let img_vals: Vec<serde_json::Value> = imgs
                .iter()
                .map(|img| {
                    serde_json::json!({
                        "type": "image",
                        "data": img.data,
                        "mimeType": img.mime_type,
                    })
                })
                .collect();
            cmd["images"] = serde_json::json!(img_vals);
        }
    }
}

fn parse_bridge_message(text: &str) -> Option<(String, BridgeEvent)> {
    let mut val: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            log!(
                "failed to parse JSON: {} (line: {})",
                e,
                &text[..text.len().min(100)]
            );
            return None;
        }
    };

    let session_id = val
        .get("sessionId")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string())?;

    // Remove sessionId so the rest of the payload matches the original RPC event shape.
    if let Some(obj) = val.as_object_mut() {
        obj.remove("sessionId");
    }

    let event = parse_pi_line_value(&val)?;
    Some((session_id, event))
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

fn parse_pi_line_value(val: &serde_json::Value) -> Option<BridgeEvent> {
    let event_type = val.get("type")?.as_str()?;
    log!(
        "raw event type={}, data={}",
        event_type,
        truncate_str(&serde_json::to_string(val).unwrap_or_default(), 200)
    );

    match event_type {
        // -- lifecycle ---------------------------------------------------
        "agent_start" => Some(BridgeEvent::AgentStart),
        "agent_end" => Some(BridgeEvent::AgentEnd),

        // -- message -----------------------------------------------------
        "message_start" => {
            let message = val.get("message").cloned();
            Some(BridgeEvent::MessageStart { message })
        }
        "message_end" => Some(BridgeEvent::MessageEnd),

        // -- streaming deltas --------------------------------------------
        "message_update" => {
            let delta = val.get("assistantMessageEvent")?;
            let delta_type = delta.get("type")?.as_str()?;
            match delta_type {
                "text_start" => Some(BridgeEvent::TextStart),
                "text_delta" => Some(BridgeEvent::TextDelta {
                    content: delta.get("delta")?.as_str()?.to_string(),
                }),
                "text_end" => Some(BridgeEvent::TextEnd {
                    content: delta
                        .get("content")
                        .and_then(|c| c.as_str())
                        .unwrap_or("")
                        .to_string(),
                }),
                "thinking_start" => Some(BridgeEvent::ThinkingStart),
                "thinking_delta" => Some(BridgeEvent::ThinkingDelta {
                    content: delta.get("delta")?.as_str()?.to_string(),
                }),
                "thinking_end" => Some(BridgeEvent::ThinkingEnd {
                    content: delta
                        .get("content")
                        .and_then(|c| c.as_str())
                        .unwrap_or("")
                        .to_string(),
                }),
                "toolcall_start" => Some(BridgeEvent::ToolCallStart {
                    name: delta
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    call_id: delta
                        .get("id")
                        .and_then(|i| i.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                }),
                "toolcall_delta" => Some(BridgeEvent::ToolCallDelta {
                    call_id: delta
                        .get("id")
                        .and_then(|i| i.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    delta: delta
                        .get("delta")
                        .and_then(|d| d.as_str())
                        .unwrap_or("")
                        .to_string(),
                }),
                "toolcall_end" => {
                    let tool_call = delta.get("toolCall")?;
                    Some(BridgeEvent::ToolCallEnd {
                        call_id: tool_call
                            .get("id")
                            .and_then(|i| i.as_str())
                            .unwrap_or("unknown")
                            .to_string(),
                        name: tool_call
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("unknown")
                            .to_string(),
                        args: tool_call
                            .get("arguments")
                            .cloned()
                            .unwrap_or(serde_json::Value::Null),
                    })
                }
                "start" | "done" | "error" => None,
                _ => {
                    log!("unknown message_update delta type: {}", delta_type);
                    None
                }
            }
        }

        // -- tool execution ----------------------------------------------
        "tool_execution_start" => Some(BridgeEvent::ToolStart {
            name: val.get("toolName")?.as_str()?.to_string(),
            args: val.get("args").cloned(),
            call_id: val
                .get("toolCallId")
                .and_then(|i| i.as_str())
                .unwrap_or("unknown")
                .to_string(),
        }),

        "tool_execution_update" => {
            let mut output = String::new();
            if let Some(pr) = val.get("partialResult") {
                if let Some(content) = pr.get("content") {
                    extract_text_parts(content, &mut output);
                }
            }
            Some(BridgeEvent::ToolUpdate {
                call_id: val
                    .get("toolCallId")
                    .and_then(|i| i.as_str())
                    .unwrap_or("unknown")
                    .to_string(),
                tool_name: val
                    .get("toolName")
                    .and_then(|n| n.as_str())
                    .unwrap_or("unknown")
                    .to_string(),
                partial_output: output,
            })
        }

        "tool_execution_end" => {
            let tool_name = val
                .get("toolName")
                .and_then(|n| n.as_str())
                .unwrap_or("unknown")
                .to_string();
            let call_id = val
                .get("toolCallId")
                .and_then(|i| i.as_str())
                .unwrap_or("unknown")
                .to_string();
            let mut output = String::new();
            if let Some(result) = val.get("result") {
                if let Some(content) = result.get("content") {
                    extract_text_parts(content, &mut output);
                }
            }
            let is_error = val
                .get("isError")
                .and_then(|e| e.as_bool())
                .unwrap_or(false);
            Some(BridgeEvent::ToolEnd {
                call_id,
                tool_name,
                output: truncate_str(&output, 500),
                is_error,
            })
        }

        // -- turn --------------------------------------------------------
        "turn_start" => Some(BridgeEvent::TurnStart),
        "turn_end" => Some(BridgeEvent::TurnEnd),

        // -- responses ---------------------------------------------------
        "response" => {
            let command = val
                .get("command")
                .and_then(|c| c.as_str())
                .unwrap_or("unknown");
            let success = val
                .get("success")
                .and_then(|s| s.as_bool())
                .unwrap_or(false);
            let error = val
                .get("error")
                .and_then(|e| e.as_str())
                .map(|e| e.to_string());
            let data = val.get("data").cloned();
            let request_id = val
                .get("id")
                .and_then(|i| i.as_str())
                .map(|i| i.to_string());

            if !success {
                log!(
                    "pi response error for '{}': {}",
                    command,
                    error.as_deref().unwrap_or("unknown error")
                );
            }

            if command == "get_messages" && success {
                if let Some(ref data_val) = data {
                    if data_val.get("messages").is_some() {
                        return Some(BridgeEvent::Response {
                            command: command.to_string(),
                            success,
                            data: data.clone(),
                            error: error.clone(),
                            request_id: request_id.clone(),
                        });
                    }
                }
                log!("get_messages response missing data.messages");
            }

            Some(BridgeEvent::Response {
                command: command.to_string(),
                success,
                data,
                error,
                request_id,
            })
        }

        // -- extension ---------------------------------------------------
        "extension_ui_request" => Some(BridgeEvent::ExtensionUiRequest {
            id: val
                .get("id")
                .and_then(|i| i.as_str())
                .unwrap_or("unknown")
                .to_string(),
            method: val
                .get("method")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown")
                .to_string(),
            payload: val.clone(),
        }),

        "extension_error" => Some(BridgeEvent::ExtensionError {
            extension_path: val
                .get("extensionPath")
                .and_then(|e| e.as_str())
                .unwrap_or("unknown")
                .to_string(),
            event: val
                .get("event")
                .and_then(|e| e.as_str())
                .unwrap_or("unknown")
                .to_string(),
            error: val
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("unknown")
                .to_string(),
        }),

        _ => {
            log!("unknown pi event type: {}", event_type);
            None
        }
    }
}

// ---------------------------------------------------------------------------
// History parsing
// ---------------------------------------------------------------------------

pub fn parse_loaded_messages(messages_val: &serde_json::Value) -> Option<Vec<LoadedMessage>> {
    let arr = messages_val.as_array()?;
    let mut loaded = Vec::new();

    for msg in arr {
        let id = msg
            .get("id")
            .and_then(|i| i.as_str())
            .map(|s| s.to_string());
        let role = msg.get("role")?.as_str()?.to_string();

        match role.as_str() {
            "user" => {
                let content = msg.get("content");
                let mut text = String::new();
                if let Some(arr) = content.and_then(|c| c.as_array()) {
                    for block in arr {
                        if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                            if !text.is_empty() {
                                text.push('\n');
                            }
                            text.push_str(t);
                        }
                    }
                } else if let Some(s) = content.and_then(|c| c.as_str()) {
                    text = s.to_string();
                }
                loaded.push(LoadedMessage {
                    id,
                    role: "user".to_string(),
                    parts: vec![LoadedPart::Text { text }],
                });
            }
            "assistant" => {
                let content = msg.get("content");
                let mut parts: Vec<LoadedPart> = vec![];
                if let Some(content_arr) = content.and_then(|c| c.as_array()) {
                    for block in content_arr {
                        let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                        match block_type {
                            "text" => {
                                if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                                    parts.push(LoadedPart::Text {
                                        text: t.to_string(),
                                    });
                                }
                            }
                            "thinking" => {
                                if let Some(t) = block.get("thinking").and_then(|t| t.as_str()) {
                                    parts.push(LoadedPart::Thinking {
                                        text: t.to_string(),
                                    });
                                }
                            }
                            "toolCall" => {
                                let name = block
                                    .get("name")
                                    .and_then(|n| n.as_str())
                                    .unwrap_or("unknown");
                                let args_str = block
                                    .get("arguments")
                                    .and_then(|a| serde_json::to_string(a).ok())
                                    .unwrap_or_default();
                                parts.push(LoadedPart::ToolCall {
                                    name: name.to_string(),
                                    args: truncate_str(&args_str, 200),
                                });
                            }
                            _ => {}
                        }
                    }
                } else if let Some(content_str) = content.and_then(|c| c.as_str()) {
                    parts.push(LoadedPart::Text {
                        text: content_str.to_string(),
                    });
                }
                if !parts.is_empty() {
                    loaded.push(LoadedMessage {
                        id,
                        role: "assistant".to_string(),
                        parts,
                    });
                }
            }
            "toolResult" => {
                let tool_name = msg
                    .get("toolName")
                    .and_then(|n| n.as_str())
                    .unwrap_or("unknown");
                let mut output = String::new();
                if let Some(content) = msg.get("content") {
                    extract_text_parts(content, &mut output);
                }
                loaded.push(LoadedMessage {
                    id,
                    role: "tool".to_string(),
                    parts: vec![LoadedPart::ToolResult {
                        name: tool_name.to_string(),
                        output: format!("{}: {}", tool_name, truncate_str(&output, 500)),
                    }],
                });
            }
            "bashExecution" => {
                let command = msg
                    .get("command")
                    .and_then(|c| c.as_str())
                    .unwrap_or("unknown");
                let output = msg.get("output").and_then(|o| o.as_str()).unwrap_or("");
                let exit_code = msg.get("exitCode").and_then(|c| c.as_i64()).unwrap_or(-1);
                loaded.push(LoadedMessage {
                    id,
                    role: "bash".to_string(),
                    parts: vec![LoadedPart::ToolResult {
                        name: "bash".to_string(),
                        output: format!(
                            "`{}` (exit {})\n{}",
                            command,
                            exit_code,
                            truncate_str(output, 500)
                        ),
                    }],
                });
            }
            _ => {
                log!("unknown message role in history: {}", role);
            }
        }
    }

    Some(loaded)
}

fn extract_text_parts(val: &serde_json::Value, out: &mut String) {
    if let Some(arr) = val.as_array() {
        for part in arr {
            if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                out.push_str(text);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum PiRpcError {
    Io(std::io::Error),
    Serde(serde_json::Error),
    Stdout,
    Spawn(String),
    WebSocket(String),
    Runtime(String),
}

impl std::fmt::Display for PiRpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PiRpcError::Io(e) => write!(f, "io error: {}", e),
            PiRpcError::Serde(e) => write!(f, "json error: {}", e),
            PiRpcError::Stdout => write!(f, "could not get stdout handle"),
            PiRpcError::Spawn(msg) => write!(f, "spawn error: {}", msg),
            PiRpcError::WebSocket(msg) => write!(f, "websocket error: {}", msg),
            PiRpcError::Runtime(msg) => write!(f, "runtime error: {}", msg),
        }
    }
}

impl std::error::Error for PiRpcError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PiRpcError::Io(e) => Some(e),
            PiRpcError::Serde(e) => Some(e),
            _ => None,
        }
    }
}
