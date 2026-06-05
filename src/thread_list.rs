use std::sync::Arc;

use gpui::{
    Bounds, BoxShadow, Context, FocusHandle, Hsla, IntoElement, ParentElement, Render,
    SharedString, Styled, Window, div, linear_color_stop, linear_gradient, point, prelude::*, px,
    rgb, size, svg,
};

use crate::actions::CloseWindow;
use crate::app::{AppStore, custom_window_options};
use crate::chat_window::ChatWindow;
use crate::store::{Store, ThreadMeta};
use crate::title_bar::{TitleBar, TitleBarEvent};
use crate::user_panel::{UserPanel, UserPanelEvent};

pub struct ThreadList {
    pub title_bar: gpui::Entity<TitleBar>,
    pub user_panel: gpui::Entity<UserPanel>,
    pub focus_handle: FocusHandle,
    pub threads: Vec<ThreadMeta>,
    pub store: Arc<Store>,
    pub confirm_delete_id: Option<i64>,
    pub show_user_panel: bool,
    pub _subscription: gpui::Subscription,
    pub _titlebar_subscription: gpui::Subscription,
    pub _user_panel_subscription: gpui::Subscription,
}

impl ThreadList {
    pub fn new(cx: &mut Context<Self>, store: Arc<Store>) -> Self {
        let threads = store.list_threads().unwrap_or_default();
        let title_bar = cx.new(|_| TitleBar::new("Mini Pi"));
        let subscription = cx.observe_global::<AppStore>(move |this, cx| {
            this.threads = this.store.list_threads().unwrap_or_default();
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
            threads,
            store,
            confirm_delete_id: None,
            show_user_panel: false,
            _subscription: subscription,
            _titlebar_subscription: titlebar_subscription,
            _user_panel_subscription: user_panel_subscription,
        }
    }

    fn cancel_delete(&mut self, _cx: &mut Context<Self>) {
        self.confirm_delete_id = None;
    }

    fn confirm_delete(&mut self, thread_id: i64, _cx: &mut Context<Self>) {
        self.confirm_delete_id = Some(thread_id);
    }

    fn do_delete(&mut self, thread_id: i64, cx: &mut Context<Self>) {
        let _ = self.store.delete_thread(thread_id);
        self.confirm_delete_id = None;
        cx.update_global(|_: &mut AppStore, _| {});
    }

    fn toggle_pin(&mut self, thread_id: i64, cx: &mut Context<Self>) {
        let _ = self.store.toggle_pin(thread_id);
        cx.update_global(|_: &mut AppStore, _| {});
    }
}

impl Render for ThreadList {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // Update titlebar avatar state
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

        let (pinned_threads, unpinned_threads): (Vec<_>, Vec<_>) = self
            .threads
            .iter()
            .partition(|t| t.pinned);

        let render_item = |thread: &ThreadMeta, list: &ThreadList, cx: &mut Context<ThreadList>| {
            let title: SharedString = if thread.title.is_empty() {
                "New Thread".into()
            } else {
                thread.title.clone().into()
            };
            let preview: SharedString = if thread.preview.is_empty() {
                "No messages yet".into()
            } else {
                thread.preview.clone().into()
            };
            let pinned = thread.pinned;
            let thread_id = thread.id;
            let confirming = list.confirm_delete_id == Some(thread_id);

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
                .on_click(move |_, _, cx| {
                    let store = cx.global::<AppStore>().0.clone();
                    let tid = thread_id;
                    let thread_meta = store.get_thread(tid).ok().flatten();
                    if let Some(thread_meta) = thread_meta {
                        cx.open_window(
                            custom_window_options(Some(Bounds::centered(
                                None,
                                size(px(600.0), px(400.0)),
                                cx,
                            ))),
                            move |_, cx| {
                                cx.new(|cx| ChatWindow::new(cx, Some(&thread_meta), store.clone()))
                            },
                        )
                        .unwrap();
                    }
                })
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
                        .gap_1()
                        .when(!confirming, |el| {
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
                                        this.toggle_pin(thread_id, cx);
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
                                        this.confirm_delete(thread_id, cx);
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
                                                this.do_delete(thread_id, cx);
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
                                                this.cancel_delete(cx);
                                                cx.notify();
                                            })),
                                    )
                            )
                        }),
                )
        };

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
                    .when(!pinned_threads.is_empty(), |el| {
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
                        .children(pinned_threads.iter().map(|t| render_item(t, self, cx)))
                    })
                    .when(!unpinned_threads.is_empty(), |el| {
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
                        .children(unpinned_threads.iter().map(|t| render_item(t, self, cx)))
                    })
                    .when(self.threads.is_empty(), |el| {
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
                                        size(px(600.0), px(400.0)),
                                        cx,
                                    ))),
                                    |_, cx| {
                                        cx.new(|cx| ChatWindow::new(cx, None, store.clone()))
                                    },
                                )
                                .unwrap();
                            }),
                    ),
            )
    }
}
