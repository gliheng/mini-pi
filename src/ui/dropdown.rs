use gpui::{
    Context, EventEmitter, FocusHandle, IntoElement, KeyDownEvent, MouseButton, ParentElement,
    Render, SharedString, Styled, Window, div, prelude::*, px, rgb, svg,
};

use crate::ui::input::TextInput;

/// An item in the dropdown list.
#[derive(Clone)]
pub struct DropdownItem {
    pub id: String,
    pub label: SharedString,
}

impl DropdownItem {
    pub fn new(id: impl Into<String>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
        }
    }
}

/// Direction the dropdown panel opens.
#[derive(Clone, Copy, Debug, Default)]
pub enum Direction {
    /// Panel opens below the button (default).
    #[default]
    Down,
    /// Panel opens above the button.
    Up,
}

/// Events emitted by the Dropdown component.
#[derive(Clone, Debug)]
pub enum DropdownEvent {
    /// An item was selected by the user.
    Selected { id: String },
}

/// A reusable dropdown component with its own state.
///
/// Supports:
/// - Optional search/filter input
/// - Keyboard navigation (up/down/enter/escape)
/// - Highlighted index tracking
/// - Click-outside-to-close via overlay
/// - Emits `DropdownEvent::Selected` when an item is chosen
pub struct Dropdown {
    pub label: SharedString,
    pub items: Vec<DropdownItem>,
    pub selected_id: Option<String>,
    pub is_open: bool,
    pub highlighted_index: Option<usize>,
    pub searchable: bool,
    pub search_input: gpui::Entity<TextInput>,
    pub focus_handle: FocusHandle,
    pub width: gpui::Pixels,
    pub max_height: gpui::Pixels,
    pub direction: Direction,
}

impl EventEmitter<DropdownEvent> for Dropdown {}

impl Dropdown {
    pub fn new(
        cx: &mut Context<Self>,
        label: impl Into<SharedString>,
        items: Vec<DropdownItem>,
    ) -> Self {
        let search_input = cx.new(|cx| TextInput::new(cx, "Search..."));
        Self {
            label: label.into(),
            items,
            selected_id: None,
            is_open: false,
            highlighted_index: None,
            searchable: false,
            search_input,
            focus_handle: cx.focus_handle(),
            width: px(200.),
            max_height: px(300.),
            direction: Direction::Down,
        }
    }

    pub fn with_selected(mut self, id: Option<String>) -> Self {
        self.selected_id = id;
        self
    }

    pub fn with_searchable(mut self, searchable: bool) -> Self {
        self.searchable = searchable;
        self
    }

    pub fn with_width(mut self, width: gpui::Pixels) -> Self {
        self.width = width;
        self
    }

    pub fn with_max_height(mut self, max_height: gpui::Pixels) -> Self {
        self.max_height = max_height;
        self
    }

    pub fn with_direction(mut self, direction: Direction) -> Self {
        self.direction = direction;
        self
    }

    /// Open the dropdown and reset internal state.
    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.is_open = true;
        self.highlighted_index = None;
        if self.searchable {
            use gpui::Focusable;
            let handle = self.search_input.read(cx).focus_handle(cx);
            window.focus(&handle);
        }
        cx.notify();
    }

    /// Close the dropdown and reset search.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.is_open = false;
        self.highlighted_index = None;
        self.search_input.update(cx, |search, _| search.reset());
        cx.notify();
    }

    /// Toggle open/closed state.
    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_open {
            self.close(cx);
        } else {
            self.open(window, cx);
        }
    }

    /// Returns the items filtered by the current search query.
    pub fn filtered_items(&self, cx: &Context<Self>) -> Vec<DropdownItem> {
        if !self.searchable {
            return self.items.clone();
        }
        let query = self
            .search_input
            .read(cx)
            .content()
            .to_string()
            .to_lowercase();
        if query.is_empty() {
            return self.items.clone();
        }
        self.items
            .iter()
            .filter(|item| item.label.to_string().to_lowercase().contains(&query))
            .cloned()
            .collect()
    }

    fn select_item(&mut self, item_id: &str, _window: &mut Window, cx: &mut Context<Self>) {
        self.selected_id = Some(item_id.to_string());
        self.close(cx);
        cx.emit(DropdownEvent::Selected {
            id: item_id.to_string(),
        });
    }
}

impl Render for Dropdown {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let filtered = self.filtered_items(cx);
        let is_open = self.is_open;
        let highlighted = self.highlighted_index;
        let selected_id = self.selected_id.clone();
        let width = self.width;
        let max_height = self.max_height;
        let searchable = self.searchable;
        let direction = self.direction;

