use std::{sync::Arc, time::{SystemTime, UNIX_EPOCH}};

use futures::StreamExt;
use gpui::{
    Context, FocusHandle, IntoElement, ParentElement, Render, SharedString, Styled, Task,
    Window, div, prelude::*, px, rgb,
};

use crate::actions::{CloseWindow, SendMessage};
use crate::app::{AppStore, truncate_str};
use crate::input::TextInput;
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
    pub focus_handle: FocusHandle,
    pub state: ChatState,
    pub store: Arc<Store>,
    pub pi: Option<PiRpc>,
    pub _pi_task: Option<Task<()>>,
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
        let title_bar = cx.new(|_| TitleBar::new(title.clone()).icon("logo.svg"));

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

        let (pi, pi_task) = match PiRpc::spawn(&session_path) {
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
            focus_handle: cx.focus_handle(),
            state: initial_state,
            store,
            pi,
            _pi_task: pi_task,
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
                            });
                        }
                        "assistant" => {
                            if !msg.content_text.is_empty() || msg.thinking.is_some() {
                                self.messages.push(Message {
                                    role: Role::Agent,
                                    content: MessageContent::Text(msg.content_text.into()),
                                    streaming: false,
                                    thinking: msg.thinking,
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
        });
        self.input.update(cx, |input, _| input.reset());

        let mut needs_refresh = false;

        if self.thread_id.is_none() {
            match self.store.create_thread("", "") {
                Ok(thread) => {
                    self.thread_id = Some(thread.id);
                    let sf = self.session_file.clone();
                    let _ = self.store.update_thread(
                        thread.id,
                        None,
                        None,
                        Some(Some(&sf)),
                        None,
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
        });
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
            ChatState::Streaming => Some(SharedString::from("Thinking...")),
            ChatState::Error(msg) => Some(msg.clone()),
        };
        let is_error = matches!(self.state, ChatState::Error(_));
        let is_streaming = matches!(self.state, ChatState::Streaming);

        div()
            .track_focus(&self.focus_handle)
            .on_action(|_: &CloseWindow, window, _| {
                window.remove_window();
            })
            .on_action(cx.listener(Self::send_message))
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
                    .children(self.messages.iter().map(|msg| match &msg.content {
                        MessageContent::Text(text) => {
                            let is_user = matches!(msg.role, Role::User);
                            let display = if msg.streaming && text.is_empty() && msg.thinking.is_none() {
                                SharedString::from("...")
                            } else {
                                text.clone()
                            };
                            div()
                                .flex()
                                .when(is_user, |this| this.justify_end())
                                .when(!is_user, |this| this.justify_start())
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .when(msg.thinking.is_some(), |this| {
                                            this.child(
                                                div()
                                                    .px_3()
                                                    .py_1()
                                                    .rounded_md()
                                                    .bg(rgb(0x2a2a2a))
                                                    .text_color(rgb(0x888888))
                                                    .text_xs()
                                                    .max_w(px(400.))
                                                    .child(format!("💭 {}", msg.thinking.as_ref().unwrap()))
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
                                                .max_w(px(400.))
                                                .child(display),
                                        )
                                )
                        }
                        MessageContent::ToolCall { name, args } => {
                            div()
                                .flex()
                                .justify_start()
                                .child(
                                    div()
                                        .px_3()
                                        .py_1()
                                        .rounded_md()
                                        .bg(rgb(0x3b2818))
                                        .text_color(rgb(0xfbbf24))
                                        .text_xs()
                                        .max_w(px(400.))
                                        .child(format!("⚙ {} {}", name, args)),
                                )
                        }
                        MessageContent::ToolResult { name, output } => {
                            div()
                                .flex()
                                .justify_start()
                                .child(
                                    div()
                                        .px_3()
                                        .py_1()
                                        .rounded_md()
                                        .bg(rgb(0x1a1a2e))
                                        .text_color(rgb(0xa5b4fc))
                                        .text_xs()
                                        .max_w(px(400.))
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
                    .px_3()
                    .py_3()
                    .border_t_1()
                    .border_color(rgb(0x333333))
                    .flex()
                    .flex_row()
                    .gap_2()
                    .items_center()
                    .child(div().flex_1().child(self.input.clone()))
                    .child(
                        div()
                            .id("send-btn")
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(28.))
                            .bg(if is_streaming { rgb(0x666666) } else { rgb(0x3b82f6) })
                            .rounded_md()
                            .text_color(rgb(0xffffff))
                            .cursor_pointer()
                            .child("➤")
                            .hover(|style| {
                                if !is_streaming {
                                    style.bg(rgb(0x2563eb))
                                } else {
                                    style
                                }
                            })
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.send_message(&SendMessage, _window, cx);
                            })),
                    ),
            )
    }
}
