use std::sync::Arc;

use gpui::{
    AnyWindowHandle, BorrowAppContext, Bounds, BoxShadow, Context, FocusHandle, Focusable, Hsla,
    IntoElement, ParentElement, Render, SharedString, Styled, Window, div, linear_color_stop,
    linear_gradient, point, prelude::*, px, rgb, size, svg,
};

use crate::auth::state::{self, AuthState};
use crate::core::actions::CloseWindow;
use crate::core::app::{AppStore, custom_window_options};
use crate::data::store::{Store, ThreadMeta};
use crate::sync::settings_sync;
use crate::utils::format::format_relative_time;
use crate::views::chat_window::ChatWindow;
use crate::views::pi_agent_import::{PiAgentImport, PiAgentImportEvent};
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
        let is_streaming = cx
            .global::<AppStore>()
            .streaming_thread_ids
            .contains(&thread_id);
        let has_new_activity = self
            .thread
            .metadata
            .as_ref()
            .and_then(|md| md.get("has_new_activity").and_then(|v| v.as_bool()))
            .unwrap_or(false);

        div()
            .id(SharedString::from(format!("thread-{}", thread_id)))
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(rgb(0x252525))
            .hover(|style| style.bg(rgb(0x252525)))
            .cursor_pointer()
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .on_click(cx.listener(move |this, _, _, cx| {
                let thread_id = this.thread.id;
                let thread_meta = (*this.thread).clone();
                let store = this.store.clone();
                let bounds = Bounds::centered(None, size(px(800.0), px(600.0)), cx);

                let existing_window: Option<AnyWindowHandle> =
                    cx.update_global::<AppStore, _>(|app_store, _| {
                        app_store.thread_windows.get(&thread_id).copied()
                    });

                if let Some(handle) = existing_window {
                    let still_open = handle.update(
                        cx,
                        |_view: gpui::AnyView, window: &mut Window, _app: &mut gpui::App| {
                            window.activate_window();
                        },
                    );
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
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .min_w_0()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_sm()
                                    .text_color(rgb(0xe0e0e0))
                                    .overflow_x_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(title),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .text_xs()
                            .text_color(rgb(0x666666))
                            .overflow_x_hidden()
                            .whitespace_nowrap()
                            .child(preview),
                    ),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .when(!confirming && !hovered, |el| {
                        el.when(is_streaming, |el| {
                            el.child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        div()
                                            .size(px(6.))
                                            .rounded_full()
                                            .bg(rgb(0x22c55e)),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(0x22c55e))
                                            .child("Thinking..."),
                                    ),
                            )
                        })
                        .when(!is_streaming && has_new_activity, |el| {
                            el.child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        div()
                                            .size(px(6.))
                                            .rounded_full()
                                            .bg(rgb(0x6366f1)),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(0x6366f1))
                                            .child("New"),
                                    ),
                            )
                        })
                        .child(div().text_xs().text_color(rgb(0x666666)).child(time_label))
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
    pub import_prompt: gpui::Entity<PiAgentImport>,
    pub focus_handle: FocusHandle,
    pub thread_items: Vec<gpui::Entity<ThreadItem>>,
    pub store: Arc<Store>,
    pub show_import_prompt: bool,
    pub _subscription: gpui::Subscription,
    pub _titlebar_subscription: gpui::Subscription,
    pub _user_panel_subscription: gpui::Subscription,
    pub _import_prompt_subscription: gpui::Subscription,
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

        let user_panel = cx.new(UserPanel::new);
        let import_prompt = cx.new(|_| PiAgentImport::new());

        let titlebar_subscription =
            cx.subscribe(&title_bar, move |_this, _, _event: &TitleBarEvent, cx| {
                cx.update_global(|app: &mut AppStore, _| {
                    app.user_panel_active = !app.user_panel_active;
                });
            });

        let user_panel_subscription =
            cx.subscribe(&user_panel, move |_this, _, _event: &UserPanelEvent, cx| {
                cx.update_global(|app: &mut AppStore, _| {
                    app.user_panel_active = false;
                });
                match _event {
                    UserPanelEvent::AuthStateChanged => {
                        let auth = cx.global::<AppStore>().auth.clone();
                        if let AuthState::LoggedIn(_) = &auth {
                            let session = cx.global::<AppStore>().session.clone();
                            if let Some(s) = session {
                                cx.update_global(|app: &mut AppStore, _| {
                                    app.sync_status = settings_sync::SyncStatus::Syncing;
                                });
                                cx.notify();
                                let access_token = s.access_token.clone();
                                let user_id = s.user.id.clone();
                                cx.spawn(async move |_, cx| {
                                    let result = smol::unblock(move || {
                                        settings_sync::sync_changes(&access_token, &user_id)
                                    })
                                    .await;
                                    let _ =
                                        cx.update_global(|app: &mut AppStore, _| match result {
                                            Ok(meta) => {
                                                app.sync_meta = meta;
                                                app.sync_status = settings_sync::SyncStatus::Synced;
                                            }
                                            Err(e) => {
                                                app.sync_status =
                                                    settings_sync::SyncStatus::Error(e);
                                            }
                                        });
                                })
                                .detach();
                            }
                        }
                    }
                    UserPanelEvent::BackPressed => {}
                }
                cx.notify();
            });

        let import_prompt_subscription = cx.subscribe(
            &import_prompt,
            move |this, _, event: &PiAgentImportEvent, _cx| {
                match event {
                    PiAgentImportEvent::ImportRequested => {
                        this.show_import_prompt = false;
                    }
                    PiAgentImportEvent::SkipRequested => {
                        this.show_import_prompt = false;
                    }
                }
                _cx.notify();
            },
        );

        let is_first = state::is_first_run();
        let has_pi_settings = import_prompt.read(cx).has_files();
        let show_import_prompt = is_first && has_pi_settings;

        Self {
            title_bar,
            user_panel,
            import_prompt,
            focus_handle: cx.focus_handle(),
            thread_items,
            store,
            show_import_prompt,
            _subscription: subscription,
            _titlebar_subscription: titlebar_subscription,
            _user_panel_subscription: user_panel_subscription,
            _import_prompt_subscription: import_prompt_subscription,
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
}

impl Render for ThreadList {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if cx.global::<AppStore>().user_panel_active {
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
            .iter().cloned()
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
                        .children(pinned.iter().cloned())
                    })
                    .when(!unpinned.is_empty(), |el| {
                        el.child(
                            div()
                                .px_3()
                                .py_1()
                                .bg(rgb(0x1f1f1f))
                                .child(div().text_xs().text_color(rgb(0x888888)).child("Threads")),
                        )
                        .children(unpinned.iter().cloned())
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
                                            let input_handle =
                                                chat.chat_input.read(cx).focus_handle(cx);
                                            window.focus(&input_handle);
                                            chat
                                        })
                                    },
                                )
                                .unwrap();
                            }),
                    ),
            )
            .when(self.show_import_prompt, |el| {
                el.child(self.import_prompt.clone())
            })
    }
}
