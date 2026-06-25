use gpui::{Context, IntoElement, Render, Window, div, prelude::*};
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, Size};

/// A mini-app launcher that displays a grid of available mini-apps as icons.
pub struct MiniApp;

impl MiniApp {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self
    }
}

impl Render for MiniApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let apps = vec![
            (IconName::Cpu, "System"),
            (IconName::HardDrive, "Storage"),
            (IconName::MemoryStick, "Memory"),
            (IconName::Bot, "AI Tools"),
            (IconName::BookOpen, "Docs"),
            (IconName::Github, "GitHub"),
            (IconName::Search, "Search"),
            (IconName::Settings, "Settings"),
        ];

        div().size_full().p_4().bg(cx.theme().background).child(
            div()
                .grid()
                .grid_cols(4)
                .gap_4()
                .children(apps.into_iter().map(|(icon, label)| {
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .p_3()
                        .rounded(cx.theme().radius)
                        .hover(|this| this.bg(cx.theme().secondary))
                        .child(
                            Icon::new(icon)
                                .with_size(Size::Large)
                                .text_color(cx.theme().primary),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().foreground)
                                .child(label),
                        )
                })),
        )
    }
}
