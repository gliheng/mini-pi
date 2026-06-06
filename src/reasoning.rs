use gpui::{Context, IntoElement, ParentElement, Render, SharedString, Styled, Window, div, prelude::*, px, rgb, svg};

/// A self-contained reasoning/thinking display component.
///
/// Manages its own collapsed/expanded state internally.
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

        div()
            .px_3()
            .py_1()
            .rounded_md()
            .bg(rgb(0x2a2a2a))
            .text_color(rgb(0x888888))
            .text_xs()
            .child(
                div()
                    .id("reasoning-toggle")
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
                        div().child(format!(
                            "Thinking {}",
                            if collapsed { "▶" } else { "▼" }
                        )),
                    )
                    .on_click(cx.listener(|this, _, _window, cx| {
                        this.collapsed = !this.collapsed;
                        cx.notify();
                    }))
                    .into_any_element(),
            )
            .when(!collapsed, |this| {
                this.child(div().mt_1().child(content))
            })
    }
}
