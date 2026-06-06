use futures::StreamExt;
use serde::Deserialize;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

pub struct PiRpc {
    stdin: std::process::ChildStdin,
    _child: Child,
}

#[derive(Debug, Clone)]
pub enum BridgeEvent {
    AgentStart,
    AgentEnd,
    TextDelta { content: String },
    ThinkingDelta { content: String },
    ToolStart { name: String, args: Option<serde_json::Value> },
    ToolOutput { name: String, output: String },
    Error { message: String },
    MessagesLoaded { messages: Vec<LoadedMessage> },
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

macro_rules! log {
    ($($arg:tt)*) => {
        eprintln!("[pi-rpc] {}", format!($($arg)*))
    };
}

impl PiRpc {
    pub fn spawn(
        session_path: &PathBuf,
        model: Option<&str>,
    ) -> Result<(Self, futures::channel::mpsc::UnboundedReceiver<BridgeEvent>), PiRpcError> {
        #[cfg(windows)]
        let mut cmd = {
            let mut cmd = Command::new("cmd");
            cmd.arg("/c").arg("pi");
            if let Some(model_id) = model {
                cmd.arg("--model").arg(model_id);
            }
            cmd.arg("--provider").arg("cloudflare-ai-gateway");
            cmd.arg("--mode").arg("rpc");
            cmd.arg("--session").arg(session_path);
            cmd.stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit());
            cmd
        };

        #[cfg(not(windows))]
        let mut cmd = {
            let mut cmd = Command::new("pi");
            if let Some(model_id) = model {
                cmd.arg("--model").arg(model_id);
            }
            cmd.arg("--provider").arg("cloudflare-ai-gateway");
            cmd.arg("--mode").arg("rpc");
            cmd.arg("--session").arg(session_path);
            cmd.stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit());
            cmd
        };

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

    pub fn send_prompt(&mut self, message: &str) -> Result<(), PiRpcError> {
        log!("sending prompt ({} chars)", message.len());
        self.write_json(&serde_json::json!({
            "type": "prompt",
            "message": message,
        }))
    }

    pub fn send_abort(&mut self) -> Result<(), PiRpcError> {
        log!("sending abort");
        self.write_json(&serde_json::json!({ "type": "abort" }))
    }

    pub fn send_new_session(&mut self) -> Result<(), PiRpcError> {
        log!("sending new_session");
        self.write_json(&serde_json::json!({ "type": "new_session" }))
    }

    pub fn send_get_messages(&mut self) -> Result<(), PiRpcError> {
        log!("sending get_messages");
        self.write_json(&serde_json::json!({ "type": "get_messages" }))
    }

    pub fn send_set_model(&mut self, model_id: &str) -> Result<(), PiRpcError> {
        log!("sending set_model modelId={}", model_id);
        self.write_json(&serde_json::json!({
            "type": "set_model",
            "provider": "cloudflare-ai-gateway",
            "modelId": model_id,
        }))
    }

    pub fn send_set_thinking_level(&mut self, level: &str) -> Result<(), PiRpcError> {
        log!("sending set_thinking_level level={}", level);
        self.write_json(&serde_json::json!({
            "type": "set_thinking_level",
            "level": level,
        }))
    }

    fn write_json(&mut self, json: &serde_json::Value) -> Result<(), PiRpcError> {
        let mut line = serde_json::to_string(json).map_err(PiRpcError::Serde)?;
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).map_err(PiRpcError::Io)?;
        self.stdin.flush().map_err(PiRpcError::Io)?;
        Ok(())
    }
}

