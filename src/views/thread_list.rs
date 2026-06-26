use std::sync::Arc;

use gpui::{
    div, prelude::*, px, size, svg, AnyWindowHandle, App, Bounds, ClickEvent, Context, ElementId,
    FocusHandle, IntoElement, ParentElement, Render, RenderOnce, SharedString, Styled, Window,
};

use crate::auth::state;
use crate::core::actions::CloseWindow;
use crate::core::app::{custom_window_options, AppStore};
use crate::data::store::{PaginatedThreads, Store, ThreadMeta};
use crate::utils::format::format_relative_time;
use crate::views::chat_app::open_chat_window;
use crate::views::create_thread_button::{CreateThreadButton, CreateThreadButtonEvent};
use crate::views::pi_agent_import::{PiAgentImport, PiAgentImportEvent};
use gpui_component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_component::{
    list::{List, ListDelegate, ListEvent, ListState},
    ActiveTheme, Icon, IndexPath, Selectable, Sizable as _, Size,
};

#[derive(IntoElement)]
struct ThreadListItem {
    ix: IndexPath,
    thread: Arc<ThreadMeta>,
    store: Arc<Store>,
    list_state: gpui::Entity<ListState<ThreadListDelegate>>,
    selected: bool,
    hovered: bool,
    confirming: bool,
    is_streaming: bool,
    has_new_activity: bool,
}

impl Selectable for ThreadListItem {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.selected
    }
}

impl RenderOnce for ThreadListItem {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let Self {
            ix,
            thread,
            store,
            list_state,
            selected,
            hovered,
            confirming,
            is_streaming,
            has_new_activity,
        } = self;

        let theme = cx.theme().clone();
        let thread_id = thread.id.clone();
        let pinned = thread.pinned;

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
        let time_label: SharedString = if thread.updated_at.is_empty() {
            "".into()
        } else {
            format_relative_time(&thread.updated_at).into()
        };

        let list_state_hover = list_state.clone();
        let list_state_delete = list_state.clone();
        let list_state_cancel = list_state.clone();

        let store_pin = store.clone();
        let store_delete = store.clone();

        let pin_thread_id = thread_id.clone();
        let delete_thread_id = thread_id.clone();
        let confirm_thread_id = thread_id.clone();
        let cancel_thread_id = thread_id.clone();

