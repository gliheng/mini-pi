use std::{path::PathBuf, sync::Arc, time::Duration};

use crate::views::title_bar::{TitleBarEvent, TitleBarVariant};
use gpui::{
    Bounds, ClipboardItem, Context, Entity, FocusHandle, Focusable, InteractiveElement,
    IntoElement, KeyDownEvent, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement,
    PathPromptOptions, Pixels, Render, ScrollHandle, SharedString, Styled, Window, canvas, div,
    fill, point, prelude::*, px, rgb, svg,
};

use crate::config::model_config::{all_models, model_display_name};
use crate::core::actions::{CancelInlineEdit, CloseWindow, ConfirmInlineEdit, SendMessage};
use crate::core::app::AppStore;
use crate::core::session_handle::{SessionEvent, SessionHandle, WorkspaceInfo};
use crate::data::models::{ChatState, Message, MessagePart, PartState, Role};
use crate::data::store::{Store, ThreadMeta, WorkspaceMeta};
use crate::ui::dropdown::{Direction, Dropdown, DropdownEvent, DropdownItem};
use crate::ui::loader::{loader, spinner_with, text_loader};
use crate::ui::markdown::MarkdownRenderer;
use crate::ui::text_area::TextArea;
use crate::ui::toast::Toast;
use crate::utils::voice::{VoiceRecorder, VoiceState, start_recording, transcribe};
use crate::views::reasoning::Reasoning;
use crate::views::title_bar::TitleBar;
use crate::views::workspace_manager::{WorkspaceManager, WorkspaceManagerEvent};

pub struct ChatWindow {
    pub thread_id: Option<String>,
    pub session_file: String,
    pub title_bar: gpui::Entity<TitleBar>,
    pub messages: Vec<Message>,
    pub chat_input: gpui::Entity<TextArea>,
    pub focus_handle: FocusHandle,
    pub state: ChatState,
    pub store: Arc<Store>,
    pub session: Option<Entity<SessionHandle>>,
    pub session_subscription: Option<gpui::Subscription>,
    pub at_mention_scroll_handle: ScrollHandle,
    pub command_scroll_handle: ScrollHandle,
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
    pub selected_workspace_id: Option<String>,
    pub show_workspace_manager: bool,
    pub workspace_manager: gpui::Entity<WorkspaceManager>,
    pub editing_message_id: Option<String>,
    pub inline_edit_input: Option<gpui::Entity<TextArea>>,
    pub toast: gpui::Entity<Toast>,
    pub voice_state: VoiceState,
    pub voice_recorder: Option<VoiceRecorder>,
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
        let chat_input =
            cx.new(|cx| TextArea::new(cx, "Type a message...").with_text_color(rgb(0xe5e5e5)));
        let title_bar = cx.new(|_| TitleBar::new(title.clone(), TitleBarVariant::Chat));

        let thread_id = thread.map(|t| t.id.clone());
        let selected_model: Option<String> = thread
            .and_then(|t| t.model.clone())
            .or_else(|| cx.global::<AppStore>().config.default_model.clone());
        let selected_thinking_level: Option<String> = thread.and_then(|t| t.thinking_level.clone());

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
        let selected_workspace_id = workspaces
            .iter()
            .find(|ws| ws.name == "Default")
            .map(|ws| ws.id.clone())
            .or_else(|| workspaces.first().map(|ws| ws.id.clone()));

        // Build model dropdown items
        let models = cx.global::<AppStore>().models.clone();
        let model_items: Vec<DropdownItem> = all_models(&models)
            .iter()
            .map(|m| DropdownItem::new(m.id.clone(), m.name.clone()))
            .collect();

        let model_dropdown = cx.new(|cx| {
            Dropdown::new(
                cx,
                model_display_name(&models, selected_model.as_deref()),
                model_items,
            )
            .with_selected(selected_model.clone())
            .with_searchable(true)
            .with_width(px(280.))
            .with_max_height(px(400.))
            .with_direction(Direction::Up)
        });

