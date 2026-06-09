use std::sync::Arc;

use gpui::{
    AnyWindowHandle, Bounds, BoxShadow, Context, FocusHandle, Focusable, Hsla,
    IntoElement, ParentElement, Render, SharedString, Styled, Window, div,
    linear_color_stop, linear_gradient, point, prelude::*, px, rgb, size, svg,
    BorrowAppContext,
};

use crate::auth::state::{self, AuthState};
use crate::core::actions::CloseWindow;
use crate::core::app::{AppStore, custom_window_options};
use crate::data::store::{Store, ThreadMeta};
use crate::sync::settings_sync;
use crate::utils::format::format_relative_time;
use crate::views::chat_window::ChatWindow;
use crate::views::title_bar::{TitleBar, TitleBarEvent, TitleBarVariant};
use crate::views::user_panel::{UserPanel, UserPanelEvent};

pub struct ThreadItem {
    thread: Arc<ThreadMeta>,
    hovered: bool,
    confirming: bool,
    store: Arc<Store>,
}

impl ThreadItem {
    pub fn new(thread: ThreadMeta, store: Arc<Store>) -> Self {
        Self {
            thread: Arc::new(thread),
            hovered: false,
            confirming: false,
            store,
        }
    }
}

impl Render for ThreadItem {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let thread_id = self.thread.id;
        let title: SharedString = if self.thread.title.is_empty() {
            "New Thread".into()
        } else {
            self.thread.title.clone().into()
        };
        let preview: SharedString = if self.thread.preview.is_empty() {
            "No messages yet".into()
        } else {
            self.thread.preview.clone().into()
        };
        let time_label: SharedString = if self.thread.updated_at.is_empty() {
            "".into()
        } else {
            format_relative_time(&self.thread.updated_at).into()
        };
        let pinned = self.thread.pinned;
        let confirming = self.confirming;
        let hovered = self.hovered;

        div()
            .id(SharedString::from(format!("thread-{}", thread_id)))
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(rgb(0x252525))
            .hover(|style| style.bg(rgb(0x252525)))
            .cursor_pointer()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .on_click(cx.listener(move |this, _, _, cx| {
                let thread_id = this.thread.id;
                let thread_meta = (*this.thread).clone();
                let store = this.store.clone();
                let bounds = Bounds::centered(None, size(px(800.0), px(600.0)), cx);

                let existing_window: Option<AnyWindowHandle> = cx
                    .update_global::<AppStore, _>(|app_store, _| {
                        app_store.thread_windows.get(&thread_id).copied()
                    });

                if let Some(handle) = existing_window {
                    let still_open = handle.update(cx, |_view: gpui::AnyView, window: &mut Window, _app: &mut gpui::App| {
                        window.activate_window();
                    });
                    if still_open.is_ok() {
                        return;
                    }
                    cx.update_global::<AppStore, _>(|app_store, _| {
                        app_store.thread_windows.remove(&thread_id);
                    });
                }

                let handle = cx
                    .open_window(custom_window_options(Some(bounds)), move |window, cx| {
                        cx.new(|cx| {
                            let chat = ChatWindow::new(cx, Some(&thread_meta), store.clone());
                            let input_handle = chat.chat_input.read(cx).focus_handle(cx);
                            window.focus(&input_handle);
                            chat
                        })
                    })
                    .unwrap();
                cx.update_global::<AppStore, _>(|app_store, _| {
                    app_store.thread_windows.insert(thread_id, handle.into());
                });
            }))
            .on_hover(cx.listener(move |this, hovered: &bool, _, _cx| {
                this.hovered = *hovered;
                _cx.notify();
            }))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div().flex().flex_row().items_center().gap_1().child(
                            div()
                                .text_sm()
                                .text_color(rgb(0xe0e0e0))
                                .overflow_x_hidden()
                                .child(title),
                        ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x666666))
                            .overflow_x_hidden()
                            .child(preview),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .when(!confirming && !hovered, |el| {
                        el.child(div().text_xs().text_color(rgb(0x666666)).child(time_label))
                    })
                    .when(!confirming && hovered, |el| {
                        el.child(
                            div()
                                .id(SharedString::from(format!("pin-btn-{}", thread_id)))
                                .flex()
                                .items_center()
                                .justify_center()
                                .size(px(24.))
                                .rounded_md()
                                .cursor_pointer()
                                .child(
                                    svg()
                                        .path(if pinned { "unpin.svg" } else { "pin.svg" })
                                        .size(px(14.))
                                        .text_color(rgb(0x666666)),
                                )
                                .hover(|style| style.bg(rgb(0x333333)))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    let _ = this.store.toggle_pin(this.thread.id);
                                    cx.update_global(|_: &mut AppStore, _| {});
                                })),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("remove-btn-{}", thread_id)))
                                .flex()
                                .items_center()
                                .justify_center()
                                .size(px(24.))
                                .rounded_md()
                                .text_color(rgb(0x666666))
                                .cursor_pointer()
                                .child(
                                    svg()
                                        .path("close.svg")
                                        .size(px(14.))
                                        .text_color(rgb(0x666666)),
                                )
                                .hover(|style| style.bg(rgb(0x7f1d1d)).text_color(rgb(0xfca5a5)))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.confirming = true;
                                    cx.notify();
                                })),
                        )
                    })
                    .when(confirming, |el| {
                        el.child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_1()
                                .child(div().text_xs().text_color(rgb(0xfca5a5)).child("Delete?"))
                                .child(
                                    div()
                                        .id(SharedString::from(format!(
                                            "confirm-delete-btn-{}",
                                            thread_id
                                        )))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .px_2()
                                        .py_1()
                                        .rounded_md()
                                        .bg(rgb(0x7f1d1d))
                                        .text_color(rgb(0xffffff))
                                        .text_xs()
                                        .cursor_pointer()
                                        .child("Yes")
                                        .hover(|style| style.bg(rgb(0x991b1b)))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            let _ = this.store.delete_thread(this.thread.id);
                                            this.confirming = false;
                                            cx.update_global(|_: &mut AppStore, _| {});
                                        })),
                                )
                                .child(
                                    div()
                                        .id(SharedString::from(format!(
                                            "cancel-delete-btn-{}",
                                            thread_id
                                        )))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .px_2()
                                        .py_1()
                                        .rounded_md()
                                        .bg(rgb(0x333333))
                                        .text_color(rgb(0x888888))
                                        .text_xs()
                                        .cursor_pointer()
                                        .child("No")
                                        .hover(|style| style.bg(rgb(0x444444)))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            this.confirming = false;
                                            cx.notify();
                                        })),
                                ),
                        )
                    }),
            )
    }
}