        div()
            .id(ElementId::from(SharedString::from(format!(
                "thread-item-{}",
                thread_id
            ))))
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(theme.border)
            .when(selected, |el| el.bg(theme.list_active))
            .when(!selected && hovered, |el| el.bg(theme.secondary_hover))
            .cursor_pointer()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .on_hover(move |is_hovered: &bool, _window: &mut Window, cx: &mut App| {
                cx.update_entity(&list_state_hover, |state, cx| {
                    state.delegate_mut().set_hovered(ix, *is_hovered);
                    cx.notify();
                });
            })
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
                                    .text_color(theme.foreground)
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
                                .text_color(theme.muted_foreground)
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
                                    .child(div().size(px(6.)).rounded_full().bg(theme.green))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme.green)
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
                                    .child(div().size(px(6.)).rounded_full().bg(theme.blue))
                                    .child(div().text_xs().text_color(theme.blue).child("New")),
                            )
                        })
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(time_label),
                        )
                    })
                    .when(!confirming && hovered, |el| {
                        el.child(
                            Button::new(SharedString::from(format!("pin-btn-{}", pin_thread_id)))
                                .with_size(Size::XSmall)
                                .custom(
                                    ButtonCustomVariant::new(cx)
                                        .color(gpui::rgba(0x00000000).into())
                                        .foreground(theme.muted_foreground.into())
                                        .hover(theme.secondary_hover.into())
                                        .active(theme.secondary_active.into()),
                                )
                                .icon(
                                    Icon::empty()
                                        .path(if pinned {
                                            "icons/unpin.svg"
                                        } else {
                                            "icons/pin.svg"
                                        })
                                        .size(px(14.))
                                        .text_color(theme.muted_foreground),
                                )
                                .on_click(move |_event: &ClickEvent, _window: &mut Window, cx: &mut App| {
                                    cx.stop_propagation();
                                    let _ = store_pin.toggle_pin(&pin_thread_id);
                                    cx.update_global::<AppStore, _>(|_, _| {});
                                }),
                        )
                        .child(
                            Button::new(SharedString::from(format!("remove-btn-{}", delete_thread_id)))
                                .with_size(Size::XSmall)
                                .custom(
                                    ButtonCustomVariant::new(cx)
                                        .color(gpui::rgba(0x00000000).into())
                                        .foreground(theme.muted_foreground.into())
                                        .hover(theme.danger_hover.into())
                                        .active(theme.danger_active.into()),
                                )
                                .icon(
                                    Icon::empty()
                                        .path("icons/close.svg")
                                        .size(px(14.))
                                        .text_color(theme.muted_foreground),
                                )
                                .on_click(move |_event: &ClickEvent, _window: &mut Window, cx: &mut App| {
                                    cx.stop_propagation();
                                    cx.update_entity(&list_state_delete, |state, cx| {
                                        state.delegate_mut().confirming_id = Some(delete_thread_id.clone());
                                        cx.notify();
                                    });
                                }),
                        )
                    })
                    .when(confirming, |el| {
                        el.child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_1()
                                .child(div().text_xs().text_color(theme.danger).child("Delete?"))
                                .child(
                                    Button::new(SharedString::from(format!(
                                        "confirm-delete-btn-{}",
                                        confirm_thread_id
                                    )))
                                    .label("Yes")
                                    .with_size(Size::XSmall)
                                    .danger()
                                    .on_click(move |_event: &ClickEvent, _window: &mut Window, cx: &mut App| {
                                        cx.stop_propagation();
                                        let _ = store_delete.delete_thread(&confirm_thread_id);
                                        cx.update_global::<AppStore, _>(|_, _| {});
                                    }),
                                )
                                .child(
                                    Button::new(SharedString::from(format!(
                                        "cancel-delete-btn-{}",
                                        cancel_thread_id
                                    )))
                                    .label("No")
                                    .with_size(Size::XSmall)
                                    .on_click(move |_event: &ClickEvent, _window: &mut Window, cx: &mut App| {
                                        cx.stop_propagation();
                                        cx.update_entity(&list_state_cancel, |state, cx| {
                                            state.delegate_mut().confirming_id = None;
                                            cx.notify();
                                        })
                                        ;
                                    }),
                                ),
                        )
                    }),
            )
    }
}

struct ThreadListDelegate {
    store: Arc<Store>,
    query: String,
    pinned: Vec<Arc<ThreadMeta>>,
    unpinned: Vec<Arc<ThreadMeta>>,
    selected_index: Option<IndexPath>,
    hovered_index: Option<IndexPath>,
    confirming_id: Option<String>,
    page: usize,
    per_page: usize,
    total: usize,
    eof: bool,
}

impl ThreadListDelegate {
    fn new(store: Arc<Store>) -> Self {
        Self {
            store,
            query: String::new(),
            pinned: Vec::new(),
            unpinned: Vec::new(),
            selected_index: None,
            hovered_index: None,
            confirming_id: None,
            page: 1,
            per_page: 20,
            total: 0,
            eof: false,
        }
    }

    fn set_hovered(&mut self, ix: IndexPath, hovered: bool) {
        if hovered {
            self.hovered_index = Some(ix);
        } else if self.hovered_index == Some(ix) {
            self.hovered_index = None;
        }
    }

    fn section_for(&self, section: usize) -> Option<(&[Arc<ThreadMeta>], &'static str)> {
        let mut i = 0;
        if !self.pinned.is_empty() {
            if i == section {
                return Some((&self.pinned, "Pinned threads"));
            }
            i += 1;
        }
        if !self.unpinned.is_empty() {
            if i == section {
                return Some((&self.unpinned, "Threads"));
            }
        }
        None
    }

    fn thread_at(&self, ix: IndexPath) -> Option<Arc<ThreadMeta>> {
        self.section_for(ix.section)
            .and_then(|(threads, _)| threads.get(ix.row))
            .cloned()
    }

