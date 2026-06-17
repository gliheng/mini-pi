use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use gpui::{AppContext, BorrowAppContext, Context, Entity, EventEmitter, Subscription, Task, WeakEntity};
use serde_json::json;

use crate::config::app_config::RemoteControlConfig;
use crate::config::model_config;
use crate::core::app::AppStore;
use crate::core::session_handle::{SessionEvent, SessionHandle, WorkspaceInfo};
use crate::data::models::{ChatState, Message, MessagePart, Role};
use crate::data::store::{StoreError, ThreadMeta};
use crate::remote::server;
use crate::remote::tunnel;
use crate::remote::types::{CommandEnvelope, RemoteCommand, RemoteResponse, SseEvent};

#[derive(Clone, Debug, PartialEq)]
pub enum RemoteStatus {
    Disabled,
    Starting,
    Running,
    Error(String),
}

impl RemoteStatus {
    /// A short, stable label suitable for the UI and API status field.
    pub fn label(&self) -> &'static str {
        match self {
            RemoteStatus::Disabled => "disabled",
            RemoteStatus::Starting => "starting",
            RemoteStatus::Running => "running",
            RemoteStatus::Error(_) => "error",
        }
    }

    /// Structured detail for API responses. Avoids leaking the internal Debug format.
    pub fn detail(&self) -> serde_json::Value {
        match self {
            RemoteStatus::Disabled => json!("disabled"),
            RemoteStatus::Starting => json!("starting"),
            RemoteStatus::Running => json!("running"),
            RemoteStatus::Error(msg) => json!({ "error": msg }),
        }
    }
}

#[derive(Clone, Debug)]
pub enum RemoteControllerEvent {
    StatusChanged,
}

pub struct RemoteController {
    pub config: RemoteControlConfig,
    pub status: RemoteStatus,
    pub tunnel_url: Option<String>,
    pub error_message: Option<String>,
    command_sender: Option<UnboundedSender<CommandEnvelope>>,
    command_task: Option<Task<()>>,
    server_handle: Option<server::RemoteServerHandle>,
    tunnel_handle: Option<tunnel::TunnelHandle>,
    target_thread_id: Option<i64>,
    target_session: Option<WeakEntity<SessionHandle>>,
    session_subscription: Option<Subscription>,
    sse_senders: Arc<Mutex<HashMap<i64, Vec<UnboundedSender<SseEvent>>>>>,
    // Cached snapshots for delta-aware SSE streaming.
    last_state_json: Option<serde_json::Value>,
    last_messages: HashMap<String, serde_json::Value>,
}

impl EventEmitter<RemoteControllerEvent> for RemoteController {}

