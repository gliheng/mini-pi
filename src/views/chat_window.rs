use std::{path::PathBuf, sync::Arc};

use gpui::{
    AnyElement, AnyWindowHandle, ClipboardItem, Context, Entity, FocusHandle,
    InteractiveElement, IntoElement, KeyDownEvent, Length, ParentElement, PathPromptOptions,
    Pixels, Render, ScrollHandle, SharedString, Styled, Window, div, prelude::*, px, rems, svg,
};


use crate::config::model_config::all_models;
use crate::core::actions::{CancelInlineEdit, CloseWindow, ConfirmInlineEdit, SendMessage, StopStreaming};
use crate::core::app::AppStore;
use crate::core::session_handle::{SessionEvent, SessionHandle, SessionStats, WorkspaceInfo};
use crate::data::models::{ChatState, Message, MessagePart, PartState, Role};
use crate::data::store::{Store, ThreadMeta, WorkspaceMeta};
use crate::rpc::pi_rpc::ImageContent;
use crate::ui::chat_input::ChatInput;
use crate::ui::loader::{loader, text_loader};
use crate::utils::voice::{VoiceRecorder, VoiceState, start_recording, transcribe};
use crate::views::reasoning::Reasoning;
use crate::views::tool_call::ToolCall;
use crate::views::workspace_manager::{WorkspaceManager, WorkspaceManagerEvent};
use gpui_component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_component::input::{Enter, IndentInline, Input, MoveDown, MoveUp};
use gpui_component::notification::Notification;
use gpui_component::select::{SearchableVec, Select, SelectEvent, SelectItem, SelectState};
use gpui_component::text::{TextView, TextViewState};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, IndexPath, Sizable as _, Size, WindowExt as _,
    h_flex, scroll::Scrollbar, status_bar::StatusBar,
};

type ReasoningEntities = Vec<Vec<Option<Entity<Reasoning>>>>;
type MarkdownEntities = Vec<Vec<Option<Entity<TextViewState>>>>;

#[derive(Clone, Debug)]
pub enum PendingAttachment {
    Image {
        path: PathBuf,
        name: String,
        mime_type: String,
        base64: String,
    },
    Text {
        path: PathBuf,
        name: String,
        content: String,
    },
}

fn is_image_file(path: &std::path::Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg")
    )
}