    fn set_threads(&mut self, threads: &[ThreadMeta], cx: &mut Context<ListState<Self>>) {
        self.pinned.clear();
        self.unpinned.clear();
        self.append_threads(threads, cx);
    }

    fn append_threads(&mut self, threads: &[ThreadMeta], _cx: &mut Context<ListState<Self>>) {
        for thread in threads {
            let arc = Arc::new(thread.clone());
            let target = if arc.pinned {
                &mut self.pinned
            } else {
                &mut self.unpinned
            };
            if !target.iter().any(|t| t.id == arc.id) {
                target.push(arc);
            }
        }
    }

    fn refresh(&mut self, query: &str, cx: &mut Context<ListState<Self>>) {
        self.query = query.to_string();
        self.confirming_id = None;
        self.hovered_index = None;
        self.selected_index = None;

        if query.is_empty() {
            self.page = 1;
            self.eof = false;
            match self.store.list_threads_paginated(1, self.per_page.max(1)) {
                Ok(paginated) => self.set_paginated(paginated, cx),
                Err(_) => {
                    self.pinned.clear();
                    self.unpinned.clear();
                    self.total = 0;
                    self.eof = true;
                }
            }
        } else {
            self.page = 1;
            self.eof = true;
            match self.store.search_threads(query) {
                Ok(threads) => {
                    self.total = threads.len();
                    self.per_page = threads.len().max(1);
                    self.set_threads(&threads, cx);
                }
                Err(_) => {
                    self.pinned.clear();
                    self.unpinned.clear();
                    self.total = 0;
                }
            }
        }

        cx.notify();
    }

    fn set_paginated(&mut self, paginated: PaginatedThreads, cx: &mut Context<ListState<Self>>) {
        self.page = paginated.page;
        self.per_page = paginated.per_page;
        self.total = paginated.total;
        self.eof = paginated.page * paginated.per_page >= paginated.total;
        self.set_threads(&paginated.threads, cx);
    }
}

impl ListDelegate for ThreadListDelegate {
    type Item = ThreadListItem;

    fn sections_count(&self, _cx: &App) -> usize {
        let mut count = 0;
        if !self.pinned.is_empty() {
            count += 1;
        }
        if !self.unpinned.is_empty() {
            count += 1;
        }
        count.max(1)
    }

    fn items_count(&self, section: usize, _cx: &App) -> usize {
        self.section_for(section)
            .map(|(threads, _)| threads.len())
            .unwrap_or(0)
    }

    fn perform_search(
        &mut self,
        query: &str,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> gpui::Task<()> {
        self.refresh(query.trim(), cx);
        gpui::Task::ready(())
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        self.selected_index = ix;
        cx.notify();
    }

    fn render_section_header(
        &mut self,
        section: usize,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<impl IntoElement> {
        let (_, label) = self.section_for(section)?;
        Some(
            div().px_3().py_1().bg(cx.theme().muted).child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(label),
            ),
        )
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let thread = self.thread_at(ix)?;
        let is_streaming = cx
            .global::<AppStore>()
            .streaming_thread_ids
            .contains(&thread.id);
        let has_new_activity = thread
            .metadata
            .as_ref()
            .and_then(|md| md.get("has_new_activity").and_then(|v| v.as_bool()))
            .unwrap_or(false);
        let confirming = self.confirming_id.as_ref() == Some(&thread.id);

        Some(ThreadListItem {
            ix,
            thread,
            store: self.store.clone(),
            list_state: cx.entity(),
            selected: self.selected_index == Some(ix),
            hovered: self.hovered_index == Some(ix),
            confirming,
            is_streaming,
            has_new_activity,
        })
    }

    fn render_empty(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                svg()
                    .path("icons/pi.svg")
                    .text_color(cx.theme().border)
                    .size(px(180.)),
            )
    }

    fn has_more(&self, _cx: &App) -> bool {
        !self.eof && self.query.is_empty()
    }

