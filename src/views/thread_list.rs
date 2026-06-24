use std::sync::Arc;

use gpui::{
    AnyWindowHandle, BorrowAppContext, Bounds, Context, FocusHandle, IntoElement,
    ParentElement, Render, ScrollHandle, ScrollWheelEvent, SharedString, Styled, Window, div,
    prelude::*, px, rgb, size, svg,
};

use crate::auth::state::{self, AuthState};
use crate::core::actions::CloseWindow;
use crate::core::app::{AppStore, custom_window_options};
use crate::data::store::{PaginatedThreads, Store, ThreadMeta};
use crate::sync::settings_sync;
use gpui_component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{Icon, Size, Sizable as _};
use crate::ui::loader::loader;
use crate::utils::format::format_relative_time;
use crate::views::chat_app::open_chat_window;
use crate::views::create_thread_button::{CreateThreadButton, CreateThreadButtonEvent};
use crate::views::pi_agent_import::{PiAgentImport, PiAgentImportEvent};
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
        let thread_id = self.thread.id.clone();
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
                let thread_id = this.thread.id.clone();
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

                let handle = open_chat_window(
                    cx,
                    Some(&thread_meta),
                    store.clone(),
                    custom_window_options(Some(bounds)),
                );
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
                        div().min_w_0().flex().flex_row().items_center().child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(rgb(0x666666))
                                .overflow_x_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(preview),
                        ),
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
                                    .child(div().size(px(6.)).rounded_full().bg(rgb(0x22c55e)))
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
                                    .child(div().size(px(6.)).rounded_full().bg(rgb(0x6366f1)))
                                    .child(div().text_xs().text_color(rgb(0x6366f1)).child("New")),
                            )
                        })
                        .child(div().text_xs().text_color(rgb(0x666666)).child(time_label))
                    })
                    .when(!confirming && hovered, |el| {
                        el.child(
                            Button::new(SharedString::from(format!("pin-btn-{}", thread_id)))
                                .with_size(Size::XSmall)
                                .custom(
                                    ButtonCustomVariant::new(cx)
                                        .color(gpui::rgba(0x00000000).into())
                                        .foreground(rgb(0x666666).into())
                                        .hover(rgb(0x333333).into())
                                        .active(rgb(0x444444).into()),
                                )
                                .icon(
                                    Icon::empty()
                                        .path(if pinned { "unpin.svg" } else { "pin.svg" })
                                        .size(px(14.))
                                        .text_color(rgb(0x666666)),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    let _ = this.store.toggle_pin(&this.thread.id);
                                    cx.update_global(|_: &mut AppStore, _| {});
                                })),
                        )
                        .child(
                            Button::new(SharedString::from(format!("remove-btn-{}", thread_id)))
                                .with_size(Size::XSmall)
                                .custom(
                                    ButtonCustomVariant::new(cx)
                                        .color(gpui::rgba(0x00000000).into())
                                        .foreground(rgb(0x666666).into())
                                        .hover(rgb(0x7f1d1d).into())
                                        .active(rgb(0x991b1b).into()),
                                )
                                .icon(
                                    Icon::empty()
                                        .path("close.svg")
                                        .size(px(14.))
                                        .text_color(rgb(0x666666)),
                                )
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
                                    Button::new(SharedString::from(format!(
                                        "confirm-delete-btn-{}",
                                        thread_id
                                    )))
                                        .label("Yes")
                                        .with_size(Size::XSmall)
                                        .danger()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            let _ = this.store.delete_thread(&this.thread.id);
                                            this.confirming = false;
                                            cx.update_global(|_: &mut AppStore, _| {});
                                        })),
                                )
                                .child(
                                    Button::new(SharedString::from(format!(
                                        "cancel-delete-btn-{}",
                                        thread_id
                                    )))
                                        .label("No")
                                        .with_size(Size::XSmall)
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
    pub user_panel: gpui::Entity<UserPanel>,
    pub import_prompt: gpui::Entity<PiAgentImport>,
    pub create_thread_button: gpui::Entity<CreateThreadButton>,
    pub focus_handle: FocusHandle,
    pub thread_items: Vec<gpui::Entity<ThreadItem>>,
    pub store: Arc<Store>,
    pub show_import_prompt: bool,
    pub scroll_handle: ScrollHandle,
    pub page: usize,
    pub per_page: usize,
    pub total_pages: usize,
    pub total: usize,
    pub loading_more: bool,
    pub search_input: gpui::Entity<InputState>,
    pub _subscription: gpui::Subscription,
    pub _user_panel_subscription: gpui::Subscription,
    pub _import_prompt_subscription: gpui::Subscription,
    pub _create_thread_subscription: gpui::Subscription,
    pub _search_input_subscription: gpui::Subscription,
}

