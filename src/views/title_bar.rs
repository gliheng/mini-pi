use crate::auth::state::AuthState;
use crate::core::app::AppStore;
use gpui::{
    AppContext, Context, EventEmitter, Hsla, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Pixels, Render, SharedString, StatefulInteractiveElement, Styled, Window,
    WindowControlArea, div, prelude::FluentBuilder, px, rgb,
};
#[cfg(target_os = "macos")]
use objc::{msg_send, sel, sel_impl};

const TRAFFIC_LIGHT_LEFT_PADDING: f32 = 78.0;
const TITLE_BAR_MIN_HEIGHT: f32 = 34.0;

struct TitleBarTooltip {
    label: SharedString,
}

impl Render for TitleBarTooltip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(rgb(0x2a2a2a))
            .border_1()
            .border_color(rgb(0x444444))
            .text_xs()
            .text_color(rgb(0xe5e5e5))
            .child(self.label.clone())
    }
}

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
    pub pinned: bool,
    should_move: bool,
}

impl TitleBar {
    pub fn new(title: impl Into<SharedString>, variant: TitleBarVariant) -> Self {
        Self {
            title: title.into(),
            variant,
            pinned: false,
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
            .gap_1()
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

        let pinned = self.pinned;
        bar = bar.child(
            div()
                .id("titlebar-pin")
                .flex()
                .flex_row()
                .items_center()
                .h_full()
                .flex_shrink_0()
                .child(
                    div()
                        .id("pin-button")
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(26.))
                        .cursor_pointer()
                        .text_color(if pinned { rgb(0x4f46e5) } else { rgb(0x888888) })
                        .child(
                            gpui::svg()
                                .path(if pinned { "unpin.svg" } else { "pin.svg" })
                                .size(px(16.))
                                .text_color(if pinned { rgb(0x4f46e5) } else { rgb(0x888888) }),
                        )
                        .hover(|style| style.bg(rgb(0x333333)).text_color(rgb(0xcccccc)))
                        .tooltip(move |_, cx| {
                            cx.new(|_| TitleBarTooltip {
                                label: if pinned {
                                    "Unpin Window".into()
                                } else {
                                    "Pin to Top".into()
                                },
                            })
                            .into()
                        })
                        .on_click(cx.listener(|this: &mut Self, _, window, _cx| {
                            this.pinned = !this.pinned;
                            #[cfg(any(
                                target_os = "macos",
                                target_os = "windows",
                                target_os = "linux"
                            ))]
                            set_window_level(window, this.pinned);
                        })),
                ),
        );

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
                                .hover(|style| style.bg(rgb(0x333333)).text_color(rgb(0xcccccc)))
                                .tooltip(|_, cx| {
                                    cx.new(|_| TitleBarTooltip {
                                        label: "Open Workspace".into(),
                                    })
                                    .into()
                                })
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
                        .pr(px(4.0))
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
                                .hover(|style| style.bg(rgb(0x333333)).text_color(rgb(0xcccccc)))
                                .tooltip(|_, cx| {
                                    cx.new(|_| TitleBarTooltip {
                                        label: "Export HTML".into(),
                                    })
                                    .into()
                                })
                                .on_click(cx.listener(|_this: &mut Self, _, _, cx| {
                                    cx.emit(TitleBarEvent::ExportHtml);
                                })),
                        ),
                );
            }
            TitleBarVariant::Home => {
                let user_panel_active = cx.global::<AppStore>().user_panel_active;
                let auth = cx.global::<AppStore>().auth.clone();
                let is_logged_in = matches!(&auth, AuthState::LoggedIn(_));
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
                                .bg(if !is_logged_in {
                                    rgb(0x333333)
                                } else if user_panel_active {
                                    rgb(0x4f46e5)
                                } else {
                                    rgb(0x6366f1)
                                })
                                .border_2()
                                .border_color(if user_panel_active {
                                    Into::<Hsla>::into(rgb(0x818cf8))
                                } else {
                                    Into::<Hsla>::into(rgb(0x6366f1)).alpha(0.0)
                                })
                                .cursor_pointer()
                                .text_color(if is_logged_in {
                                    rgb(0xffffff)
                                } else {
                                    rgb(0x888888)
                                })
                                .text_size(px(11.))
                                .font_weight(gpui::FontWeight::BOLD)
                                .when(is_logged_in, |el| {
                                    let initials: String = match &auth {
                                        AuthState::LoggedIn(user) => user
                                            .email
                                            .chars()
                                            .next()
                                            .map(|c| c.to_uppercase().to_string())
                                            .unwrap_or_else(|| "?".to_string()),
                                        _ => "?".to_string(),
                                    };
                                    el.child(initials)
                                })
                                .when(!is_logged_in, |el| {
                                    el.child(
                                        gpui::svg()
                                            .path("account.svg")
                                            .size(px(14.))
                                            .text_color(rgb(0x888888)),
                                    )
                                })
                                .hover(|style| style.bg(rgb(0x4f46e5)).border_color(rgb(0x818cf8)))
                                .tooltip(|_, cx| {
                                    cx.new(|_| TitleBarTooltip {
                                        label: "Account".into(),
                                    })
                                    .into()
                                })
                                .on_click(cx.listener(|_this: &mut Self, _, _, cx| {
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
                    .when(controls.minimize, |el| {
                        el.child(caption_button_svg(
                            "minimize.svg",
                            WindowControlArea::Min,
                            height,
                        ))
                    })
                    .when(controls.maximize, |el| {
                        let icon = if is_maximized {
                            "restore.svg"
                        } else {
                            "maximize.svg"
                        };
                        el.child(caption_button_svg(icon, WindowControlArea::Max, height))
                    })
                    .child(caption_button_svg(
                        "close.svg",
                        WindowControlArea::Close,
                        height,
                    )),
            );
        }

        bar
    }
}