pub struct ThreadList {
    pub title_bar: gpui::Entity<TitleBar>,
    pub user_panel: gpui::Entity<UserPanel>,
    pub focus_handle: FocusHandle,
    pub thread_items: Vec<gpui::Entity<ThreadItem>>,
    pub store: Arc<Store>,
    pub show_user_panel: bool,
    pub show_import_prompt: bool,
    pub import_result: Option<Result<usize, String>>,
    pub sync_status: settings_sync::SyncStatus,
    pub _subscription: gpui::Subscription,
    pub _titlebar_subscription: gpui::Subscription,
    pub _user_panel_subscription: gpui::Subscription,
}

impl ThreadList {
    pub fn new(cx: &mut Context<Self>, store: Arc<Store>) -> Self {
        let threads = store.list_threads().unwrap_or_default();
        let thread_items = threads
            .iter()
            .map(|t| cx.new(|_| ThreadItem::new(t.clone(), store.clone())))
            .collect();
        let title_bar = cx.new(|_| TitleBar::new("Mini Pi", TitleBarVariant::Home));
        let subscription = cx.observe_global::<AppStore>(move |this, cx| {
            let threads = this.store.list_threads().unwrap_or_default();
            this.sync_thread_items(&threads, cx);
            cx.notify();
        });

        let user_panel = cx.new(|cx| UserPanel::new(cx));

        let titlebar_subscription =
            cx.subscribe(&title_bar, move |this, _, _event: &TitleBarEvent, cx| {
                this.show_user_panel = !this.show_user_panel;
                cx.notify();
            });

        let user_panel_subscription =
            cx.subscribe(&user_panel, move |this, _, _event: &UserPanelEvent, cx| {
                this.show_user_panel = false;
                match _event {
                    UserPanelEvent::AuthStateChanged => {
                        let auth = cx.global::<AppStore>().auth.clone();
                        if let AuthState::LoggedIn(_) = &auth {
                            let session = cx.global::<AppStore>().session.clone();
                            if let Some(s) = session {
                                let access_token = s.access_token.clone();
                                let user_id = s.user.id.clone();
                                cx.spawn(async move |weak, cx| {
                                    let result = smol::unblock(move || {
                                        settings_sync::sync_changes(&access_token, &user_id)
                                    }).await;
                                    let _ = weak.update(cx, |this, cx| {
                                        match result {
                                            Ok(meta) => {
                                                this.sync_status = settings_sync::SyncStatus::Synced;
                                                cx.update_global(|app: &mut AppStore, _| {
                                                    app.sync_meta = meta;
                                                });
                                            }
                                            Err(e) => {
                                                this.sync_status = settings_sync::SyncStatus::Error(e);
                                            }
                                        }
                                        cx.notify();
                                    });
                                }).detach();
                            }
                        }
                    }
                    UserPanelEvent::BackPressed => {}
                }
                cx.notify();
            });

        let is_first = state::is_first_run();
        let has_pi_settings = !state::list_pi_agent_json_files().is_empty();

        Self {
            title_bar,
            user_panel,
            focus_handle: cx.focus_handle(),
            thread_items,
            store,
            show_user_panel: false,
            show_import_prompt: is_first && has_pi_settings,
            import_result: None,
            sync_status: settings_sync::SyncStatus::Idle,
            _subscription: subscription,
            _titlebar_subscription: titlebar_subscription,
            _user_panel_subscription: user_panel_subscription,
        }
    }

