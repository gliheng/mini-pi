use gpui::{
    Context, Decorations, InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels,
    Render, SharedString, StatefulInteractiveElement, Styled, Window, WindowControlArea, div,
    px, rgb, svg,
};

use gpui::prelude::FluentBuilder;

const TRAFFIC_LIGHT_LEFT_PADDING: f32 = 78.0;
const TITLE_BAR_MIN_HEIGHT: f32 = 34.0;

pub struct TitleBar {
    pub title: SharedString,
    pub icon: Option<SharedString>,
    should_move: bool,
}

impl TitleBar {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            icon: None,
            should_move: false,
        }
    }

    pub fn icon(mut self, path: impl Into<SharedString>) -> Self {
        self.icon = Some(path.into());
        self
    }

    pub fn height(window: &Window) -> Pixels {
        (1.75 * window.rem_size()).max(px(TITLE_BAR_MIN_HEIGHT))
    }
}

impl Render for TitleBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let height = Self::height(window);
        let fullscreen = window.is_fullscreen();
        let decorations = window.window_decorations();
        let controls = window.window_controls();
        let client_side = matches!(decorations, Decorations::Client { .. });

        let left_padding = if fullscreen {
            px(8.0)
        } else if cfg!(target_os = "macos") {
            px(TRAFFIC_LIGHT_LEFT_PADDING)
        } else {
            px(12.0)
        };

        let mut title_content = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2();

        if let Some(icon_path) = &self.icon {
            title_content = title_content.child(
                svg()
                    .path(icon_path.clone())
                    .size(px(20.))
                    .text_color(rgb(0xe0e0e0)),
            );
        }

        title_content = title_content.child(
            div()
                .text_sm()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(0x888888))
                .overflow_x_hidden()
                .child(self.title.clone()),
        );

        let mut bar = div()
            .id("titlebar")
            .window_control_area(WindowControlArea::Drag)
            .w_full()
            .h(height)
            .bg(rgb(0x1e1e1e))
            .border_b_1()
            .border_color(rgb(0x333333))
            .flex()
            .items_center()
            .pl(left_padding)
            .pr(px(12.))
            .on_mouse_down_out(cx.listener(|this: &mut Self, _, _, _| {
                this.should_move = false;
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this: &mut Self, _, _, _| {
                this.should_move = false;
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(|this: &mut Self, _, _, _| {
                this.should_move = true;
            }))
            .on_mouse_move(cx.listener(|this: &mut Self, _, window, _| {
                if this.should_move {
                    this.should_move = false;
                    window.start_window_move();
                }
            }))
            .on_click(|event: &gpui::ClickEvent, window, _| {
                if event.click_count() == 2 {
                    window.titlebar_double_click();
                }
            })
            .child(title_content);

        if client_side && !cfg!(target_os = "macos") {
            bar = bar.child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .justify_end()
                    .items_center()
                    .when(controls.minimize, |el: gpui::Div| {
                        el.child(window_caption_button(
                            "─",
                            WindowControlArea::Min,
                            rgb(0x999999),
                            height,
                        ))
                    })
                    .when(controls.maximize, |el: gpui::Div| {
                        let label = if window.is_maximized() { "⧉" } else { "□" };
                        el.child(window_caption_button(
                            label,
                            WindowControlArea::Max,
                            rgb(0x999999),
                            height,
                        ))
                    })
                    .when(controls.fullscreen || controls.maximize, |el: gpui::Div| {
                        el.child(window_caption_button(
                            "✕",
                            WindowControlArea::Close,
                            rgb(0xcc4444),
                            height,
                        ))
                    }),
            );
        }

        bar
    }
}

fn window_caption_button(
    label: &str,
    area: WindowControlArea,
    text_color: gpui::Rgba,
    height: Pixels,
) -> gpui::Div {
    div()
        .window_control_area(area)
        .flex()
        .items_center()
        .justify_center()
        .size(height)
        .text_color(text_color)
        .text_size(px(13.))
        .hover(|s: gpui::StyleRefinement| s.bg(rgb(0x333333)).text_color(rgb(0xffffff)))
        .child(label.to_string())
}