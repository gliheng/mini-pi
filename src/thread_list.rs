use std::sync::Arc;

use gpui::{
    Bounds, BoxShadow, Context, FocusHandle, Focusable, Hsla, IntoElement, ParentElement, Render,
    SharedString, Styled, Window, div, linear_color_stop, linear_gradient, point, prelude::*, px,
    rgb, size, svg,
};

use crate::actions::CloseWindow;
use crate::app::{AppStore, custom_window_options};
use crate::chat_window::ChatWindow;
use crate::store::{Store, ThreadMeta};
use crate::title_bar::{TitleBar, TitleBarEvent};
use crate::user_panel::{UserPanel, UserPanelEvent};

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
            crate::utils::format_relative_time(&self.thread.updated_at).into()
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
                let thread_meta = (*this.thread).clone();
                let store = this.store.clone();
                let bounds = Bounds::centered(None, size(px(800.0), px(600.0)), cx);
                cx.open_window(
                    custom_window_options(Some(bounds)),
                    move |window, cx| {
                        cx.new(|cx| {
                            let chat = ChatWindow::new(cx, Some(&thread_meta), store.clone());
                            let input_handle = chat.input.read(cx).focus_handle(cx);
                            window.focus(&input_handle);
                            chat
                        })
                    },
                )
                .unwrap();
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
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .child(
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
                        el.child(
                            div()
                                .text_xs()
                                .text_color(rgb(0x666666))
                                .child(time_label),
                        )
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
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(0xfca5a5))
                                        .child("Delete?"),
                                )
                                .child(
                                    div()
                                        .id(SharedString::from(format!("confirm-delete-btn-{}", thread_id)))
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
                                        .id(SharedString::from(format!("cancel-delete-btn-{}", thread_id)))
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
        let title_bar = cx.new(|_| TitleBar::new("Mini Pi"));
        let subscription = cx.observe_global::<AppStore>(move |this, cx| {
            let threads = this.store.list_threads().unwrap_or_default();
            this.sync_thread_items(&threads, cx);
            cx.notify();
        });

        let user_panel = cx.new(|_| UserPanel::new());

        let titlebar_subscription = cx.subscribe(
            &title_bar,
            move |this, _, _event: &TitleBarEvent, cx| {
                this.show_user_panel = !this.show_user_panel;
                cx.notify();
            },
        );

        let user_panel_subscription = cx.subscribe(
            &user_panel,
            move |this, _, _event: &UserPanelEvent, cx| {
                this.show_user_panel = false;
                cx.notify();
            },
        );

        Self {
            title_bar,
            user_panel,
            focus_handle: cx.focus_handle(),
            thread_items,
            store,
            show_user_panel: false,
            _subscription: subscription,
            _titlebar_subscription: titlebar_subscription,
            _user_panel_subscription: user_panel_subscription,
        }
    }

    fn sync_thread_items(&mut self, threads: &[ThreadMeta], cx: &mut Context<Self>) {
        self.thread_items.retain(|item| {
            threads.iter().any(|t| t.id == item.read(cx).thread.id)
        });
        for thread in threads {
            if !self.thread_items.iter().any(|item| item.read(cx).thread.id == thread.id) {
                let item = cx.new(|_| ThreadItem::new(thread.clone(), self.store.clone()));
                self.thread_items.push(item);
            }
        }
        for item in &self.thread_items {
            if let Some(thread) = threads.iter().find(|t| t.id == item.read(cx).thread.id) {
                item.update(cx, |item, _| item.thread = Arc::new(thread.clone()));
            }
        }
    }
}

impl Render for ThreadList {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
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
            .child(
                div()
                    .id("thread-list")
                    .flex_1()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .when(!pinned.is_empty(), |el| {
                        el.child(
                            div()
                                .px_3()
                                .py_1()
                                .bg(rgb(0x1f1f1f))
                                .child(
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
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(0x888888))
                                        .child("Threads"),
                                ),
                        )
                        .children(unpinned.iter().map(|item| item.clone()))
                    })
                    .when(self.thread_items.is_empty(), |el| {
                        el.items_center()
                            .justify_center()
                            .child(
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
                                let store = cx.global::<AppStore>().0.clone();
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
                                            let input_handle = chat.input.read(cx).focus_handle(cx);
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
