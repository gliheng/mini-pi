use std::{sync::Arc, time::{SystemTime, UNIX_EPOCH}};

use futures::StreamExt;
use gpui::{
    Context, FocusHandle, IntoElement, KeyDownEvent, ParentElement, Render, SharedString, Styled, Task,
    Window, div, prelude::*, px, rgb,
};
use uuid::Uuid;

use crate::core::actions::{CloseWindow, SendMessage};
use crate::core::app::{AppStore, custom_window_options};
use crate::ui::dropdown::{Direction, Dropdown, DropdownEvent, DropdownItem};
use crate::ui::input::TextInput;
use crate::ui::loader::{loader, text_loader};
use crate::config::model_config::{model_display_name, all_models};
use crate::data::models::{ChatState, Message, MessagePart, PartState, Role};
use crate::rpc::pi_rpc::{BridgeEvent, PiRpc};
use crate::views::reasoning::Reasoning;
use crate::data::store::{Store, ThreadMeta};
use crate::views::title_bar::TitleBar;
use crate::utils::format::truncate_str;

pub struct ChatWindow {
    pub thread_id: Option<i64>,
    pub session_file: String,
    pub title_bar: gpui::Entity<TitleBar>,
    pub messages: Vec<Message>,
    pub input: gpui::Entity<TextInput>,
    pub focus_handle: FocusHandle,
    pub state: ChatState,
    pub store: Arc<Store>,
    pub pi: Option<PiRpc>,
    pub _pi_task: Option<Task<()>>,
    pub selected_model: Option<String>,
    pub thinking_level: Option<String>,
    pub model_dropdown: gpui::Entity<Dropdown>,
    pub thinking_dropdown: gpui::Entity<Dropdown>,
    pub reasoning_displays: Vec<Vec<Option<gpui::Entity<Reasoning>>>>,
}

