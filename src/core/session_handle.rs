use std::path::PathBuf;
use std::sync::Arc;

use futures::StreamExt;
use gpui::{Context, EventEmitter, SharedString, Task};
use uuid::Uuid;

use crate::config::model_config::parse_model_id;
use crate::core::app::AppStore;
use crate::data::models::{ChatState, Message, MessagePart, PartState, Role};
use crate::data::store::Store;
use crate::rpc::pi_rpc::{BridgeEvent, LoadedMessage, LoadedPart, PiRpc};
use crate::ui::text_area::CommandItem;
use crate::utils::format::truncate_str;
use crate::utils::llm::generate_title;

#[derive(Clone, Debug)]
pub struct WorkspaceInfo {
    pub id: i64,
    pub path: PathBuf,
    pub name: String,
}

#[derive(Clone, Debug)]
pub enum SessionEvent {
    Changed,
    ExportHtmlSucceeded { path: PathBuf },
}

pub struct SessionHandle {
    pub thread_id: Option<i64>,
    pub _session_id: String,
    pub session_file: String,
    pub title: SharedString,
    pub messages: Vec<Message>,
    pub state: ChatState,
    pub commands: Vec<CommandItem>,
    pub selected_model: Option<String>,
    pub thinking_level: Option<String>,
    pub workspace: Option<WorkspaceInfo>,
    pub rpc: Option<PiRpc>,
    pub _event_task: Option<Task<()>>,
    pub pending_fork: Option<(String, String)>,
    pub refresh_entry_ids_after_streaming: bool,
    pub pi_restart_count: u32,
    pub store: Arc<Store>,
}

impl EventEmitter<SessionEvent> for SessionHandle {}

impl SessionHandle {
    pub fn new(
        cx: &mut Context<Self>,
        thread_id: Option<i64>,
        session_file: String,
        workspace: Option<WorkspaceInfo>,
        model: Option<String>,
        thinking_level: Option<String>,
        store: Arc<Store>,
        restore_history: bool,
    ) -> Self {
        let session_id = session_file.clone();
        let mut handle = Self {
            thread_id,
            _session_id: session_id,
            session_file,
            title: SharedString::from("New Thread"),
            messages: vec![],
            state: ChatState::Idle,
            commands: vec![],
            selected_model: model.clone(),
            thinking_level: thinking_level.clone(),
            workspace,
            rpc: None,
            _event_task: None,
            pending_fork: None,
            refresh_entry_ids_after_streaming: false,
            pi_restart_count: 0,
            store,
        };

        if let Some(tid) = thread_id
            && let Ok(Some(thread)) = handle.store.get_thread(tid) {
                handle.title = if thread.title.is_empty() {
                    "New Thread".into()
                } else {
                    thread.title.into()
                };
            }

        handle.spawn_pi(restore_history, cx);
        handle
    }

    pub fn is_streaming(&self) -> bool {
        matches!(self.state, ChatState::Streaming)
    }

    pub fn has_error(&self) -> bool {
        matches!(self.state, ChatState::Error(_))
    }

    pub fn set_thread_id(&mut self, thread_id: i64) {
        self.thread_id = Some(thread_id);
    }

    pub fn set_model(&mut self, model: Option<String>, cx: &mut Context<Self>) {
        self.selected_model = model.clone();
        if let Some(ref id) = model {
            if let Some(tid) = self.thread_id {
                let _ = self.store.update_thread(
                    tid,
                    None,
                    None,
                    None,
                    Some(Some(id)),
                    None,
                    None,
                    None,
                );
            }
            if let Some(ref mut rpc) = self.rpc
                && let Some((provider, model_id)) = parse_model_id(id) {
                    eprintln!("[mini-pi] setting model: provider={} model={}", provider, model_id);
                    if let Err(e) = rpc.send_set_model(provider, model_id, None) {
                        eprintln!("[mini-pi] send_set_model failed: {}", e);
                    }
                }
        }
        cx.emit(SessionEvent::Changed);
    }