        // Build thinking level dropdown items based on the selected model's map
        let thinking_items =
            Self::thinking_level_items_for_model(&models, selected_model.as_deref());
        let thinking_dropdown = cx.new(|cx| {
            Dropdown::new(
                cx,
                Self::thinking_level_label(selected_thinking_level.as_deref()),
                thinking_items,
            )
            .with_selected(selected_thinking_level.clone())
            .with_width(px(160.))
            .with_max_height(px(300.))
            .with_direction(Direction::Up)
        });
        let workspace_manager = cx.new(|_| WorkspaceManager::new(workspaces.clone()));
        let toast = cx.new(|_| Toast::new(""));
        let voice_state = VoiceState::Idle;
        let voice_recorder = None;

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
            title_bar: title_bar.clone(),
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
            scrollbar_drag_offset_y: None,
            workspaces,
            selected_workspace_id,
            show_workspace_manager: false,
            workspace_manager: workspace_manager.clone(),
            editing_message_id: None,
            inline_edit_input: None,
            toast: toast.clone(),
            voice_state,
            voice_recorder,
        };

        // Attach to an existing session for restored threads.
        if thread.is_some() {
            let default_model = cx.global::<AppStore>().config.default_model.clone();
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
        cx.observe(&window.chat_input, |_, _, cx| {
            cx.notify();
        })
        .detach();

        // Re-render when the toast visibility/message changes
        cx.observe(&window.toast, |_, _, cx| {
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
                    let session = this.session.clone();
                    let session_file = session
                        .as_ref()
                        .map(|s| s.read(cx).session_file.clone())
                        .unwrap_or_default();
                    cx.spawn(async move |_, cx| {
                        if let Ok(Ok(Some(paths))) = rx.await
                            && let Some(dir) = paths.first()
                        {
                            let file_name = session_file
                                .rsplit_once('.')
                                .map(|(name, _)| format!("{}.html", name))
                                .unwrap_or_else(|| "session.html".to_string());
                            let output_path = dir.join(&file_name);
                            let path_str = output_path.to_string_lossy().to_string();
                            if let Some(ref s) = session {
                                let _ = s.update(cx, |session, _cx| {
                                    session.export_html(&path_str);
                                });
                            }
                        }
                    })
                    .detach();
                }
                TitleBarEvent::OpenWorkspace => {
                    let workspace_dir: Option<PathBuf> = this
                        .selected_workspace_id
                        .as_ref()
                        .and_then(|id| this.workspaces.iter().find(|ws| ws.id == *id))
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
                if let Some(ref session) = this.session {
                    session.update(cx, |session, cx| {
                        session.set_model(Some(id.clone()), cx);
                    });
                }
                this.refresh_thinking_dropdown(cx);
            },
        )
        .detach();

        // Subscribe to thinking dropdown selection events
        cx.subscribe(
            &thinking_dropdown,
            |this, _dropdown, event: &DropdownEvent, cx| {
                let DropdownEvent::Selected { id } = event;
                this.thinking_level = Some(id.clone());
                if let Some(ref session) = this.session {
                    session.update(cx, |session, cx| {
                        session.set_thinking_level(Some(id.clone()), cx);
                    });
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
    ) -> Vec<DropdownItem> {
        let map = model_id
            .and_then(|id| models.iter().find(|m| m.id == id))
            .and_then(|m| m.thinking_level_map.as_ref());

        Self::DEFAULT_THINKING_LEVELS
            .iter()
            .filter(|(id, _)| match map {
                Some(m) => !matches!(m.get(*id), Some(None)),
                None => true,
            })
            .map(|(id, label)| DropdownItem::new(*id, *label))
            .collect()
    }

    fn thinking_level_label(level: Option<&str>) -> SharedString {
        level
            .map(|l| match l {
                "off" => "Off".into(),
                "minimal" => "Minimal".into(),
                "low" => "Low".into(),
                "medium" => "Medium".into(),
                "high" => "High".into(),
                "xhigh" => "Extra High".into(),
                _ => l.to_string().into(),
            })
            .unwrap_or_else(|| "Default".into())
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
            .or_else(|| items.first().map(|i| i.id.clone()));

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

        self.thinking_dropdown.update(cx, |dropdown, _| {
            dropdown.items = items;
            dropdown.selected_id = self.thinking_level.clone();
            dropdown.label = Self::thinking_level_label(self.thinking_level.as_deref());
        });
        cx.notify();
    }

    fn attach_session(&mut self, session: Entity<SessionHandle>, cx: &mut Context<Self>) {
        self.session = Some(session.clone());
        self.sync_from_session(cx);
        self.session_subscription = Some(cx.subscribe(
            &session,
            |this, _session, event: &SessionEvent, cx| {
                this.sync_from_session(cx);
                if let SessionEvent::ExportHtmlSucceeded { path } = event {
                    this.toast.update(cx, |toast, cx| {
                        toast.set_message("Session exported to HTML");
                        toast.set_action("Reveal", path.clone());
                        toast.show_for(Duration::from_secs(5), cx);
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
        self.session_file = session_file;
        self.selected_model = selected_model.clone();
        self.thinking_level = thinking_level.clone();

        self.title_bar.update(cx, |tb, _| {
            tb.title = title;
        });
        self.chat_input.update(cx, |ci, cx| {
            ci.set_commands(commands, cx);
        });
        let models = cx.global::<AppStore>().models.clone();
        self.model_dropdown.update(cx, |dropdown, _| {
            dropdown.selected_id = selected_model.clone();
            dropdown.label = model_display_name(&models, selected_model.as_deref());
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

        self.chat_input.update(cx, |ci, cx| ci.reset(cx));
        self.scroll_locked = true;

        let session = self.session.clone().unwrap();
        session.update(cx, |session, cx| {
            session.send_message(content, cx);
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

    fn start_edit_message(&mut self, msg_id: String, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(msg) = self.messages.iter().find(|m| m.id == msg_id)
            && let Some(MessagePart::Text { text, .. }) = msg.parts.first()
        {
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

    pub fn toggle_voice_input(
        &mut self,
        _: &gpui::ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.voice_state {
            VoiceState::Idle => self.start_voice_input(cx),
            VoiceState::Recording => self.stop_voice_input(cx),
            VoiceState::Transcribing => {}
        }
    }

    fn start_voice_input(&mut self, cx: &mut Context<Self>) {
        match start_recording() {
            Ok(recorder) => {
                self.voice_recorder = Some(recorder);
                self.voice_state = VoiceState::Recording;
                cx.notify();
            }
            Err(err) => {
                self.toast.update(cx, |toast, cx| {
                    toast.set_message(&format!("Voice input error: {}", err));
                    toast.show_for(Duration::from_secs(5), cx);
                });
                cx.notify();
            }
        }
    }

    fn stop_voice_input(&mut self, cx: &mut Context<Self>) {
        let Some(recorder) = self.voice_recorder.take() else {
            return;
        };
        let wav_bytes = recorder.stop();
        self.voice_state = VoiceState::Transcribing;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = transcribe(&wav_bytes).await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(text) if !text.is_empty() => {
                        let current = this.chat_input.read(cx).content().to_string();
                        let new_text = if current.is_empty() {
                            text
                        } else if current.ends_with(' ') {
                            current + &text
                        } else {
                            current + " " + &text
                        };
                        this.chat_input.update(cx, |ci, cx| {
                            ci.set_content(new_text, cx);
                        });
                    }
                    Ok(_) => {}
                    Err(err) => {
                        this.toast.update(cx, |toast, cx| {
                            toast.set_message(&format!("Transcription failed: {}", err));
                            toast.show_for(Duration::from_secs(5), cx);
                        });
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
                        let icon = if item.is_dir {
                            "folder.svg"
                        } else {
                            "file.svg"
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
                            .when(is_highlighted, |s| s.bg(rgb(0x2a2a2a)))
                            .hover(|style| style.bg(rgb(0x2a2a2a)))
                            .child(
                                svg()
                                    .path(icon)
                                    .size(px(14.))
                                    .text_color(if is_highlighted {
                                        rgb(0x6366f1)
                                    } else {
                                        rgb(0x888888)
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
                                                rgb(0xffffff)
                                            } else {
                                                rgb(0xcccccc)
                                            })
                                            .child(label),
                                    )
                                    .when(!detail.is_empty(), |s| {
                                        s.child(
                                            div().text_xs().text_color(rgb(0x666666)).child(detail),
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
                        let thumb_height = ((viewport_height / content_height) * track_height)
                            .clamp(px(36.), track_height);
                        let progress = (-scroll_handle.offset().y / max_scroll).clamp(0., 1.);
                        let thumb_top =
                            track_bounds.top() + (track_height - thumb_height) * progress;
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

                                let Some(drag_offset_y) = entity.read(cx).scrollbar_drag_offset_y
                                else {
                                    return;
                                };

                                let draggable_height = (track_height - thumb_height).max(px(0.));
                                if draggable_height <= px(0.) {
                                    return;
                                }

                                let thumb_top = (ev.position.y - drag_offset_y).clamp(
                                    track_bounds.top(),
                                    track_bounds.bottom() - thumb_height,
                                );
                                let progress = ((thumb_top - track_bounds.top())
                                    / draggable_height)
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
                            .when(is_highlighted, |s| s.bg(rgb(0x2a2a2a)))
                            .hover(|style| style.bg(rgb(0x2a2a2a)))
                            .child(
                                div()
                                    .w(px(160.))
                                    .overflow_hidden()
                                    .text_sm()
                                    .text_color(if is_highlighted {
                                        rgb(0xffffff)
                                    } else {
                                        rgb(0xcccccc)
                                    })
                                    .child(div().whitespace_nowrap().text_ellipsis().child(label)),
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
        let input_empty = self.chat_input.read(cx).content().is_empty();
        let is_disabled = is_streaming || is_loading || input_empty;

        // Sync dropdown labels with current state
        let models = cx.global::<AppStore>().models.clone();
        let model_label = model_display_name(&models, self.selected_model.as_deref());
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
            let part_count = msg.parts.len();
            if let Some(row) = self.reasoning_displays.get_mut(msg_idx) {
                row.truncate(part_count);
            }
            for (part_idx, part) in msg.parts.iter().enumerate() {
                if let MessagePart::Reasoning { text, .. } = part {
                    if msg_idx >= self.reasoning_displays.len() {
                        self.reasoning_displays
                            .resize_with(msg_idx + 1, std::vec::Vec::new);
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
        let mut markdown_entities: Vec<Vec<Option<gpui::Entity<MarkdownRenderer>>>> = Vec::new();
        let assistant_text_width = (window.viewport_size().width - px(80.)).max(px(320.));
        for (msg_idx, msg) in self.messages.iter().enumerate() {
            let mut msg_markdown: Vec<Option<gpui::Entity<MarkdownRenderer>>> = Vec::new();
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
                                                .min_w_0()
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
                                                                    .when(!is_user, |this| this.w_full().min_w_0())
                                                                    .child(
                                                                        div()
                                                                            .py_2()
                                                                            .rounded_md()
                                                                            .when(!is_user, |this| this.w_full().min_w_0())
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
                                                                                    this.child(
                                                                                        div()
                                                                                            .flex()
                                                                                            .w(assistant_text_width)
                                                                                            .min_w_0()
                                                                                            .child(
                                                                                                div()
                                                                                                    .flex_1()
                                                                                                    .min_w_0()
                                                                                                    .child(md),
                                                                                            ),
                                                                                    )
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
            .when(self.session.is_none(), |el| {
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
                            let is_selected = self.selected_workspace_id == Some(ws.id.clone());
                            let ws_id = ws.id.clone();
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
                                    this.selected_workspace_id = Some(ws_id.clone());
                                    let ws_dir = this.workspaces.iter().find(|w| w.id == ws_id).map(|w| PathBuf::from(&w.path));
                                    if let Some(dir) = ws_dir {
                                        this.chat_input.update(cx, |ci, cx| {
                                            ci.set_workspace(ws_id.clone(), dir, ws_name.clone(), cx);
                                        });
                                    }
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
                                    .child({
                                        let is_recording = self.voice_state == VoiceState::Recording;
                                        let is_transcribing = self.voice_state == VoiceState::Transcribing;
                                        let btn = div()
                                            .id("voice-btn")
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .size(px(28.))
                                            .rounded_md()
                                            .text_color(rgb(0xffffff))
                                            .bg(if is_recording {
                                                rgb(0xef4444)
                                            } else if is_transcribing {
                                                rgb(0x666666)
                                            } else {
                                                rgb(0x4b5563)
                                            })
                                            .when(!is_transcribing, |this| this.cursor_pointer());
                                        let btn = if is_transcribing {
                                            btn.child(spinner_with(14.0, 0xffffff))
                                        } else {
                                            btn.child(
                                                svg()
                                                    .path("mic.svg")
                                                    .size(px(14.))
                                                    .text_color(rgb(0xffffff)),
                                            )
                                        };
                                        btn.when(!is_transcribing, |this| {
                                            this.on_click(cx.listener(Self::toggle_voice_input))
                                        })
                                    })
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
            .when(self.toast.read(cx).visible, |this| {
                this.child(
                    div()
                        .absolute()
                        .top(px(48.))
                        .left(px(0.))
                        .right(px(0.))
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_center()
                        .child(self.toast.clone()),
                )
            })
    }
}
