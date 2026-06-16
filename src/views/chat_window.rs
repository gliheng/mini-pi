use std::{
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::views::title_bar::{TitleBarEvent, TitleBarVariant};
use futures::StreamExt;
use gpui::{
    Bounds, ClipboardItem, Context, FocusHandle, Focusable, IntoElement, InteractiveElement,
    KeyDownEvent, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels,
    PathPromptOptions, Render, ScrollHandle, SharedString, Styled, Task, Window, div, prelude::*,
    canvas, fill, point, px, rgb, svg,
};
use uuid::Uuid;

use crate::config::model_config::{all_models, model_display_name, parse_model_id};
use crate::core::actions::{CancelInlineEdit, CloseWindow, ConfirmInlineEdit, SendMessage};
use crate::core::app::AppStore;
use crate::data::models::{ChatState, Message, MessagePart, PartState, Role};
use crate::data::store::{Store, ThreadMeta, WorkspaceMeta};
use crate::rpc::pi_rpc::{BridgeEvent, PiRpc};
use crate::ui::text_area::TextArea;
use crate::ui::dropdown::{Direction, Dropdown, DropdownEvent, DropdownItem};
use crate::ui::loader::{loader, text_loader};
use crate::ui::markdown::MarkdownRenderer;
use crate::utils::format::truncate_str;
use crate::utils::llm::generate_title;
use crate::views::reasoning::Reasoning;
use crate::views::title_bar::TitleBar;
use crate::views::workspace_manager::{WorkspaceManager, WorkspaceManagerEvent};

pub struct ChatWindow {
    pub thread_id: Option<i64>,
    pub session_file: String,
    pub title_bar: gpui::Entity<TitleBar>,
    pub messages: Vec<Message>,
    pub chat_input: gpui::Entity<TextArea>,
    pub focus_handle: FocusHandle,
    pub state: ChatState,
    pub store: Arc<Store>,
    pub pi: Option<PiRpc>,
    pub at_mention_scroll_handle: ScrollHandle,
    pub command_scroll_handle: ScrollHandle,
    pub _pi_task: Option<Task<()>>,
    pub selected_model: Option<String>,
    pub thinking_level: Option<String>,
    pub model_dropdown: gpui::Entity<Dropdown>,
    pub thinking_dropdown: gpui::Entity<Dropdown>,
    pub reasoning_displays: Vec<Vec<Option<gpui::Entity<Reasoning>>>>,
    pub markdown_displays: Vec<Vec<Option<gpui::Entity<MarkdownRenderer>>>>,
    pub scroll_handle: ScrollHandle,
    pub scroll_locked: bool,
    pub scrollbar_drag_offset_y: Option<Pixels>,
    pub workspaces: Vec<WorkspaceMeta>,
    pub selected_workspace_id: Option<i64>,
    pub show_workspace_manager: bool,
    pub workspace_manager: gpui::Entity<WorkspaceManager>,
    pub pi_restart_count: u32,
    pub editing_message_id: Option<String>,
    pub inline_edit_input: Option<gpui::Entity<TextArea>>,
}

impl ChatWindow {
    pub fn new(cx: &mut Context<Self>, thread: Option<&ThreadMeta>, store: Arc<Store>) -> Self {
        let title: SharedString = thread
            .map(|t| {
                if t.title.is_empty() {
                    "New Thread".into()
                } else {
                    t.title.clone().into()
                }
            })
            .unwrap_or_else(|| "New Thread".into());
        let chat_input = cx.new(|cx| TextArea::new(cx, "Type a message..."));
        let title_bar = cx.new(|_| TitleBar::new(title.clone(), TitleBarVariant::Chat));

        let session_file: String =
            thread
                .and_then(|t| t.session_file.clone())
                .unwrap_or_else(|| {
                    let ns = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos();
                    format!("session_{}.jsonl", ns)
                });
        let is_restoring = thread.is_some();
        let selected_model: Option<String> = thread
            .and_then(|t| t.model.clone())
            .or_else(|| cx.global::<AppStore>().config.default_model.clone());
        let selected_thinking_level: Option<String> = thread
            .and_then(|t| t.thinking_level.clone());

        let mut workspaces = store.list_workspaces().unwrap_or_default();
        if workspaces.is_empty() {
            let default_dir = store.default_workspace_dir();
            std::fs::create_dir_all(&default_dir).ok();
            let default_path_str = default_dir.to_string_lossy().to_string();
            if let Ok(ws) = store.create_workspace("Default", &default_path_str) {
                workspaces.push(ws);
            }
        }
        Self::sort_workspaces(&mut workspaces);
        let config_workspace_name = cx.global::<AppStore>().config.default_workspace_name.clone();
        let selected_workspace_id = config_workspace_name
            .and_then(|name| workspaces.iter().find(|ws| ws.name == name).map(|ws| ws.id))
            .or_else(|| workspaces.first().map(|ws| ws.id));

        // Build model dropdown items
        let model_items: Vec<DropdownItem> = all_models()
            .iter()
            .map(|m| DropdownItem::new(m.id, m.name))
            .collect();

        let model_dropdown = cx.new(|cx| {
            Dropdown::new(
                cx,
                model_display_name(selected_model.as_deref()),
                model_items,
            )
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
            let thinking_label: SharedString = selected_thinking_level
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
            Dropdown::new(cx, thinking_label, thinking_items)
                .with_selected(selected_thinking_level.clone())
                .with_width(px(160.))
                .with_max_height(px(300.))
                .with_direction(Direction::Up)
        });
        let workspace_manager = cx.new(|_| WorkspaceManager::new(workspaces.clone()));

        let mut window = Self {
            thread_id: thread.map(|t| t.id),
            session_file,
            title_bar: title_bar.clone(),
            messages: vec![],
chat_input,
            focus_handle: cx.focus_handle(),
            state: ChatState::Idle,
            store: store.clone(),
            pi: None,
            _pi_task: None,
            selected_model,
            thinking_level: selected_thinking_level,
            model_dropdown: model_dropdown.clone(),
            thinking_dropdown: thinking_dropdown.clone(),
            reasoning_displays: vec![],
            markdown_displays: vec![],
            scroll_handle: ScrollHandle::new(),
            at_mention_scroll_handle: ScrollHandle::new(),
            command_scroll_handle: ScrollHandle::new(),
            scroll_locked: true,
            scrollbar_drag_offset_y: None,
            workspaces,
            selected_workspace_id,
            show_workspace_manager: false,
            workspace_manager: workspace_manager.clone(),
            pi_restart_count: 0,
            editing_message_id: None,
            inline_edit_input: None,
        };

        if is_restoring {
            window.spawn_pi(true, cx);
        }

        // Set initial workspace on chat input
        if let Some(ws_id) = window.selected_workspace_id {
            if let Some(ws) = window.workspaces.iter().find(|ws| ws.id == ws_id) {
                window.chat_input.update(cx, |ci, cx| {
                    ci.set_workspace(ws.id, PathBuf::from(&ws.path), ws.name.clone(), cx);
                });
            }
        }

        // Subscribe to chat input events (re-render on changes)
        cx.observe(&window.chat_input, |_, _, cx| {
            cx.notify();
        })
        .detach();

        // Subscribe to title bar events
        cx.subscribe(
            &title_bar,
            |this, _title_bar, event: &TitleBarEvent, cx| match event {
                TitleBarEvent::ToggleUserPanel => {}
                TitleBarEvent::ExportHtml => {
                    let rx = cx.prompt_for_paths(PathPromptOptions {
                        files: false,
                        directories: true,
                        multiple: false,
                        prompt: Some("Choose a folder to export the session HTML".into()),
                    });
                    let session_file = this.session_file.clone();
                    cx.spawn(async move |weak, cx| {
                        if let Ok(Ok(Some(paths))) = rx.await {
                            if let Some(dir) = paths.first() {
                                let file_name = session_file
                                    .rsplit_once('.')
                                    .map(|(name, _)| format!("{}.html", name))
                                    .unwrap_or_else(|| "session.html".to_string());
                                let output_path = dir.join(&file_name);
                                let path_str = output_path.to_string_lossy().to_string();
                                let _ = weak.update(cx, |window, _cx| {
                                    if let Some(ref mut pi) = window.pi {
                                        if let Err(e) = pi.send_export_html(Some(&path_str), None) {
                                            eprintln!("[mini-pi] send_export_html failed: {}", e);
                                        }
                                    }
                                });
                            }
                        }
                    })
                    .detach();
                }
                TitleBarEvent::OpenWorkspace => {
                    let workspace_dir: Option<PathBuf> = this
                        .selected_workspace_id
                        .and_then(|id| this.workspaces.iter().find(|ws| ws.id == id))
                        .map(|ws| PathBuf::from(&ws.path));
                    if let Some(dir) = workspace_dir {
                        cx.reveal_path(&dir);
                    }
                }
            },
        )
        .detach();

        // Subscribe to model dropdown selection events
        cx.subscribe(
            &model_dropdown,
            |this, _dropdown, event: &DropdownEvent, cx| {
                let DropdownEvent::Selected { id } = event;
                this.selected_model = Some(id.clone());
                cx.update_global(|app_store: &mut AppStore, _| {
                    app_store.config.default_model = Some(id.clone());
                    if let Err(e) = app_store.config.save() {
                        eprintln!("[mini-pi] failed to save config: {}", e);
                    }
                });
                if let Some(thread_id) = this.thread_id {
                    let _ =
                        this.store
                            .update_thread(thread_id, None, None, None, Some(Some(id)), None, None);
                }
                if let Some(ref mut pi) = this.pi {
                    if let Some((provider, model)) = parse_model_id(id) {
                        println!("[mini-pi] setting model: provider={} model={}", provider, model);
                        if let Err(e) = pi.send_set_model(provider, model, None) {
                            eprintln!("[mini-pi] send_set_model failed: {}", e);
                        }
                    }
                }
            },
        )
        .detach();

        // Subscribe to thinking dropdown selection events
        cx.subscribe(
            &thinking_dropdown,
            |this, _dropdown, event: &DropdownEvent, cx| {
                let DropdownEvent::Selected { id } = event;
                this.thinking_level = Some(id.clone());
                if let Some(thread_id) = this.thread_id {
                    let _ = this.store.update_thread(
                        thread_id,
                        None,
                        None,
                        None,
                        None,
                        Some(Some(id)),
                        None,
                    );
                }
                if let Some(ref mut pi) = this.pi {
                    if let Err(e) = pi.send_set_thinking_level(id, None) {
                        eprintln!("[mini-pi] send_set_thinking_level failed: {}", e);
                    }
                }
                cx.notify();
            },
        )
        .detach();

        cx.subscribe(
            &workspace_manager,
            |this, _manager, event: &WorkspaceManagerEvent, cx| match event {
                WorkspaceManagerEvent::AddRequested => this.add_workspace(cx),
                WorkspaceManagerEvent::CloseRequested => this.close_workspace_manager(cx),
                WorkspaceManagerEvent::DeleteRequested { workspace_id } => {
                    this.delete_workspace(*workspace_id, cx);
                }
            },
        )
        .detach();

        window
    }

    fn sort_workspaces(workspaces: &mut Vec<WorkspaceMeta>) {
        workspaces.sort_by(|a, b| {
            if a.name == "Default" {
                std::cmp::Ordering::Less
            } else if b.name == "Default" {
                std::cmp::Ordering::Greater
            } else {
                a.name.cmp(&b.name)
            }
        });
    }

    fn sync_workspace_manager(&mut self, cx: &mut Context<Self>) {
        let workspaces = self.workspaces.clone();
        self.workspace_manager.update(cx, |manager, _cx| {
            manager.set_workspaces(workspaces);
        });
    }

    fn close_workspace_manager(&mut self, cx: &mut Context<Self>) {
        self.show_workspace_manager = false;
        cx.notify();
    }

    fn add_workspace(&mut self, cx: &mut Context<Self>) {
        let store = self.store.clone();
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |weak, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                if let Some(path) = paths.first() {
                    let name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("Workspace")
                        .to_string();
                    let path_str = path.to_string_lossy().to_string();
                    match store.create_workspace(&name, &path_str) {
                        Ok(workspace) => {
                            let ws_id = workspace.id;
                            let ws_name = workspace.name.clone();
                            let _ = weak.update(cx, |window, cx| {
                                window.workspaces.push(workspace);
                                Self::sort_workspaces(&mut window.workspaces);
                                window.selected_workspace_id = Some(ws_id);
                                cx.update_global(|app_store: &mut AppStore, _| {
                                    app_store.config.default_workspace_name = Some(ws_name);
                                    if let Err(e) = app_store.config.save() {
                                        eprintln!("[mini-pi] failed to save config: {}", e);
                                    }
                                });
                                window.sync_workspace_manager(cx);
                                cx.notify();
                            });
                        }
                        Err(e) => {
                            eprintln!("[mini-pi] failed to create workspace: {}", e);
                        }
                    }
                }
            }
        })
        .detach();
    }

    fn delete_workspace(&mut self, workspace_id: i64, cx: &mut Context<Self>) {
        if let Err(e) = self.store.delete_workspace(workspace_id) {
            eprintln!("[mini-pi] failed to delete workspace: {}", e);
            return;
        }

        let deleted_name = self
            .workspaces
            .iter()
            .find(|ws| ws.id == workspace_id)
            .map(|ws| ws.name.clone());
        self.workspaces
            .retain(|workspace| workspace.id != workspace_id);
        if self.selected_workspace_id == Some(workspace_id) {
            self.selected_workspace_id = self.workspaces.first().map(|workspace| workspace.id);
        }
        cx.update_global(|app_store: &mut AppStore, _| {
            if let Some(ref name) = deleted_name {
                if app_store.config.default_workspace_name.as_deref() == Some(name) {
                    app_store.config.default_workspace_name = None;
                    if let Err(e) = app_store.config.save() {
                        eprintln!("[mini-pi] failed to save config: {}", e);
                    }
                }
            }
        });
        self.sync_workspace_manager(cx);
        cx.notify();
    }

    fn spawn_pi(&mut self, restoring: bool, cx: &mut Context<Self>) -> bool {
        let session_path = self.store.sessions_dir().join(&self.session_file);
        let workspace_dir: Option<PathBuf> = self
            .selected_workspace_id
            .and_then(|id| self.workspaces.iter().find(|ws| ws.id == id))
            .map(|ws| PathBuf::from(&ws.path));

        let (mut rpc, rx) =
            match PiRpc::spawn(&session_path, self.selected_model.as_deref(), workspace_dir) {
                Ok(result) => result,
                Err(e) => {
                    eprintln!("[mini-pi] failed to spawn pi: {}", e);
                    self.state =
                        ChatState::Error("Failed to start pi agent. Is bun installed?".into());
                    cx.notify();
                    return false;
                }
            };

        eprintln!("[mini-pi] pi spawned with session {}", self.session_file);

        if let Err(e) = rpc.send_get_commands(None) {
            eprintln!("[mini-pi] failed to send get_commands: {}", e);
        }

        if restoring {
            eprintln!("[mini-pi] restoring session, requesting message history");
            if let Err(e) = rpc.send_get_messages(None) {
                eprintln!("[mini-pi] failed to send get_messages: {}", e);
            }
            self.state = ChatState::Loading;
        }

        if let Some(ref level) = self.thinking_level {
            if let Err(e) = rpc.send_set_thinking_level(level, None) {
                eprintln!("[mini-pi] send_set_thinking_level failed: {}", e);
            }
        }

        let weak = cx.entity().downgrade();
        let task = cx.spawn(async move |_, cx: &mut gpui::AsyncApp| {
            let mut rx = rx;
            while let Some(event) = rx.next().await {
                if weak
                    .update(cx, |window, cx| {
                        window.handle_bridge_event(event, cx);
                    })
                    .is_err()
                {
                    break;
                }
            }
            eprintln!("[mini-pi] event loop ended");
        });

        self.pi = Some(rpc);
        self._pi_task = Some(task);
        self.pi_restart_count = 0;
        cx.notify();
        true
    }

    pub fn handle_bridge_event(&mut self, event: BridgeEvent, cx: &mut Context<Self>) {
        eprintln!("[mini-pi] bridge event: {:?}", event);
        match event {
            BridgeEvent::AgentStart => {
                self.state = ChatState::Streaming;
            }
            BridgeEvent::MessageStart { message } => {
                let id = message
                    .as_ref()
                    .and_then(|m| m.get("id"))
                    .and_then(|id| id.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| Uuid::new_v4().to_string());
                self.messages.push(Message {
                    id,
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
                        && m.parts.iter().any(|p| {
                            matches!(
                                p,
                                MessagePart::Text {
                                    state: Some(PartState::Streaming),
                                    ..
                                }
                            )
                        })
                }) {
                    if let Some(part) = msg.parts.iter_mut().find(|p| {
                        matches!(
                            p,
                            MessagePart::Text {
                                state: Some(PartState::Streaming),
                                ..
                            }
                        )
                    }) {
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
                        && m.parts.iter().any(|p| {
                            matches!(
                                p,
                                MessagePart::Reasoning {
                                    state: Some(PartState::Streaming),
                                    ..
                                }
                            )
                        })
                }) {
                    if let Some(part) = msg.parts.iter_mut().find(|p| {
                        matches!(
                            p,
                            MessagePart::Reasoning {
                                state: Some(PartState::Streaming),
                                ..
                            }
                        )
                    }) {
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
            BridgeEvent::ToolEnd {
                tool_name,
                output,
                is_error,
                ..
            } => {
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
                            }
                            .into(),
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
                        if let Some(part) = msg.parts.iter_mut().rev().find(|p| {
                            matches!(
                                p,
                                MessagePart::Text {
                                    state: Some(PartState::Streaming),
                                    ..
                                }
                            )
                        }) {
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
                        if let Some(part) = msg.parts.iter_mut().rev().find(|p| {
                            matches!(
                                p,
                                MessagePart::Reasoning {
                                    state: Some(PartState::Streaming),
                                    ..
                                }
                            )
                        }) {
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
            BridgeEvent::ToolCallEnd {
                call_id,
                name,
                args,
            } => {
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
                eprintln!(
                    "[mini-pi] extension_ui_request method={}, id={}, auto-cancelling",
                    method, id
                );
                if let Some(ref mut pi) = self.pi {
                    let _ = pi
                        .send_extension_ui_response(&id, &serde_json::json!({ "cancelled": true }));
                }
            }
            BridgeEvent::ExtensionError {
                extension_path,
                event,
                error,
            } => {
                eprintln!(
                    "[mini-pi] extension error in {} (event: {}): {}",
                    extension_path, event, error
                );
            }
            BridgeEvent::Disconnected => {
                eprintln!("[mini-pi] pi process disconnected");
                self.pi = None;
                if self.pi_restart_count < 1 {
                    self.pi_restart_count += 1;
                    eprintln!(
                        "[mini-pi] attempting to restart pi (attempt {})",
                        self.pi_restart_count
                    );
                    if !self.spawn_pi(false, cx) {
                        self.state = ChatState::Error(
                            "Pi agent disconnected and could not be restarted.".into(),
                        );
                    }
                } else {
                    self.state = ChatState::Error(
                        "Pi agent disconnected and could not be restarted.".into(),
                    );
                }
            }
            BridgeEvent::Response { command, success, data, error, .. } => {
                if command == "fork" && !success {
                    let err = error.unwrap_or_else(|| "fork failed".to_string());
                    eprintln!("[mini-pi] fork failed: {}", err);
                    self.state = ChatState::Error(format!("Fork failed: {}", err).into());
                }
                if command == "get_commands" && success {
                    if let Some(ref data_val) = data {
                        if let Some(commands) = data_val.get("commands") {
                            if let Some(arr) = commands.as_array() {
                                let items: Vec<crate::ui::text_area::CommandItem> = arr
                                    .iter()
                                    .filter_map(|cmd| {
                                        let name = cmd.get("name")?.as_str()?.to_string();
                                        let description = cmd
                                            .get("description")
                                            .and_then(|d| d.as_str())
                                            .map(|s| s.to_string());
                                        let source = cmd
                                            .get("source")
                                            .and_then(|s| s.as_str())
                                            .unwrap_or("unknown")
                                            .to_string();
                                        Some(crate::ui::text_area::CommandItem {
                                            name,
                                            description,
                                            source,
                                        })
                                    })
                                    .collect();
                                self.chat_input.update(cx, |ci, cx| {
                                    ci.set_commands(items, cx);
                                });
                            }
                        }
                    }
                }
            }
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
                                    id: msg.id.unwrap_or_else(|| Uuid::new_v4().to_string()),
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
                                    id: msg.id.unwrap_or_else(|| Uuid::new_v4().to_string()),
                                    role: Role::Assistant,
                                    parts,
                                });
                            }
                        }
                        "tool" => {
                            for part in msg.parts {
                                if let crate::rpc::pi_rpc::LoadedPart::ToolResult { name, output } =
                                    part
                                {
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
        if matches!(self.state, ChatState::Streaming) && self.scroll_locked {
            self.scroll_handle.scroll_to_bottom();
        }
        cx.notify();
    }

    pub fn send_message(&mut self, _: &SendMessage, _window: &mut Window, cx: &mut Context<Self>) {
        if self.chat_input.read(cx).is_just_selected_mention() {
            self.chat_input.update(cx, |ci, _| ci.clear_just_selected_mention());
            return;
        }
        if self.chat_input.read(cx).is_popup_visible() {
            return;
        }
        // When inline-editing a message, read from the inline input instead of
        // the bottom chat input.
        let content = if self.editing_message_id.is_some() {
            self.inline_edit_input
                .as_ref()
                .map(|i| i.read(cx).content().clone())
                .unwrap_or_else(|| self.chat_input.read(cx).content().clone())
        } else {
            self.chat_input.read(cx).content().clone()
        };
        eprintln!("[mini-pi] send_message: {} chars", content.len());
        if content.is_empty() {
            return;
        }

        // Handle editing an existing user message: fork from it and send the
        // edited prompt into the new branch.
        if let Some(editing_id) = self.editing_message_id.take() {
            self.chat_input.update(cx, |ci, cx| ci.reset(cx));
            let Some(edit_idx) = self.messages.iter().position(|m| m.id == editing_id) else {
                eprintln!("[mini-pi] edited message {} not found", editing_id);
                self.clear_inline_edit_state(cx);
                return;
            };
            if !matches!(self.messages[edit_idx].role, Role::User) {
                eprintln!("[mini-pi] cannot edit non-user message");
                self.clear_inline_edit_state(cx);
                return;
            }
            let entry_id = self.messages[edit_idx].id.clone();
            // Update the edited message text.
            self.messages[edit_idx].parts = vec![MessagePart::Text {
                text: content.clone(),
                state: Some(PartState::Done),
            }];
            // Truncate any messages that came after the edited one.
            self.messages.truncate(edit_idx + 1);
            // Clear stale display entities.
            self.reasoning_displays.truncate(edit_idx + 1);
            self.markdown_displays.truncate(edit_idx + 1);
            self.clear_inline_edit_state(cx);
            self.send_edited_prompt(entry_id, content, cx);
            return;
        }

        if self.pi.is_none() {
            if !self.spawn_pi(false, cx) {
                return;
            }
        }

        self.messages.push(Message {
            id: Uuid::new_v4().to_string(),
            role: Role::User,
            parts: vec![MessagePart::Text {
                text: content.clone(),
                state: Some(PartState::Done),
            }],
        });
        self.chat_input.update(cx, |ci, cx| ci.reset(cx));

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
        let user_count = self
            .messages
            .iter()
            .filter(|m| matches!(m.role, Role::User))
            .count();
        let (title, is_first_message) = if user_count == 1 {
            let temp_title: String = content.chars().take(80).collect();

            let content_clone = content.clone();
            let weak = cx.entity().downgrade();
            cx.spawn(async move |_, cx| {
                let result = smol::unblock(move || generate_title(&content_clone)).await;
                match result {
                    Ok(title) => {
                        let _ = weak.update(cx, |window, cx| {
                            if let Some(tid) = window.thread_id {
                                let _ = window
                                    .store
                                    .update_thread(tid, Some(&title), None, None, None, None, None);
                            }
                            window.title_bar.update(cx, |tb, _| {
                                tb.title = title.into();
                            });
                            cx.update_global(|_: &mut AppStore, _| {});
                            cx.notify();
                        });
                    }
                    Err(e) => {
                        eprintln!("[mini-pi] failed to generate title: {}", e);
                    }
                }
            })
            .detach();

            (temp_title, true)
        } else {
            let existing_title = self
                .store
                .get_thread(tid)
                .ok()
                .flatten()
                .map(|t| t.title)
                .unwrap_or_default();
            (existing_title, false)
        };
        let preview: String = content.chars().take(120).collect();
        let _ = self
            .store
            .update_thread(tid, Some(&title), Some(&preview), None, None, None, None);
        needs_refresh = true;

        if needs_refresh {
            cx.update_global(|_: &mut AppStore, _| {});
        }

        self.state = ChatState::Streaming;
        self.scroll_locked = true;

        if let Some(ref mut pi) = self.pi {
            let _ = pi.send_prompt(&content);
        }

        cx.notify();
    }

    fn send_edited_prompt(&mut self, entry_id: String, content: SharedString, cx: &mut Context<Self>) {
        if self.pi.is_none() {
            // Spawn without requesting history; we already have the local
            // message list and are about to fork from it.
            if !self.spawn_pi(false, cx) {
                return;
            }
        }

        // Ensure a thread row exists.
        if self.thread_id.is_none() {
            match self.store.create_thread("", "") {
                Ok(thread) => {
                    self.thread_id = Some(thread.id);
                    let sf = self.session_file.clone();
                    let model_opt = self.selected_model.as_deref();
                    let thinking_opt = self.thinking_level.as_deref();
                    let _ = self.store.update_thread(
                        thread.id,
                        None,
                        None,
                        Some(Some(&sf)),
                        Some(model_opt),
                        Some(thinking_opt),
                        None,
                    );
                }
                Err(_) => {
                    self.state = ChatState::Error("Failed to create thread".into());
                    cx.notify();
                    return;
                }
            }
        }
        if let Some(tid) = self.thread_id {
            let preview: String = content.chars().take(120).collect();
            let _ = self
                .store
                .update_thread(tid, None, Some(&preview), None, None, None, None);
        }
        self.state = ChatState::Streaming;
        self.scroll_locked = true;
        if let Some(ref mut pi) = self.pi {
            let _ = pi.send_fork(&entry_id, None);
            let _ = pi.send_prompt(&content);
        }
        cx.notify();
    }

    pub fn confirm_inline_edit(
        &mut self,
        _: &ConfirmInlineEdit,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editing_id) = self.editing_message_id.take() else {
            self.clear_inline_edit_state(cx);
            return;
        };
        let Some(edit_idx) = self.messages.iter().position(|m| m.id == editing_id) else {
            eprintln!("[mini-pi] inline edited message {} not found", editing_id);
            self.clear_inline_edit_state(cx);
            return;
        };
        if !matches!(self.messages[edit_idx].role, Role::User) {
            eprintln!("[mini-pi] cannot edit non-user message");
            self.clear_inline_edit_state(cx);
            return;
        }
        let content = self
            .inline_edit_input
            .as_ref()
            .map(|i| i.read(cx).content().clone())
            .unwrap_or_default();
        if content.is_empty() {
            self.clear_inline_edit_state(cx);
            return;
        }
        let entry_id = self.messages[edit_idx].id.clone();
        self.messages[edit_idx].parts = vec![MessagePart::Text {
            text: content.clone(),
            state: Some(PartState::Done),
        }];
        self.messages.truncate(edit_idx + 1);
        self.reasoning_displays.truncate(edit_idx + 1);
        self.markdown_displays.truncate(edit_idx + 1);
        self.clear_inline_edit_state(cx);
        self.send_edited_prompt(entry_id, content, cx);
    }

    pub fn cancel_inline_edit(
        &mut self,
        _: &CancelInlineEdit,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_inline_edit_state(cx);
    }

    fn clear_inline_edit_state(&mut self, cx: &mut Context<Self>) {
        self.editing_message_id = None;
        self.inline_edit_input = None;
        cx.notify();
    }

    fn start_edit_message(
        &mut self,
        msg_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(msg) = self.messages.iter().find(|m| m.id == msg_id) {
            if let Some(MessagePart::Text { text, .. }) = msg.parts.first() {
                self.editing_message_id = Some(msg_id);
                let text = text.clone();
                let inline_input = cx.new(|cx| {
                    TextArea::new(cx, "Edit message...")
                        .with_at_mention(false)
                        .with_slash_commands(false)
                });
                inline_input.update(cx, |ci, cx| {
                    ci.set_content(text, cx);
                });
                inline_input.focus_handle(cx).focus(window);
                self.inline_edit_input = Some(inline_input);
                cx.notify();
            }
        }
    }

    fn render_at_mention_popup(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let chat_input = self.chat_input.read(cx);
        let items = chat_input.popup_items();
        let highlighted = chat_input.popup_highlighted();

        if !items.is_empty() && highlighted < items.len() {
            self.at_mention_scroll_handle.scroll_to_item(highlighted);
        }

        div()
            .relative()
            .px_3()
            .pb_1()
            .child(
                div()
                    .id("at-mention-overlay")
                    .absolute()
                    .occlude()
                    .top(px(-5000.))
                    .left(px(-5000.))
                    .w(px(10000.))
                    .h(px(10000.))
                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _, _, cx| {
                        this.chat_input.update(cx, |ci, cx| ci.close_popup(cx));
                    })),
            )
            .child(
                div()
                    .id("at-mention-popup")
                    .track_scroll(&self.at_mention_scroll_handle)
                    .absolute()
                    .occlude()
                    .bottom(px(0.))
                    .left(px(12.))
                    .right(px(12.))
                    .max_h(px(240.))
                    .overflow_y_scroll()
                    .bg(rgb(0x1e1e1e))
                    .border_1()
                    .border_color(rgb(0x6366f1))
                    .rounded_md()
                    .py_1()
                    .shadow(vec![gpui::BoxShadow {
                        color: gpui::rgba(0x000000aa).into(),
                        offset: gpui::point(px(0.), px(4.)),
                        blur_radius: px(12.),
                        spread_radius: px(0.),
                    }])
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .children(items.iter().enumerate().map(|(idx, item)| {
                        let is_highlighted = idx == highlighted;
                        let icon = if item.is_dir { "folder.svg" } else { "file.svg" };
                        let label: SharedString = item.name.clone().into();
                        let detail: SharedString = if item.relative_path != item.name {
                            item.relative_path.clone().into()
                        } else {
                            "".into()
                        };
                        let item_idx = idx;
                        div()
                            .id(SharedString::from(format!("mention-{}", idx)))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .px_3()
                            .py_1p5()
                            .cursor_pointer()
                            .when(is_highlighted, |s| s.bg(rgb(0x2a2a2a)))
                            .hover(|style| style.bg(rgb(0x2a2a2a)))
                            .child(
                                svg()
                                    .path(icon)
                                    .size(px(14.))
                                    .text_color(if is_highlighted { rgb(0x6366f1) } else { rgb(0x888888) }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_baseline()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(if is_highlighted { rgb(0xffffff) } else { rgb(0xcccccc) })
                                            .child(label),
                                    )
                                    .when(!detail.is_empty(), |s| {
                                        s.child(
                                            div()
                                                .text_xs()
                                                .text_color(rgb(0x666666))
                                                .child(detail),
                                        )
                                    }),
                            )
                            .on_click(cx.listener(move |this, _, _window, cx| {
                                this.chat_input.update(cx, |ci, cx| {
                                    ci.select_mention_at(item_idx, cx);
                                });
                            }))
                    })),
            )
    }

    fn render_messages_scrollbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let scroll_handle = self.scroll_handle.clone();
        let entity = cx.entity();
        div()
            .id("messages-scrollbar")
            .absolute()
            .top_0()
            .right_1()
            .bottom_0()
            .w(px(8.))
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, _| {
                        let viewport_height = scroll_handle.bounds().size.height;
                        let max_scroll = scroll_handle.max_offset().height;
                        if viewport_height <= px(0.) || max_scroll <= px(0.) {
                            return;
                        }

                        let track_bounds = Bounds::from_corners(
                            point(bounds.left() + px(2.), bounds.top() + px(6.)),
                            point(bounds.right() - px(2.), bounds.bottom() - px(6.)),
                        );
                        let track_height = track_bounds.size.height;
                        if track_height <= px(0.) {
                            return;
                        }

                        let content_height = viewport_height + max_scroll;
                        let thumb_height =
                            ((viewport_height / content_height) * track_height).clamp(px(36.), track_height);
                        let progress = (-scroll_handle.offset().y / max_scroll).clamp(0., 1.);
                        let thumb_top = track_bounds.top() + (track_height - thumb_height) * progress;
                        let thumb_bounds = Bounds::from_corners(
                            point(track_bounds.left(), thumb_top),
                            point(track_bounds.right(), thumb_top + thumb_height),
                        );

                        window.on_mouse_event({
                            let entity = entity.clone();
                            move |ev: &MouseDownEvent, _, _, cx| {
                                if !thumb_bounds.contains(&ev.position) {
                                    return;
                                }

                                entity.update(cx, |this, _| {
                                    this.scrollbar_drag_offset_y =
                                        Some(ev.position.y - thumb_bounds.origin.y);
                                });
                            }
                        });
                        window.on_mouse_event({
                            let entity = entity.clone();
                            move |_: &MouseUpEvent, _, _, cx| {
                                entity.update(cx, |this, _| {
                                    this.scrollbar_drag_offset_y = None;
                                });
                            }
                        });
                        window.on_mouse_event({
                            let entity = entity.clone();
                            let scroll_handle = scroll_handle.clone();
                            move |ev: &MouseMoveEvent, _, _, cx| {
                                if !ev.dragging() {
                                    return;
                                }

                                let Some(drag_offset_y) = entity.read(cx).scrollbar_drag_offset_y else {
                                    return;
                                };

                                let draggable_height = (track_height - thumb_height).max(px(0.));
                                if draggable_height <= px(0.) {
                                    return;
                                }

                                let thumb_top = (ev.position.y - drag_offset_y)
                                    .clamp(track_bounds.top(), track_bounds.bottom() - thumb_height);
                                let progress = ((thumb_top - track_bounds.top()) / draggable_height)
                                    .clamp(0., 1.);
                                let offset_y = (max_scroll * progress).clamp(px(0.), max_scroll);
                                scroll_handle.set_offset(point(px(0.), -offset_y));
                                cx.notify(entity.entity_id());
                            }
                        });

                        window.paint_quad(fill(track_bounds, rgb(0x2a2a2a)));
                        window.paint_quad(fill(thumb_bounds, rgb(0x666666)));
                    },
                )
                .size_full(),
            )
    }

    fn render_command_popup(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let chat_input = self.chat_input.read(cx);
        let items = chat_input.slash_command_items();
        let highlighted = chat_input.slash_command_highlighted();

        if !items.is_empty() && highlighted < items.len() {
            self.command_scroll_handle.scroll_to_item(highlighted);
        }

        div()
            .relative()
            .px_3()
            .pb_1()
            .child(
                div()
                    .id("command-overlay")
                    .absolute()
                    .occlude()
                    .top(px(-5000.))
                    .left(px(-5000.))
                    .w(px(10000.))
                    .h(px(10000.))
                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _, _, cx| {
                        this.chat_input.update(cx, |ci, cx| ci.close_popup(cx));
                    })),
            )
            .child(
                div()
                    .id("command-popup")
                    .track_scroll(&self.command_scroll_handle)
                    .absolute()
                    .occlude()
                    .bottom(px(0.))
                    .left(px(12.))
                    .right(px(12.))
                    .max_h(px(240.))
                    .overflow_y_scroll()
                    .bg(rgb(0x1e1e1e))
                    .border_1()
                    .border_color(rgb(0x6366f1))
                    .rounded_md()
                    .py_1()
                    .shadow(vec![gpui::BoxShadow {
                        color: gpui::rgba(0x000000aa).into(),
                        offset: gpui::point(px(0.), px(4.)),
                        blur_radius: px(12.),
                        spread_radius: px(0.),
                    }])
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .children(items.iter().enumerate().map(|(idx, item)| {
                        let is_highlighted = idx == highlighted;
                        let label: SharedString = format!("/{}", item.name).into();
                        let detail: SharedString = item.description.clone().unwrap_or_default().into();
                        let source_label: SharedString = (match item.source.as_str() {
                            "extension" => "Extension",
                            "prompt" => "Prompt",
                            "skill" => "Skill",
                            _ => &item.source,
                        }).to_string().into();
                        let item_idx = idx;
                        div()
                            .id(SharedString::from(format!("command-{}", idx)))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .px_3()
                            .py_1p5()
                            .cursor_pointer()
                            .when(is_highlighted, |s| s.bg(rgb(0x2a2a2a)))
                            .hover(|style| style.bg(rgb(0x2a2a2a)))
                            .child(
                                div()
                                    .w(px(160.))
                                    .overflow_hidden()
                                    .text_sm()
                                    .text_color(if is_highlighted { rgb(0xffffff) } else { rgb(0xcccccc) })
                                    .child(
                                        div()
                                            .whitespace_nowrap()
                                            .text_ellipsis()
                                            .child(label),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .text_xs()
                                    .text_color(rgb(0x666666))
                                    .line_clamp(2)
                                    .when(!detail.is_empty(), |s| s.child(detail)),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .px_1()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(rgb(0x333333))
                                    .text_color(rgb(0x888888))
                                    .child(source_label),
                            )
                            .on_click(cx.listener(move |this, _, _window, cx| {
                                this.chat_input.update(cx, |ci, cx| {
                                    ci.select_command_at(item_idx, cx);
                                });
                            }))
                    })),
            )
    }
}

