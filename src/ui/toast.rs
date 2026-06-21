use std::path::PathBuf;
use std::time::Duration;

use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Task, div, px, rgb,
};

/// A simple auto-dismissing toast notification with an optional action button.
///
/// Render it as an absolutely positioned overlay inside a `relative()` container.
/// Use `show_for` to display a message for a fixed duration.
/// Use `set_action` to add a button that reveals a file in the system file manager.
pub struct Toast {
    pub message: SharedString,
    pub visible: bool,
    pub action_label: Option<SharedString>,
    pub action_path: Option<PathBuf>,
    generation: u64,
    dismiss_task: Option<Task<()>>,
}

impl Toast {
    pub fn new(message: impl Into<SharedString>) -> Self {
        Self {
            message: message.into(),
            visible: false,
            action_label: None,
            action_path: None,
            generation: 0,
            dismiss_task: None,
        }
    }

    pub fn set_message(&mut self, message: impl Into<SharedString>) {
        self.message = message.into();
    }

    pub fn set_action(&mut self, label: impl Into<SharedString>, path: impl Into<PathBuf>) {
        self.action_label = Some(label.into());
        self.action_path = Some(path.into());
    }

    /// Show the toast for the given duration, then hide it automatically.
    pub fn show_for(&mut self, duration: Duration, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        self.visible = true;
        cx.notify();

        self.dismiss_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(duration).await;
            this.update(cx, |toast, cx| toast.hide_if_generation(generation, cx))
                .ok();
        }));
    }

    fn hide_if_generation(&mut self, generation: u64, cx: &mut Context<Self>) {
        if self.generation == generation {
            self.hide(cx);
        }
    }

    pub fn hide(&mut self, cx: &mut Context<Self>) {
        self.visible = false;
        self.dismiss_task = None;
        cx.notify();
    }
}

impl Render for Toast {
    fn render(&mut self, _window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let has_action = self.action_label.is_some() && self.action_path.is_some();
        let mut row = div().flex().flex_row().items_center().gap_3().child(
            div()
                .text_sm()
                .text_color(rgb(0xe5e5e5))
                .child(self.message.clone()),
        );

        if has_action {
            let path = self.action_path.clone().unwrap();
            row = row.child(
                div()
                    .id("toast-action-button")
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(rgb(0x3a3a3a))
                    .border_1()
                    .border_color(rgb(0x555555))
                    .cursor_pointer()
                    .text_sm()
                    .text_color(rgb(0xffffff))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .hover(|style| style.bg(rgb(0x4a4a4a)).border_color(rgb(0x666666)))
                    .on_click(cx.listener(move |_this, _event, _window, cx| {
                        cx.reveal_path(&path);
                    }))
                    .child(self.action_label.clone().unwrap()),
            );
        }

        div()
            .px_4()
            .py_2()
            .rounded_lg()
            .bg(rgb(0x2a2a2a))
            .border_1()
            .border_color(rgb(0x444444))
            .shadow_md()
            .max_w(px(560.))
            .child(row)
    }
}