impl ThreadList {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, store: Arc<Store>) -> Self {
        let subscription = cx.observe_global::<AppStore>(move |this, cx| {
            this.load_threads(cx);
        });

        let user_panel = cx.new(|cx| UserPanel::new(window, cx));
        let import_prompt = cx.new(|_| PiAgentImport::new());
        let create_thread_button = cx.new(|_| CreateThreadButton::new());

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
                                let initial_meta = cx.global::<AppStore>().sync_meta.clone();
                                cx.spawn(async move |_, cx| {
                                    let result = smol::unblock(move || {
                                        settings_sync::sync_changes(&access_token, &user_id, initial_meta)
                                    })
                                    .await;
                                    let _ =
                                        cx.update_global(|app: &mut AppStore, _| match result {
                                            Ok(meta) => {
                                                let _ = settings_sync::save_sync_meta(&app.store, &meta);
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

        let create_thread_subscription = cx.subscribe(
            &create_thread_button,
            move |this, _, event: &CreateThreadButtonEvent, cx| match event {
                CreateThreadButtonEvent::Clicked => {
                    this.open_new_thread_window(cx);
                }
            },
        );

        let search_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Search threads...")
        });
        let _search_input_subscription =
            cx.subscribe_in(&search_input, window, |this, _, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.load_threads(cx);
                }
            });

        let is_first = state::is_first_run();
        let has_pi_settings = import_prompt.read(cx).has_files();
        let show_import_prompt = is_first && has_pi_settings;

        let mut thread_list = Self {
            user_panel,
            import_prompt,
            create_thread_button,
            focus_handle: cx.focus_handle(),
            thread_items: Vec::new(),
            store,
            show_import_prompt,
            scroll_handle: ScrollHandle::new(),
            page: 1,
            per_page: 20,
            total_pages: 0,
            total: 0,
            loading_more: false,
            search_input,
            _subscription: subscription,
            _user_panel_subscription: user_panel_subscription,
            _import_prompt_subscription: import_prompt_subscription,
            _create_thread_subscription: create_thread_subscription,
            _search_input_subscription,
        };
        thread_list.load_threads(cx);
        thread_list
    }

    fn search_query(&self, cx: &Context<Self>) -> String {
        self.search_input
            .read(cx)
            .value()
            .to_string()
            .trim()
            .to_lowercase()
    }

    fn is_searching(&self, cx: &Context<Self>) -> bool {
        !self.search_query(cx).is_empty()
    }

    fn all_threads_loaded(&self, cx: &Context<Self>) -> bool {
        !self.is_searching(cx) && self.total_pages > 0 && self.page >= self.total_pages
    }

    fn open_new_thread_window(&mut self, cx: &mut Context<Self>) {
        let store = cx.global::<AppStore>().store.clone();
        let _sessions_dir = store.sessions_dir().clone();
        let bounds = Bounds::centered(None, size(px(800.0), px(600.0)), cx);
        open_chat_window(cx, None, store.clone(), custom_window_options(Some(bounds)));
    }