fn caption_button_svg(
    icon_path: impl Into<SharedString>,
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
        .on_click(move |_event, window, _cx| match area {
            WindowControlArea::Close => window.remove_window(),
            WindowControlArea::Max => window.zoom_window(),
            WindowControlArea::Min => window.minimize_window(),
            _ => {}
        })
        .child(
            gpui::svg()
                .path(icon_path)
                .size(px(10.))
                .text_color(rgb(0x999999)),
        )
}

#[cfg(target_os = "macos")]
fn set_window_level(window: &Window, pinned: bool) {
    use objc::runtime::Object;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    const NSFLOATING_WINDOW_LEVEL: isize = 3;
    const NSNORMAL_WINDOW_LEVEL: isize = 0;

    if let Ok(handle) = HasWindowHandle::window_handle(window) {
        if let RawWindowHandle::AppKit(appkit) = handle.as_raw() {
            let ns_view = appkit.ns_view.as_ptr() as *mut Object;
            unsafe {
                let ns_window: *mut Object = msg_send![ns_view, window];
                let level = if pinned {
                    NSFLOATING_WINDOW_LEVEL
                } else {
                    NSNORMAL_WINDOW_LEVEL
                };
                let () = msg_send![ns_window, setLevel: level];
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn set_window_level(window: &Window, pinned: bool) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    type HWND = *mut std::ffi::c_void;
    const HWND_TOPMOST: HWND = -1isize as HWND;
    const HWND_NOTOPMOST: HWND = -2isize as HWND;
    const SWP_NOSIZE: u32 = 0x0001;
    const SWP_NOMOVE: u32 = 0x0002;
    const SWP_SHOWWINDOW: u32 = 0x0040;

    unsafe extern "system" {
        fn SetWindowPos(
            hwnd: HWND,
            hwnd_insert_after: HWND,
            x: i32,
            y: i32,
            cx: i32,
            cy: i32,
            u_flags: u32,
        ) -> i32;
    }

    if let Ok(handle) = HasWindowHandle::window_handle(window) {
        if let RawWindowHandle::Win32(win32) = handle.as_raw() {
            let hwnd = win32.hwnd.get() as *mut std::ffi::c_void;
            unsafe {
                SetWindowPos(
                    hwnd,
                    if pinned { HWND_TOPMOST } else { HWND_NOTOPMOST },
                    0,
                    0,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOMOVE | SWP_SHOWWINDOW,
                );
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn set_window_level(_window: &Window, pinned: bool) {
    // Best-effort X11 support via wmctrl. Wayland has no standard always-on-top protocol.
    let _ = std::process::Command::new("wmctrl")
        .args([
            "-r",
            ":ACTIVE:",
            "-b",
            if pinned { "add,above" } else { "remove,above" },
        ])
        .spawn();
}
