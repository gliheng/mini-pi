use std::{sync::Arc, time::{SystemTime, UNIX_EPOCH}};

use futures::StreamExt;
use gpui::{
    Context, FocusHandle, IntoElement, KeyDownEvent, MouseButton, ParentElement, Render, SharedString, Styled, Task,
    Window, div, prelude::*, px, rgb, svg,
};

use crate::actions::{CloseWindow, SendMessage};
use crate::app::{AppStore, truncate_str};
use crate::input::TextInput;
use crate::model_config::{list_models, model_display_name, all_models};
use crate::models::{ChatState, Message, MessageContent, Role};
use crate::pi_rpc::{BridgeEvent, PiRpc};
use crate::store::{Store, ThreadMeta};
use crate::title_bar::TitleBar;

pub struct ChatWindow {
    pub thread_id: Option<i64>,
    pub session_file: String,
    pub title_bar: gpui::Entity<TitleBar>,
    pub messages: Vec<Message>,
    pub input: gpui::Entity<TextInput>,
    pub model_search: gpui::Entity<TextInput>,
    pub focus_handle: FocusHandle,
    pub state: ChatState,
    pub store: Arc<Store>,
    pub pi: Option<PiRpc>,
    pub _pi_task: Option<Task<()>>,
    pub selected_model: Option<String>,
    pub show_model_selector: bool,
    pub highlighted_model_index: Option<usize>,
    pub thinking_level: Option<String>,
    pub show_thinking_selector: bool,
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
        let model_search = cx.new(|cx| TextInput::new(cx, "Search models..."));
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
                    if let Err(e) = rpc.send_get_messages() {
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
            if is_restoring { ChatState::Streaming } else { ChatState::Idle }
        } else {
            ChatState::Error("Failed to start pi agent. Is bun installed?".into())
        };

