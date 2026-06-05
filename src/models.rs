use gpui::SharedString;

#[derive(Clone, PartialEq)]
pub enum Role {
    User,
    Agent,
    Tool,
}

#[derive(Clone)]
pub enum MessageContent {
    Text(SharedString),
    ToolCall { name: SharedString, args: SharedString },
    ToolResult { name: SharedString, output: SharedString },
}

#[derive(Clone)]
pub struct Message {
    pub role: Role,
    pub content: MessageContent,
    pub streaming: bool,
    pub thinking: Option<String>,
    pub thinking_collapsed: bool,
}

#[derive(Clone, PartialEq)]
pub enum ChatState {
    Idle,
    Streaming,
    Error(SharedString),
}