    fn sync_thread_items(&mut self, threads: &[ThreadMeta], cx: &mut Context<Self>) {
        self.thread_items
            .retain(|item| threads.iter().any(|t| t.id == item.read(cx).thread.id));
        for thread in threads {
            if !self
                .thread_items
                .iter()
                .any(|item| item.read(cx).thread.id == thread.id)
            {
                let item = cx.new(|_| ThreadItem::new(thread.clone(), self.store.clone()));
                self.thread_items.push(item);
            }
        }
        for item in &self.thread_items {
            if let Some(thread) = threads.iter().find(|t| t.id == item.read(cx).thread.id) {
                item.update(cx, |item, _| item.thread = Arc::new(thread.clone()));
            }
        }
        // Reorder to match the database sort: pinned first, then updated_at descending
        let order: std::collections::HashMap<i64, usize> =
            threads.iter().enumerate().map(|(i, t)| (t.id, i)).collect();
        self.thread_items.sort_by_key(|item| {
            order
                .get(&item.read(cx).thread.id)
                .copied()
                .unwrap_or(usize::MAX)
        });
    }

    fn render_import_prompt(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let files = state::list_pi_agent_json_files();
        let file_names: String = files.iter().map(|(name, _)| format!("  • {}", name)).collect::<Vec<_>>().join("\n");

        let mut prompt = div()
            .id("import-prompt")
            .mx_3()
            .mt_3()
            .mb_2()
            .px_4()
            .py_3()
            .rounded_lg()
            .bg(rgb(0x252525))
            .border_1()
            .border_color(rgb(0x4f46e5))
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        svg()
                            .path("folder.svg")
                            .size(px(16.))
                            .text_color(rgb(0x818cf8)),
                    )
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgb(0xe0e0e0))
                            .child("Import Settings"),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(0x888888))
                    .child(format!(
                        "Detected settings from ~/.pi/agent/.\nFound {} JSON file(s):\n{}",
                        files.len(),
                        file_names
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(
                        div()
                            .id("import-btn")
                            .flex_1()
                            .flex()
                            .items_center()
                            .justify_center()
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .bg(rgb(0x4f46e5))
                            .cursor_pointer()
                            .text_color(rgb(0xffffff))
                            .text_xs()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .hover(|style| style.bg(rgb(0x6366f1)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                match state::import_from_pi_agent() {
                                    Ok(count) => {
                                        this.import_result = Some(Ok(count));
                                        this.show_import_prompt = false;
                                    }
                                    Err(e) => {
                                        this.import_result = Some(Err(e.to_string()));
                                    }
                                }
                                cx.notify();
                            }))
                            .child("Import"),
                    )
                    .child(
                        div()
                            .id("skip-import-btn")
                            .flex_1()
                            .flex()
                            .items_center()
                            .justify_center()
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .bg(rgb(0x333333))
                            .cursor_pointer()
                            .text_color(rgb(0x888888))
                            .text_xs()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .hover(|style| style.bg(rgb(0x444444)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_import_prompt = false;
                                cx.notify();
                            }))
                            .child("Skip"),
                    ),
            );
        if self.import_result.is_some() {
            let msg = match self.import_result.as_ref().unwrap() {
                Ok(count) => format!("Imported {} file(s) successfully", count),
                Err(e) => format!("Import failed: {}", e),
            };
            let color = if self.import_result.as_ref().unwrap().is_ok() {
                rgb(0x22c55e)
            } else {
                rgb(0xef4444)
            };
            prompt = prompt.child(
                div()
                    .text_xs()
                    .text_color(color)
                    .child(msg),
            );
        }
        prompt
    }
}