        Self {
            thread_id: thread.map(|t| t.id),
            session_file,
            title_bar,
            messages: vec![],
            input,
            model_search,
            focus_handle: cx.focus_handle(),
            state: initial_state,
            store,
            pi,
            _pi_task: pi_task,
            selected_model,
            show_model_selector: false,
            highlighted_model_index: None,
            thinking_level: None,
            show_thinking_selector: false,
        }
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
            BridgeEvent::AgentEnd => {
                for msg in self.messages.iter_mut() {
                    msg.streaming = false;
                }
                self.state = ChatState::Idle;
            }
            BridgeEvent::TextDelta { content } => {
                if let Some(msg) = self.messages.iter_mut().find(|m| m.streaming && matches!(m.role, Role::Agent)) {
                    if let MessageContent::Text(ref mut text) = msg.content {
                        let new_text = format!("{}{}", text, content);
                        *text = new_text.into();
                    }
                }
            }
            BridgeEvent::ThinkingDelta { content } => {
                if let Some(msg) = self.messages.iter_mut().find(|m| m.streaming && matches!(m.role, Role::Agent)) {
                    if let Some(ref mut thinking) = msg.thinking {
                        thinking.push_str(&content);
                    } else {
                        msg.thinking = Some(content);
                    }
                }
            }
            BridgeEvent::ToolStart { name, args } => {
                for msg in self.messages.iter_mut() {
                    msg.streaming = false;
                }
                self.messages.push(Message {
                    role: Role::Tool,
                    content: MessageContent::ToolCall {
                        name: name.into(),
                        args: args
                            .as_ref()
                            .map(|v| serde_json::to_string(v).unwrap_or_default())
                            .unwrap_or_default()
                            .into(),
                    },
                    streaming: true,
                    thinking: None,
                    thinking_collapsed: false,
                });
            }
            BridgeEvent::ToolOutput { name, output } => {
                if let Some(msg) = self.messages.iter_mut().find(|m| m.streaming && matches!(m.role, Role::Tool)) {
                    msg.streaming = false;
                }
                self.messages.push(Message {
                    role: Role::Tool,
                    content: MessageContent::ToolResult {
                        name: name.into(),
                        output: truncate_str(&output, 500).into(),
                    },
                    streaming: false,
                    thinking: None,
                    thinking_collapsed: false,
                });
            }
            BridgeEvent::Error { message } => {
                self.state = ChatState::Error(message.into());
            }
            BridgeEvent::MessagesLoaded { messages } => {
                eprintln!("[mini-pi] loaded {} messages from history", messages.len());
                for msg in messages {
                    match msg.role.as_str() {
                        "user" => {
                            self.messages.push(Message {
                                role: Role::User,
                                content: MessageContent::Text(if msg.content_text.is_empty() {
                                    SharedString::from("(empty)")
                                } else {
                                    msg.content_text.into()
                                }),
                                streaming: false,
                                thinking: None,
                                thinking_collapsed: false,
                            });
                        }
                        "assistant" => {
                            if !msg.content_text.is_empty() || msg.thinking.is_some() {
                                self.messages.push(Message {
                                    role: Role::Agent,
                                    content: MessageContent::Text(msg.content_text.into()),
                                streaming: false,
                                thinking: msg.thinking,
                                thinking_collapsed: false,
                                });
                            }
                        }
                        "tool" => {
                            if let Some(name) = &msg.tool_name {
                                self.messages.push(Message {
                                    role: Role::Tool,
                                    content: MessageContent::ToolCall {
                                        name: name.clone().into(),
                                        args: msg.tool_args.clone().unwrap_or_default().into(),
                                    },
                                    streaming: false,
                                    thinking: None,
                                    thinking_collapsed: false,
                                });
                            }
                            if let Some(output) = &msg.tool_output {
                                self.messages.push(Message {
                                    role: Role::Tool,
                                    content: MessageContent::ToolResult {
                                        name: SharedString::from(""),
                                        output: output.clone().into(),
                                    },
                                    streaming: false,
                                    thinking: None,
                                    thinking_collapsed: false,
                                });
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
            role: Role::User,
            content: MessageContent::Text(content.clone()),
            streaming: false,
            thinking: None,
            thinking_collapsed: false,
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

        self.messages.push(Message {
            role: Role::Agent,
            content: MessageContent::Text(SharedString::from("")),
            streaming: true,
            thinking: None,
            thinking_collapsed: false,
        });
        self.state = ChatState::Streaming;

        if let Some(ref mut pi) = self.pi {
            let _ = pi.send_prompt(&content);
        }

        cx.notify();
    }

    fn flattened_filtered_models(&self, cx: &Context<Self>) -> Vec<crate::model_config::ModelInfo> {
        let search_query = self.model_search.read(cx).content().to_string().to_lowercase();
        all_models()
            .iter()
            .filter(|model| {
                search_query.is_empty()
                    || model.name.to_lowercase().contains(&search_query)
                    || model.provider.to_lowercase().contains(&search_query)
            })
            .cloned()
            .collect()
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
            ChatState::Streaming => Some(SharedString::from("Thinking...")),
            ChatState::Error(msg) => Some(msg.clone()),
        };
        let is_error = matches!(self.state, ChatState::Error(_));
        let is_streaming = matches!(self.state, ChatState::Streaming);
        let input_empty = self.input.read(cx).content().is_empty();
        let is_disabled = is_streaming || input_empty;

        let current_model = self.selected_model.clone();
        let models = self.flattened_filtered_models(cx);
        let highlighted = self.highlighted_model_index;
        let mut dropdown_items: Vec<gpui::AnyElement> = Vec::new();
        let mut thinking_dropdown_items: Vec<gpui::AnyElement> = Vec::new();

        let thinking_levels = ["off", "minimal", "low", "medium", "high", "xhigh"];
        if self.show_thinking_selector {
            for (idx, level) in thinking_levels.iter().enumerate() {
                let is_selected = self.thinking_level.as_deref() == Some(level);
                let level_owned = level.to_string();
                let display = match *level {
                    "off" => "Off",
                    "minimal" => "Minimal",
                    "low" => "Low",
                    "medium" => "Medium",
                    "high" => "High",
                    "xhigh" => "Extra High",
                    _ => level,
                };
                let item = div()
                    .id(("thinking-item", idx))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .py_1p5()
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(0x2a2a2a)))
                    .child(
                        div()
                            .text_sm()
                            .text_color(if is_selected { rgb(0xffffff) } else { rgb(0xcccccc) })
                            .child(display),
                    )
                    .child(
                        div()
                            .text_color(rgb(0x3b82f6))
                            .child(if is_selected { "✓" } else { "" }),
                    )
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        this.thinking_level = Some(level_owned.clone());
                        this.show_thinking_selector = false;
                        if let Some(ref mut pi) = this.pi {
                            if let Err(e) = pi.send_set_thinking_level(&level_owned) {
                                eprintln!("[mini-pi] send_set_thinking_level failed: {}", e);
                            }
                        }
                        cx.notify();
                    }))
                    .into_any_element();
                thinking_dropdown_items.push(item);
            }
        }

        if self.show_model_selector {
            dropdown_items.push(
                div()
                    .px_2()
                    .py_1p5()
                    .border_b_1()
                    .border_color(rgb(0x333333))
                    .child(self.model_search.clone())
                    .into_any_element(),
            );

            let mut last_provider: Option<&'static str> = None;
            for (idx, model) in models.iter().enumerate() {
                if last_provider != Some(model.provider) {
                    last_provider = Some(model.provider);
                    dropdown_items.push(
                        div()
                            .px_3()
                            .py_1()
                            .text_color(rgb(0x666666))
                            .text_xs()
                            .child(model.provider.to_uppercase())
                            .into_any_element(),
                    );
                }

                let is_selected = current_model.as_deref() == Some(model.id);
                let is_highlighted = highlighted == Some(idx);
                let model_id = model.id.to_string();
                let model_name = model.name;
                let provider_color = match model.provider {
                    "anthropic" => rgb(0xd97757),
                    "openai" => rgb(0x10a37f),
                    "google" => rgb(0x4285f4),
                    "deepseek" => rgb(0x4d6bfa),
                    "xai" => rgb(0xff6b35),
                    _ => rgb(0x888888),
                };
                let item = div()
                    .id(("model-item", idx))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .py_1p5()
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(0x2a2a2a)))
                    .when(is_highlighted, |s| s.bg(rgb(0x2a2a2a)))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .size(px(8.))
                                    .rounded_full()
                                    .bg(provider_color),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(if is_selected { rgb(0xffffff) } else { rgb(0xcccccc) })
                                    .child(model_name),
                            ),
                    )
                    .child(
                        div()
                            .text_color(rgb(0x3b82f6))
                            .child(if is_selected { "✓" } else { "" }),
                    )
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        this.selected_model = Some(model_id.clone());
                        this.show_model_selector = false;
                        this.highlighted_model_index = None;
                        this.model_search.update(cx, |search, _cx| search.reset());
                        if let Some(thread_id) = this.thread_id {
                            let _ = this.store.update_thread(
                                thread_id,
                                None,
                                None,
                                None,
                                Some(Some(&model_id)),
                                None,
                            );
                        }
                        if let Some(ref mut pi) = this.pi {
                            let (provider, model) = model_id.split_once('/').unwrap_or(("anthropic", &model_id));
                            if let Err(e) = pi.send_set_model(provider, model) {
                                eprintln!("[mini-pi] send_set_model failed: {}", e);
                            }
                        }
                        cx.notify();
                    }))
                    .into_any_element();
                dropdown_items.push(item);
            }

            if models.is_empty() && !self.model_search.read(cx).content().is_empty() {
                dropdown_items.push(
                    div()
                        .px_3()
                        .py_3()
                        .text_color(rgb(0x666666))
                        .text_sm()
                        .child("No models found")
                        .into_any_element(),
                );
            }
        }

        div()
            .relative()
            .track_focus(&self.focus_handle)
            .on_action(|_: &CloseWindow, window, _| {
                window.remove_window();
            })
            .on_action(cx.listener(Self::send_message))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                if this.show_model_selector {
                    match event.keystroke.key.as_str() {
                        "escape" => {
                            this.show_model_selector = false;
                            this.highlighted_model_index = None;
                            this.model_search.update(cx, |search, _| search.reset());
                            cx.notify();
                        }
                        "down" => {
                            let models = this.flattened_filtered_models(cx);
                            let count = models.len();
                            if count > 0 {
                                let next = this.highlighted_model_index
                                    .map(|i| (i + 1) % count)
                                    .unwrap_or(0);
                                this.highlighted_model_index = Some(next);
                            }
                            cx.notify();
                        }
                        "up" => {
                            let models = this.flattened_filtered_models(cx);
                            let count = models.len();
                            if count > 0 {
                                let prev = this.highlighted_model_index
                                    .map(|i| if i == 0 { count - 1 } else { i - 1 })
                                    .unwrap_or(count - 1);
                                this.highlighted_model_index = Some(prev);
                            }
                            cx.notify();
                        }
                        "enter" => {
                            if let Some(idx) = this.highlighted_model_index {
                                let models = this.flattened_filtered_models(cx);
                                if let Some(model) = models.get(idx) {
                                    this.selected_model = Some(model.id.to_string());
                                    this.show_model_selector = false;
                                    this.highlighted_model_index = None;
                                    this.model_search.update(cx, |search, _| search.reset());
                                    if let Some(thread_id) = this.thread_id {
                                        let model_id = model.id.to_string();
                                        let _ = this.store.update_thread(
                                            thread_id,
                                            None,
                                            None,
                                            None,
                                            Some(Some(&model_id)),
                                            None,
                                        );
                                    }
                                    if let Some(ref mut pi) = this.pi {
                                        let (provider, model_name) = model.id.split_once('/').unwrap_or(("anthropic", model.id));
                                        let _ = pi.send_set_model(provider, model_name);
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                } else if this.show_thinking_selector {
                    match event.keystroke.key.as_str() {
                        "escape" => {
                            this.show_thinking_selector = false;
                            cx.notify();
                        }
                        _ => {}
                    }
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
                    .children(self.messages.iter().enumerate().map(|(idx, msg)| match &msg.content {
                        MessageContent::Text(text) => {
                            let is_user = matches!(msg.role, Role::User);
                            let display = if msg.streaming && text.is_empty() && msg.thinking.is_none() {
                                SharedString::from("...")
                            } else {
                                text.clone()
                            };
                            let thinking = msg.thinking.clone();
                            let thinking_collapsed = msg.thinking_collapsed;
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
                                        .when(thinking.is_some(), |this| {
                                            this.child(
                                                div()
                                                    .px_3()
                                                    .py_1()
                                                    .rounded_md()
                                                    .bg(rgb(0x2a2a2a))
                                                    .text_color(rgb(0x888888))
                                                    .text_xs()
                                                    .child(
                                                        div()
                                                            .id(("thinking-toggle", idx))
                                                            .flex()
                                                            .flex_row()
                                                            .gap_1()
                                                            .items_center()
                                                            .cursor_pointer()
                                                            .child(
                                                                svg()
                                                                    .path("thinking.svg")
                                                                    .size(px(12.))
                                                                    .text_color(rgb(0x888888)),
                                                            )
                                                            .child(
                                                                div()
                                                                    .child(format!("Thinking {}", if thinking_collapsed { "▶" } else { "▼" }))
                                                            )
                                                            .on_click(cx.listener(move |this, _, _window, cx| {
                                                                if let Some(msg) = this.messages.get_mut(idx) {
                                                                    msg.thinking_collapsed = !msg.thinking_collapsed;
                                                                    cx.notify();
                                                                }
                                                            }))
                                                    )
                                                    .when(!thinking_collapsed, |this| {
                                                        this.child(
                                                            div()
                                                                .mt_1()
                                                                .child(thinking.as_ref().unwrap().clone())
                                                        )
                                                    })
                                            )
                                        })
                                        .child(
                                            div()
                                                .px_3()
                                                .py_2()
                                                .rounded_md()
                                                .when(is_user, |this| {
                                                    this.bg(rgb(0x3b82f6)).text_color(rgb(0xffffff))
                                                })
                                                .when(matches!(msg.role, Role::Agent), |this| {
                                                    this.text_color(rgb(0xe5e5e5))
                                                })
                                                .text_sm()
                                                .child(display),
                                        )
                                )
                        }
                        MessageContent::ToolCall { name, args } => {
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
                        MessageContent::ToolResult { name, output } => {
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
                    }))
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
                    .child(
                        div()
                            .id("model-selector-btn")
                            .flex()
                            .items_center()
                            .gap_1()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(rgb(0x252525))
                            .text_color(rgb(0xaaaaaa))
                            .text_sm()
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(0x333333)))
                            .child(model_display_name(self.selected_model.as_deref()))
                            .child(if self.show_model_selector { "▲" } else { "▼" })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.show_model_selector = !this.show_model_selector;
                                this.highlighted_model_index = None;
                                if this.show_model_selector {
                                    use gpui::Focusable;
                                    let focus_handle = this.model_search.read(cx).focus_handle(cx);
                                    window.focus(&focus_handle);
                                } else {
                                    this.model_search.update(cx, |search, _| search.reset());
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("thinking-level-selector-btn")
                            .flex()
                            .items_center()
                            .gap_1()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(rgb(0x252525))
                            .text_color(rgb(0xaaaaaa))
                            .text_sm()
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(0x333333)))
                            .child(
                                self.thinking_level
                                    .as_ref()
                                    .map(|l| match l.as_str() {
                                        "off" => "Off".to_string(),
                                        "minimal" => "Minimal".to_string(),
                                        "low" => "Low".to_string(),
                                        "medium" => "Medium".to_string(),
                                        "high" => "High".to_string(),
                                        "xhigh" => "Extra High".to_string(),
                                        _ => l.clone(),
                                    })
                                    .unwrap_or_else(|| "Default".to_string()),
                            )
                            .child(if self.show_thinking_selector { "▲" } else { "▼" })
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.show_thinking_selector = !this.show_thinking_selector;
                                this.show_model_selector = false;
                                this.highlighted_model_index = None;
                                this.model_search.update(cx, |search, _| search.reset());
                                cx.notify();
                            })),
                    )
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
            .when(self.show_model_selector || self.show_thinking_selector, |this| {
                this.child(
                    div()
                        .id("dropdown-overlay")
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _window, cx| {
                            this.show_model_selector = false;
                            this.show_thinking_selector = false;
                            this.highlighted_model_index = None;
                            this.model_search.update(cx, |search, _| search.reset());
                            cx.notify();
                        })),
                )
            })
            .when(self.show_model_selector, |this| {
                this.child(
                    div()
                        .id("model-dropdown")
                        .absolute()
                        .bottom(px(56.))
                        .left(px(12.))
                        .w(px(280.))
                        .max_h(px(400.))
                        .overflow_y_scroll()
                        .bg(rgb(0x1e1e1e))
                        .border_1()
                        .border_color(rgb(0x333333))
                        .rounded_md()
                        .py_1()
                        .shadow(vec![gpui::BoxShadow {
                            color: gpui::rgba(0x000000aa).into(),
                            offset: gpui::point(px(0.), px(4.)),
                            blur_radius: px(12.),
                            spread_radius: px(0.),
                        }])
                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                            cx.stop_propagation();
                        })
                        .children(dropdown_items),
                )
            })
            .when(self.show_thinking_selector, |this| {
                this.child(
                    div()
                        .id("thinking-dropdown")
                        .absolute()
                        .bottom(px(56.))
                        .left(px(12.))
                        .w(px(160.))
                        .max_h(px(300.))
                        .overflow_y_scroll()
                        .bg(rgb(0x1e1e1e))
                        .border_1()
                        .border_color(rgb(0x333333))
                        .rounded_md()
                        .py_1()
                        .shadow(vec![gpui::BoxShadow {
                            color: gpui::rgba(0x000000aa).into(),
                            offset: gpui::point(px(0.), px(4.)),
                            blur_radius: px(12.),
                            spread_radius: px(0.),
                        }])
                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                            cx.stop_propagation();
                        })
                        .children(thinking_dropdown_items),
                )
            })
    }
}