impl ChatWindow {
    pub fn new(
        cx: &mut Context<Self>,
        thread: Option<&ThreadMeta>,
        store: Arc<Store>,
    ) -> Self {
        let title: SharedString = thread
            .map(|t| if t.title.is_empty() { "New Thread".into() } else { t.title.clone().into() })
            .unwrap_or_else(|| "New Thread".into());
        let input = cx.new(|cx| TextInput::new(cx, "Type a message..."));
        let title_bar = cx.new(|_| TitleBar::new(title.clone()));

        let session_file: String = thread
            .and_then(|t| t.session_file.clone())
            .unwrap_or_else(|| {
                let ns = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos();
                format!("session_{}.jsonl", ns)
            });
        let session_path = store.sessions_dir().join(&session_file);
        let is_restoring = thread.is_some();
        let selected_model: Option<String> = thread.and_then(|t| t.model.clone());

        let (pi, pi_task) = match PiRpc::spawn(&session_path, selected_model.as_deref()) {
            Ok((mut rpc, rx)) => {
                eprintln!("[mini-pi] pi spawned with session {}", session_file);
                if is_restoring {
                    eprintln!("[mini-pi] restoring session, requesting message history");
                    if let Err(e) = rpc.send_get_messages(None) {
                        eprintln!("[mini-pi] failed to send get_messages: {}", e);
                    }
                }
                let weak = cx.entity().downgrade();
                let task = cx.spawn(async move |_, cx: &mut gpui::AsyncApp| {
                    let mut rx = rx;
                    while let Some(event) = rx.next().await {
                        if weak.update(cx, |window, cx| {
                            window.handle_bridge_event(event, cx);
                        }).is_err() {
                            break;
                        }
                    }
                    eprintln!("[mini-pi] event loop ended");
                });
                (Some(rpc), Some(task))
            }
            Err(e) => {
                eprintln!("[mini-pi] failed to spawn pi: {}", e);
                (None, None)
            }
        };

        let initial_state = if pi.is_some() {
            if is_restoring { ChatState::Loading } else { ChatState::Idle }
        } else {
            ChatState::Error("Failed to start pi agent. Is bun installed?".into())
        };

        // Build model dropdown items
        let model_items: Vec<DropdownItem> = all_models()
            .iter()
            .map(|m| DropdownItem::new(m.id, m.name))
            .collect();

        let model_dropdown = cx.new(|cx| {
            Dropdown::new(cx, model_display_name(selected_model.as_deref()), model_items)
                .with_selected(selected_model.clone())
                .with_searchable(true)
                .with_width(px(280.))
                .with_max_height(px(400.))
                .with_direction(Direction::Up)
        });

        // Build thinking level dropdown items
        let thinking_items = vec![
            DropdownItem::new("off", "Off"),
            DropdownItem::new("minimal", "Minimal"),
            DropdownItem::new("low", "Low"),
            DropdownItem::new("medium", "Medium"),
            DropdownItem::new("high", "High"),
            DropdownItem::new("xhigh", "Extra High"),
        ];
        let thinking_dropdown = cx.new(|cx| {
            Dropdown::new(cx, "Default", thinking_items)
                .with_width(px(160.))
                .with_max_height(px(300.))
                .with_direction(Direction::Up)
        });

        let window = Self {
            thread_id: thread.map(|t| t.id),
            session_file,
            title_bar,
            messages: vec![],
            input,
            focus_handle: cx.focus_handle(),
            state: initial_state,
            store: store.clone(),
            pi,
            _pi_task: pi_task,
            selected_model,
            thinking_level: None,
            model_dropdown: model_dropdown.clone(),
            thinking_dropdown: thinking_dropdown.clone(),
            reasoning_displays: vec![],
        };

        // Subscribe to model dropdown selection events
        cx.subscribe(&model_dropdown, |this, _dropdown, event: &DropdownEvent, _cx| {
            let DropdownEvent::Selected { id } = event;
            this.selected_model = Some(id.clone());
            if let Some(thread_id) = this.thread_id {
                let _ = this.store.update_thread(
                    thread_id,
                    None,
                    None,
                    None,
                    Some(Some(id)),
                    None,
                );
            }
            if let Some(ref mut pi) = this.pi {
                if let Err(e) = pi.send_set_model("cloudflare-ai-gateway", id, None) {
                    eprintln!("[mini-pi] send_set_model failed: {}", e);
                }
            }
        }).detach();

        // Subscribe to thinking dropdown selection events
        cx.subscribe(&thinking_dropdown, |this, _dropdown, event: &DropdownEvent, _cx| {
            let DropdownEvent::Selected { id } = event;
            this.thinking_level = Some(id.clone());
            if let Some(ref mut pi) = this.pi {
                if let Err(e) = pi.send_set_thinking_level(id, None) {
                    eprintln!("[mini-pi] send_set_thinking_level failed: {}", e);
                }
            }
        }).detach();

        window
    }