impl Render for ThreadList {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.title_bar.update(cx, |title_bar, _cx| {
            title_bar.avatar_active = self.show_user_panel;
        });

        if self.show_user_panel {
            return div()
                .track_focus(&self.focus_handle)
                .on_action(|_: &CloseWindow, window, _| {
                    window.remove_window();
                })
                .flex()
                .flex_col()
                .size_full()
                .bg(rgb(0x1a1a1a))
                .child(self.title_bar.clone())
                .child(self.user_panel.clone());
        }

        let (pinned, unpinned): (Vec<_>, Vec<_>) = self
            .thread_items
            .iter()
            .map(|item| item.clone())
            .partition(|item| item.read(cx).thread.pinned);

        div()
            .track_focus(&self.focus_handle)
            .on_action(|_: &CloseWindow, window, _| {
                window.remove_window();
            })
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x1a1a1a))
            .child(self.title_bar.clone())
            .when(self.show_import_prompt, |el| {
                el.child(self.render_import_prompt(cx))
            })
            .child(
                div()
                    .id("thread-list")
                    .flex_1()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .when(!pinned.is_empty(), |el| {
                        el.child(
                            div().px_3().py_1().bg(rgb(0x1f1f1f)).child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(0x888888))
                                    .child("Pinned threads"),
                            ),
                        )
                        .children(pinned.iter().map(|item| item.clone()))
                    })
                    .when(!unpinned.is_empty(), |el| {
                        el.child(
                            div()
                                .px_3()
                                .py_1()
                                .bg(rgb(0x1f1f1f))
                                .child(div().text_xs().text_color(rgb(0x888888)).child("Threads")),
                        )
                        .children(unpinned.iter().map(|item| item.clone()))
                    })
                    .when(self.thread_items.is_empty(), |el| {
                        el.items_center().justify_center().child(
                            svg()
                                .path("logo.svg")
                                .text_color(rgb(0x252525))
                                .size(px(180.)),
                        )
                    }),
            )
            .child(
                div()
                    .px_3()
                    .py_3()
                    .border_t_1()
                    .border_color(rgb(0x333333))
                    .child(
                        div()
                            .id("create-thread-btn")
                            .flex()
                            .items_center()
                            .justify_center()
                            .px_8()
                            .py_3()
                            .bg(linear_gradient(
                                180.0,
                                linear_color_stop(rgb(0x818cf8), 0.),
                                linear_color_stop(rgb(0x6366f1), 1.),
                            ))
                            .rounded_full()
                            .text_color(rgb(0xffffff))
                            .cursor_pointer()
                            .text_base()
                            .shadow(vec![BoxShadow {
                                color: Into::<Hsla>::into(rgb(0x6366f1)).alpha(0.4),
                                offset: point(px(0.), px(4.)),
                                blur_radius: px(12.),
                                spread_radius: px(0.),
                            }])
                            .gap(px(8.))
                            .child(
                                svg()
                                    .path("logo.svg")
                                    .text_color(rgb(0xffffff))
                                    .size(px(20.)),
                            )
                            .child("Create Thread")
                            .hover(|style| style.bg(rgb(0x4f46e5)))
                            .on_click(|_, _, cx| {
                                let store = cx.global::<AppStore>().store.clone();
                                let _sessions_dir = store.sessions_dir().clone();
                                cx.open_window(
                                    custom_window_options(Some(Bounds::centered(
                                        None,
                                        size(px(800.0), px(600.0)),
                                        cx,
                                    ))),
                                    |window, cx| {
                                        cx.new(|cx| {
                                            let chat = ChatWindow::new(cx, None, store.clone());
                                            let input_handle = chat.chat_input.read(cx).focus_handle(cx);
                                            window.focus(&input_handle);
                                            chat
                                        })
                                    },
                                )
                                .unwrap();
                            }),
                    ),
            )
    }
}
