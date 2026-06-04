use gpui::{
    Context, InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels,
    Render, SharedString, StatefulInteractiveElement, Styled, Window,
    WindowControlArea, div, px, rgb,
};

use gpui::prelude::FluentBuilder;

const TRAFFIC_LIGHT_LEFT_PADDING: f32 = 78.0;
const TITLE_BAR_MIN_HEIGHT: f32 = 34.0;
const WINDOWS_ICON_SIZE: f32 = 10.0;

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
        let controls = window.window_controls();

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
                div()
                    .child(
                        gpui::svg()
                            .path(icon_path.clone())
                            .size(px(20.))
                            .text_color(rgb(0xe0e0e0)),
                    )
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

        let drag_region = div()
            .id("titlebar-drag-region")
            .window_control_area(WindowControlArea::Drag)
            .flex()
            .items_center()
            .justify_between()
            .flex_shrink_0()
            .flex_1()
            .min_w(px(0.0))
            .h_full()
            .pl(left_padding)
            .pr(if cfg!(target_os = "macos") {
                px(12.0)
            } else {
                px(0.0)
            })
            .child(title_content);

        let mut bar = div()
            .id("titlebar")
            .w_full()
            .h(height)
            .flex_shrink_0()
            .bg(rgb(0x1e1e1e))
            .border_b_1()
            .border_color(rgb(0x333333))
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
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
                    if cfg!(target_os = "macos") {
                        window.titlebar_double_click();
                    } else {
                        window.zoom_window();
                    }
                }
            })
            .child(drag_region);

        // Add window controls for non-Mac platforms with client-side decorations
        #[cfg(not(target_os = "macos"))]
        {
            let is_maximized = window.is_maximized();
            
            bar = bar.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .h_full()
                    .flex_shrink_0()
                    .map(|el| {
                        if cfg!(target_os = "windows") {
                            el.font_family("Segoe Fluent Icons")
                        } else {
                            el
                        }
                    })
                    .when(controls.minimize, |el| {
                        el.child(caption_button(
                            "\u{e921}",
                            WindowControlArea::Min,
                            height,
                        ))
                    })
                    .when(controls.maximize, |el| {
                        let icon = if is_maximized {
                            "\u{e923}"
                        } else {
                            "\u{e922}"
                        };
                        el.child(caption_button(
                            icon,
                            WindowControlArea::Max,
                            height,
                        ))
                    })
                    .child(caption_button(
                        "\u{e8bb}",
                        WindowControlArea::Close,
                        height,
                    )),
            );
        }

        bar
    }
}

fn caption_button(
    icon: &str,
    area: WindowControlArea,
    height: Pixels,
) -> impl IntoElement {
    let is_close = matches!(area, WindowControlArea::Close);
    
    div()
        .id(match area {
            WindowControlArea::Close => "close",
            WindowControlArea::Max => "maximize",
            WindowControlArea::Min => "minimize",
            _ => "caption-button",
        })
        .window_control_area(area)
        .occlude()
        .flex()
        .items_center()
        .justify_center()
        .w(px(46.))
        .h(height)
        .text_size(px(WINDOWS_ICON_SIZE))
        .text_color(rgb(0x999999))
        .hover(move |s: gpui::StyleRefinement| {
            if is_close {
                s.bg(rgb(0xe81123)).text_color(rgb(0xffffff))
            } else {
                s.bg(rgb(0x333333)).text_color(rgb(0xffffff))
            }
        })
        .active(move |s: gpui::StyleRefinement| {
            if is_close {
                s.bg(rgb(0xf1707a)).text_color(rgb(0xffffff))
            } else {
                s.bg(rgb(0x444444)).text_color(rgb(0xffffff))
            }
        })
        .child(icon.to_string())
}