        // Build dropdown list items
        let mut dropdown_children: Vec<gpui::AnyElement> = Vec::new();

        if is_open {
            if searchable {
                dropdown_children.push(
                    div()
                        .px_2()
                        .py_1p5()
                        .border_b_1()
                        .border_color(rgb(0x333333))
                        .child(self.search_input.clone())
                        .into_any_element(),
                );
            }

            for (idx, item) in filtered.iter().enumerate() {
                let is_selected = selected_id.as_deref() == Some(&item.id);
                let is_highlighted = highlighted == Some(idx);
                let item_id = item.id.clone();

                dropdown_children.push(
                    div()
                        .id(("dropdown-item", idx))
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .px_3()
                        .py_1p5()
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(0x2a2a2a)))
                        .when(is_highlighted, |s| s.bg(rgb(0x2a2a2a)))
                        .child(
                            div()
                                .text_sm()
                                .text_color(if is_selected {
                                    rgb(0xffffff)
                                } else {
                                    rgb(0xcccccc)
                                })
                                .child(item.label.clone()),
                        )
                        .child(div().text_color(rgb(0x3b82f6)).child(if is_selected {
                            "✓"
                        } else {
                            ""
                        }))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.select_item(&item_id, window, cx);
                        }))
                        .into_any_element(),
                );
            }

            if filtered.is_empty() && searchable && !self.search_input.read(cx).content().is_empty()
            {
                dropdown_children.push(
                    div()
                        .px_3()
                        .py_3()
                        .text_color(rgb(0x666666))
                        .text_sm()
                        .child("No items found")
                        .into_any_element(),
                );
            }
        }

        // Button label text
        let button_label = self.label.clone();

        div()
            .id("dropdown")
            .relative()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if !this.is_open {
                    return;
                }
                match event.keystroke.key.as_str() {
                    "escape" => {
                        this.close(cx);
                    }
                    "down" => {
                        let items = this.filtered_items(cx);
                        let count = items.len();
                        if count > 0 {
                            let next = highlighted.map(|i| (i + 1) % count).unwrap_or(0);
                            this.highlighted_index = Some(next);
                        }
                        cx.notify();
                    }
                    "up" => {
                        let items = this.filtered_items(cx);
                        let count = items.len();
                        if count > 0 {
                            let prev = highlighted
                                .map(|i| if i == 0 { count - 1 } else { i - 1 })
                                .unwrap_or(count - 1);
                            this.highlighted_index = Some(prev);
                        }
                        cx.notify();
                    }
                    "enter" => {
                        if let Some(idx) = this.highlighted_index {
                            let items = this.filtered_items(cx);
                            if let Some(item) = items.get(idx) {
                                let id = item.id.clone();
                                this.select_item(&id, window, cx);
                            }
                        }
                    }
                    _ => {}
                }
            }))
            .child(
                div()
                    .id("dropdown-btn")
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(rgb(0x252525))
                    .text_color(rgb(0xaaaaaa))
                    .text_sm()
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(0x333333)))
                    .child(button_label)
                    .child(
                        svg()
                            .path(if is_open {
                                "chevron-up.svg"
                            } else {
                                "chevron-down.svg"
                            })
                            .size(px(14.))
                            .text_color(rgb(0xaaaaaa)),
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle(window, cx);
                    })),
            )
            .when(is_open, |this| {
                this.child(
                    // Invisible overlay to capture clicks outside the dropdown panel.
                    // Positioned to cover the nearest positioned ancestor.
                    div()
                        .id("dropdown-overlay")
                        .absolute()
                        .occlude()
                        .top(px(-5000.))
                        .left(px(-5000.))
                        .w(px(10000.))
                        .h(px(10000.))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.close(cx);
                            }),
                        ),
                )
                .child(
                    // Dropdown panel
                    div()
                        .id("dropdown-panel")
                        .absolute()
                        .occlude()
                        .when(matches!(direction, Direction::Down), |this| {
                            this.top(px(36.)).left(px(0.))
                        })
                        .when(matches!(direction, Direction::Up), |this| {
                            this.bottom(px(36.)).left(px(0.))
                        })
                        .w(width)
                        .max_h(max_height)
                        .overflow_y_scroll()
                        .bg(rgb(0x1e1e1e))
                        .border_1()
                        .border_color(rgb(0x333333))
                        .rounded_md()
                        .py_1()
                        .shadow(vec![gpui::BoxShadow {
                            color: gpui::rgba(0x000000aa).into(),
                            offset: gpui::point(px(0.), px(4.)),
                            blur_radius: px(12.),
                            spread_radius: px(0.),
                        }])
                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                            cx.stop_propagation();
                        })
                        .children(dropdown_children),
                )
            })
    }
}