fn parse_pi_line(line: &str) -> Option<BridgeEvent> {
    let val: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            log!("failed to parse JSON: {} (line: {})", e, &line[..line.len().min(100)]);
            return None;
        }
    };

    let event_type = val.get("type")?.as_str()?;
    log!("raw event type={}, data={}", event_type, truncate_str(line, 200));

    match event_type {
        "agent_start" => Some(BridgeEvent::AgentStart),
        "agent_end" => Some(BridgeEvent::AgentEnd),

        "message_update" => {
            let delta = val.get("assistantMessageEvent")?;
            let delta_type = delta.get("type")?.as_str()?;
            match delta_type {
                "text_delta" => Some(BridgeEvent::TextDelta {
                    content: delta.get("delta")?.as_str()?.to_string(),
                }),
                "thinking_delta" => Some(BridgeEvent::ThinkingDelta {
                    content: delta.get("delta")?.as_str()?.to_string(),
                }),
                "toolcall_start" | "toolcall_end" => None,
                "done" => None,
                "error" => Some(BridgeEvent::Error {
                    message: "agent error".to_string(),
                }),
                _ => {
                    log!("unknown message_update delta type: {}", delta_type);
                    None
                }
            }
        }

        "tool_execution_start" => {
            let name = val.get("toolName")?.as_str()?.to_string();
            let args = val.get("args").cloned();
            Some(BridgeEvent::ToolStart { name, args })
        }

        "tool_execution_end" => {
            let name = val.get("toolName")?.as_str()?.to_string();
            let mut output = String::new();
            if let Some(result) = val.get("result") {
                if let Some(content) = result.get("content") {
                    extract_text_parts(content, &mut output);
                }
            }
            let truncated = truncate_str(&output, 500);
            Some(BridgeEvent::ToolOutput { name, output: truncated })
        }

        "response" => {
            let command = val.get("command").and_then(|v| v.as_str()).unwrap_or("unknown");
            let success = val.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
            if !success {
                let error = val.get("error").and_then(|v| v.as_str()).unwrap_or("unknown error");
                log!("pi response error for '{}': {}", command, error);
                Some(BridgeEvent::Error {
                    message: format!("pi error ({}): {}", command, error),
                })
            } else {
                if command == "get_messages" {
                    if let Some(data) = val.get("data") {
                        if let Some(messages) = data.get("messages") {
                            return parse_messages(messages);
                        }
                    }
                    log!("get_messages response missing data.messages");
                }
                log!("pi response ok for '{}'", command);
                None
            }
        }

        "extension_ui_request" => {
            log!("extension_ui_request: {} (auto-cancelling)", val.get("method").and_then(|v| v.as_str()).unwrap_or("?"));
            None
        }

        "turn_start" | "turn_end" => None,

        _ => {
            log!("unknown pi event type: {}", event_type);
            None
        }
    }
}

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
                            if !text.is_empty() { text.push('\n'); }
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
                        if block_type == "text" {
                            if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                                parts.push(LoadedPart::Text { text: t.to_string() });
                            }
                        } else if block_type == "thinking" {
                            if let Some(t) = block.get("thinking").and_then(|t| t.as_str()) {
                                parts.push(LoadedPart::Thinking { text: t.to_string() });
                            }
                        } else if block_type == "toolCall" {
                            let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("unknown");
                            let args_str = block.get("arguments")
                                .and_then(|a| serde_json::to_string(a).ok())
                                .unwrap_or_default();
                            parts.push(LoadedPart::ToolCall {
                                name: name.to_string(),
                                args: truncate_str(&args_str, 200),
                            });
                        }
                    }
                } else if let Some(content_str) = content.and_then(|c| c.as_str()) {
                    parts.push(LoadedPart::Text { text: content_str.to_string() });
                }
                if !parts.is_empty() {
                    loaded.push(LoadedMessage {
                        role: "assistant".to_string(),
                        parts,
                    });
                }
            }
            "toolResult" => {
                let tool_name = msg.get("toolName").and_then(|n| n.as_str()).unwrap_or("unknown");
                let mut output = String::new();
                if let Some(_content_arr) = msg.get("content").and_then(|c| c.as_array()) {
                    extract_text_parts(msg.get("content").unwrap(), &mut output);
                }
                loaded.push(LoadedMessage {
                    role: "tool".to_string(),
                    parts: vec![LoadedPart::ToolResult {
                        name: tool_name.to_string(),
                        output: format!("{}: {}", tool_name, truncate_str(&output, 500)),
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

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut truncated = s[..s.floor_char_boundary(max)].to_string();
        truncated.push_str("...");
        truncated
    }
}

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