impl RemoteController {
    pub fn new(_cx: &mut Context<Self>, config: RemoteControlConfig) -> Self {
        // The caller is responsible for ensuring `config.enabled` reflects the
        // desired startup state. We never auto-start from the constructor.
        Self {
            config,
            status: RemoteStatus::Disabled,
            tunnel_url: None,
            error_message: None,
            command_sender: None,
            command_task: None,
            server_handle: None,
            tunnel_handle: None,
            target_thread_id: None,
            target_session: None,
            session_subscription: None,
            sse_senders: Arc::new(Mutex::new(HashMap::new())),
            last_state_json: None,
            last_messages: HashMap::new(),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    pub fn is_starting(&self) -> bool {
        matches!(self.status, RemoteStatus::Starting)
    }

    pub fn set_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.config.enabled == enabled || self.is_starting() {
            return;
        }
        self.config.enabled = enabled;
        self.save_config(cx);

        if enabled {
            self.start(cx);
        } else {
            self.stop(cx);
        }
    }

    fn save_config(&self, cx: &mut Context<Self>) {
        cx.update_global(|app: &mut AppStore, _| {
            app.config.remote_control = self.config.clone();
            if let Err(e) = app.config.save() {
                eprintln!("[remote] failed to save config: {}", e);
            }
        });
    }

    pub fn start(&mut self, cx: &mut Context<Self>) {
        if matches!(self.status, RemoteStatus::Starting | RemoteStatus::Running) {
            return;
        }

        self.status = RemoteStatus::Starting;
        self.error_message = None;
        self.tunnel_url = None;
        cx.emit(RemoteControllerEvent::StatusChanged);
        cx.notify();

        let (command_tx, command_rx) = mpsc::unbounded_channel::<CommandEnvelope>();
        self.command_sender = Some(command_tx.clone());
        self.start_command_task(command_rx, cx);

        let (server_handle, bound_port) = match server::start(
            self.config.bind_port,
            self.config.bearer_token.clone(),
            command_tx,
        ) {
            Ok(v) => v,
            Err(e) => {
                self.set_error(format!("HTTP server failed: {}", e), cx);
                return;
            }
        };
        self.server_handle = Some(server_handle);

        let this = cx.entity().downgrade();
        let command_path = self.config.cloudflared.command.clone();
        let token = self.config.cloudflared.tunnel_token.clone();
        let hostname = self.config.cloudflared.hostname.clone();

        let watchdog_this = this.clone();
        cx.spawn(async move |_, cx| {
            smol::Timer::after(std::time::Duration::from_secs(20)).await;
            let _ = watchdog_this.update(cx, |this, cx| {
                if matches!(this.status, RemoteStatus::Starting) {
                    this.set_error(
                        "remote control startup timed out; check that cloudflared is installed and reachable"
                            .to_string(),
                        cx,
                    );
                }
            });
        })
        .detach();

        cx.spawn(async move |_, cx| {
            let start_result = smol::unblock(move || {
                tunnel::start(&command_path, token.as_deref(), hostname.as_deref(), bound_port)
            })
            .await;

            match start_result {
                Ok((handle, url_rx)) => {
                    let keep = this
                        .update(cx, |this, cx| {
                            if !matches!(this.status, RemoteStatus::Starting) {
                                return false;
                            }
                            this.tunnel_handle = Some(handle);
                            cx.emit(RemoteControllerEvent::StatusChanged);
                            cx.notify();
                            true
                        })
                        .unwrap_or(false);
                    if !keep {
                        return;
                    }

                    // Forward tunnel outcomes from a blocking thread so we can keep
                    // monitoring for process exit after the URL is known.
                    let (fwd_tx, mut fwd_rx) = mpsc::unbounded_channel::<tunnel::TunnelOutcome>();
                    std::thread::spawn(move || {
                        let mut url_seen = false;
                        loop {
                            let timeout = if url_seen {
                                std::time::Duration::from_secs(5)
                            } else {
                                tunnel::URL_TIMEOUT
                            };
                            match url_rx.recv_timeout(timeout) {
                                Ok(outcome) => {
                                    if matches!(outcome, tunnel::TunnelOutcome::Url(_)) {
                                        url_seen = true;
                                    }
                                    if fwd_tx.send(outcome).is_err() {
                                        break;
                                    }
                                }
                                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                                    if !url_seen {
                                        let _ = fwd_tx.send(
                                            tunnel::TunnelOutcome::Error(
                                                "timed out waiting for cloudflared URL"
                                                    .to_string(),
                                            ),
                                        );
                                        break;
                                    }
                                }
                                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                            }
                        }
                    });

                    let mut url_seen = false;
                    while let Some(outcome) = fwd_rx.recv().await {
                        match outcome {
                            tunnel::TunnelOutcome::Url(url) if !url_seen => {
                                url_seen = true;
                                let _ = this.update(cx, |this, cx| {
                                    if !matches!(this.status, RemoteStatus::Starting) {
                                        return;
                                    }
                                    this.tunnel_url = Some(url);
                                    this.status = RemoteStatus::Running;
                                    this.error_message = None;
                                    cx.emit(RemoteControllerEvent::StatusChanged);
                                    cx.notify();
                                });
                            }
                            tunnel::TunnelOutcome::Error(e) => {
                                let _ = this.update(cx, |this, cx| {
                                    if matches!(
                                        this.status,
                                        RemoteStatus::Starting | RemoteStatus::Running
                                    ) {
                                        this.set_error(e, cx);
                                    }
                                });
                                break;
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    let _ = this.update(cx, |this, cx| {
                        if matches!(this.status, RemoteStatus::Starting) {
                            this.set_error(e, cx);
                        }
                    });
                }
            }
        })
        .detach();
    }

    pub fn stop(&mut self, cx: &mut Context<Self>) {
        self.shutdown_services();
        self.tunnel_url = None;
        self.error_message = None;
        self.status = RemoteStatus::Disabled;
        self.target_thread_id = None;
        self.target_session = None;
        self.session_subscription = None;
        self.last_state_json = None;
        self.last_messages.clear();
        cx.emit(RemoteControllerEvent::StatusChanged);
        cx.notify();
    }

    fn shutdown_services(&mut self) {
        self.tunnel_handle = None;
        self.server_handle = None;
        self.command_sender = None;
        self.command_task = None;
        self.clear_sse_senders();
    }

    fn set_error(&mut self, message: String, cx: &mut Context<Self>) {
        eprintln!("[remote] {}", message);
        self.shutdown_services();
        self.status = RemoteStatus::Error(message.clone());
        self.error_message = Some(message);
        self.tunnel_url = None;
        self.config.enabled = false;
        self.target_thread_id = None;
        self.target_session = None;
        self.session_subscription = None;
        self.last_state_json = None;
        self.last_messages.clear();
        self.save_config(cx);
        cx.emit(RemoteControllerEvent::StatusChanged);
        cx.notify();
    }

    fn start_command_task(&mut self, mut rx: UnboundedReceiver<CommandEnvelope>, cx: &mut Context<Self>) {
        let task = cx.spawn(async move |this, cx| {
            while let Some(envelope) = rx.recv().await {
                let respond_to = envelope.respond_to;
                let command = envelope.command;
                let result = this.update(cx, |this, cx| this.handle_command(command, cx));
                let value = match result {
                    Ok(value) => value,
                    Err(_) => json!({ "error": "command failed" }),
                };
                let _ = respond_to.send(value);
            }
        });
        self.command_task = Some(task);
    }

    fn handle_command(&mut self, command: RemoteCommand, cx: &mut Context<Self>) -> RemoteResponse {
        match command {
            RemoteCommand::Status => self.status_response(),
            RemoteCommand::ListThreads => self.list_threads_response(cx),
            RemoteCommand::CreateThread {
                workspace_id,
                model_id,
            } => self.create_thread(workspace_id, model_id, cx),
            RemoteCommand::OpenThread { thread_id } => self.open_thread(thread_id, cx),
            RemoteCommand::SendMessage { thread_id, message } => {
                self.send_message(thread_id, message, cx)
            }
            RemoteCommand::GetMessages { thread_id, since_id } => {
                self.get_messages(thread_id, since_id, cx)
            }
            RemoteCommand::Abort { thread_id } => self.abort(thread_id, cx),
            RemoteCommand::SetModel { thread_id, model_id } => {
                self.set_model(thread_id, model_id, cx)
            }
            RemoteCommand::SetWorkspace { thread_id, workspace_id } => {
                self.set_workspace(thread_id, workspace_id, cx)
            }
            RemoteCommand::AddSseSubscriber { thread_id, sender } => {
                self.add_sse_subscriber(thread_id, sender, cx);
                json!(null)
            }
        }
    }

    fn status_response(&self) -> RemoteResponse {
        json!({
            "enabled": self.config.enabled,
            "status": self.status.label(),
            "status_detail": self.status.detail(),
            "tunnel_url": self.tunnel_url,
            "target_thread_id": self.target_thread_id,
        })
    }

    fn list_threads_response(&self, cx: &mut Context<Self>) -> RemoteResponse {
        let store = cx.global::<AppStore>().store.clone();
        match store.list_threads() {
            Ok(threads) => json!(threads.iter().map(thread_to_json).collect::<Vec<_>>()),
            Err(e) => json!({ "error": e.to_string() }),
        }
    }

    fn create_thread(
        &mut self,
        workspace_id: Option<i64>,
        model_id: Option<String>,
        cx: &mut Context<Self>,
    ) -> RemoteResponse {
        if let Some(ref id) = model_id {
            if model_config::get_model_name(id).is_none() {
                return json!({ "error": format!("unknown model_id: {}", id) });
            }
        }

        let store = cx.global::<AppStore>().store.clone();
        let thread = match store.create_thread("", "") {
            Ok(t) => t,
            Err(e) => return json!({ "error": e.to_string() }),
        };

        let workspace = match self.resolve_workspace(workspace_id, cx) {
            Ok(ws) => ws,
            Err(e) => return json!({ "error": e }),
        };

        let mut metadata = json!({});
        if let Some(ref ws) = workspace {
            metadata["workspace_id"] = json!(ws.id);
        }

        let session_file = self.generate_session_file();
        if let Err(e) = store.update_thread(
            thread.id,
            None,
            None,
            Some(Some(&session_file)),
            Some(model_id.as_deref()),
            None,
            None,
            Some(Some(&metadata)),
        ) {
            return json!({ "error": e.to_string() });
        }

        let session = cx.new(|cx| {
            SessionHandle::new(
                cx,
                Some(thread.id),
                session_file,
                workspace,
                model_id,
                None,
                store,
                false,
            )
        });
        let session_file = session.read(cx).session_file.clone();
        cx.update_global(|app: &mut AppStore, _| {
            app.session_manager.register(session_file, session.clone());
        });

        self.set_target_internal(thread.id, Some(session.downgrade()), cx);

        json!({ "thread_id": thread.id })
    }

    fn open_thread(&mut self, thread_id: i64, cx: &mut Context<Self>) -> RemoteResponse {
        let store = cx.global::<AppStore>().store.clone();
        let thread = match store.get_thread(thread_id) {
            Ok(Some(t)) => t,
            Ok(None) => return json!({ "error": "thread not found" }),
            Err(e) => return json!({ "error": e.to_string() }),
        };

        let session_file = match thread.session_file.clone() {
            Some(sf) => sf,
            None => {
                let sf = self.generate_session_file();
                if let Err(e) = store.update_thread(
                    thread_id,
                    None,
                    None,
                    Some(Some(&sf)),
                    None,
                    None,
                    None,
                    None,
                ) {
                    return json!({ "error": e.to_string() });
                }
                sf
            }
        };

        let session = cx.update_global(|app: &mut AppStore, _| {
            app.session_manager.get(&session_file)
        });

        let session = match session {
            Some(s) => s,
            None => {
                let workspace = match thread
                    .metadata
                    .as_ref()
                    .and_then(|md| md.get("workspace_id").and_then(|v| v.as_i64()))
                    .map(|id| self.resolve_workspace(Some(id), cx))
                    .unwrap_or_else(|| self.resolve_workspace(None, cx))
                {
                    Ok(ws) => ws,
                    Err(e) => return json!({ "error": e }),
                };
                let session = cx.new(|cx| {
                    SessionHandle::new(
                        cx,
                        Some(thread_id),
                        session_file,
                        workspace,
                        thread.model.clone(),
                        thread.thinking_level.clone(),
                        store,
                        true,
                    )
                });
                let session_file = session.read(cx).session_file.clone();
                cx.update_global(|app: &mut AppStore, _| {
                    app.session_manager.register(session_file, session.clone());
                });
                session
            }
        };

        self.set_target_internal(thread_id, Some(session.downgrade()), cx);
        json!({ "thread_id": thread_id })
    }

    fn send_message(
        &mut self,
        thread_id: i64,
        message: String,
        cx: &mut Context<Self>,
    ) -> RemoteResponse {
        let session = match self.ensure_target_session(thread_id, cx) {
            Ok(Some(s)) => s,
            Ok(None) => return json!({ "error": "could not open thread" }),
            Err(e) => return json!({ "error": e }),
        };

        let message_id = session.update(cx, |session, cx| {
            session.send_message(message.into(), cx);
            session
                .messages
                .iter()
                .rev()
                .find(|m| matches!(m.role, Role::User))
                .map(|m| m.id.clone())
        });

        match message_id {
            Some(id) => json!({ "message_id": id, "status": "accepted" }),
            None => json!({ "error": "message was not accepted" }),
        }
    }

    fn get_messages(
        &self,
        thread_id: i64,
        since_id: Option<String>,
        cx: &mut Context<Self>,
    ) -> RemoteResponse {
        let messages = self
            .resolve_session(thread_id, cx)
            .map(|e| e.read(cx).messages.clone());

        match messages {
            Some(msgs) => {
                let start_idx = since_id
                    .as_ref()
                    .and_then(|sid| msgs.iter().position(|m| m.id == *sid))
                    .map(|i| i + 1)
                    .unwrap_or(0);
                json!(msgs
                    .iter()
                    .skip(start_idx)
                    .map(message_to_json)
                    .collect::<Vec<_>>())
            }
            None => json!({ "error": "thread not found" }),
        }
    }

    fn abort(&self, thread_id: i64, cx: &mut Context<Self>) -> RemoteResponse {
        if let Some(session) = self.find_session(thread_id, cx) {
            if let Some(session) = session.upgrade() {
                session.update(cx, |session, _cx| session.abort(_cx));
            }
        }
        json!({ "status": "aborted" })
    }

    fn set_model(
        &self,
        thread_id: i64,
        model_id: String,
        cx: &mut Context<Self>,
    ) -> RemoteResponse {
        if model_config::get_model_name(&model_id).is_none() {
            return json!({ "error": format!("unknown model_id: {}", model_id) });
        }
        if let Some(session) = self.find_session(thread_id, cx) {
            if let Some(session) = session.upgrade() {
                session.update(cx, |session, cx| session.set_model(Some(model_id), cx));
                return json!({ "status": "ok" });
            }
        }
        json!({ "error": "thread not found" })
    }

    fn set_workspace(
        &self,
        thread_id: i64,
        workspace_id: i64,
        cx: &mut Context<Self>,
    ) -> RemoteResponse {
        let workspace = match self.resolve_workspace(Some(workspace_id), cx) {
            Ok(Some(ws)) => ws,
            Ok(None) => return json!({ "error": "workspace not found" }),
            Err(e) => return json!({ "error": e }),
        };
        if let Some(session) = self.find_session(thread_id, cx) {
            if let Some(session) = session.upgrade() {
                let ws = workspace.clone();
                session.update(cx, |session, _cx| session.set_workspace(ws));
                if let Err(e) = self.persist_workspace_id(thread_id, workspace_id, cx) {
                    return json!({ "error": e.to_string() });
                }
                return json!({ "status": "ok" });
            }
        }
        json!({ "error": "thread not found" })
    }

    fn persist_workspace_id(
        &self,
        thread_id: i64,
        workspace_id: i64,
        cx: &mut Context<Self>,
    ) -> Result<(), StoreError> {
        let store = cx.global::<AppStore>().store.clone();
        let md = store
            .get_thread(thread_id)?
            .map(|t| t.metadata.unwrap_or_else(|| json!({})))
            .unwrap_or_else(|| json!({}));
        let mut md = md;
        md["workspace_id"] = json!(workspace_id);
        store.update_thread(thread_id, None, None, None, None, None, None, Some(Some(&md)))
    }

    fn ensure_target_session(
        &mut self,
        thread_id: i64,
        cx: &mut Context<Self>,
    ) -> Result<Option<Entity<SessionHandle>>, String> {
        if self.target_thread_id == Some(thread_id) {
            if let Some(ref weak) = self.target_session {
                if let Some(session) = weak.upgrade() {
                    return Ok(Some(session));
                }
            }
        }
        let response = self.open_thread(thread_id, cx);
        if response.get("error").is_some() {
            return Err(response["error"].as_str().unwrap_or("open thread failed").to_string());
        }
        Ok(self.target_session.as_ref().and_then(|w| w.upgrade()))
    }

    fn resolve_session(
        &self,
        thread_id: i64,
        cx: &mut Context<Self>,
    ) -> Option<Entity<SessionHandle>> {
        if self.target_thread_id == Some(thread_id) {
            if let Some(ref weak) = self.target_session {
                if let Some(session) = weak.upgrade() {
                    return Some(session);
                }
            }
        }
        let store = cx.global::<AppStore>().store.clone();
        store
            .get_thread(thread_id)
            .ok()
            .flatten()
            .and_then(|t| t.session_file)
            .and_then(|sf| cx.update_global(|app: &mut AppStore, _| app.session_manager.get(&sf)))
    }

    fn find_session(&self, thread_id: i64, cx: &mut Context<Self>) -> Option<WeakEntity<SessionHandle>> {
        self.resolve_session(thread_id, cx).map(|e| e.downgrade())
    }

    fn set_target_internal(
        &mut self,
        thread_id: i64,
        session: Option<WeakEntity<SessionHandle>>,
        cx: &mut Context<Self>,
    ) {
        self.target_thread_id = Some(thread_id);
        self.target_session = session.clone();
        self.last_state_json = None;
        self.last_messages.clear();
        self.session_subscription = session.and_then(|session| {
            let entity = session.upgrade()?;
            Some(cx.subscribe(
                &entity,
                move |this: &mut RemoteController, _session, _event: &SessionEvent, cx| {
                    this.on_session_changed(cx);
                },
            ))
        });
        cx.emit(RemoteControllerEvent::StatusChanged);
        cx.notify();
    }

    fn on_session_changed(&mut self, cx: &mut Context<Self>) {
        let Some(thread_id) = self.target_thread_id else { return };
        let Some(ref weak) = self.target_session else { return };
        let Some(session) = weak.upgrade() else {
            // Session was dropped; release the target so we don't leak references.
            self.target_thread_id = None;
            self.target_session = None;
            self.session_subscription = None;
            return;
        };
        let session = session.read(cx);
        let messages = session.messages.clone();
        let state = session.state.clone();

        // Build a delta by comparing freshly serialized messages against the cache.
        // This is slightly more work than only serializing the last message, but it
        // guarantees that edits or state changes to any message are detected.
        let mut added = Vec::new();
        let mut updated = Vec::new();
        let mut removed = Vec::new();
        let mut current_ids = std::collections::HashSet::new();

        for msg in messages.iter() {
            current_ids.insert(msg.id.clone());
            let json = message_to_json(msg);
            match self.last_messages.get(&msg.id) {
                Some(prev) if prev == &json => {}
                Some(_) => updated.push(json.clone()),
                None => added.push(json.clone()),
            }
            self.last_messages.insert(msg.id.clone(), json);
        }

        for id in self.last_messages.keys().cloned().collect::<Vec<_>>() {
            if !current_ids.contains(&id) {
                removed.push(id.clone());
                self.last_messages.remove(&id);
            }
        }

        // Prevent the cache from growing without bound on long conversations.
        const MAX_CACHED_MESSAGES: usize = 1000;
        const CACHE_TRIM_TO: usize = 500;
        if self.last_messages.len() > MAX_CACHED_MESSAGES {
            // Send a full snapshot so clients don't misinterpret rebuilt cache entries
            // as brand-new messages.
            let snapshot = json!({
                "state": chat_state_to_json(&state),
                "messages": messages.iter().map(message_to_json).collect::<Vec<_>>(),
            });
            self.broadcast(thread_id, SseEvent::new("update", snapshot));
            // Rebuild the cache from the most recent messages.
            self.last_messages.clear();
            for msg in messages.iter().rev().take(CACHE_TRIM_TO) {
                self.last_messages.insert(msg.id.clone(), message_to_json(msg));
            }
            self.last_state_json = Some(chat_state_to_json(&state));
            return;
        }

        let state_json = chat_state_to_json(&state);
        let state_changed = self.last_state_json.as_ref() != Some(&state_json);
        if state_changed {
            self.last_state_json = Some(state_json.clone());
        }

        if !added.is_empty() || !updated.is_empty() || !removed.is_empty() || state_changed {
            let data = json!({
                "state": if state_changed { Some(state_json) } else { None::<serde_json::Value> },
                "added_messages": added,
                "updated_messages": updated,
                "removed_message_ids": removed,
            });
            let event = SseEvent::new("delta", data);
            self.broadcast(thread_id, event);
        }
    }

    fn add_sse_subscriber(
        &mut self,
        thread_id: i64,
        sender: UnboundedSender<SseEvent>,
        cx: &mut Context<Self>,
    ) {
        // Register the sender before building the snapshot so any session change
        // that happens during snapshot construction is not missed.
        {
            let mut map = self.sse_senders.lock().unwrap();
            map.entry(thread_id).or_default().push(sender.clone());
        }

        // Send an initial full snapshot so the client never waits on an empty stream.
        let snapshot = self.build_full_snapshot(thread_id, cx);
        let _ = sender.send(SseEvent::new("update", snapshot));
    }

    fn build_full_snapshot(&self, thread_id: i64, cx: &mut Context<Self>) -> serde_json::Value {
        let (messages, state) = self
            .resolve_session(thread_id, cx)
            .map(|e| {
                let s = e.read(cx);
                (s.messages.clone(), s.state.clone())
            })
            .unwrap_or_else(|| (Vec::new(), ChatState::Idle));

        json!({
            "state": chat_state_to_json(&state),
            "messages": messages.iter().map(message_to_json).collect::<Vec<_>>(),
        })
    }

    fn broadcast(&mut self, thread_id: i64, event: SseEvent) {
        let mut map = self.sse_senders.lock().unwrap();
        let senders = map.get_mut(&thread_id);
        if let Some(senders) = senders {
            senders.retain(|s| s.send(event.clone()).is_ok());
        }
    }

    fn clear_sse_senders(&self) {
        let mut map = self.sse_senders.lock().unwrap();
        map.clear();
    }

    fn resolve_workspace(
        &self,
        workspace_id: Option<i64>,
        cx: &mut Context<Self>,
    ) -> Result<Option<WorkspaceInfo>, String> {
        let store = cx.global::<AppStore>().store.clone();
        let workspaces = store.list_workspaces().map_err(|e| e.to_string())?;
        match workspace_id {
            Some(id) => workspaces
                .into_iter()
                .find(|w| w.id == id)
                .map(|ws| {
                    Some(WorkspaceInfo {
                        id: ws.id,
                        path: PathBuf::from(&ws.path),
                        name: ws.name,
                    })
                })
                .ok_or_else(|| format!("workspace {} not found", id)),
            None => Ok(workspaces.first().map(|ws| WorkspaceInfo {
                id: ws.id,
                path: PathBuf::from(&ws.path),
                name: ws.name.clone(),
            })),
        }
    }

    fn generate_session_file(&self) -> String {
        format!("session_{}.jsonl", uuid::Uuid::new_v4())
    }
}

fn thread_to_json(t: &ThreadMeta) -> serde_json::Value {
    json!({
        "id": t.id,
        "title": t.title,
        "preview": t.preview,
        "session_file": t.session_file,
        "model": t.model,
        "thinking_level": t.thinking_level,
        "pinned": t.pinned,
        "metadata": t.metadata,
        "created_at": t.created_at,
        "updated_at": t.updated_at,
    })
}

fn message_to_json(m: &Message) -> serde_json::Value {
    json!({
        "id": m.id,
        "entry_id": m.entry_id,
        "role": match m.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        },
        "parts": m.parts.iter().map(part_to_json).collect::<Vec<_>>(),
    })
}

fn part_to_json(p: &MessagePart) -> serde_json::Value {
    match p {
        MessagePart::Text { text, state } => json!({
            "type": "text",
            "text": text.to_string(),
            "state": state.as_ref().map(|s| format!("{:?}", s)),
        }),
        MessagePart::Reasoning { text, state, .. } => json!({
            "type": "thinking",
            "text": text.to_string(),
            "state": state.as_ref().map(|s| format!("{:?}", s)),
        }),
        MessagePart::ToolCall {
            name,
            args,
            state,
            ..
        } => json!({
            "type": "tool_call",
            "name": name.to_string(),
            "args": args.to_string(),
            "state": state.as_ref().map(|s| format!("{:?}", s)),
        }),
        MessagePart::ToolResult {
            name,
            output,
            state,
            ..
        } => json!({
            "type": "tool_result",
            "name": name.to_string(),
            "output": output.to_string(),
            "state": state.as_ref().map(|s| format!("{:?}", s)),
        }),
    }
}

fn chat_state_to_json(state: &ChatState) -> serde_json::Value {
    match state {
        ChatState::Idle => json!("idle"),
        ChatState::Loading => json!("loading"),
        ChatState::Streaming => json!("streaming"),
        ChatState::Error(msg) => json!({ "error": msg.to_string() }),
    }
}
