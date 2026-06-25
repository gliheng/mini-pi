use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px, svg,
};
use gpui_component::ActiveTheme as _;
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
            .bg(cx.theme().secondary)
            .rounded_md()
            .child(
                div()
                    .id("reasoning-toggle")
                    .px_2()
                    .py_1()
                    .flex()
                    .flex_row()
                    .gap_1()
                    .items_center()
                    .cursor_pointer()
                    .child(
                        svg()
                            .path("icons/thinking.svg")
                            .size(px(12.))
                            .text_color(cx.theme().muted_foreground),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("Thinking {}", if collapsed { "▶" } else { "▼" })),
                    )
                    .on_click(cx.listener(|this, _, _window, cx| {
                        this.collapsed = !this.collapsed;
                        cx.notify();
                    })),
            )
            .content(
                div()
                    .px_2()
                    .pb_2()
                    .text_xs()
                    .text_color(cx.theme().secondary_foreground)
                    .child(content),
            )
    }
}
