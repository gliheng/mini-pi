use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px, rgb, svg,
};
use gpui_component::collapsible::Collapsible;

/// A self-contained reasoning/thinking display component.
///
/// Manages its own collapsed/expanded state internally and renders via
/// `gpui_component::Collapsible`.
pub struct Reasoning {
    content: SharedString,
    collapsed: bool,
}

impl Reasoning {
    pub fn new(content: impl Into<SharedString>) -> Self {
        Self {
            content: content.into(),
            collapsed: false,
        }
    }

    pub fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }

    pub fn set_content(&mut self, content: impl Into<SharedString>) {
        self.content = content.into();
    }
}

impl Render for Reasoning {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let collapsed = self.collapsed;
        let content = self.content.clone();

        Collapsible::new()
            .open(!collapsed)
            .bg(rgb(0x2a2a2a))
            .rounded_md()
            .child(
                div()
                    .id("reasoning-toggle")
                    .px_3()
                    .py_1()
                    .flex()
                    .flex_row()
                    .gap_1()
                    .items_center()
                    .cursor_pointer()
                    .child(
                        svg()
                            .path("thinking.svg")
                            .size(px(12.))
                            .text_color(rgb(0x888888)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x888888))
                            .child(format!("Thinking {}", if collapsed { "▶" } else { "▼" })),
                    )
                    .on_click(cx.listener(|this, _, _window, cx| {
                        this.collapsed = !this.collapsed;
                        cx.notify();
                    })),
            )
            .content(
                div()
                    .px_3()
                    .pb_2()
                    .text_xs()
                    .text_color(rgb(0x888888))
                    .child(content),
            )
    }
}