impl Render for ChatWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = match &self.state {
            ChatState::Idle => None,
            ChatState::Loading => Some(SharedString::from("Loading...")),
            ChatState::Streaming => Some(SharedString::from("Thinking...")),
            ChatState::Error(msg) => Some(msg.clone()),
        };
        let is_error = matches!(self.state, ChatState::Error(_));
        let is_loading = matches!(self.state, ChatState::Loading);
        let is_streaming = matches!(self.state, ChatState::Streaming);
        let input_empty = self.chat_input.read(cx).content().is_empty();
        let is_disabled = is_streaming || is_loading || input_empty;

        // Sync dropdown labels with current state
        let model_label = model_display_name(self.selected_model.as_deref());
        self.model_dropdown.update(cx, |dropdown, _cx| {
            dropdown.label = model_label;
            dropdown.selected_id = self.selected_model.clone();
        });

        let thinking_label: SharedString = self
            .thinking_level
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

        // Ensure markdown displays exist for assistant text parts only
        let mut markdown_entities: Vec<Vec<Option<gpui::Entity<MarkdownRenderer>>>> = Vec::new();
        for (msg_idx, msg) in self.messages.iter().enumerate() {
            let mut msg_markdown: Vec<Option<gpui::Entity<MarkdownRenderer>>> = Vec::new();
            let is_assistant = matches!(msg.role, Role::Assistant);
            for (part_idx, part) in msg.parts.iter().enumerate() {
                if is_assistant && matches!(part, MessagePart::Text { .. }) {
                    if let MessagePart::Text { text, .. } = part {
                        if msg_idx >= self.markdown_displays.len() {
                            self.markdown_displays.resize_with(msg_idx + 1, || vec![]);
                        }
                        let row = &mut self.markdown_displays[msg_idx];
                        if part_idx >= row.len() {
                            row.resize_with(part_idx + 1, || None);
                        }
                        let entity = if let Some(Some(existing)) = row.get(part_idx) {
                            existing.update(cx, |display, _cx| {
                                display.set_content(text);
                            });
                            existing.clone()
                        } else {
                            let new = cx.new(|_cx| MarkdownRenderer::new(text));
                            row[part_idx] = Some(new.clone());
                            new
                        };
                        msg_markdown.push(Some(entity));
                    } else {
                        msg_markdown.push(None);
                    }
                } else {
                    msg_markdown.push(None);
                }
            }
            markdown_entities.push(msg_markdown);
        }

        div()
            .relative()
            .track_focus(&self.focus_handle)
            .on_action(|_: &CloseWindow, window, _| {
                window.remove_window();
            })
            .on_action(cx.listener(Self::send_message))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                if event.keystroke.key == "escape" {
                    if this.chat_input.read(cx).is_popup_visible() {
                        this.chat_input.update(cx, |ci, cx| ci.close_popup(cx));
                    } else if this.show_workspace_manager {
                        this.close_workspace_manager(cx);
                    } else {
                        let model_open = this.model_dropdown.read(cx).is_open;
                        let thinking_open = this.thinking_dropdown.read(cx).is_open;
                        if model_open || thinking_open {
                            // Dropdowns handle their own escape; this is a fallback
                        }
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
                    .flex_1()
                    .min_h(px(0.0))
                    .relative()
                    .child(
                        div()
                            .id("messages")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll_handle)
                            .on_scroll_wheel(cx.listener(|this, event: &gpui::ScrollWheelEvent, window, cx| {
                                let delta = event.delta.pixel_delta(window.line_height());
                                if this.scroll_locked && delta.y > gpui::px(0.) {
                                    this.scroll_locked = false;
                                }
                                if !this.scroll_locked {
                                    let offset_y = this.scroll_handle.offset().y;
                                    let max_y = this.scroll_handle.max_offset().height;
                                    if offset_y.abs() >= max_y - gpui::px(5.) {
                                        this.scroll_locked = true;
                                    }
                                }
                                cx.notify();
                            }))
                            .flex()
                            .flex_col()
                            .p_3()
                            .pr_4()
                            .gap_2()
                            .children(
                                self.messages.iter().enumerate().map(|(msg_idx, msg)| {
                                    let is_user = matches!(msg.role, Role::User);
                                    let msg_id = msg.id.clone();
                                    let msg_reasoning = reasoning_entities.get(msg_idx).cloned().unwrap_or_default();
                                    let msg_markdown = markdown_entities.get(msg_idx).cloned().unwrap_or_default();
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
                                                            let markdown_entity = msg_markdown.get(part_idx).and_then(|e| e.clone());
                                                            let text_to_copy: SharedString = text.clone();
                                                            let is_editing = is_user && self.editing_message_id.as_ref() == Some(&msg_id);
                                                            if is_editing {
                                                                let inline_input = self.inline_edit_input.clone().unwrap_or_else(|| self.chat_input.clone());
                                                                div()
                                                                    .flex()
                                                                    .flex_col()
                                                                    .gap_1()
                                                                    .w_full()
                                                                    .child(
                                                                        div()
                                                                            .px_3()
                                                                            .py_2()
                                                                            .rounded_md()
                                                                            .bg(rgb(0xffffff))
                                                                            .text_color(rgb(0x000000))
                                                                            .text_sm()
                                                                            .child(inline_input)
                                                                    )
                                                                    .child(
                                                                        div()
                                                                            .flex()
                                                                            .gap_2()
                                                                            .justify_end()
                                                                            .child(
                                                                                div()
                                                                                    .id("inline-edit-save")
                                                                                    .px_2()
                                                                                    .py_1()
                                                                                    .rounded_md()
                                                                                    .bg(rgb(0x4f46e5))
                                                                                    .text_color(rgb(0xffffff))
                                                                                    .text_xs()
                                                                                    .cursor_pointer()
                                                                                    .hover(|style| style.bg(rgb(0x4338ca)))
                                                                                    .child("Save")
                                                                                    .on_click(cx.listener(|this, _, _window, cx| {
                                                                                        this.confirm_inline_edit(&ConfirmInlineEdit, _window, cx);
                                                                                    }))
                                                                            )
                                                                            .child(
                                                                                div()
                                                                                    .id("inline-edit-cancel")
                                                                                    .px_2()
                                                                                    .py_1()
                                                                                    .rounded_md()
                                                                                    .bg(rgb(0x3f3f46))
                                                                                    .text_color(rgb(0xd4d4d8))
                                                                                    .text_xs()
                                                                                    .cursor_pointer()
                                                                                    .hover(|style| style.bg(rgb(0x52525b)))
                                                                                    .child("Cancel")
                                                                                    .on_click(cx.listener(|this, _, _window, cx| {
                                                                                        this.cancel_inline_edit(&CancelInlineEdit, _window, cx);
                                                                                    }))
                                                                            )
                                                                    )
                                                            } else {
                                                                div()
                                                                    .flex()
                                                                    .flex_col()
                                                                    .gap_1()
                                                                    .child(
                                                                        div()
                                                                            .py_2()
                                                                            .rounded_md()
                                                                            .when(is_user, |this| {
                                                                                this.px_3()
                                                                                    .bg(rgb(0x6366f1))
                                                                                    .text_color(rgb(0xffffff))
                                                                            })
                                                                            .when(matches!(msg.role, Role::Assistant), |this| {
                                                                                this.text_color(rgb(0xe5e5e5))
                                                                            })
                                                                            .text_sm()
                                                                            .when(is_streaming_empty, |this| {
                                                                                this.child(text_loader())
                                                                            })
                                                                            .when(!is_streaming_empty, |this| {
                                                                                if let Some(md) = markdown_entity {
                                                                                    this.child(md)
                                                                                } else {
                                                                                    this.child(text.clone())
                                                                                }
                                                                            })
                                                                    )
                                                                    .when(!is_user && !is_streaming_empty, |this| {
                                                                        this.child(
                                                                            div()
                                                                                .id(("copy-btn", msg_idx as u64))
                                                                                .flex()
                                                                                .items_center()
                                                                                .cursor_pointer()
                                                                                .child(
                                                                                    svg()
                                                                                        .path("clipboard.svg")
                                                                                        .size(px(12.))
                                                                                        .text_color(rgb(0x888888))
                                                                                        .hover(|style| style.text_color(rgb(0xcccccc))),
                                                                                )
                                                                                .on_click(cx.listener(move |_this, _, _window, cx| {
                                                                                    cx.write_to_clipboard(ClipboardItem::new_string(text_to_copy.to_string()));
                                                                                }))
                                                                        )
                                                                    })
                                                                    .when(is_user && !is_streaming_empty, |this| {
                                                                        let edit_msg_id = msg_id.clone();
                                                                        this.child(
                                                                            div()
                                                                                .id(("edit-btn", msg_idx as u64))
                                                                                .flex()
                                                                                .items_center()
                                                                                .cursor_pointer()
                                                                                .child(
                                                                                    svg()
                                                                                        .path("edit.svg")
                                                                                        .size(px(12.))
                                                                                        .text_color(rgb(0x888888))
                                                                                        .hover(|style| style.text_color(rgb(0xcccccc))),
                                                                                )
                                                                                .on_click(cx.listener(move |this, _, _window, cx| {
                                                                                    this.start_edit_message(edit_msg_id.clone(), _window, cx);
                                                                                }))
                                                                        )
                                                                    })
                                                            }
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
                            .when(self.messages.is_empty(), |el| {
                                el.items_center()
                                    .justify_center()
                                    .child(
                                        svg()
                                            .path("logo.svg")
                                            .text_color(rgb(0x252525))
                                            .size(px(180.)),
                                    )
                            })
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
                                        .w_full()
                                        .px_3()
                                        .py_1()
                                        .text_color(rgb(0x888888))
                                        .text_xs()
                                        .child(text_loader()),
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
                    .child(self.render_messages_scrollbar(cx)),
            )
            .when(self.pi.is_none(), |el| {
                el.child(
                    div()
                        .px_3()
                        .pb_1()
                        .flex()
                        .flex_row()
                        .gap_1()
                        .items_center()
                        .child(
                            div()
                                .id("manage-workspace-btn")
                                .flex()
                                .items_center()
                                .gap_1()
                                .px(px(7.))
                                .py(px(1.))
                                .rounded_md()
                                .border_1()
                                .border_color(if self.show_workspace_manager { rgb(0x4f46e5) } else { rgb(0x555555) })
                                .text_color(if self.show_workspace_manager { rgb(0x4f46e5) } else { rgb(0xaaaaaa) })
                                .text_xs()
                                .cursor_pointer()
                                .hover(|style| {
                                    if self.show_workspace_manager {
                                        style
                                    } else {
                                        style.border_color(rgb(0x888888)).text_color(rgb(0xcccccc))
                                    }
                                })
                                .child(
                                    svg()
                                        .path("manage.svg")
                                        .size(px(10.))
                                        .text_color(if self.show_workspace_manager { rgb(0x4f46e5) } else { rgb(0xaaaaaa) })
                                )
                                .child("Workspace")
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    this.show_workspace_manager = !this.show_workspace_manager;
                                    cx.notify();
                                }))
                        )
                        .children(self.workspaces.iter().map(|ws| {
                            let is_selected = self.selected_workspace_id == Some(ws.id);
                            let ws_id = ws.id;
                            let ws_name = ws.name.clone();
                            let name: SharedString = ws_name.clone().into();
                            div()
                                .id(SharedString::from(format!("ws-{}", ws_id)))
                                .flex()
                                .items_center()
                                .px_2()
                                .py_0p5()
                                .rounded_md()
                                .bg(if is_selected { rgb(0x6366f1) } else { rgb(0x2a2a2a) })
                                .text_color(if is_selected { rgb(0xffffff) } else { rgb(0xaaaaaa) })
                                .text_xs()
                                .cursor_pointer()
                                .hover(|style| if is_selected { style } else { style.bg(rgb(0x333333)) })
                                .when(ws.name == "Default", |this| {
                                    this.gap_1().child(
                                        svg()
                                            .path("folder.svg")
                                            .size(px(10.))
                                            .text_color(if is_selected { rgb(0xffffff) } else { rgb(0xaaaaaa) })
                                    )
                                })
                                .child(name)
                                .on_click(cx.listener(move |this, _, _window, cx| {
                                    this.selected_workspace_id = Some(ws_id);
                                    let ws_dir = this.workspaces.iter().find(|w| w.id == ws_id).map(|w| PathBuf::from(&w.path));
                                    let ws_name_for_global = ws_name.clone();
                                    let ws_name_for_input = ws_name.clone();
                                    if let Some(dir) = ws_dir {
                                        this.chat_input.update(cx, |ci, cx| {
                                            ci.set_workspace(ws_id, dir, ws_name_for_input, cx);
                                        });
                                    }
                                    cx.update_global(|app_store: &mut AppStore, _| {
                                        app_store.config.default_workspace_name = Some(ws_name_for_global);
                                        if let Err(e) = app_store.config.save() {
                                            eprintln!("[mini-pi] failed to save config: {}", e);
                                        }
                                    });
                                    cx.notify();
                                }))
                        }))
                )
            })
            .child(
                div()
                    .px_3()
                    .pb_3()
                    .when(self.chat_input.read(cx).is_at_popup_visible(), |this| {
                        this.child(self.render_at_mention_popup(cx))
                    })
                    .when(self.chat_input.read(cx).is_command_popup_visible(), |this| {
                        this.child(self.render_command_popup(cx))
                    })
                    .child(
                        div()
                            .bg(rgb(0x252525))
                            .rounded_xl()
                            .border_1()
                            .border_color(rgb(0x333333))
                            .px_3()
                            .py_3()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .flex()
                                    .flex_1()
                                    .child(self.chat_input.clone())
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap_2()
                                    .items_center()
                                    .child(self.model_dropdown.clone())
                                    .child(self.thinking_dropdown.clone())
                                    .child(div().flex_1())
                                    .child(
                                        div()
                                            .id("send-btn")
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .size(px(28.))
                                            .bg(if is_disabled { rgb(0x666666) } else { rgb(0x6366f1) })
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
                    )
            )
            .when(self.show_workspace_manager, |this| {
                this.child(self.workspace_manager.clone())
            })
    }
}