    pub fn set_thinking_level(&mut self, level: Option<String>, cx: &mut Context<Self>) {
        self.thinking_level = level.clone();
        if let Some(ref id) = level {
            if let Some(tid) = self.thread_id {
                let _ = self.store.update_thread(
                    tid,
                    None,
                    None,
                    None,
                    None,
                    Some(Some(id)),
                    None,
                    None,
                );
            }
            if let Some(ref mut rpc) = self.rpc
                && let Err(e) = rpc.send_set_thinking_level(id, None) {
                    eprintln!("[mini-pi] send_set_thinking_level failed: {}", e);
                }
        }
        cx.emit(SessionEvent::Changed);
    }

    pub fn set_workspace(&mut self, workspace: WorkspaceInfo) {
        self.workspace = Some(workspace);
    }

    pub fn send_message(
        &mut self,
        content: SharedString,
        cx: &mut Context<Self>,
    ) {
        eprintln!("[mini-pi] send_message: {} chars", content.len());
        if content.is_empty() {
            return;
        }

        if self.rpc.is_none()
            && !self.spawn_pi(false, cx) {
                return;
            }

        self.messages.push(Message {
            id: Uuid::new_v4().to_string(),
            entry_id: None,
            role: Role::User,
            parts: vec![MessagePart::Text {
                text: content.clone(),
                state: Some(PartState::Done),
            }],
        });

        if self.thread_id.is_none() {
            match self.store.create_thread("", "") {
                Ok(thread) => {
                    self.thread_id = Some(thread.id);
                    self.title = content.chars().take(80).collect::<String>().into();
                    let sf = self.session_file.clone();
                    let model_opt = self.selected_model.as_deref();
                    let thinking_opt = self.thinking_level.as_deref();
                    let _ = self.store.update_thread(
                        thread.id,
                        Some(&self.title),
                        None,
                        Some(Some(&sf)),
                        Some(model_opt),
                        Some(thinking_opt),
                        None,
                        None,
                    );
                }
                Err(_) => {
                    self.state = ChatState::Error("Failed to create thread".into());
                    cx.emit(SessionEvent::Changed);
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
        let title = if user_count == 1 {
            let temp_title: String = content.chars().take(80).collect();

            let content_clone = content.clone();
            let weak = cx.entity().downgrade();
            cx.spawn(async move |_, cx| {
                let result = smol::unblock(move || generate_title(&content_clone)).await;
                match result {
                    Ok(title) => {
                        let _ = weak.update(cx, |session, cx| {
                            session.title = title.clone().into();
                            if let Some(tid) = session.thread_id {
                                let _ = session.store.update_thread(
                                    tid,
                                    Some(&title),
                                    None,
                                    None,
                                    None,
                                    None,
                                    None,
                                    None,
                                );
                            }
                            cx.emit(SessionEvent::Changed);
                        });
                        let _ = cx.update_global(|_: &mut AppStore, _| {});
                    }
                    Err(e) => {
                        eprintln!("[mini-pi] failed to generate title: {}", e);
                    }
                }
            })
            .detach();

            temp_title
        } else {
            self.store
                .get_thread(tid)
                .ok()
                .flatten()
                .map(|t| t.title)
                .unwrap_or_default()
        };
        let preview: String = content.chars().take(120).collect();
        let _ = self
            .store
            .update_thread(tid, Some(&title), Some(&preview), None, None, None, None, None);

        self.state = ChatState::Streaming;

        if let Some(ref mut rpc) = self.rpc {
            let _ = rpc.send_prompt(&content);
        }

        cx.emit(SessionEvent::Changed);
    }

    pub fn send_edited_prompt(
        &mut self,
        message_id: String,
        content: SharedString,
        cx: &mut Context<Self>,
    ) {
        if self.rpc.is_none()
            && !self.spawn_pi(false, cx) {
                return;
            }

        let Some(edit_idx) = self.messages.iter().position(|m| m.id == message_id) else {
            eprintln!("[mini-pi] edited message {} not found", message_id);
            return;
        };

        if self.messages[edit_idx].entry_id.is_none() {
            self.pending_fork = Some((message_id, content.to_string()));
            if let Some(ref mut rpc) = self.rpc {
                let _ = rpc.send_get_messages(None);
            }
            cx.emit(SessionEvent::Changed);
            return;
        }

        let entry_id = self.messages[edit_idx].entry_id.clone().unwrap();

        self.messages[edit_idx].parts = vec![MessagePart::Text {
            text: content.clone(),
            state: Some(PartState::Done),
        }];
        self.messages.truncate(edit_idx + 1);

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
                        None,
                    );
                }
                Err(_) => {
                    self.state = ChatState::Error("Failed to create thread".into());
                    cx.emit(SessionEvent::Changed);
                    return;
                }
            }
        }
        if let Some(tid) = self.thread_id {
            let preview: String = content.chars().take(120).collect();
            let _ = self
                .store
                .update_thread(tid, None, Some(&preview), None, None, None, None, None);
        }
        self.state = ChatState::Streaming;
        if let Some(ref mut rpc) = self.rpc {
            let _ = rpc.send_navigate_tree(&entry_id, None);
            let _ = rpc.send_prompt(&content);
        }
        self.refresh_entry_ids_after_streaming = true;
        cx.emit(SessionEvent::Changed);
    }

    pub fn abort(&mut self, _cx: &mut Context<Self>) {
        if let Some(ref mut rpc) = self.rpc {
            let _ = rpc.send_abort(None);
        }
    }

    pub fn request_history(&mut self) {
        if let Some(ref mut rpc) = self.rpc {
            let _ = rpc.send_get_messages(None);
        }
    }

    pub fn request_commands(&mut self) {
        if let Some(ref mut rpc) = self.rpc {
            let _ = rpc.send_get_commands(None);
        }
    }

    pub fn export_html(&mut self, output_path: &str) {
        if let Some(ref mut rpc) = self.rpc
            && let Err(e) = rpc.send_export_html(Some(output_path), None) {
                eprintln!("[mini-pi] send_export_html failed: {}", e);
            }
    }

    fn spawn_pi(&mut self, restoring: bool, cx: &mut Context<Self>) -> bool {
        let Some(bridge) = cx.global::<AppStore>().pi_bridge.clone() else {
            self.state = ChatState::Error(
                "Failed to start pi SDK bridge. Run `cd pi-bridge && npm install`.".into(),
            );
            cx.emit(SessionEvent::Changed);
            return false;
        };

        let session_path = self.store.sessions_dir().join(&self.session_file);
        let workspace_dir: Option<PathBuf> = self
            .workspace
            .as_ref()
            .map(|ws| ws.path.clone());

        let session_id = self.session_file.clone();
        let rx = match bridge.create_session(
            session_id.clone(),
            Some(session_path),
            workspace_dir,
            self.selected_model.clone(),
            self.thinking_level.clone(),
        ) {
            Ok(rx) => rx,
            Err(e) => {
                eprintln!("[mini-pi] failed to create SDK session: {}", e);
                self.state = ChatState::Error("Failed to create pi SDK session.".into());
                cx.emit(SessionEvent::Changed);
                return false;
            }
        };

        let mut rpc = PiRpc::new(session_id.clone(), bridge);
        eprintln!("[mini-pi] pi SDK session created: {}", self.session_file);

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

        if let Some(ref level) = self.thinking_level
            && let Err(e) = rpc.send_set_thinking_level(level, None) {
                eprintln!("[mini-pi] send_set_thinking_level failed: {}", e);
            }

        let weak = cx.entity().downgrade();
        let task = cx.spawn(async move |_, cx: &mut gpui::AsyncApp| {
            let mut rx = rx;
            while let Some(event) = rx.next().await {
                let result = weak.update(cx, |session, cx| {
                    session.handle_bridge_event(event, cx)
                });
                match result {
                    Ok((streaming_changed, new_activity)) => {
                        if streaming_changed || new_activity {
                            let (thread_id, is_streaming) = match weak
                                .update(cx, |session, _cx| (session.thread_id, session.is_streaming()))
                            {
                                Ok(v) => v,
                                Err(_) => break,
                            };
                            if let Some(tid) = thread_id {
                                let _ = cx.update_global(|app: &mut AppStore, _cx| {
                                    if is_streaming {
                                        app.streaming_thread_ids.insert(tid);
                                    } else {
                                        app.streaming_thread_ids.remove(&tid);
                                    }
                                });
                            }
                            if new_activity {
                                let _ = cx.update_global(|_: &mut AppStore, _cx| {});
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            eprintln!("[mini-pi] event loop ended");
        });

        self.rpc = Some(rpc);
        self._event_task = Some(task);
        self.pi_restart_count = 0;
        cx.emit(SessionEvent::Changed);
        true
    }

    fn set_has_new_activity_db(&self, value: bool) {
        if let Some(tid) = self.thread_id {
            let md = self
                .store
                .get_thread(tid)
                .ok()
                .flatten()
                .and_then(|t| t.metadata)
                .unwrap_or_else(|| serde_json::json!({}));
            let mut md = md;
            md["has_new_activity"] = serde_json::Value::Bool(value);
            let _ = self.store.update_thread(
                tid,
                None,
                None,
                None,
                None,
                None,
                None,
                Some(Some(&md)),
            );
        }
    }

    pub fn clear_new_activity(&mut self, cx: &mut Context<Self>) {
        self.set_has_new_activity_db(false);
        cx.emit(SessionEvent::Changed);
    }

    fn handle_bridge_event(&mut self, event: BridgeEvent, cx: &mut Context<Self>) -> (bool, bool) {
        eprintln!("[mini-pi] bridge event: {:?}", event);
        let was_streaming = self.is_streaming();
        match event {
            BridgeEvent::AgentStart => {
                self.state = ChatState::Streaming;
            }
            BridgeEvent::MessageStart { message } => {
                let entry_id = message
                    .as_ref()
                    .and_then(|m| m.get("id"))
                    .and_then(|id| id.as_str())
                    .map(|s| s.to_string());
                let id = Uuid::new_v4().to_string();
                self.messages.push(Message {
                    id,
                    entry_id,
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
                if self.refresh_entry_ids_after_streaming {
                    if let Some(ref mut rpc) = self.rpc {
                        let _ = rpc.send_get_messages(None);
                    } else {
                        self.refresh_entry_ids_after_streaming = false;
                    }
                }
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
                    })
                        && let MessagePart::Text { text, .. } = part {
                            let new_text = format!("{}{}", text, content);
                            *text = new_text.into();
                        }
                } else if let Some(msg) = self.messages.last_mut()
                    && matches!(msg.role, Role::Assistant) {
                        msg.parts.push(MessagePart::Text {
                            text: content.into(),
                            state: Some(PartState::Streaming),
                        });
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
                    })
                        && let MessagePart::Reasoning { text, .. } = part {
                            let new_text = format!("{}{}", text, content);
                            *text = new_text.into();
                        }
                } else if let Some(msg) = self.messages.last_mut()
                    && matches!(msg.role, Role::Assistant) {
                        msg.parts.push(MessagePart::Reasoning {
                            text: content.into(),
                            state: Some(PartState::Streaming),
                            provider_metadata: None,
                        });
                    }
            }
            BridgeEvent::ToolStart { name, args, .. } => {
                if let Some(msg) = self.messages.last_mut()
                    && matches!(msg.role, Role::Assistant) {
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
            BridgeEvent::ToolEnd {
                tool_name,
                output,
                is_error,
                ..
            } => {
                if let Some(msg) = self.messages.last_mut()
                    && matches!(msg.role, Role::Assistant) {
                        for part in msg.parts.iter_mut() {
                            if let MessagePart::ToolCall { state, .. } = part
                                && let Some(s) = state {
                                    *s = PartState::Done;
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
            BridgeEvent::Error { message } => {
                self.state = ChatState::Error(message.into());
            }
            BridgeEvent::TextStart => {
                if let Some(msg) = self.messages.last_mut()
                    && matches!(msg.role, Role::Assistant) {
                        msg.parts.push(MessagePart::Text {
                            text: SharedString::from(""),
                            state: Some(PartState::Streaming),
                        });
                    }
            }
            BridgeEvent::TextEnd { .. } => {
                if let Some(msg) = self.messages.last_mut()
                    && matches!(msg.role, Role::Assistant)
                        && let Some(part) = msg.parts.iter_mut().rev().find(|p| {
                            matches!(
                                p,
                                MessagePart::Text {
                                    state: Some(PartState::Streaming),
                                    ..
                                }
                            )
                        })
                            && let MessagePart::Text { state, .. } = part {
                                *state = Some(PartState::Done);
                            }
            }
            BridgeEvent::ThinkingStart => {
                if let Some(msg) = self.messages.last_mut()
                    && matches!(msg.role, Role::Assistant) {
                        msg.parts.push(MessagePart::Reasoning {
                            text: SharedString::from(""),
                            state: Some(PartState::Streaming),
                            provider_metadata: None,
                        });
                    }
            }
            BridgeEvent::ThinkingEnd { .. } => {
                if let Some(msg) = self.messages.last_mut()
                    && matches!(msg.role, Role::Assistant)
                        && let Some(part) = msg.parts.iter_mut().rev().find(|p| {
                            matches!(
                                p,
                                MessagePart::Reasoning {
                                    state: Some(PartState::Streaming),
                                    ..
                                }
                            )
                        })
                            && let MessagePart::Reasoning { state, .. } = part {
                                *state = Some(PartState::Done);
                            }
            }
            BridgeEvent::ToolCallStart { name, call_id } => {
                if let Some(msg) = self.messages.last_mut()
                    && matches!(msg.role, Role::Assistant) {
                        msg.parts.push(MessagePart::ToolCall {
                            tool_call_id: call_id.into(),
                            name: name.into(),
                            args: SharedString::from(""),
                            state: Some(PartState::Streaming),
                        });
                    }
            }
            BridgeEvent::ToolCallDelta { call_id, delta } => {
                if let Some(msg) = self.messages.last_mut()
                    && matches!(msg.role, Role::Assistant)
                        && let Some(part) = msg.parts.iter_mut().find(|p| matches!(p, MessagePart::ToolCall { tool_call_id: id, .. } if id == &SharedString::from(call_id.clone())))
                            && let MessagePart::ToolCall { args, .. } = part {
                                let new_args = format!("{}{}", args, delta);
                                *args = new_args.into();
                            }
            }
            BridgeEvent::ToolCallEnd {
                call_id,
                name,
                args,
            } => {
                if let Some(msg) = self.messages.last_mut()
                    && matches!(msg.role, Role::Assistant)
                        && let Some(part) = msg.parts.iter_mut().find(|p| matches!(p, MessagePart::ToolCall { tool_call_id: id, .. } if id == &SharedString::from(call_id.clone())))
                            && let MessagePart::ToolCall { name: part_name, args: part_args, state, .. } = part {
                                if *part_name == SharedString::from("") || *part_name == SharedString::from(name.clone()) {
                                    *part_name = name.into();
                                }
                                let args_str = serde_json::to_string(&args).unwrap_or_default();
                                *part_args = args_str.into();
                                *state = Some(PartState::Done);
                            }
            }
            BridgeEvent::ToolUpdate { .. } => {}
            BridgeEvent::TurnStart | BridgeEvent::TurnEnd => {}
            BridgeEvent::MessageEnd => {
                if let Some(msg) = self.messages.last_mut()
                    && matches!(msg.role, Role::Assistant) {
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
            BridgeEvent::ExtensionUiRequest { id, method, .. } => {
                eprintln!(
                    "[mini-pi] extension_ui_request method={}, id={}, auto-cancelling",
                    method, id
                );
                if let Some(ref mut rpc) = self.rpc {
                    let _ = rpc
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
                self.rpc = None;
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
            BridgeEvent::Response {
                command,
                success,
                data,
                error,
                ..
            } => {
                if command == "create_session" && success
                    && let Some(ref data_val) = data
                        && let Some(session_file) =
                            data_val.get("sessionFile").and_then(|s| s.as_str())
                        {
                            let file_name = std::path::Path::new(session_file)
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or(session_file)
                                .to_string();
                            if self.session_file != file_name {
                                eprintln!("[mini-pi] SDK session file: {}", file_name);
                                self.session_file = file_name.clone();
                                if let Some(tid) = self.thread_id {
                                    let _ = self.store.update_thread(
                                        tid,
                                        None,
                                        None,
                                        Some(Some(&file_name)),
                                        None,
                                        None,
                                        None,
                                        None,
                                    );
                                }
                            }
                        }
                if command == "fork" && !success {
                    let err = error.as_deref().unwrap_or("fork failed");
                    eprintln!("[mini-pi] fork failed: {}", err);
                    self.state = ChatState::Error(format!("Fork failed: {}", err).into());
                }
                if command == "get_messages" {
                    if !success {
                        let err = error.as_deref().unwrap_or("failed to load messages");
                        eprintln!("[mini-pi] get_messages failed: {}", err);
                        self.state = ChatState::Error(format!("Load failed: {}", err).into());
                        self.pending_fork = None;
                        self.refresh_entry_ids_after_streaming = false;
                    } else if let Some(ref data_val) = data {
                        if let Some(messages_val) = data_val.get("messages") {
                            if let Some(loaded) =
                                crate::rpc::pi_rpc::parse_loaded_messages(messages_val)
                            {
                                if self.pending_fork.is_some() {
                                    self.update_entry_ids(&loaded);
                                    if let Some((msg_id, content)) = self.pending_fork.take() {
                                        if let Some(msg) =
                                            self.messages.iter().find(|m| m.id == msg_id)
                                        {
                                            if msg.entry_id.is_some() {
                                                self.send_edited_prompt(
                                                    msg_id,
                                                    content.into(),
                                                    cx,
                                                );
                                            } else {
                                                eprintln!(
                                                    "[mini-pi] could not resolve entry id for edited message"
                                                );
                                                self.state = ChatState::Error(
                                                    "Could not resolve message entry id".into(),
                                                );
                                            }
                                        } else {
                                            eprintln!(
                                                "[mini-pi] pending fork target message disappeared"
                                            );
                                            self.state =
                                                ChatState::Error("Edited message not found".into());
                                        }
                                    }
                                } else if self.refresh_entry_ids_after_streaming {
                                    self.refresh_entry_ids_after_streaming = false;
                                    self.update_entry_ids(&loaded);
                                } else {
                                    self.load_messages(loaded, cx);
                                }
                            } else {
                                self.state = ChatState::Idle;
                            }
                        } else {
                            self.state = ChatState::Idle;
                        }
                    } else {
                        self.state = ChatState::Idle;
                    }
                }
                if command == "export_html" {
                    if success {
                        if let Some(ref data_val) = data
                            && let Some(path) = data_val.get("path").and_then(|p| p.as_str()) {
                                eprintln!("[mini-pi] session exported to: {}", path);
                                cx.emit(SessionEvent::ExportHtmlSucceeded {
                                    path: PathBuf::from(path),
                                });
                            }
                    } else {
                        let err = error.as_deref().unwrap_or("export failed");
                        eprintln!("[mini-pi] export_html failed: {}", err);
                        self.state = ChatState::Error(format!("Export failed: {}", err).into());
                    }
                }
                if command == "get_commands" && success
                    && let Some(ref data_val) = data
                        && let Some(commands) = data_val.get("commands")
                            && let Some(arr) = commands.as_array() {
                                let items: Vec<CommandItem> = arr
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
                                        Some(CommandItem {
                                            name,
                                            description,
                                            source,
                                        })
                                    })
                                    .collect();
                                self.commands = items;
                            }
            }
        }

        let is_streaming = self.is_streaming();
        let streaming_changed = was_streaming != is_streaming;
        let new_activity = was_streaming && !is_streaming;
        if new_activity {
            self.set_has_new_activity_db(true);
        }
        cx.emit(SessionEvent::Changed);
        (streaming_changed, new_activity)
    }

    fn load_messages(
        &mut self,
        messages: Vec<LoadedMessage>,
        cx: &mut Context<Self>,
    ) {
        eprintln!("[mini-pi] loaded {} messages from history", messages.len());
        self.messages.clear();
        for msg in messages {
            match msg.role.as_str() {
                "user" => {
                    let mut parts = vec![];
                    for part in msg.parts {
                        if let LoadedPart::Text { text } = part {
                            parts.push(MessagePart::Text {
                                text: if text.is_empty() {
                                    SharedString::from("(empty)")
                                } else {
                                    text.into()
                                },
                                state: Some(PartState::Done),
                            });
                        }
                    }
                    if !parts.is_empty() {
                        self.messages.push(Message {
                            id: Uuid::new_v4().to_string(),
                            entry_id: msg.id,
                            role: Role::User,
                            parts,
                        });
                    }
                }
                "assistant" => {
                    let mut parts = vec![];
                    for part in msg.parts {
                        match part {
                            LoadedPart::Text { text } => {
                                parts.push(MessagePart::Text {
                                    text: text.into(),
                                    state: Some(PartState::Done),
                                });
                            }
                            LoadedPart::Thinking { text } => {
                                parts.push(MessagePart::Reasoning {
                                    text: text.into(),
                                    state: Some(PartState::Done),
                                    provider_metadata: None,
                                });
                            }
                            LoadedPart::ToolCall { name, args } => {
                                parts.push(MessagePart::ToolCall {
                                    tool_call_id: SharedString::from(""),
                                    name: name.into(),
                                    args: args.into(),
                                    state: Some(PartState::Done),
                                });
                            }
                            LoadedPart::ToolResult { name, output } => {
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
                            entry_id: msg.id,
                            role: Role::Assistant,
                            parts,
                        });
                    }
                }
                "tool" => {
                    for part in msg.parts {
                        if let LoadedPart::ToolResult { name, output } = part
                            && let Some(last_msg) = self.messages.last_mut()
                                && matches!(last_msg.role, Role::Assistant) {
                                    last_msg.parts.push(MessagePart::ToolResult {
                                        tool_call_id: SharedString::from(""),
                                        name: name.into(),
                                        output: output.into(),
                                        state: Some(PartState::Done),
                                    });
                                }
                    }
                }
                _ => {}
            }
        }
        self.state = ChatState::Idle;
        cx.emit(SessionEvent::Changed);
    }

    fn update_entry_ids(&mut self, loaded: &[LoadedMessage]) {
        use std::collections::HashMap;
        let mut by_role: HashMap<&str, Vec<&LoadedMessage>> = HashMap::new();
        for msg in loaded {
            by_role.entry(msg.role.as_str()).or_default().push(msg);
        }
        let mut indices: HashMap<&str, usize> = HashMap::new();
        for ui_msg in self.messages.iter_mut() {
            let role_str = match ui_msg.role {
                Role::User => "user",
                Role::Assistant => "assistant",
            };
            let idx = indices.entry(role_str).or_insert(0);
            if let Some(loaded_msg) = by_role.get(role_str).and_then(|v| v.get(*idx)) {
                if ui_msg.entry_id.is_none() {
                    ui_msg.entry_id = loaded_msg.id.clone();
                }
                *idx += 1;
            }
        }
    }
}
