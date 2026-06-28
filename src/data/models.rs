use gpui::SharedString;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PartState {
    Streaming,
    Done,
}

#[derive(Clone, PartialEq, Debug)]
pub enum MessagePart {
    Text {
        text: SharedString,
        state: Option<PartState>,
    },
    Reasoning {
        text: SharedString,
        state: Option<PartState>,
        provider_metadata: Option<Value>,
    },
    ToolCall {
        tool_call_id: SharedString,
        name: SharedString,
        args: SharedString,
        state: Option<PartState>,
    },
    ToolResult {
        tool_call_id: SharedString,
        name: SharedString,
        output: SharedString,
        state: Option<PartState>,
        details: Option<serde_json::Value>,
    },
}

#[derive(Clone)]
pub struct Message {
    /// Local UI identifier.
    pub id: String,
    /// SDK session entry id, used for operations such as fork.
    pub entry_id: Option<String>,
    pub role: Role,
    pub parts: Vec<MessagePart>,
}

#[derive(Clone, PartialEq, Debug)]
pub enum ChatState {
    Idle,
    Loading,
    Streaming,
    Error(SharedString),
}
