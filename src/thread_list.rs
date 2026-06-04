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
use crate::title_bar::TitleBar;

pub struct ThreadList {
    pub title_bar: gpui::Entity<TitleBar>,
    pub focus_handle: FocusHandle,
    pub threads: Vec<ThreadMeta>,
    pub store: Arc<Store>,
    pub _subscription: gpui::Subscription,
}

impl ThreadList {
    pub fn new(cx: &mut Context<Self>, store: Arc<Store>) -> Self {
        let threads = store.list_threads().unwrap_or_default();
        let title_bar = cx.new(|_| TitleBar::new("Mini Pi").icon("logo.svg"));
        let subscription = cx.observe_global::<AppStore>(move |this, cx| {
            this.threads = this.store.list_threads().unwrap_or_default();
            cx.notify();
        });
        Self {
            title_bar,
            focus_handle: cx.focus_handle(),
            threads,
            store,
            _subscription: subscription,
        }
    }
}

impl Render for ThreadList {
    fn render(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
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
                    .children(self.threads.iter().map(|thread| {
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
                                            .when(pinned, |el| {
                                                el.child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(rgb(0xfbbf24))
                                                        .child("📌"),
                                                )
                                            })
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
                    }))
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