    fn load_more(&mut self, _window: &mut Window, cx: &mut Context<ListState<Self>>) {
        if !self.query.is_empty() || self.eof {
            return;
        }

        let next_page = self.page + 1;
        let per_page = self.per_page.max(1);
        if let Ok(paginated) = self.store.list_threads_paginated(next_page, per_page) {
            self.page = paginated.page;
            self.per_page = paginated.per_page;
            self.total = paginated.total;
            self.eof = paginated.page * paginated.per_page >= paginated.total;
            self.append_threads(&paginated.threads, cx);
        }
        cx.notify();
    }
}

pub struct ThreadList {
    pub import_prompt: gpui::Entity<PiAgentImport>,
    pub create_thread_button: gpui::Entity<CreateThreadButton>,
    pub focus_handle: FocusHandle,
    list_state: gpui::Entity<ListState<ThreadListDelegate>>,
    pub store: Arc<Store>,
    pub show_import_prompt: bool,
    pub _global_subscription: gpui::Subscription,
    pub _import_prompt_subscription: gpui::Subscription,
    pub _create_thread_subscription: gpui::Subscription,
    pub _list_subscription: gpui::Subscription,
}

impl ThreadList {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, store: Arc<Store>) -> Self {
        let delegate = ThreadListDelegate::new(store.clone());
        let list_state = cx.new(|cx| ListState::new(delegate, window, cx).searchable(true));

        let global_subscription = cx.observe_global::<AppStore>(move |this, cx| {
            this.refresh_threads(cx);
        });

        let list_subscription = cx.subscribe(&list_state, |this, _, event: &ListEvent, cx| {
            if let ListEvent::Confirm(ix) = event {
                if let Some(thread) = this.list_state.read(cx).delegate().thread_at(*ix) {
                    this.open_thread_window(&thread, cx);
                }
            }
        });

        let import_prompt = cx.new(|_| PiAgentImport::new());
        let create_thread_button = cx.new(|_| CreateThreadButton::new());

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

        let is_first = state::is_first_run();
        let has_pi_settings = import_prompt.read(cx).has_files();
        let show_import_prompt = is_first && has_pi_settings;

        let mut thread_list = Self {
            import_prompt,
            create_thread_button,
            focus_handle: cx.focus_handle(),
            list_state: list_state.clone(),
            store,
            show_import_prompt,
            _global_subscription: global_subscription,
            _import_prompt_subscription: import_prompt_subscription,
            _create_thread_subscription: create_thread_subscription,
            _list_subscription: list_subscription,
        };

        thread_list.refresh_threads(cx);
        thread_list
    }

    fn refresh_threads(&mut self, cx: &mut Context<Self>) {
        let query = self.list_state.read(cx).delegate().query.clone();
        self.list_state.update(cx, |state, cx| {
            state.delegate_mut().refresh(&query, cx);
        });
    }

    fn open_new_thread_window(&mut self, cx: &mut Context<Self>) {
        let store = cx.global::<AppStore>().store.clone();
        let bounds = Bounds::centered(None, size(px(800.0), px(600.0)), cx);
        open_chat_window(cx, None, store.clone(), custom_window_options(Some(bounds)));
    }

    fn open_thread_window(&mut self, thread: &ThreadMeta, cx: &mut Context<Self>) {
        let thread_id = thread.id.clone();
        let thread_meta = thread.clone();
        let store = self.store.clone();

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

        let bounds = Bounds::centered(None, size(px(800.0), px(600.0)), cx);
        let handle = open_chat_window(
            cx,
            Some(&thread_meta),
            store.clone(),
            custom_window_options(Some(bounds)),
        );
        cx.update_global::<AppStore, _>(|app_store, _| {
            app_store.thread_windows.insert(thread_id, handle.into());
        });
    }
}

impl Render for ThreadList {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();

        div()
            .track_focus(&self.focus_handle)
            .on_action(|_: &CloseWindow, window, _| {
                window.remove_window();
            })
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .bg(theme.background)
            .child(
                List::new(&self.list_state)
                    .flex_1()
                    .w_full()
                    .search_placeholder("Search threads..."),
            )
            .child(
                div()
                    .px_3()
                    .py_3()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(self.create_thread_button.clone()),
            )
            .when(self.show_import_prompt, |el| {
                el.child(self.import_prompt.clone())
            })
    }
}