    fn load_threads(&mut self, cx: &mut Context<Self>) {
        self.page = 1;
        self.loading_more = false;

        let query = self.search_query(cx);
        if !query.is_empty() {
            match self.store.search_threads(&query) {
                Ok(threads) => {
                    self.total = threads.len();
                    self.total_pages = 0;
                    self.sync_thread_items(&threads, cx);
                }
                Err(_) => {
                    self.thread_items.clear();
                    self.total = 0;
                    self.total_pages = 0;
                }
            }
            cx.notify();
            return;
        }

        let per_page = self.per_page.max(1);
        match self.store.list_threads_paginated(1, per_page) {
            Ok(PaginatedThreads {
                threads,
                page,
                per_page,
                total,
            }) => {
                self.page = page;
                self.per_page = per_page;
                self.total = total;
                self.total_pages = if total == 0 {
                    0
                } else {
                    (total + per_page - 1) / per_page
                };
                self.sync_thread_items(&threads, cx);
            }
            Err(_) => {
                self.thread_items.clear();
                self.total = 0;
                self.total_pages = 0;
            }
        }
        cx.notify();
    }

    fn load_more_threads(&mut self, cx: &mut Context<Self>) {
        if self.is_searching(cx)
            || self.loading_more
            || self.total_pages == 0
            || self.page >= self.total_pages
        {
            return;
        }
        self.loading_more = true;
        cx.notify();

        let next_page = self.page + 1;
        let per_page = self.per_page.max(1);
        let store = self.store.clone();

        cx.spawn(async move |this, cx| {
            let result = store.list_threads_paginated(next_page, per_page);
            let _ = this.update(cx, |this, cx| {
                this.loading_more = false;
                match result {
                    Ok(PaginatedThreads {
                        threads,
                        page,
                        per_page,
                        total,
                    }) => {
                        this.page = page;
                        this.per_page = per_page;
                        this.total = total;
                        this.total_pages = if total == 0 {
                            0
                        } else {
                            (total + per_page - 1) / per_page
                        };
                        this.append_threads(&threads, cx);
                    }
                    Err(_) => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn append_threads(&mut self, threads: &[ThreadMeta], cx: &mut Context<Self>) {
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
    }

    fn check_scroll_for_more(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.all_threads_loaded(cx) {
            return;
        }
        let max_y = self.scroll_handle.max_offset().y;
        if max_y <= px(0.) {
            return;
        }
        let offset_y = self.scroll_handle.offset().y;
        let threshold = px(80.);
        if offset_y.abs() >= max_y - threshold {
            self.load_more_threads(cx);
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
        let order: std::collections::HashMap<String, usize> =
            threads.iter().enumerate().map(|(i, t)| (t.id.clone(), i)).collect();
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
                .flex_1()
                .min_h(px(0.))
                .bg(rgb(0x1a1a1a))
                .child(self.user_panel.clone());
        }

        let (pinned, unpinned): (Vec<_>, Vec<_>) = self
            .thread_items
            .iter()
            .cloned()
            .partition(|item| item.read(cx).thread.pinned);

        div()
            .track_focus(&self.focus_handle)
            .on_action(|_: &CloseWindow, window, _| {
                window.remove_window();
            })
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .bg(rgb(0x1a1a1a))
            .child(
                div()
                    .id("thread-search-bar")
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(rgb(0x333333))
                    .bg(rgb(0x1f1f1f))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        svg()
                            .path("search.svg")
                            .size(px(16.))
                            .text_color(rgb(0x666666)),
                    )
                    .child(Input::new(&self.search_input).appearance(false).w_full()),
            )
            .child(
                div()
                    .id("thread-list")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll_handle)
                    .on_scroll_wheel(cx.listener(|this, _event: &ScrollWheelEvent, window, cx| {
                        this.check_scroll_for_more(window, cx);
                    }))
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
                    })
                    .when(self.loading_more, |el| {
                        el.child(
                            div()
                                .id("thread-list-loader")
                                .py_4()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(loader()),
                        )
                    })
                    .when(self.all_threads_loaded(cx) && !self.thread_items.is_empty(), |el| {
                        el.child(
                            div()
                                .id("thread-list-end")
                                .py_4()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(0x666666))
                                        .child("No more threads"),
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
                    .child(self.create_thread_button.clone()),
            )
            .when(self.show_import_prompt, |el| {
                el.child(self.import_prompt.clone())
            })
    }
}
