use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use crate::utils::format::truncate_str;

pub struct PiRpc {
    stdin: std::process::ChildStdin,
    _child: Child,
}

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
    MessagesLoaded {
        messages: Vec<LoadedMessage>,
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
// PiRpc
// ---------------------------------------------------------------------------

impl PiRpc {
    pub fn spawn(
        session_path: &PathBuf,
        model: Option<&str>,
        workspace_dir: Option<PathBuf>,
    ) -> Result<(Self, futures::channel::mpsc::UnboundedReceiver<BridgeEvent>), PiRpcError> {
        let program = if cfg!(windows) { "pi.cmd" } else { "pi" };
        let mut cmd = Command::new(program);
        if let Some(model_str) = model {
            if let Some((provider, model_id)) =
                crate::config::model_config::parse_model_id(model_str)
            {
                cmd.arg("--provider").arg(provider);
                cmd.arg("--model").arg(model_id);
            } else {
                cmd.arg("--model").arg(model_str);
            }
        }
        cmd.arg("--mode").arg("rpc");
        cmd.arg("--session").arg(session_path);
        if let Some(ref dir) = workspace_dir {
            std::fs::create_dir_all(dir).map_err(PiRpcError::Io)?;
            cmd.current_dir(dir);
        }
        if let Some(home) = dirs::home_dir() {
            cmd.env("PI_CODING_AGENT_DIR", home.join(".mini-pi/agent"));
            cmd.env(
                "PI_CODING_AGENT_SESSION_DIR",
                home.join(".mini-pi/sessions"),
            );
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());

        #[cfg(windows)]
        {
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }

        log!("spawning: {:?}", cmd);
        let mut child = cmd.spawn().map_err(|e| {
            log!("failed to spawn pi: {}", e);
            PiRpcError::Io(e)
        })?;
        log!("pi process spawned, pid={}", child.id());

        let stdin = child.stdin.take().ok_or_else(|| {
            log!("failed to get stdin handle");
            PiRpcError::Stdin
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            log!("failed to get stdout handle");
            PiRpcError::Stdout
        })?;

        let (tx, rx) = futures::channel::mpsc::unbounded();

        std::thread::spawn(move || {
            log!("reader thread started");
            let reader = std::io::BufReader::new(stdout);
            for line in reader.lines() {
                match line {
                    Ok(text) => {
                        if text.is_empty() {
                            continue;
                        }
                        if let Some(event) = parse_pi_line(&text) {
                            log!("event: {:?}", event);
                            if tx.unbounded_send(event).is_err() {
                                log!("channel closed, stopping reader");
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        log!("stdout read error: {}, stopping", e);
                        let _ = tx.unbounded_send(BridgeEvent::Disconnected);
                        break;
                    }
                }
            }
            log!("reader thread exiting");
        });

        let pi_rpc = Self {
            stdin,
            _child: child,
        };

        Ok((pi_rpc, rx))
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
        self.write_json(&cmd)
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
        self.write_json(&cmd)
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
        self.write_json(&cmd)
    }

    pub fn send_abort(&mut self, request_id: Option<&str>) -> Result<(), PiRpcError> {
        let mut cmd = serde_json::json!({ "type": "abort" });
        add_request_id(&mut cmd, request_id);
        self.write_json(&cmd)
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
        self.write_json(&cmd)
    }

    // ------------------------------------------------------------------
    // State
    // ------------------------------------------------------------------

    pub fn send_get_state(&mut self, request_id: Option<&str>) -> Result<(), PiRpcError> {
        let mut cmd = serde_json::json!({ "type": "get_state" });
        add_request_id(&mut cmd, request_id);
        self.write_json(&cmd)
    }

    pub fn send_get_messages(&mut self, request_id: Option<&str>) -> Result<(), PiRpcError> {
        let mut cmd = serde_json::json!({ "type": "get_messages" });
        add_request_id(&mut cmd, request_id);
        self.write_json(&cmd)
    }

    pub fn send_get_commands(&mut self, request_id: Option<&str>) -> Result<(), PiRpcError> {
        let mut cmd = serde_json::json!({ "type": "get_commands" });
        add_request_id(&mut cmd, request_id);
        self.write_json(&cmd)
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
        self.write_json(&cmd)
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
        self.write_json(&cmd)
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
        self.write_json(&cmd)
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
        self.write_json(&cmd)
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
        self.write_json(&cmd)
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
        self.write_json(&cmd)
    }

    // ------------------------------------------------------------------
    // Private helpers
    // ------------------------------------------------------------------

    fn write_json(&mut self, json: &serde_json::Value) -> Result<(), PiRpcError> {
        let mut line = serde_json::to_string(json).map_err(PiRpcError::Serde)?;
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .map_err(PiRpcError::Io)?;
        self.stdin.flush().map_err(PiRpcError::Io)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
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

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

fn parse_pi_line(line: &str) -> Option<BridgeEvent> {
    let val: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            log!(
                "failed to parse JSON: {} (line: {})",
                e,
                &line[..line.len().min(100)]
            );
            return None;
        }
    };

    let event_type = val.get("type")?.as_str()?;
    log!(
        "raw event type={}, data={}",
        event_type,
        truncate_str(line, 200)
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
                    if let Some(messages) = data_val.get("messages") {
                        return parse_messages(messages);
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

fn parse_messages(messages_val: &serde_json::Value) -> Option<BridgeEvent> {
    let arr = messages_val.as_array()?;
    let mut loaded = Vec::new();

    for msg in arr {
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

    Some(BridgeEvent::MessagesLoaded { messages: loaded })
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
    Stdin,
    Stdout,
}

impl std::fmt::Display for PiRpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PiRpcError::Io(e) => write!(f, "io error: {}", e),
            PiRpcError::Serde(e) => write!(f, "json error: {}", e),
            PiRpcError::Stdin => write!(f, "could not get stdin handle"),
            PiRpcError::Stdout => write!(f, "could not get stdout handle"),
        }
    }
}
