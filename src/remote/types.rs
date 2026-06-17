use futures::channel::oneshot;
use serde::{Deserialize, Serialize};
use std::sync::mpsc::Sender;

/// A command plus a channel to return the response.
#[derive(Debug)]
pub struct CommandEnvelope {
    pub command: RemoteCommand,
    pub respond_to: oneshot::Sender<RemoteResponse>,
}

/// A command sent from the HTTP server thread into the GPUI main thread.
#[derive(Debug, Clone)]
pub enum RemoteCommand {
    Status,
    ListThreads,
    CreateThread {
        workspace_id: Option<i64>,
        model_id: Option<String>,
    },
    OpenThread {
        thread_id: i64,
    },
    SendMessage {
        thread_id: i64,
        message: String,
    },
    GetMessages {
        thread_id: i64,
        since_id: Option<String>,
    },
    Abort {
        thread_id: i64,
    },
    SetModel {
        thread_id: i64,
        model_id: String,
    },
    SetWorkspace {
        thread_id: i64,
        workspace_id: i64,
    },
    AddSseSubscriber {
        thread_id: i64,
        sender: Sender<SseEvent>,
    },
}

/// JSON response returned for ordinary (non-SSE) requests.
pub type RemoteResponse = serde_json::Value;

/// Events streamed to SSE clients.
#[derive(Debug, Clone)]
pub struct SseEvent {
    pub event: String,
    pub data: serde_json::Value,
}

impl SseEvent {
    pub fn new(event: impl Into<String>, data: impl Serialize) -> Self {
        Self {
            event: event.into(),
            data: serde_json::to_value(data).unwrap_or(serde_json::Value::Null),
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        encode_sse(&self.event, &self.data.to_string())
    }

    /// An SSE comment frame suitable for heartbeats. EventSource ignores it.
    pub fn heartbeat_bytes() -> Vec<u8> {
        b":ping\n\n".to_vec()
    }
}

/// Encode an SSE frame. Newlines in the event name and multi-line data are escaped
/// so the frame cannot be corrupted by user content.
fn encode_sse(event: &str, data: &str) -> Vec<u8> {
    // Event names cannot contain newlines; replace them with spaces.
    let event = event.replace('\n', " ");
    let mut out = format!("event: {}\n", event);
    for line in data.split('\n') {
        out.push_str(&format!("data: {}\n", line));
    }
    out.push('\n');
    out.into_bytes()
}

/// Request bodies.
#[derive(Debug, Deserialize)]
pub struct SendMessageBody {
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct SetModelBody {
    pub model_id: String,
}

#[derive(Debug, Deserialize)]
pub struct SetWorkspaceBody {
    pub workspace_id: i64,
}

#[derive(Debug, Deserialize)]
pub struct CreateThreadBody {
    pub workspace_id: Option<i64>,
    pub model_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_event_single_line() {
        let ev = SseEvent::new("update", serde_json::json!({"x": 1}));
        assert_eq!(String::from_utf8(ev.to_bytes()).unwrap(), "event: update\ndata: {\"x\":1}\n\n");
    }

    #[test]
    fn sse_event_escapes_newlines() {
        let ev = SseEvent::new("up\ndate", serde_json::json!("line1\nline2"));
        assert_eq!(String::from_utf8(ev.to_bytes()).unwrap(), "event: up date\ndata: \"line1\\nline2\"\n\n");
    }

    #[test]
    fn sse_heartbeat_is_comment() {
        assert_eq!(String::from_utf8(SseEvent::heartbeat_bytes()).unwrap(), ":ping\n\n");
    }
}