    pub fn handle_bridge_event(&mut self,
        event: BridgeEvent,
        cx: &mut Context<Self>,
    ) {
        eprintln!("[mini-pi] bridge event: {:?}", event);
        match event {
            BridgeEvent::AgentStart => {
                self.state = ChatState::Streaming;
            }
            BridgeEvent::MessageStart { .. } => {
                self.messages.push(Message {
                    id: Uuid::new_v4().to_string(),
                    role: Role::Assistant,
                    parts: vec![],
                });
            }
            BridgeEvent::AgentEnd => {
                for msg in self.messages.iter_mut() {
                    for part in msg.parts.iter_mut() {
                        match part {
                            MessagePart::Text { state, .. }
                            | MessagePart::Reasoning { state, .. }
                            | MessagePart::ToolCall { state, .. }
                            | MessagePart::ToolResult { state, .. } => {
                                if let Some(s) = state {
                                    *s = PartState::Done;
                                }
                            }
                        }
                    }
                }
                self.state = ChatState::Idle;
            }
            BridgeEvent::TextDelta { content } => {
                if let Some(msg) = self.messages.iter_mut().find(|m| {
                    matches!(m.role, Role::Assistant)
                        && m.parts.iter().any(|p| matches!(p, MessagePart::Text { state: Some(PartState::Streaming), .. }))
                }) {
                    if let Some(part) = msg.parts.iter_mut().find(|p| matches!(p, MessagePart::Text { state: Some(PartState::Streaming), .. })) {
                        if let MessagePart::Text { text, .. } = part {
                            let new_text = format!("{}{}", text, content);
                            *text = new_text.into();
                        }
                    }
                } else if let Some(msg) = self.messages.last_mut() {
                    if matches!(msg.role, Role::Assistant) {
                        msg.parts.push(MessagePart::Text {
                            text: content.into(),
                            state: Some(PartState::Streaming),
                        });
                    }
                }
            }
            BridgeEvent::ThinkingDelta { content } => {
                if let Some(msg) = self.messages.iter_mut().find(|m| {
                    matches!(m.role, Role::Assistant)
                        && m.parts.iter().any(|p| matches!(p, MessagePart::Reasoning { state: Some(PartState::Streaming), .. }))
                }) {
                    if let Some(part) = msg.parts.iter_mut().find(|p| matches!(p, MessagePart::Reasoning { state: Some(PartState::Streaming), .. })) {
                        if let MessagePart::Reasoning { text, .. } = part {
                            let new_text = format!("{}{}", text, content);
                            *text = new_text.into();
                        }
                    }
                } else if let Some(msg) = self.messages.last_mut() {
                    if matches!(msg.role, Role::Assistant) {
                        msg.parts.push(MessagePart::Reasoning {
                            text: content.into(),
                            state: Some(PartState::Streaming),
                            provider_metadata: None,
                        });
                    }
                }
            }
            BridgeEvent::ToolStart { name, args, .. } => {
                if let Some(msg) = self.messages.last_mut() {
                    if matches!(msg.role, Role::Assistant) {
                        msg.parts.push(MessagePart::ToolCall {
                            tool_call_id: SharedString::from(""),
                            name: name.into(),
                            args: args
                                .as_ref()
                                .map(|v| serde_json::to_string(v).unwrap_or_default())
                                .unwrap_or_default()
                                .into(),
                            state: Some(PartState::Streaming),
                        });
                    }
                }
            }
            BridgeEvent::ToolEnd { tool_name, output, is_error, .. } => {
                if let Some(msg) = self.messages.last_mut() {
                    if matches!(msg.role, Role::Assistant) {
                        // Mark any existing streaming tool call as done
                        for part in msg.parts.iter_mut() {
                            if let MessagePart::ToolCall { state, .. } = part {
                                if let Some(s) = state {
                                    *s = PartState::Done;
                                }
                            }
                        }
                        msg.parts.push(MessagePart::ToolResult {
                            tool_call_id: SharedString::from(""),
                            name: tool_name.into(),
                            output: if is_error {
                                format!("ERROR: {}", truncate_str(&output, 500))
                            } else {
                                truncate_str(&output, 500)
                            }.into(),
                            state: Some(PartState::Done),
                        });
                    }
                }
            }
            BridgeEvent::Error { message } => {
                self.state = ChatState::Error(message.into());
            }
            BridgeEvent::TextStart => {
                if let Some(msg) = self.messages.last_mut() {
                    if matches!(msg.role, Role::Assistant) {
                        msg.parts.push(MessagePart::Text {
                            text: SharedString::from(""),
                            state: Some(PartState::Streaming),
                        });
                    }
                }
            }
            BridgeEvent::TextEnd { .. } => {
                if let Some(msg) = self.messages.last_mut() {
                    if matches!(msg.role, Role::Assistant) {
                        if let Some(part) = msg.parts.iter_mut().rev().find(|p| matches!(p, MessagePart::Text { state: Some(PartState::Streaming), .. })) {
                            if let MessagePart::Text { state, .. } = part {
                                *state = Some(PartState::Done);
                            }
                        }
                    }
                }
            }
            BridgeEvent::ThinkingStart => {
                if let Some(msg) = self.messages.last_mut() {
                    if matches!(msg.role, Role::Assistant) {
                        msg.parts.push(MessagePart::Reasoning {
                            text: SharedString::from(""),
                            state: Some(PartState::Streaming),
                            provider_metadata: None,
                        });
                    }
                }
            }
            BridgeEvent::ThinkingEnd { .. } => {
                if let Some(msg) = self.messages.last_mut() {
                    if matches!(msg.role, Role::Assistant) {
                        if let Some(part) = msg.parts.iter_mut().rev().find(|p| matches!(p, MessagePart::Reasoning { state: Some(PartState::Streaming), .. })) {
                            if let MessagePart::Reasoning { state, .. } = part {
                                *state = Some(PartState::Done);
                            }
                        }
                    }
                }
            }
            BridgeEvent::ToolCallStart { name, call_id } => {
                if let Some(msg) = self.messages.last_mut() {
                    if matches!(msg.role, Role::Assistant) {
                        msg.parts.push(MessagePart::ToolCall {
                            tool_call_id: call_id.into(),
                            name: name.into(),
                            args: SharedString::from(""),
                            state: Some(PartState::Streaming),
                        });
                    }
                }
            }
            BridgeEvent::ToolCallDelta { call_id, delta } => {
                if let Some(msg) = self.messages.last_mut() {
                    if matches!(msg.role, Role::Assistant) {
                        if let Some(part) = msg.parts.iter_mut().find(|p| matches!(p, MessagePart::ToolCall { tool_call_id: id, .. } if id == &SharedString::from(call_id.clone()))) {
                            if let MessagePart::ToolCall { args, .. } = part {
                                let new_args = format!("{}{}", args, delta);
                                *args = new_args.into();
                            }
                        }
                    }
                }
            }
            BridgeEvent::ToolCallEnd { call_id, name, args } => {
                if let Some(msg) = self.messages.last_mut() {
                    if matches!(msg.role, Role::Assistant) {
                        if let Some(part) = msg.parts.iter_mut().find(|p| matches!(p, MessagePart::ToolCall { tool_call_id: id, .. } if id == &SharedString::from(call_id.clone()))) {
                            if let MessagePart::ToolCall { name: part_name, args: part_args, state, .. } = part {
                                if *part_name == SharedString::from("") || *part_name == SharedString::from(name.clone()) {
                                    *part_name = name.into();
                                }
                                let args_str = serde_json::to_string(&args).unwrap_or_default();
                                *part_args = args_str.into();
                                *state = Some(PartState::Done);
                            }
                        }
                    }
                }
            }
            BridgeEvent::ToolUpdate { .. } => {}
            BridgeEvent::TurnStart | BridgeEvent::TurnEnd => {}
            BridgeEvent::MessageEnd => {
                if let Some(msg) = self.messages.last_mut() {
                    if matches!(msg.role, Role::Assistant) {
                        for part in msg.parts.iter_mut() {
                            match part {
                                MessagePart::Text { state, .. }
                                | MessagePart::Reasoning { state, .. }
                                | MessagePart::ToolCall { state, .. }
                                | MessagePart::ToolResult { state, .. } => {
                                    if let Some(s) = state {
                                        *s = PartState::Done;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            BridgeEvent::ExtensionUiRequest { id, method, .. } => {
                eprintln!("[mini-pi] extension_ui_request method={}, id={}, auto-cancelling", method, id);
                if let Some(ref mut pi) = self.pi {
                    let _ = pi.send_extension_ui_response(
                        &id,
                        &serde_json::json!({ "cancelled": true }),
                    );
                }
            }
            BridgeEvent::ExtensionError { extension_path, event, error } => {
                eprintln!("[mini-pi] extension error in {} (event: {}): {}", extension_path, event, error);
            }
            BridgeEvent::Disconnected => {
                eprintln!("[mini-pi] pi process disconnected");
                self.state = ChatState::Error("Pi agent process disconnected".into());
            }
            BridgeEvent::Response { .. } => {}
            BridgeEvent::MessagesLoaded { messages } => {
                eprintln!("[mini-pi] loaded {} messages from history", messages.len());
                for msg in messages {
                    match msg.role.as_str() {
                        "user" => {
                            let mut parts = vec![];
                            for part in msg.parts {
                                match part {
                                    crate::rpc::pi_rpc::LoadedPart::Text { text } => {
                                        parts.push(MessagePart::Text {
                                            text: if text.is_empty() {
                                                SharedString::from("(empty)")
                                            } else {
                                                text.into()
                                            },
                                            state: Some(PartState::Done),
                                        });
                                    }
                                    _ => {}
                                }
                            }
                            if !parts.is_empty() {
                                self.messages.push(Message {
                                    id: Uuid::new_v4().to_string(),
                                    role: Role::User,
                                    parts,
                                });
                            }
                        }
                        "assistant" => {
                            let mut parts = vec![];
                            for part in msg.parts {
                                match part {
                                    crate::rpc::pi_rpc::LoadedPart::Text { text } => {
                                        parts.push(MessagePart::Text {
                                            text: text.into(),
                                            state: Some(PartState::Done),
                                        });
                                    }
                                    crate::rpc::pi_rpc::LoadedPart::Thinking { text } => {
                                        parts.push(MessagePart::Reasoning {
                                            text: text.into(),
                                            state: Some(PartState::Done),
                                            provider_metadata: None,
                                        });
                                    }
                                    crate::rpc::pi_rpc::LoadedPart::ToolCall { name, args } => {
                                        parts.push(MessagePart::ToolCall {
                                            tool_call_id: SharedString::from(""),
                                            name: name.into(),
                                            args: args.into(),
                                            state: Some(PartState::Done),
                                        });
                                    }
                                    crate::rpc::pi_rpc::LoadedPart::ToolResult { name, output } => {
                                        parts.push(MessagePart::ToolResult {
                                            tool_call_id: SharedString::from(""),
                                            name: name.into(),
                                            output: output.into(),
                                            state: Some(PartState::Done),
                                        });
                                    }
                                }
                            }
                            if !parts.is_empty() {
                                self.messages.push(Message {
                                    id: Uuid::new_v4().to_string(),
                                    role: Role::Assistant,
                                    parts,
                                });
                            }
                        }
                        "tool" => {
                            for part in msg.parts {
                                if let crate::rpc::pi_rpc::LoadedPart::ToolResult { name, output } = part {
                                    if let Some(last_msg) = self.messages.last_mut() {
                                        if matches!(last_msg.role, Role::Assistant) {
                                            last_msg.parts.push(MessagePart::ToolResult {
                                                tool_call_id: SharedString::from(""),
                                                name: name.into(),
                                                output: output.into(),
                                                state: Some(PartState::Done),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                self.state = ChatState::Idle;
            }
        }
        cx.notify();
    }

    pub fn send_message(
        &mut self,
        _: &SendMessage,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let content = self.input.read(cx).content().clone();
        eprintln!("[mini-pi] send_message: {} chars", content.len());
        if content.is_empty() {
            return;
        }

        self.messages.push(Message {
            id: Uuid::new_v4().to_string(),
            role: Role::User,
            parts: vec![MessagePart::Text {
                text: content.clone(),
                state: Some(PartState::Done),
            }],
        });
        self.input.update(cx, |input, _| input.reset());

        let mut needs_refresh = false;

        if self.thread_id.is_none() {
            match self.store.create_thread("", "") {
                Ok(thread) => {
                    self.thread_id = Some(thread.id);
                    let sf = self.session_file.clone();
                    let model_opt = self.selected_model.as_deref();
                    let _ = self.store.update_thread(
                        thread.id,
                        None,
                        None,
                        Some(Some(&sf)),
                        Some(model_opt),
                        None,
                    );
                    needs_refresh = true;
                }
                Err(_) => {
                    self.state = ChatState::Error("Failed to create thread".into());
                    cx.notify();
                    return;
                }
            }
        }

        let tid = self.thread_id.unwrap();
        let user_count = self.messages.iter().filter(|m| matches!(m.role, Role::User)).count();
        if user_count == 1 {
            let title: String = content.chars().take(80).collect();
            let preview: String = content.chars().take(120).collect();
            let _ = self.store.update_thread(
                tid,
                Some(&title),
                Some(&preview),
                None,
                None,
                None,
            );
            needs_refresh = true;
        }

        if needs_refresh {
            cx.update_global(|_: &mut AppStore, _| {});
        }

        self.state = ChatState::Streaming;

        if let Some(ref mut pi) = self.pi {
            let _ = pi.send_prompt(&content);
        }

        cx.notify();
    }
}

impl Render for ChatWindow {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let status = match &self.state {
            ChatState::Idle => None,
            ChatState::Loading => Some(SharedString::from("Loading...")),
            ChatState::Streaming => Some(SharedString::from("Thinking...")),
            ChatState::Error(msg) => Some(msg.clone()),
        };
        let is_error = matches!(self.state, ChatState::Error(_));
        let is_loading = matches!(self.state, ChatState::Loading);
        let is_streaming = matches!(self.state, ChatState::Streaming);
        let input_empty = self.input.read(cx).content().is_empty();
        let is_disabled = is_streaming || is_loading || input_empty;

        // Sync dropdown labels with current state
        let model_label = model_display_name(self.selected_model.as_deref());
        self.model_dropdown.update(cx, |dropdown, _cx| {
            dropdown.label = model_label;
            dropdown.selected_id = self.selected_model.clone();
        });

        let thinking_label: SharedString = self.thinking_level
            .as_ref()
            .map(|l| match l.as_str() {
                "off" => "Off".into(),
                "minimal" => "Minimal".into(),
                "low" => "Low".into(),
                "medium" => "Medium".into(),
                "high" => "High".into(),
                "xhigh" => "Extra High".into(),
                _ => l.clone().into(),
            })
            .unwrap_or_else(|| "Default".into());
        self.thinking_dropdown.update(cx, |dropdown, _cx| {
            dropdown.label = thinking_label;
            dropdown.selected_id = self.thinking_level.clone();
        });

        // Ensure reasoning displays exist for reasoning parts
        let mut reasoning_entities: Vec<Vec<Option<gpui::Entity<Reasoning>>>> = Vec::new();
        for (msg_idx, msg) in self.messages.iter().enumerate() {
            let mut msg_reasoning: Vec<Option<gpui::Entity<Reasoning>>> = Vec::new();
            for (part_idx, part) in msg.parts.iter().enumerate() {
                if let MessagePart::Reasoning { text, .. } = part {
                    if msg_idx >= self.reasoning_displays.len() {
                        self.reasoning_displays.resize_with(msg_idx + 1, || vec![]);
                    }
                    let row = &mut self.reasoning_displays[msg_idx];
                    if part_idx >= row.len() {
                        row.resize_with(part_idx + 1, || None);
                    }
                    let entity = if let Some(Some(existing)) = row.get(part_idx) {
                        existing.update(cx, |display, _cx| {
                            display.set_content(text);
                        });
                        existing.clone()
                    } else {
                        let new = cx.new(|_cx| Reasoning::new(text));
                        row[part_idx] = Some(new.clone());
                        new
                    };
                    msg_reasoning.push(Some(entity));
                } else {
                    msg_reasoning.push(None);
                }
            }
            reasoning_entities.push(msg_reasoning);
        }

        div()
            .relative()
            .track_focus(&self.focus_handle)
            .on_action(|_: &CloseWindow, window, _| {
                window.remove_window();
            })
            .on_action(cx.listener(Self::send_message))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                // Close any open dropdowns on escape if neither dropdown consumed it
                let model_open = this.model_dropdown.read(cx).is_open;
                let thinking_open = this.thinking_dropdown.read(cx).is_open;
                if event.keystroke.key == "escape" && (model_open || thinking_open) {
                    // Dropdowns handle their own escape; this is a fallback
                }
            }))
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x1a1a1a))
            .child(self.title_bar.clone())
            .child(
                div()
                    .id("messages")
                    .flex_1()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .p_3()
                    .gap_2()
                    .children(
                        self.messages.iter().enumerate().map(|(msg_idx, msg)| {
                            let is_user = matches!(msg.role, Role::User);
                            let msg_reasoning = reasoning_entities.get(msg_idx).cloned().unwrap_or_default();
                            div()
                                .flex()
                                .w_full()
                                .when(is_user, |this| this.justify_end())
                                .when(!is_user, |this| this.justify_start())
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .w_full()
                                        .when(is_user, |this| this.items_end())
                                        .gap_1()
                                        .children(msg.parts.iter().enumerate().map(|(part_idx, part)| {
                                            match part {
                                                MessagePart::Text { text, state } => {
                                                    let is_streaming_empty = *state == Some(PartState::Streaming) && text.is_empty();
                                                    div()
                                                        .px_3()
                                                        .py_2()
                                                        .rounded_md()
                                                        .when(is_user, |this| {
                                                            this.bg(rgb(0x3b82f6)).text_color(rgb(0xffffff))
                                                        })
                                                        .when(matches!(msg.role, Role::Assistant), |this| {
                                                            this.text_color(rgb(0xe5e5e5))
                                                        })
                                                        .text_sm()
                                                        .when(is_streaming_empty, |this| {
                                                            this.child(text_loader())
                                                        })
                                                        .when(!is_streaming_empty, |this| {
                                                            this.child(text.clone())
                                                        })
                                                }
                                                MessagePart::Reasoning { .. } => {
                                                    let reasoning_entity = msg_reasoning.get(part_idx).and_then(|e| e.clone());
                                                    div()
                                                        .flex()
                                                        .flex_col()
                                                        .w_full()
                                                        .when(reasoning_entity.is_some(), |this| {
                                                            this.child(reasoning_entity.unwrap())
                                                        })
                                                }
                                                MessagePart::ToolCall { name, args, .. } => {
                                                    div()
                                                        .flex()
                                                        .w_full()
                                                        .justify_start()
                                                        .child(
                                                            div()
                                                                .px_3()
                                                                .py_1()
                                                                .rounded_md()
                                                                .bg(rgb(0x3b2818))
                                                                .text_color(rgb(0xfbbf24))
                                                                .text_xs()
                                                                .w_full()
                                                                .child(format!("⚙ {} {}", name, args)),
                                                        )
                                                }
                                                MessagePart::ToolResult { name, output, .. } => {
                                                    div()
                                                        .flex()
                                                        .w_full()
                                                        .justify_start()
                                                        .child(
                                                            div()
                                                                .px_3()
                                                                .py_1()
                                                                .rounded_md()
                                                                .bg(rgb(0x1a1a2e))
                                                                .text_color(rgb(0xa5b4fc))
                                                                .text_xs()
                                                                .w_full()
                                                                .child(format!("↳ {}: {}", name, output)),
                                                        )
                                                }
                                            }
                                        }))
                                )
                        })
                    )
                    .when(is_loading, |el| {
                        el.child(
                            div()
                                .flex()
                                .justify_center()
                                .child(
                                    div()
                                        .px_3()
                                        .py_1()
                                        .rounded_md()
                                        .bg(rgb(0x252525))
                                        .text_color(rgb(0x888888))
                                        .text_xs()
                                        .child(loader()),
                                ),
                        )
                    })
                    .when(is_streaming, |el| {
                        el.child(
                            div()
                                .flex()
                                .justify_center()
                                .child(
                                    div()
                                        .px_3()
                                        .py_1()
                                        .rounded_md()
                                        .bg(rgb(0x252525))
                                        .text_color(rgb(0x888888))
                                        .text_xs()
                                        .child("Thinking..."),
                                ),
                        )
                    })
                    .when(is_error, |el| {
                        el.child(
                            div()
                                .flex()
                                .justify_center()
                                .child(
                                    div()
                                        .px_3()
                                        .py_1()
                                        .rounded_md()
                                        .bg(rgb(0x7f1d1d))
                                        .text_color(rgb(0xfca5a5))
                                        .text_xs()
                                        .child(status.unwrap_or_default()),
                                ),
                        )
                    }),
            )
            .child(
                div()
                    .relative()
                    .px_3()
                    .py_3()
                    .border_t_1()
                    .border_color(rgb(0x333333))
                    .flex()
                    .flex_row()
                    .gap_2()
                    .items_center()
                    .child(self.model_dropdown.clone())
                    .child(self.thinking_dropdown.clone())
                    .child(div().flex_1().child(self.input.clone()))
                    .child(
                        div()
                            .id("send-btn")
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(28.))
                            .bg(if is_disabled { rgb(0x666666) } else { rgb(0x3b82f6) })
                            .rounded_md()
                            .text_color(rgb(0xffffff))
                            .when(!is_disabled, |this| this.cursor_pointer())
                            .child("➤")
                            .hover(|style| {
                                if !is_disabled {
                                    style.bg(rgb(0x2563eb))
                                } else {
                                    style
                                }
                            })
                            .when(!is_disabled, |this| {
                                this.on_click(cx.listener(|this, _, _window, cx| {
                                    this.send_message(&SendMessage, _window, cx);
                                }))
                            }),
                    )
            )
    }
}