fn mime_type_for_path(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

#[derive(Clone)]
pub struct SelectModelItem {
    id: String,
    name: SharedString,
}

impl SelectItem for SelectModelItem {
    type Value = String;

    fn title(&self) -> SharedString {
        self.name.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.id
    }
}

pub struct ChatWindow {
    pub thread_id: Option<String>,
    pub session_file: String,
    pub title: SharedString,
    pub messages: Vec<Message>,
    pub chat_input: gpui::Entity<ChatInput>,
    pub focus_handle: FocusHandle,
    pub state: ChatState,
    pub store: Arc<Store>,
    pub session: Option<Entity<SessionHandle>>,
    pub session_subscription: Option<gpui::Subscription>,
    pub at_mention_scroll_handle: ScrollHandle,
    pub command_scroll_handle: ScrollHandle,
    pub selected_model: Option<String>,
    pub thinking_level: Option<String>,
    pub model_dropdown: gpui::Entity<SelectState<SearchableVec<SelectModelItem>>>,
    pub thinking_dropdown: gpui::Entity<SelectState<SearchableVec<SelectModelItem>>>,
    pub reasoning_displays: ReasoningEntities,
    pub markdown_displays: MarkdownEntities,
    pub scroll_handle: ScrollHandle,
    pub scroll_locked: bool,
    pub workspaces: Vec<WorkspaceMeta>,
    pub selected_workspace_id: Option<String>,
    pub workspace_manager: gpui::Entity<WorkspaceManager>,
    pub editing_message_id: Option<String>,
    pub inline_edit_input: Option<gpui::Entity<ChatInput>>,
    pub window_handle: AnyWindowHandle,
    pub voice_state: VoiceState,
    pub voice_recorder: Option<VoiceRecorder>,
    pub session_stats: Option<SessionStats>,
    pub pending_attachments: Vec<PendingAttachment>,
}

impl ChatWindow {
    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        thread: Option<&ThreadMeta>,
        store: Arc<Store>,
    ) -> Self {
        let title: SharedString = thread
            .map(|t| {
                if t.title.is_empty() {
                    "New Thread".into()
                } else {
                    t.title.clone().into()
                }
            })
            .unwrap_or_else(|| "New Thread".into());
        let chat_input = cx.new(|cx| ChatInput::new(window, cx, "Type a message..."));

        let thread_id = thread.map(|t| t.id.clone());
        let selected_model: Option<String> = thread
            .and_then(|t| t.model.clone())
            .or_else(|| cx.global::<AppStore>().config.default_model.clone());
        let models = cx.global::<AppStore>().models.clone();
        let selected_thinking_level: Option<String> = if thread.is_some() {
            thread.and_then(|t| t.thinking_level.clone())
        } else {
            cx.global::<AppStore>()
                .config
                .default_thinking_level
                .clone()
                .or(Some("off".to_string()))
        };

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
        // Prefer the workspace saved on this thread's metadata (e.g. set by the
        // remote controller) when reopening it, so the bridged session runs in
        // the originalcwd instead of the always-default workspace.
        let saved_workspace_id = thread.and_then(|t| {
            t.metadata
                .as_ref()
                .and_then(|md| md.get("workspace_id"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        });
        let selected_workspace_id = saved_workspace_id
            .as_ref()
            .and_then(|id| {
                workspaces
                    .iter()
                    .find(|ws| ws.id == *id)
                    .map(|ws| ws.id.clone())
            })
            .or_else(|| {
                workspaces
                    .iter()
                    .find(|ws| ws.name == "Default")
                    .map(|ws| ws.id.clone())
            })
            .or_else(|| workspaces.first().map(|ws| ws.id.clone()));

        // Build model dropdown items
        let model_items: Vec<SelectModelItem> = all_models(&models)
            .iter()
            .map(|m| SelectModelItem {
                id: m.id.clone(),
                name: m.name.clone().into(),
            })
            .collect();
        let model_selected_index = selected_model
            .as_ref()
            .and_then(|id| model_items.iter().position(|m| &m.id == id))
            .map(|row| IndexPath::default().row(row));
        let model_dropdown = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(model_items),
                model_selected_index,
                window,
                cx,
            )
            .searchable(true)
        });

        // Build thinking level dropdown items based on the selected model's map
        let thinking_items =
            Self::thinking_level_items_for_model(&models, selected_model.as_deref());
        let thinking_selected_index = selected_thinking_level
            .as_ref()
            .and_then(|id| thinking_items.iter().position(|m| &m.id == id))
            .map(|row| IndexPath::default().row(row));
        let thinking_dropdown = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(thinking_items),
                thinking_selected_index,
                window,
                cx,
            )
        });
        let workspace_manager = cx.new(|_| WorkspaceManager::new(workspaces.clone()));
        let window_handle = window.window_handle();
        let voice_state = VoiceState::Idle;
        let voice_recorder = None;
        let session_stats = None;
        let pending_attachments = Vec::new();

        let workspace_info = selected_workspace_id
            .as_ref()
            .and_then(|id| workspaces.iter().find(|ws| ws.id == *id))
            .map(|ws| WorkspaceInfo {
                id: ws.id.clone(),
                path: PathBuf::from(&ws.path),
                name: ws.name.clone(),
            });

        let mut window = Self {
            thread_id,
            session_file: String::new(),
            title: title.clone(),
            messages: vec![],
            chat_input,
            focus_handle: cx.focus_handle(),
            state: ChatState::Idle,
            store: store.clone(),
            session: None,
            session_subscription: None,
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
            workspaces,
            selected_workspace_id,
            workspace_manager: workspace_manager.clone(),
            editing_message_id: None,
            inline_edit_input: None,
            window_handle,
            voice_state,
            voice_recorder,
            session_stats,
            pending_attachments,
        };

        let default_model = cx.global::<AppStore>().config.default_model.clone();
        // Only create/attach a session immediately for restored threads so
        // history + slash commands are available right away. For new threads
        // leave the session uncreated so the workspace picker is shown; the
        // session is created lazily in `ensure_session` once the user picks a
        // workspace and sends the first message.
        if thread.is_some() {
            let session = Self::get_or_create_session(
                thread,
                workspace_info.clone(),
                default_model,
                None,
                cx,
            );
            window.attach_session(session, cx);
        }

        // Set initial workspace on chat input
        if let Some(ref ws) = workspace_info {
            window.chat_input.update(cx, |ci, cx| {
                ci.set_workspace(ws.id.clone(), ws.path.clone(), ws.name.clone(), cx);
            });
        }

        // Subscribe to chat input events (re-render on changes)
        cx.subscribe(
            &window.chat_input,
            |_this, _input, _event: &crate::ui::chat_input::ChatInputEvent, cx| {
                cx.notify();
            },
        )
        .detach();

        // Subscribe to model dropdown selection events
        cx.subscribe(
            &model_dropdown,
            |this, _dropdown, event: &SelectEvent<SearchableVec<SelectModelItem>>, cx| {
                if let SelectEvent::Confirm(Some(id)) = event {
                    this.selected_model = Some(id.clone());
                    cx.update_global(|app_store: &mut AppStore, _| {
                        app_store.config.default_model = Some(id.clone());
                        if let Err(e) = app_store.config.save() {
                            eprintln!("[mini-pi] failed to save config: {}", e);
                        }
                    });
                    if let Some(ref session) = this.session {
                        session.update(cx, |session, cx| {
                            session.set_model(Some(id.clone()), cx);
                        });
                    }
                    // When a new model is selected, preserve the current thinking level if
                    // it is still valid for the new model; otherwise fall back to off.
                    this.refresh_thinking_dropdown(cx);
                }
            },
        )
        .detach();

        // Subscribe to thinking dropdown selection events
        cx.subscribe(
            &thinking_dropdown,
            |this, _dropdown, event: &SelectEvent<SearchableVec<SelectModelItem>>, cx| {
                if let SelectEvent::Confirm(Some(id)) = event {
                    this.thinking_level = Some(id.clone());
                    cx.update_global(|app_store: &mut AppStore, _| {
                        app_store.config.default_thinking_level = Some(id.clone());
                        if let Err(e) = app_store.config.save() {
                            eprintln!("[mini-pi] failed to save config: {}", e);
                        }
                    });
                    if let Some(ref session) = this.session {
                        session.update(cx, |session, cx| {
                            session.set_thinking_level(Some(id.clone()), cx);
                        });
                    }
                    cx.notify();
                }
            },
        )
        .detach();

        cx.subscribe(
            &workspace_manager,
            |this, _manager, event: &WorkspaceManagerEvent, cx| match event {
                WorkspaceManagerEvent::AddRequested => this.add_workspace(cx),
                WorkspaceManagerEvent::CloseRequested => {}
                WorkspaceManagerEvent::DeleteRequested { workspace_id } => {
                    this.delete_workspace(workspace_id.clone(), cx);
                }
            },
        )
        .detach();

        window
    }

    const DEFAULT_THINKING_LEVELS: [(&'static str, &'static str); 6] = [
        ("off", "Off"),
        ("minimal", "Minimal"),
        ("low", "Low"),
        ("medium", "Medium"),
        ("high", "High"),
        ("xhigh", "Extra High"),
    ];

    fn thinking_level_items_for_model(
        models: &[crate::config::model_config::ModelInfo],
        model_id: Option<&str>,
    ) -> Vec<SelectModelItem> {
        let map = model_id
            .and_then(|id| models.iter().find(|m| m.id == id))
            .and_then(|m| m.thinking_level_map.as_ref());

        Self::DEFAULT_THINKING_LEVELS
            .iter()
            .filter(|(id, _)| match map {
                Some(m) => !matches!(m.get(*id), Some(None)),
                None => true,
            })
            .map(|(id, label)| SelectModelItem {
                id: (*id).to_string(),
                name: (*label).into(),
            })
            .collect()
    }

    fn refresh_thinking_dropdown(&mut self, cx: &mut Context<Self>) {
        let models = cx.global::<AppStore>().models.clone();
        let items = Self::thinking_level_items_for_model(&models, self.selected_model.as_deref());
        let valid_ids: std::collections::HashSet<String> =
            items.iter().map(|i| i.id.clone()).collect();

        let new_level = self
            .thinking_level
            .as_ref()
            .filter(|id| valid_ids.contains(*id))
            .cloned()
            .or_else(|| {
                items
                    .iter()
                    .find(|i| i.id == "off")
                    .or_else(|| items.first())
                    .map(|i| i.id.clone())
            });

        if new_level != self.thinking_level {
            self.thinking_level = new_level.clone();
            if let Some(ref session) = self.session {
                if let Some(ref level) = new_level {
                    session.update(cx, |session, cx| {
                        session.set_thinking_level(Some(level.clone()), cx);
                    });
                }
            }
        }

        let selected_value = self.thinking_level.clone();
        let items = SearchableVec::new(items);
        let _ = cx.update_window(self.window_handle, |_, window, cx| {
            self.thinking_dropdown.update(cx, |dropdown, cx| {
                dropdown.set_items(items.clone(), window, cx);
                if let Some(ref value) = selected_value {
                    dropdown.set_selected_value(value, window, cx);
                } else {
                    dropdown.set_selected_index(None, window, cx);
                }
            });
        });
        cx.notify();
    }

    fn attach_session(&mut self, session: Entity<SessionHandle>, cx: &mut Context<Self>) {
        self.session = Some(session.clone());
        self.sync_from_session(cx);
        if self.scroll_locked {
            self.scroll_handle.scroll_to_bottom();
        }
        self.session_subscription = Some(cx.subscribe(
            &session,
            |this, _session, event: &SessionEvent, cx| {
                this.sync_from_session(cx);
                if let SessionEvent::ExportHtmlSucceeded { path } = event {
                    let reveal_path = path.clone();
                    let _ = this.window_handle.update(cx, |_, window, cx| {
                        window.push_notification(
                            Notification::success("Session exported to HTML").action({
                                let reveal_path = reveal_path.clone();
                                move |_, _, _| {
                                    let reveal_path = reveal_path.clone();
                                    Button::new("reveal").label("Reveal").on_click(
                                        move |_, _, cx| {
                                            cx.reveal_path(&reveal_path);
                                        },
                                    )
                                }
                            }),
                            cx,
                        );
                    });
                }
                cx.notify();
            },
        ));
        if self.thread_id.is_some() {
            if let Some(ref session) = self.session {
                session.update(cx, |session, cx| {
                    session.clear_new_activity(cx);
                });
            }
            cx.update_global(|_: &mut AppStore, _| {});
        }

        // Ask the SDK for the current session stats so the status bar is
        // populated immediately when opening or switching to a thread.
        if let Some(ref session) = self.session {
            session.update(cx, |session, _cx| {
                session.request_session_stats();
            });
        }
    }

    fn sync_from_session(&mut self, cx: &mut Context<Self>) {
        let Some(ref session) = self.session else {
            return;
        };
        let s = session.read(cx);
        let messages = s.messages.clone();
        let state = s.state.clone();
        let session_file = s.session_file.clone();
        let selected_model = s.selected_model.clone();
        let thinking_level = s.thinking_level.clone();
        let title = s.title.clone();
        let commands = s.commands.clone();

        self.messages = messages;
        self.state = state;
        if matches!(self.state, ChatState::Streaming) && self.scroll_locked {
            self.scroll_handle.scroll_to_bottom();
        }
        self.session_file = session_file;
        self.title = title;
        self.selected_model = selected_model.clone();
        self.thinking_level = thinking_level.clone();
        self.session_stats = s.session_stats.clone();

        self.chat_input.update(cx, |ci, cx| {
            ci.set_commands(commands, cx);
        });
        let _ = cx.update_window(self.window_handle, |_, window, cx| {
            self.model_dropdown.update(cx, |dropdown, cx| {
                if let Some(ref value) = selected_model {
                    dropdown.set_selected_value(value, window, cx);
                } else {
                    dropdown.set_selected_index(None, window, cx);
                }
            });
        });
        self.refresh_thinking_dropdown(cx);
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

    fn open_workspace_manager(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_workspace_manager(cx);
        let manager = self.workspace_manager.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let manager_for_content = manager.clone();
            dialog
                .title("Workspaces")
                .content(move |content, window, cx| {
                    manager_for_content.update(cx, |manager, cx| {
                        content.child(manager.render_dialog_content(window, cx))
                    })
                })
        });
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
            if let Ok(Ok(Some(paths))) = rx.await
                && let Some(path) = paths.first()
            {
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("Workspace")
                    .to_string();
                let path_str = path.to_string_lossy().to_string();
                match store.create_workspace(&name, &path_str) {
                    Ok(workspace) => {
                        let ws_id = workspace.id.clone();
                        let _ = weak.update(cx, |window, cx| {
                            window.workspaces.push(workspace);
                            Self::sort_workspaces(&mut window.workspaces);
                            window.selected_workspace_id = Some(ws_id);
                            window.sync_workspace_manager(cx);
                            cx.notify();
                        });
                    }
                    Err(e) => {
                        eprintln!("[mini-pi] failed to create workspace: {}", e);
                    }
                }
            }
        })
        .detach();
    }

    fn delete_workspace(&mut self, workspace_id: String, cx: &mut Context<Self>) {
        if let Err(e) = self.store.delete_workspace(&workspace_id) {
            eprintln!("[mini-pi] failed to delete workspace: {}", e);
            return;
        }

        self.workspaces
            .retain(|workspace| workspace.id != workspace_id);
        if self.selected_workspace_id == Some(workspace_id.clone()) {
            self.selected_workspace_id = self
                .workspaces
                .first()
                .map(|workspace| workspace.id.clone());
        }
        self.sync_workspace_manager(cx);
        cx.notify();
    }

    fn session_file_from_thread(thread: Option<&ThreadMeta>) -> String {
        thread
            .and_then(|t| t.session_file.clone())
            .unwrap_or_else(|| {
                use std::time::{SystemTime, UNIX_EPOCH};
                let ns = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos();
                format!("session_{}.jsonl", ns)
            })
    }

    fn get_or_create_session(
        thread: Option<&ThreadMeta>,
        workspace: Option<WorkspaceInfo>,
        default_model: Option<String>,
        default_thinking_level: Option<String>,
        cx: &mut Context<Self>,
    ) -> Entity<SessionHandle> {
        let session_file = Self::session_file_from_thread(thread);

        if let Some(session) =
            cx.update_global(|app: &mut AppStore, _| app.session_manager.get(&session_file))
        {
            return session;
        }

        let thread_id = thread.map(|t| t.id.clone());
        let model = thread.and_then(|t| t.model.clone()).or(default_model);
        let thinking_level = thread
            .and_then(|t| t.thinking_level.clone())
            .or(default_thinking_level);
        let store = cx.global::<AppStore>().store.clone();
        let restore_history = thread.is_some();

        let handle = cx.new(|cx| {
            SessionHandle::new(
                cx,
                thread_id,
                session_file.clone(),
                workspace,
                model,
                thinking_level,
                store,
                restore_history,
            )
        });

        cx.update_global(|app: &mut AppStore, _| {
            app.session_manager.register(session_file, handle.clone());
        });

        handle
    }

    fn ensure_session(&mut self, cx: &mut Context<Self>) -> bool {
        if self.session.is_some() {
            return true;
        }
        let workspace_info = self
            .selected_workspace_id
            .as_ref()
            .and_then(|id| self.workspaces.iter().find(|ws| ws.id == *id))
            .map(|ws| WorkspaceInfo {
                id: ws.id.clone(),
                path: PathBuf::from(&ws.path),
                name: ws.name.clone(),
            });
        let model = self.selected_model.clone();
        let thinking_level = self.thinking_level.clone();
        let session = Self::get_or_create_session(None, workspace_info, model, thinking_level, cx);
        self.attach_session(session, cx);
        self.session.is_some()
    }

    pub fn send_message(&mut self, _: &SendMessage, _window: &mut Window, cx: &mut Context<Self>) {
        if self.chat_input.read(cx).is_just_selected_mention() {
            self.chat_input
                .update(cx, |ci, _| ci.clear_just_selected_mention());
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
                .map(|i| i.read(cx).content(cx).clone())
                .unwrap_or_else(|| self.chat_input.read(cx).content(cx).clone())
        } else {
            self.chat_input.read(cx).content(cx).clone()
        };
        eprintln!("[mini-pi] send_message: {} chars", content.len());
        let has_attachment = !self.pending_attachments.is_empty();
        if content.is_empty() && !has_attachment {
            return;
        }

        // Handle editing an existing user message: fork from it and send the
        // edited prompt into the new branch. Attachments are not carried over
        // when editing; discard them to keep the flow simple.
        if let Some(editing_id) = self.editing_message_id.take() {
            self.pending_attachments.clear();
            self.chat_input.update(cx, |ci, cx| ci.reset(_window, cx));
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
            self.send_edited_prompt(editing_id, content, cx);
            return;
        }

        if !self.ensure_session(cx) {
            return;
        }

        self.chat_input.update(cx, |ci, cx| ci.reset(_window, cx));
        self.scroll_locked = true;
        let attachments = std::mem::take(&mut self.pending_attachments);

        let mut images: Vec<ImageContent> = Vec::new();
        let mut file_parts: Vec<String> = Vec::new();
        for attachment in attachments {
            match attachment {
                PendingAttachment::Image {
                    mime_type, base64, ..
                } => {
                    images.push(ImageContent { data: base64, mime_type });
                }
                PendingAttachment::Text { content, path, .. } => {
                    file_parts.push(format!(
                        "<file path=\"{}\">\n{}\n</file>",
                        path.to_string_lossy(),
                        content
                    ));
                }
            }
        }

        let combined_content: SharedString = if file_parts.is_empty() {
            content
        } else {
            let files_block = file_parts.join("\n\n");
            if content.is_empty() {
                files_block
            } else {
                format!("{}\n\n{}", content, files_block)
            }
            .into()
        };

        let session = self.session.clone().unwrap();
        if !images.is_empty() {
            session.update(cx, |session, cx| {
                session.send_message_with_images(combined_content, images, cx);
            });
        } else {
            session.update(cx, |session, cx| {
                session.send_message(combined_content, cx);
            });
        }

        let thread_id = session.read(cx).thread_id.clone();
        if let Some(ref tid) = thread_id {
            if self.thread_id.is_none() {
                self.thread_id = Some(tid.clone());
            }
            cx.update_global(|app: &mut AppStore, _| {
                app.streaming_thread_ids.insert(tid.clone());
            });
        }

        cx.notify();
    }

    pub fn stop_streaming(
        &mut self,
        _: &StopStreaming,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(self.state, ChatState::Streaming) {
            return;
        }

        eprintln!("[mini-pi] stop_streaming requested");

        if let Some(ref session) = self.session {
            session.update(cx, |session, cx| {
                session.abort(cx);
            });
        }

        self.state = ChatState::Idle;
        if let Some(ref tid) = self.thread_id {
            cx.update_global(|app: &mut AppStore, _| {
                app.streaming_thread_ids.remove(tid);
            });
        }

        cx.notify();
    }

    pub fn pick_and_send_file(
        &mut self,
        _: &gpui::ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.state, ChatState::Streaming | ChatState::Loading) {
            return;
        }

        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: None,
        });

        cx.spawn_in(window, async move |this, cx| {
            let paths = match rx.await {
                Ok(Ok(Some(paths))) => paths,
                _ => return,
            };
            if paths.is_empty() {
                return;
            }

            let results: Vec<Result<PendingAttachment, String>> = smol::unblock(move || {
                paths
                    .into_iter()
                    .map(|path| {
                        let metadata = std::fs::metadata(&path)
                            .map_err(|e| format!("Cannot read file metadata: {}", e))?;
                        if metadata.is_dir() {
                            return Err(format!(
                                "{}: please select a file, not a directory",
                                path.file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("selected item")
                            ));
                        }
                        let size = metadata.len();
                        if size > 5 * 1024 * 1024 {
                            return Err(format!(
                                "{}: file is larger than 5 MB",
                                path.file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("selected file")
                            ));
                        }
                        let name = path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("file")
                            .to_string();
                        let bytes = std::fs::read(&path)
                            .map_err(|e| format!("Failed to read file: {}", e))?;

                        if is_image_file(&path) {
                            let mime_type = mime_type_for_path(&path).to_string();
                            let base64 = base64::Engine::encode(
                                &base64::engine::general_purpose::STANDARD,
                                &bytes,
                            );
                            Ok(PendingAttachment::Image {
                                path,
                                name,
                                mime_type,
                                base64,
                            })
                        } else {
                            const MAX_TEXT_BYTES: usize = 100 * 1024;
                            if bytes.len() > MAX_TEXT_BYTES {
                                return Err(format!(
                                    "{}: text file is larger than 100 KB",
                                    name
                                ));
                            }
                            let content = String::from_utf8(bytes).map_err(|_| {
                                format!(
                                    "{}: binary files are not supported",
                                    name
                                )
                            })?;
                            Ok(PendingAttachment::Text { path, name, content })
                        }
                    })
                    .collect()
            })
            .await;

            this.update_in(cx, |this, window, cx| {
                let mut errors = Vec::new();
                for result in results {
                    match result {
                        Ok(attachment) => this.pending_attachments.push(attachment),
                        Err(err) => errors.push(err),
                    }
                }
                if !errors.is_empty() {
                    let message = if errors.len() == 1 {
                        errors.into_iter().next().unwrap()
                    } else {
                        format!("{} files could not be attached:\n{}", errors.len(), errors.join("\n"))
                    };
                    window.push_notification(Notification::error(message), cx);
                }
                if !this.pending_attachments.is_empty() {
                    this.chat_input.update(cx, |ci, cx| ci.focus(window, cx));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn send_edited_prompt(
        &mut self,
        message_id: String,
        content: SharedString,
        cx: &mut Context<Self>,
    ) {
        if !self.ensure_session(cx) {
            return;
        }

        let Some(edit_idx) = self.messages.iter().position(|m| m.id == message_id) else {
            eprintln!("[mini-pi] edited message {} not found", message_id);
            return;
        };

        self.messages[edit_idx].parts = vec![MessagePart::Text {
            text: content.clone(),
            state: Some(PartState::Done),
        }];
        self.messages.truncate(edit_idx + 1);
        self.reasoning_displays.truncate(edit_idx + 1);
        self.markdown_displays.truncate(edit_idx + 1);
        self.scroll_locked = true;

        let session = self.session.clone().unwrap();
        session.update(cx, |session, cx| {
            session.send_edited_prompt(message_id, content, cx);
        });

        let thread_id = session.read(cx).thread_id.clone();
        if let Some(ref tid) = thread_id {
            if self.thread_id.is_none() {
                self.thread_id = Some(tid.clone());
            }
            cx.update_global(|app: &mut AppStore, _| {
                app.streaming_thread_ids.insert(tid.clone());
            });
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
            .map(|i| i.read(cx).content(cx).clone())
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

    fn start_edit_message(&mut self, msg_id: String, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(msg) = self.messages.iter().find(|m| m.id == msg_id)
            && let Some(MessagePart::Text { text, .. }) = msg.parts.first()
        {
            self.editing_message_id = Some(msg_id);
            let text = text.clone();
            let inline_input = cx.new(|cx| {
                ChatInput::new(window, cx, "Edit message...")
                    .with_at_mention(false)
                    .with_slash_commands(false)
            });
            inline_input.update(cx, |ci, cx| {
                ci.set_content(text, window, cx);
                ci.focus(window, cx);
            });
            self.inline_edit_input = Some(inline_input);
            cx.notify();
        }
    }

    pub fn toggle_voice_input(
        &mut self,
        _: &gpui::ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.voice_state {
            VoiceState::Idle => self.start_voice_input(window, cx),
            VoiceState::Recording => self.stop_voice_input(window, cx),
            VoiceState::Transcribing => {}
        }
    }

    fn start_voice_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match start_recording() {
            Ok(recorder) => {
                self.voice_recorder = Some(recorder);
                self.voice_state = VoiceState::Recording;
                cx.notify();
            }
            Err(err) => {
                window.push_notification(
                    Notification::error(format!("Voice input error: {}", err)),
                    cx,
                );
                cx.notify();
            }
        }
    }

    fn stop_voice_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(recorder) = self.voice_recorder.take() else {
            return;
        };
        let wav_bytes = recorder.stop();
        self.voice_state = VoiceState::Transcribing;
        cx.notify();

        cx.spawn_in(window, async move |this, cx| {
            let result = transcribe(&wav_bytes).await;
            this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(text) if !text.is_empty() => {
                        let current = this.chat_input.read(cx).content(cx).to_string();
                        let new_text = if current.is_empty() {
                            text
                        } else if current.ends_with(' ') {
                            current + &text
                        } else {
                            current + " " + &text
                        };
                        this.chat_input.update(cx, |ci, cx| {
                            ci.set_content(new_text, window, cx);
                        });
                    }
                    Ok(_) => {}
                    Err(err) => {
                        window.push_notification(
                            Notification::error(format!("Transcription failed: {}", err)),
                            cx,
                        );
                    }
                }
                this.voice_state = VoiceState::Idle;
                cx.notify();
            })
            .ok();
        })
        .detach();
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
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.chat_input.update(cx, |ci, cx| ci.close_popup(cx));
                        }),
                    ),
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
                    .bg(cx.theme().popover)
                    .border_1()
                    .border_color(cx.theme().primary)
                    .rounded_md()
                    .py_1()
                    .shadow(vec![gpui::BoxShadow {
                        color: cx.theme().overlay,
                        offset: gpui::point(px(0.), px(4.)),
                        blur_radius: px(12.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .children(items.iter().enumerate().map(|(idx, item)| {
                        let is_highlighted = idx == highlighted;
                        let icon = if item.is_dir {
                            "icons/folder.svg"
                        } else {
                            "icons/file.svg"
                        };
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
                            .when(is_highlighted, |s| s.bg(cx.theme().accent))
                            .hover(|style| style.bg(cx.theme().accent))
                            .child(
                                svg()
                                    .path(icon)
                                    .size(px(14.))
                                    .text_color(if is_highlighted {
                                        cx.theme().primary
                                    } else {
                                        cx.theme().muted_foreground
                                    }),
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
                                            .text_color(if is_highlighted {
                                                cx.theme().foreground
                                            } else {
                                                cx.theme().muted_foreground
                                            })
                                            .child(label),
                                    )
                                    .when(!detail.is_empty(), |s| {
                                        s.child(
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(detail),
                                        )
                                    }),
                            )
                            .on_click(cx.listener(move |this, _, _window, cx| {
                                this.chat_input.update(cx, |ci, cx| {
                                    ci.select_mention_at(item_idx, _window, cx);
                                });
                            }))
                    })),
            )
    }

    fn render_messages_scrollbar(&self, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .bottom_0()
            .child(Scrollbar::vertical(&self.scroll_handle))
    }

    fn render_scroll_to_bottom_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("scroll-to-bottom-btn")
            .absolute()
            .bottom(px(12.))
            .left(px(0.))
            .right(px(0.))
            .flex()
            .items_center()
            .justify_center()
            .when(!self.scroll_locked, |this| {
                this.child(
                    div()
                        .rounded_full()
                        .bg(cx.theme().popover)
                        .border_1()
                        .border_color(cx.theme().border)
                        .shadow(vec![gpui::BoxShadow {
                            color: cx.theme().overlay,
                            offset: gpui::point(px(0.), px(4.)),
                            blur_radius: px(12.),
                            spread_radius: px(0.),
                            inset: false,
                        }])
                        .child(
                            Button::new("scroll-to-bottom")
                                .with_size(Size::Small)
                                .ghost()
                                .icon(
                                    Icon::empty()
                                        .path("icons/chevron-down.svg")
                                        .size(px(16.))
                                        .text_color(cx.theme().muted_foreground),
                                )
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    this.scroll_locked = true;
                                    this.scroll_handle.scroll_to_bottom();
                                    cx.notify();
                                })),
                        ),
                )
            })
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
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.chat_input.update(cx, |ci, cx| ci.close_popup(cx));
                        }),
                    ),
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
                    .bg(cx.theme().popover)
                    .border_1()
                    .border_color(cx.theme().primary)
                    .rounded_md()
                    .py_1()
                    .shadow(vec![gpui::BoxShadow {
                        color: cx.theme().overlay,
                        offset: gpui::point(px(0.), px(4.)),
                        blur_radius: px(12.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .children(items.iter().enumerate().map(|(idx, item)| {
                        let is_highlighted = idx == highlighted;
                        let label: SharedString = format!("/{}", item.name).into();
                        let detail: SharedString =
                            item.description.clone().unwrap_or_default().into();
                        let source_label: SharedString = (match item.source.as_str() {
                            "extension" => "Extension",
                            "prompt" => "Prompt",
                            "skill" => "Skill",
                            _ => &item.source,
                        })
                        .to_string()
                        .into();
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
                            .when(is_highlighted, |s| s.bg(cx.theme().accent))
                            .hover(|style| style.bg(cx.theme().accent))
                            .child(
                                div()
                                    .w(px(160.))
                                    .overflow_hidden()
                                    .text_sm()
                                    .text_color(if is_highlighted {
                                        cx.theme().foreground
                                    } else {
                                        cx.theme().muted_foreground
                                    })
                                    .child(div().whitespace_nowrap().text_ellipsis().child(label)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .line_clamp(2)
                                    .when(!detail.is_empty(), |s| s.child(detail)),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .px_1()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(cx.theme().secondary)
                                    .text_color(cx.theme().secondary_foreground)
                                    .child(source_label),
                            )
                            .on_click(cx.listener(move |this, _, _window, cx| {
                                this.chat_input.update(cx, |ci, cx| {
                                    ci.select_command_at(item_idx, _window, cx);
                                });
                            }))
                    })),
            )
    }

    /// Resolve the active workspace directory, used by `ToolCall` to resolve
    /// relative `send_file` paths. Returns `None` when no workspace is
    /// selected.
    fn workspace_dir_for_send_file(&self) -> Option<PathBuf> {
        self.selected_workspace_id
            .as_ref()
            .and_then(|id| self.workspaces.iter().find(|ws| ws.id == *id))
            .map(|ws| PathBuf::from(&ws.path))
    }

    fn sync_display_entities(
        &mut self,
        cx: &mut Context<Self>,
    ) -> (ReasoningEntities, MarkdownEntities) {
        // Ensure reasoning displays exist for reasoning parts
        let mut reasoning_entities: ReasoningEntities = Vec::new();
        for (msg_idx, msg) in self.messages.iter().enumerate() {
            let mut msg_reasoning: Vec<Option<gpui::Entity<Reasoning>>> = Vec::new();
            let part_count = msg.parts.len();
            if let Some(row) = self.reasoning_displays.get_mut(msg_idx) {
                row.truncate(part_count);
            }
            for (part_idx, part) in msg.parts.iter().enumerate() {
                if let MessagePart::Reasoning { text, state, .. } = part {
                    if msg_idx >= self.reasoning_displays.len() {
                        self.reasoning_displays
                            .resize_with(msg_idx + 1, std::vec::Vec::new);
                    }
                    let row = &mut self.reasoning_displays[msg_idx];
                    if part_idx >= row.len() {
                        row.resize_with(part_idx + 1, || None);
                    }
                    let reasoning_state = state.clone();
                    let entity = if let Some(Some(existing)) = row.get(part_idx) {
                        existing.update(cx, |display, _cx| {
                            display.set_content(text, reasoning_state);
                        });
                        existing.clone()
                    } else {
                        let new = cx.new(|_cx| {
                            Reasoning::new(
                                format!("{}-{}", msg_idx, part_idx),
                                text,
                                reasoning_state,
                            )
                        });
                        row[part_idx] = Some(new.clone());
                        new
                    };
                    msg_reasoning.push(Some(entity));
                } else {
                    if let Some(row) = self.reasoning_displays.get_mut(msg_idx)
                        && part_idx < row.len()
                    {
                        row[part_idx] = None;
                    }
                    msg_reasoning.push(None);
                }
            }
            reasoning_entities.push(msg_reasoning);
        }
        self.reasoning_displays.truncate(self.messages.len());

        // Ensure markdown displays exist for assistant text parts only
        let mut markdown_entities: MarkdownEntities = Vec::new();
        for (msg_idx, msg) in self.messages.iter().enumerate() {
            let mut msg_markdown: Vec<Option<gpui::Entity<TextViewState>>> = Vec::new();
            let is_assistant = matches!(msg.role, Role::Assistant);
            let part_count = msg.parts.len();
            if let Some(row) = self.markdown_displays.get_mut(msg_idx) {
                row.truncate(part_count);
                if !is_assistant {
                    row.clear();
                }
            }
            for (part_idx, part) in msg.parts.iter().enumerate() {
                if is_assistant && matches!(part, MessagePart::Text { .. }) {
                    if let MessagePart::Text { text, .. } = part {
                        if msg_idx >= self.markdown_displays.len() {
                            self.markdown_displays
                                .resize_with(msg_idx + 1, std::vec::Vec::new);
                        }
                        let row = &mut self.markdown_displays[msg_idx];
                        if part_idx >= row.len() {
                            row.resize_with(part_idx + 1, || None);
                        }
                        let entity = if let Some(Some(existing)) = row.get(part_idx) {
                            existing.update(cx, |display, _cx| {
                                display.set_text(text, _cx);
                            });
                            existing.clone()
                        } else {
                            let new = cx.new(|cx| TextViewState::markdown(text, cx));
                            row[part_idx] = Some(new.clone());
                            new
                        };
                        msg_markdown.push(Some(entity));
                    } else {
                        msg_markdown.push(None);
                    }
                } else {
                    if let Some(row) = self.markdown_displays.get_mut(msg_idx)
                        && part_idx < row.len()
                    {
                        row[part_idx] = None;
                    }
                    msg_markdown.push(None);
                }
            }
            markdown_entities.push(msg_markdown);
        }
        self.markdown_displays.truncate(self.messages.len());

        (reasoning_entities, markdown_entities)
    }

    fn render_workspace_selector(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_3()
            .pb_1()
            .flex()
            .flex_row()
            .gap_1()
            .items_center()
            .child(
                Button::new("manage-workspace-btn")
                    .with_size(Size::XSmall)
                    .compact()
                    .secondary()
                    .icon(
                        Icon::empty()
                            .path("icons/manage.svg")
                            .size(px(10.))
                            .text_color(cx.theme().muted_foreground),
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_workspace_manager(window, cx);
                    })),
            )
            .children(self.workspaces.iter().map(|ws| {
                let is_selected = self.selected_workspace_id == Some(ws.id.clone());
                let ws_id = ws.id.clone();
                let ws_name = ws.name.clone();
                let name: SharedString = ws_name.clone().into();
                let button = Button::new(SharedString::from(format!("ws-{}", ws_id)))
                    .with_size(Size::XSmall)
                    .compact();
                let button = if is_selected {
                    let variant = ButtonCustomVariant::new(cx)
                        .color(cx.theme().primary.into())
                        .foreground(cx.theme().primary_foreground.into())
                        .hover(cx.theme().primary_hover.into())
                        .active(cx.theme().primary_active.into());
                    button.custom(variant)
                } else {
                    button.outline()
                };
                button
                    .when(ws.name == "Default", |this| {
                        this.icon(
                            Icon::empty()
                                .path("icons/folder.svg")
                                .size(px(10.))
                                .text_color(if is_selected {
                                    cx.theme().primary_active
                                } else {
                                    cx.theme().muted_foreground
                                }),
                        )
                    })
                    .label(name)
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        this.selected_workspace_id = Some(ws_id.clone());
                        let ws_dir = this
                            .workspaces
                            .iter()
                            .find(|w| w.id == ws_id)
                            .map(|w| PathBuf::from(&w.path));
                        if let Some(dir) = ws_dir {
                            this.chat_input.update(cx, |ci, cx| {
                                ci.set_workspace(ws_id.clone(), dir, ws_name.clone(), cx);
                            });
                        }
                        cx.notify();
                    }))
            }))
    }

    #[allow(clippy::too_many_arguments)]
    fn render_messages(
        &mut self,
        cx: &mut Context<Self>,
        reasoning_entities: &ReasoningEntities,
        markdown_entities: &MarkdownEntities,
        assistant_text_width: Pixels,
        status: Option<SharedString>,
        is_error: bool,
        is_loading: bool,
        is_streaming: bool,
    ) -> impl IntoElement {
        div()
            .id("messages")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll_handle)
            .on_scroll_wheel(
                cx.listener(|this, event: &gpui::ScrollWheelEvent, window, cx| {
                    let delta = event.delta.pixel_delta(window.line_height());
                    if this.scroll_locked && delta.y > gpui::px(0.) {
                        this.scroll_locked = false;
                    }
                    if !this.scroll_locked {
                        let offset_y = this.scroll_handle.offset().y;
                        let max_y = this.scroll_handle.max_offset().y;
                        if offset_y.abs() >= max_y - gpui::px(5.) {
                            this.scroll_locked = true;
                        }
                    }
                    cx.notify();
                }),
            )
            .flex()
            .flex_col()
            .p_3()
            .pr_4()
            .gap_2()
            .children(self.messages.iter().enumerate().map(|(msg_idx, msg)| {
                let msg_reasoning = reasoning_entities.get(msg_idx).cloned().unwrap_or_default();
                let msg_markdown = markdown_entities.get(msg_idx).cloned().unwrap_or_default();
                self.render_message(
                    cx,
                    msg_idx,
                    msg,
                    msg_reasoning,
                    msg_markdown,
                    assistant_text_width,
                )
            }))
            .when(self.messages.is_empty(), |el| {
                el.items_center().justify_center().child(
                    svg()
                        .path("icons/pi.svg")
                        .text_color(cx.theme().muted)
                        .size(px(180.)),
                )
            })
            .when(is_loading, |el| {
                el.child(
                    div().flex().justify_center().child(
                        div()
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .bg(cx.theme().secondary)
                            .text_color(cx.theme().muted_foreground)
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
                        .text_color(cx.theme().muted_foreground)
                        .text_xs()
                        .child(text_loader()),
                )
            })
            .when(is_error, |el| {
                el.child(
                    div().flex().justify_center().child(
                        div()
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .bg(cx.theme().danger)
                            .text_color(cx.theme().danger_foreground)
                            .text_xs()
                            .child(status.unwrap_or_default()),
                    ),
                )
            })
    }

    fn render_message(
        &self,
        cx: &mut Context<Self>,
        msg_idx: usize,
        msg: &Message,
        msg_reasoning: Vec<Option<Entity<Reasoning>>>,
        msg_markdown: Vec<Option<Entity<TextViewState>>>,
        assistant_text_width: Pixels,
    ) -> impl IntoElement + use<> {
        let is_user = matches!(msg.role, Role::User);
        let msg_id = msg.id.clone();
        div()
            .block()
            .w_full()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .when(is_user, |this| this.items_end())
                    .when(!is_user, |this| this.items_start())
                    .gap_1()
                    .children({
                        let n = msg.parts.len();
                        let mut paired: Vec<bool> = vec![false; n];
                        let mut pairs: Vec<(usize, usize)> = Vec::new();

                        // First pass: pair each ToolCall with the earliest unpaired
                        // ToolResult that follows it. Loaded sessions often place all
                        // tool calls before all tool results in the same message.
                        for i in 0..n {
                            if matches!(msg.parts[i], MessagePart::ToolCall { .. }) && !paired[i] {
                                if let Some(j) = (i + 1..n).find(|&k| {
                                    matches!(msg.parts[k], MessagePart::ToolResult { .. })
                                        && !paired[k]
                                }) {
                                    paired[i] = true;
                                    paired[j] = true;
                                    pairs.push((i, j));
                                }
                            }
                        }

                        // Second pass: also pair any remaining ToolCall with the
                        // immediately following Text part (some tools emit results as
                        // regular assistant text).
                        for i in 0..n {
                            if matches!(msg.parts[i], MessagePart::ToolCall { .. }) && !paired[i] {
                                if let Some(j) = (i + 1..n).find(|&k| {
                                    matches!(msg.parts[k], MessagePart::Text { .. }) && !paired[k]
                                }) {
                                    // Only pair if there is no unpaired ToolResult in between.
                                    let has_result_between = (i + 1..j).any(|k| {
                                        matches!(msg.parts[k], MessagePart::ToolResult { .. })
                                            && !paired[k]
                                    });
                                    if !has_result_between {
                                        paired[i] = true;
                                        paired[j] = true;
                                        pairs.push((i, j));
                                    }
                                }
                            }
                        }

                        let mut children: Vec<AnyElement> = Vec::new();
                        let mut i = 0;
                        while i < n {
                            if paired[i] {
                                if let Some(&(call_idx, result_idx)) =
                                    pairs.iter().find(|(call_idx, _)| *call_idx == i)
                                {
                                    if let (
                                        MessagePart::ToolCall { name, args, .. },
                                        MessagePart::ToolResult {
                                            output, details, ..
                                        },
                                    ) = (&msg.parts[call_idx], &msg.parts[result_idx])
                                    {
                                        let tool = ToolCall::paired(
                                            name.clone(),
                                            args.clone(),
                                            Some(output.clone()),
                                            details.clone(),
                                            None,
                                            self.workspace_dir_for_send_file(),
                                            assistant_text_width,
                                        );
                                        children.push(tool.render(cx, msg_idx));
                                    } else if let (
                                        MessagePart::ToolCall { name, args, .. },
                                        MessagePart::Text { text, .. },
                                    ) = (&msg.parts[call_idx], &msg.parts[result_idx])
                                    {
                                        let markdown_entity =
                                            msg_markdown.get(result_idx).and_then(|e| e.clone());
                                        let tool = ToolCall::paired(
                                            name.clone(),
                                            args.clone(),
                                            Some(text.clone()),
                                            None,
                                            markdown_entity,
                                            self.workspace_dir_for_send_file(),
                                            assistant_text_width,
                                        );
                                        children.push(tool.render(cx, msg_idx));
                                    }
                                    i = result_idx + 1;
                                    continue;
                                }
                                // This index is a paired result; skip it.
                                i += 1;
                                continue;
                            }

                            let markdown_entity = msg_markdown.get(i).and_then(|e| e.clone());
                            let reasoning_entity = msg_reasoning.get(i).and_then(|e| e.clone());
                            children.push(self.render_message_part(
                                cx,
                                msg_idx,
                                i,
                                &msg.parts[i],
                                markdown_entity,
                                reasoning_entity,
                                assistant_text_width,
                                is_user,
                                msg_id.clone(),
                            ));
                            i += 1;
                        }
                        children
                    }),
            )
    }

    #[allow(clippy::too_many_arguments)]
    fn render_message_part(
        &self,
        cx: &mut Context<Self>,
        msg_idx: usize,
        _part_idx: usize,
        part: &MessagePart,
        markdown_entity: Option<Entity<TextViewState>>,
        reasoning_entity: Option<Entity<Reasoning>>,
        assistant_text_width: Pixels,
        is_user: bool,
        msg_id: String,
    ) -> AnyElement {
        match part {
            MessagePart::Text { text, state } => self.render_text_part(
                cx,
                msg_idx,
                text.clone(),
                state.clone(),
                markdown_entity,
                assistant_text_width,
                is_user,
                msg_id,
            ),
            MessagePart::Reasoning { .. } => div()
                .flex()
                .flex_col()
                .w_full()
                .min_w_0()
                .self_stretch()
                .when(reasoning_entity.is_some(), |this| {
                    this.child(reasoning_entity.unwrap())
                })
                .into_any_element(),
            MessagePart::ToolCall { name, args, .. } => {
                ToolCall::call_only(name.clone(), args.clone()).render(cx, msg_idx)
            }
            MessagePart::ToolResult {
                name,
                output,
                details,
                ..
            } => ToolCall::result_only(
                name.clone(),
                output.clone(),
                details.clone(),
                self.workspace_dir_for_send_file(),
            )
            .render(cx, msg_idx),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_text_part(
        &self,
        cx: &mut Context<Self>,
        msg_idx: usize,
        text: SharedString,
        state: Option<PartState>,
        markdown_entity: Option<Entity<TextViewState>>,
        assistant_text_width: Pixels,
        is_user: bool,
        msg_id: String,
    ) -> AnyElement {
        let is_streaming_empty = state == Some(PartState::Streaming) && text.is_empty();
        let is_editing = is_user && self.editing_message_id.as_ref() == Some(&msg_id);

        if is_editing {
            let inline_input = self
                .inline_edit_input
                .clone()
                .unwrap_or_else(|| self.chat_input.clone());
            return div()
                .flex()
                .flex_col()
                .gap_1()
                .w_full()
                .child(
                    div()
                        .rounded_md()
                        .bg(cx.theme().background)
                        .text_color(cx.theme().foreground)
                        .text_sm()
                        .child(Input::new(&inline_input.read(cx).input_state).w_full()),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .justify_end()
                        .child(
                            Button::new("inline-edit-save")
                                .label("Save")
                                .with_size(Size::XSmall)
                                .primary()
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    this.confirm_inline_edit(&ConfirmInlineEdit, _window, cx);
                                })),
                        )
                        .child(
                            Button::new("inline-edit-cancel")
                                .label("Cancel")
                                .with_size(Size::XSmall)
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    this.cancel_inline_edit(&CancelInlineEdit, _window, cx);
                                })),
                        ),
                )
                .into_any_element();
        }

        div()
            .flex()
            .flex_col()
            .gap_1()
            .when(!is_user, |this| this.w_full().min_w_0())
            .child(
                div()
                    .py_2()
                    .rounded_md()
                    .when(!is_user, |this| this.w_full().min_w_0())
                    .when(is_user, |this| {
                        this.px_3()
                            .bg(cx.theme().primary)
                            .text_color(cx.theme().primary_foreground)
                    })
                    .when(!is_user, |this| this.text_color(cx.theme().foreground))
                    .text_sm()
                    .when(is_streaming_empty, |this| this.child(text_loader()))
                    .when(!is_streaming_empty, |this| {
                        if let Some(ref md) = markdown_entity {
                            this.child(
                                div().flex().w(assistant_text_width).min_w_0().child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .child(TextView::new(md).selectable(true).w_full()),
                                ),
                            )
                        } else {
                            this.child(text.clone())
                        }
                    }),
            )
            .when(!is_user && !is_streaming_empty, |this| {
                let copy_text = text.clone();
                this.child(
                    Button::new(("copy-btn", msg_idx as u64))
                        .with_size(Size::XSmall)
                        .ghost()
                        .icon(
                            Icon::empty()
                                .path("icons/clipboard.svg")
                                .size(px(12.))
                                .text_color(cx.theme().muted_foreground),
                        )
                        .on_click(cx.listener(move |_this, _, _window, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copy_text.to_string()));
                        })),
                )
            })
            .when(is_user && !is_streaming_empty, |this| {
                let edit_msg_id = msg_id.clone();
                let text_to_copy = text.clone();
                this.child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .justify_end()
                        .child(
                            Button::new(("copy-btn", msg_idx as u64))
                                .with_size(Size::XSmall)
                                .ghost()
                                .icon(
                                    Icon::empty()
                                        .path("icons/clipboard.svg")
                                        .size(px(12.))
                                        .text_color(cx.theme().muted_foreground),
                                )
                                .on_click(cx.listener(move |_this, _, _window, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        text_to_copy.to_string(),
                                    ));
                                })),
                        )
                        .child(
                            Button::new(("edit-btn", msg_idx as u64))
                                .with_size(Size::XSmall)
                                .ghost()
                                .icon(
                                    Icon::empty()
                                        .path("icons/edit.svg")
                                        .size(px(12.))
                                        .text_color(cx.theme().muted_foreground),
                                )
                                .on_click(cx.listener(move |this, _, _window, cx| {
                                    this.start_edit_message(edit_msg_id.clone(), _window, cx);
                                })),
                        ),
                )
            })
            .into_any_element()
    }

    fn render_attachment_bar(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        if self.pending_attachments.is_empty() {
            return div().into_any_element();
        }
        let attachments = self.pending_attachments.clone();
        div()
            .px_3()
            .pt_2()
            .pb_1()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap_2()
            .children(attachments.into_iter().enumerate().map(|(idx, attachment)| {
                let name = match &attachment {
                    PendingAttachment::Image { name, .. } | PendingAttachment::Text { name, .. } => {
                        name.clone()
                    }
                };
                div()
                    .id(SharedString::from(format!("pending-attachment-{}", idx)))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(cx.theme().accent)
                    .text_color(cx.theme().accent_foreground)
                    .child(
                        Icon::empty()
                            .path("icons/file.svg")
                            .size(px(14.))
                            .text_color(cx.theme().accent_foreground),
                    )
                    .child(
                        div()
                            .text_sm()
                            .child(SharedString::from(name)),
                    )
                    .child(
                        Button::new(SharedString::from(format!("remove-attachment-{}", idx)))
                            .with_size(Size::XSmall)
                            .ghost()
                            .icon(
                                Icon::empty()
                                    .path("icons/close.svg")
                                    .size(px(12.))
                                    .text_color(cx.theme().accent_foreground),
                            )
                            .on_click(cx.listener(move |this, _, _window, cx| {
                                this.pending_attachments.remove(idx);
                                cx.notify();
                            })),
                    )
            }))
            .into_any_element()
    }

    fn render_input_area(
        &mut self,
        cx: &mut Context<Self>,
        is_disabled: bool,
        input_focused: bool,
    ) -> impl IntoElement {
        div()
            .px_3()
            .pb_3()
            .when(self.chat_input.read(cx).is_at_popup_visible(), |this| {
                this.child(self.render_at_mention_popup(cx))
            })
            .when(
                self.chat_input.read(cx).is_command_popup_visible(),
                |this| this.child(self.render_command_popup(cx)),
            )
            .child(
                div()
                    .bg(cx.theme().secondary)
                    .rounded_xl()
                    .border_1()
                    .border_color(if input_focused {
                        cx.theme().primary
                    } else {
                        cx.theme().border
                    })
                    .shadow_sm()
                    .px_3()
                    .pb_2()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .when(!self.pending_attachments.is_empty(), |this| {
                        this.child(self.render_attachment_bar(cx))
                    })
                    .child(
                        div()
                            .flex()
                            // These input-bound actions (up/down/enter/tab) never
                            // reach an `on_key_down` listener on a parent div, because
                            // GPUI stops propagation once an action listener handles
                            // them in the bubble phase (inside `Input`). Intercepts
                            // them in the CAPTURE phase, which runs before the Input's
                            // bubble-phase handlers, so the popup can swallow the keys
                            // when it's open and lets them pass through otherwise.
                            .capture_action(cx.listener(
                                |this, _action: &MoveUp, _window, cx| {
                                    if this.chat_input.read(cx).is_popup_visible() {
                                        this.chat_input
                                            .update(cx, |ci, cx| ci.navigate_popup(-1, cx));
                                        cx.stop_propagation();
                                    }
                                },
                            ))
                            .capture_action(cx.listener(
                                |this, _action: &MoveDown, _window, cx| {
                                    if this.chat_input.read(cx).is_popup_visible() {
                                        this.chat_input
                                            .update(cx, |ci, cx| ci.navigate_popup(1, cx));
                                        cx.stop_propagation();
                                    }
                                },
                            ))
                            .capture_action(cx.listener(
                                |this, _action: &Enter, window, cx| {
                                    if this.chat_input.read(cx).is_at_popup_visible() {
                                        this.chat_input.update(cx, |ci, cx| {
                                            ci.select_highlighted_mention(window, cx)
                                        });
                                        cx.stop_propagation();
                                    } else if this.chat_input.read(cx).is_command_popup_visible() {
                                        this.chat_input.update(cx, |ci, cx| {
                                            ci.select_highlighted_command(window, cx)
                                        });
                                        cx.stop_propagation();
                                    }
                                },
                            ))
                            .capture_action(cx.listener(
                                |this, _action: &IndentInline, window, cx| {
                                    if this.chat_input.read(cx).is_at_popup_visible() {
                                        this.chat_input.update(cx, |ci, cx| {
                                            ci.select_highlighted_mention(window, cx)
                                        });
                                        cx.stop_propagation();
                                    } else if this.chat_input.read(cx).is_command_popup_visible() {
                                        this.chat_input.update(cx, |ci, cx| {
                                            ci.select_highlighted_command(window, cx)
                                        });
                                        cx.stop_propagation();
                                    }
                                },
                            ))
                            .child(
                                Input::new(&self.chat_input.read(cx).input_state)
                                    .appearance(false)
                                    .w_full(),
                            ),
                    )
                    .child(self.render_toolbar(cx, is_disabled)),
            )
    }

    fn render_toolbar(&self, cx: &mut Context<Self>, is_disabled: bool) -> impl IntoElement {
        let is_streaming = matches!(self.state, ChatState::Streaming);
        let is_busy = matches!(self.state, ChatState::Streaming | ChatState::Loading);
        div()
            .flex()
            .flex_row()
            .gap_1()
            .items_center()
            .child(
                div().max_w_full().child(
                    Select::new(&self.model_dropdown)
                        .with_size(Size::Small)
                        .appearance(false)
                        .w(px(180.))
                        .placeholder("LLM Model")
                        .menu_width(Length::Auto)
                        .menu_max_h(rems(10.)),
                ),
            )
            .child(
                div().max_w_full().child(
                    Select::new(&self.thinking_dropdown)
                        .with_size(Size::Small)
                        .appearance(false)
                        .w(px(140.))
                        .placeholder("Thinking effort")
                        .menu_width(Length::Auto)
                        .menu_max_h(rems(10.)),
                ),
            )
            .child(div().flex_1())
            .child(
                Button::new("attach-file-btn")
                    .with_size(Size::Small)
                    .ghost()
                    .disabled(is_busy)
                    .icon(
                        Icon::empty()
                            .path("icons/plus.svg")
                            .size(px(14.))
                            .text_color(cx.theme().muted_foreground),
                    )
                    .on_click(cx.listener(Self::pick_and_send_file))
                    .into_any_element(),
            )
            .child({
                let is_recording = self.voice_state == VoiceState::Recording;
                let is_transcribing = self.voice_state == VoiceState::Transcribing;

                if is_recording {
                    Button::new("voice-btn")
                        .with_size(Size::Small)
                        .custom(
                            ButtonCustomVariant::new(cx)
                                .color(cx.theme().danger.into())
                                .foreground(cx.theme().danger_foreground.into())
                                .hover(cx.theme().danger_hover.into())
                                .active(cx.theme().danger_active.into()),
                        )
                        .icon(
                            Icon::empty()
                                .path("icons/mic.svg")
                                .size(px(14.))
                                .text_color(cx.theme().danger_foreground),
                        )
                        .on_click(cx.listener(Self::toggle_voice_input))
                        .into_any_element()
                } else {
                    Button::new("voice-btn")
                        .with_size(Size::Small)
                        .loading(is_transcribing)
                        .ghost()
                        .icon(
                            Icon::empty()
                                .path("icons/mic.svg")
                                .size(px(14.))
                                .text_color(cx.theme().muted_foreground),
                        )
                        .on_click(cx.listener(Self::toggle_voice_input))
                        .into_any_element()
                }
            })
            .child(if is_streaming {
                Button::new("stop-btn")
                    .with_size(Size::Small)
                    .custom(
                        ButtonCustomVariant::new(cx)
                            .color(cx.theme().danger.into())
                            .foreground(cx.theme().danger_foreground.into())
                            .hover(cx.theme().danger_hover.into())
                            .active(cx.theme().danger_active.into()),
                    )
                    .icon(
                        Icon::empty()
                            .path("icons/stop.svg")
                            .size(px(14.))
                            .text_color(cx.theme().danger_foreground),
                    )
                    .on_click(cx.listener(|this, _, _window, cx| {
                        this.stop_streaming(&StopStreaming, _window, cx);
                    }))
                    .into_any_element()
            } else {
                Button::new("send-btn")
                    .with_size(Size::Small)
                    .primary()
                    .icon(
                        Icon::empty()
                            .path("icons/send.svg")
                            .size(px(14.))
                            .text_color(cx.theme().primary_foreground),
                    )
                    .disabled(is_disabled)
                    .on_click(cx.listener(|this, _, _window, cx| {
                        this.send_message(&SendMessage, _window, cx);
                    }))
                    .into_any_element()
            })
    }

    fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let stats = self.session_stats.as_ref();
        let total_messages = stats.map(|s| s.total_messages).unwrap_or(0);
        let input_tokens = stats.map(|s| s.tokens_input).unwrap_or(0);
        let output_tokens = stats.map(|s| s.tokens_output).unwrap_or(0);
        let total_tokens = stats.map(|s| s.tokens_total).unwrap_or(0);
        let cache_read = stats.map(|s| s.tokens_cache_read).unwrap_or(0);
        let cache_write = stats.map(|s| s.tokens_cache_write).unwrap_or(0);
        let cost = stats.map(|s| s.cost).unwrap_or(0.0);
        let context_percent = stats.and_then(|s| s.context_percent);

        fn format_tokens(n: usize) -> String {
            if n >= 1_000_000 {
                format!("{:.1}M", n as f64 / 1_000_000.0)
            } else if n >= 1_000 {
                format!("{:.1}k", n as f64 / 1_000.0)
            } else {
                n.to_string()
            }
        }

        let muted = cx.theme().muted_foreground;

        StatusBar::new()
            .left(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Icon::empty()
                            .path("icons/pi.svg")
                            .size(px(10.))
                            .text_color(muted),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("{} messages", total_messages)),
                    )
                    .when(total_tokens > 0, |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(format!("{} tokens", format_tokens(total_tokens))),
                        )
                    }),
            )
            .right(
                h_flex()
                    .items_center()
                    .gap_3()
                    .when(input_tokens > 0 || output_tokens > 0, |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(format!(
                                    "{} in / {} out",
                                    format_tokens(input_tokens),
                                    format_tokens(output_tokens)
                                )),
                        )
                    })
                    .when(cache_read > 0 || cache_write > 0, |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(format!(
                                    "cache {}r / {}w",
                                    format_tokens(cache_read),
                                    format_tokens(cache_write)
                                )),
                        )
                    })
                    .when(cost > 0.0, |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(format!("${:.4}", cost)),
                        )
                    })
                    .when_some(context_percent, |this, pct| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(format!("context {:.1}%", pct)),
                        )
                    }),
            )
    }
}

