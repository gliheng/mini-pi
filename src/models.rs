use gpui::SharedString;
use serde_json::Value;

#[derive(Clone, PartialEq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone, PartialEq)]
pub enum PartState {
    Streaming,
    Done,
}

#[derive(Clone, PartialEq)]
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
    },
}

#[derive(Clone)]
pub struct Message {
    pub id: String,
    pub role: Role,
    pub parts: Vec<MessagePart>,
}

#[derive(Clone, PartialEq)]
pub enum ChatState {
    Idle,
    Streaming,
    Error(SharedString),
}
