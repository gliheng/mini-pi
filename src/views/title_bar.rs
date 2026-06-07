use gpui::{
    Context, EventEmitter, Hsla, InteractiveElement, IntoElement, MouseButton, ParentElement,
    Pixels, Render, SharedString, StatefulInteractiveElement, Styled, Window, WindowControlArea,
    div, px, rgb,
};

const TRAFFIC_LIGHT_LEFT_PADDING: f32 = 78.0;
const TITLE_BAR_MIN_HEIGHT: f32 = 34.0;
const WINDOWS_ICON_SIZE: f32 = 10.0;

#[derive(Clone, Copy, PartialEq)]
pub enum TitleBarVariant {
    Home,
    Chat,
}

#[derive(Clone)]
pub enum TitleBarEvent {
    ToggleUserPanel,
    ExportHtml,
    OpenWorkspace,
}

pub struct TitleBar {
    pub title: SharedString,
    pub variant: TitleBarVariant,
    pub avatar_active: bool,
    should_move: bool,
}

impl TitleBar {
    pub fn new(title: impl Into<SharedString>, variant: TitleBarVariant) -> Self {
        Self {
            title: title.into(),
            variant,
            avatar_active: false,
            should_move: false,
        }
    }

    pub fn height(window: &Window) -> Pixels {
        (1.75 * window.rem_size()).max(px(TITLE_BAR_MIN_HEIGHT))
    }
}

impl EventEmitter<TitleBarEvent> for TitleBar {}

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

        let mut title_content = div().flex().flex_row().items_center().gap_2();

        // if let Some(icon_path) = match self.variant {
        //     TitleBarVariant::Home => Some(SharedString::from("logo.svg")),
        //     TitleBarVariant::Chat => None,
        // } {
        //     title_content = title_content.child(
        //         div().child(
        //             gpui::svg()
        //                 .path(icon_path)
        //                 .size(px(20.))
        //                 .text_color(rgb(0xe0e0e0)),
        //         ),
        //     );
        // }

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
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this: &mut Self, _, _, _| {
                    this.should_move = false;
                }),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this: &mut Self, _, _, _| {
                    this.should_move = true;
                }),
            )
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

        match self.variant {
            TitleBarVariant::Chat => {
                bar = bar.child(
                    div()
                        .id("titlebar-open-workspace")
                        .flex()
                        .flex_row()
                        .items_center()
                        .h_full()
                        .flex_shrink_0()
                        .child(
                            div()
                                .id("open-workspace-button")
                                .flex()
                                .items_center()
                                .justify_center()
                                .size(px(26.))
                                .cursor_pointer()
                                .text_color(rgb(0x888888))
                                .child(
                                    gpui::svg()
                                        .path("folder.svg")
                                        .size(px(16.))
                                        .text_color(rgb(0x888888)),
                                )
                                .hover(|style| style.text_color(rgb(0xcccccc)))
                                .on_click(cx.listener(|_this: &mut Self, _, _, cx| {
                                    cx.emit(TitleBarEvent::OpenWorkspace);
                                })),
                        ),
                );

                bar = bar.child(
                    div()
                        .id("titlebar-export")
                        .flex()
                        .flex_row()
                        .items_center()
                        .h_full()
                        .flex_shrink_0()
                        .child(
                            div()
                                .id("export-button")
                                .flex()
                                .items_center()
                                .justify_center()
                                .size(px(26.))
                                .cursor_pointer()
                                .text_color(rgb(0x888888))
                                .child(
                                    gpui::svg()
                                        .path("export.svg")
                                        .size(px(16.))
                                        .text_color(rgb(0x888888)),
                                )
                                .hover(|style| style.text_color(rgb(0xcccccc)))
                                .on_click(cx.listener(|_this: &mut Self, _, _, cx| {
                                    cx.emit(TitleBarEvent::ExportHtml);
                                })),
                        ),
                );
            }
            TitleBarVariant::Home => {
                let avatar_active = self.avatar_active;
                bar = bar.child(
                    div()
                        .id("titlebar-avatar")
                        .flex()
                        .flex_row()
                        .items_center()
                        .h_full()
                        .flex_shrink_0()
                        .pr(if cfg!(target_os = "macos") {
                            px(12.0)
                        } else {
                            px(4.0)
                        })
                        .child(
                            div()
                                .id("avatar-button")
                                .flex()
                                .items_center()
                                .justify_center()
                                .size(px(26.))
                                .rounded_full()
                                .bg(if avatar_active {
                                    rgb(0x4f46e5)
                                } else {
                                    rgb(0x6366f1)
                                })
                                .border_2()
                                .border_color(if avatar_active {
                                    Into::<Hsla>::into(rgb(0x818cf8))
                                } else {
                                    Into::<Hsla>::into(rgb(0x6366f1)).alpha(0.0)
                                })
                                .cursor_pointer()
                                .text_color(rgb(0xffffff))
                                .text_size(px(11.))
                                .font_weight(gpui::FontWeight::BOLD)
                                .child("JD")
                                .hover(|style| style.bg(rgb(0x4f46e5)).border_color(rgb(0x818cf8)))
                                .on_click(cx.listener(|this: &mut Self, _, _, cx| {
                                    this.avatar_active = !this.avatar_active;
                                    cx.emit(TitleBarEvent::ToggleUserPanel);
                                })),
                        ),
                );
            }
        }

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
                        el.child(caption_button("\u{e921}", WindowControlArea::Min, height))
                    })
                    .when(controls.maximize, |el| {
                        let icon = if is_maximized { "\u{e923}" } else { "\u{e922}" };
                        el.child(caption_button(icon, WindowControlArea::Max, height))
                    })
                    .child(caption_button("\u{e8bb}", WindowControlArea::Close, height)),
            );
        }

        bar
    }
}

fn caption_button(icon: &str, area: WindowControlArea, height: Pixels) -> impl IntoElement {
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