impl Render for ChatWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = match &self.state {
            ChatState::Idle => None,
            ChatState::Loading => Some(SharedString::from("Loading...")),
            ChatState::Streaming => Some(SharedString::from("Thinking...")),
            ChatState::Error(msg) => Some(msg.clone()),
        };
        let is_error = matches!(self.state, ChatState::Error(_));
        let is_loading = matches!(self.state, ChatState::Loading);
        let is_streaming = matches!(self.state, ChatState::Streaming);
        let input_empty = self.chat_input.read(cx).content(cx).is_empty();
        let is_disabled =
            is_streaming || is_loading || (input_empty && self.pending_attachments.is_empty());
        let input_focused = self.chat_input.read(cx).focus_handle.is_focused(window);

        let (reasoning_entities, markdown_entities) = self.sync_display_entities(cx);
        let assistant_text_width = (window.viewport_size().width - px(80.)).max(px(320.));

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
                    }
                }
            }))
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .bg(cx.theme().background)
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .relative()
                    .child(self.render_messages(
                        cx,
                        &reasoning_entities,
                        &markdown_entities,
                        assistant_text_width,
                        status,
                        is_error,
                        is_loading,
                        is_streaming,
                    ))
                    .child(self.render_messages_scrollbar(cx))
                    .child(self.render_scroll_to_bottom_button(cx)),
            )
            .when(self.session.is_none(), |el| {
                el.child(self.render_workspace_selector(cx))
            })
            .child(self.render_input_area(cx, is_disabled, input_focused))
            .when(self.session.is_some(), |el| el.child(self.render_status_bar(cx)))
    }
}
