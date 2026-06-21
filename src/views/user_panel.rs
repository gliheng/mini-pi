use std::time::Duration;

use gpui::{
    AppContext, BorrowAppContext, ClipboardItem, Context, EventEmitter, InteractiveElement,
    IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window,
    div, prelude::FluentBuilder, px, rgb,
};

use crate::auth::state::{self, AuthState};
use crate::auth::supabase;
use crate::core::app::AppStore;
use crate::remote::RemoteStatus;
use crate::remote::cloudflared;
use crate::remote::controller::TunnelLog;
use crate::remote::qr::qr_image_source;
use crate::sync::settings_sync;
use crate::ui::input::TextInput;
use crate::ui::toast::Toast;

#[derive(Clone)]
pub enum UserPanelEvent {
    BackPressed,
    AuthStateChanged,
}

#[derive(Clone, Copy, PartialEq)]
pub enum AuthDialog {
    Login,
    Signup,
}

#[derive(Clone)]
pub enum CloudflaredDialog {
    Prompt,
    Downloading,
    Error(String),
}

struct StatusLogTooltip {
    text: SharedString,
}

impl Render for StatusLogTooltip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .max_w(px(320.))
            .px_3()
            .py_2()
            .rounded_md()
            .bg(rgb(0x2a2a2a))
            .border_1()
            .border_color(rgb(0x444444))
            .text_xs()
            .text_color(rgb(0xe5e5e5))
            .whitespace_normal()
            .child(self.text.clone())
    }
}

pub struct UserPanel {
    pub email_input: gpui::Entity<TextInput>,
    pub password_input: gpui::Entity<TextInput>,
    pub confirm_password_input: gpui::Entity<TextInput>,
    pub auth_error: Option<String>,
    pub auth_dialog: Option<AuthDialog>,
    pub cloudflared_dialog: Option<CloudflaredDialog>,
    pub toast: gpui::Entity<Toast>,
    pub _email_sub: gpui::Subscription,
    pub _password_sub: gpui::Subscription,
    pub _confirm_password_sub: gpui::Subscription,
    pub _remote_sub: Option<gpui::Subscription>,
    pub _toast_sub: gpui::Subscription,
}

impl UserPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let email_input = cx.new(|cx| TextInput::new(cx, "Email"));
        let password_input = cx.new(|cx| TextInput::new(cx, "Password").with_password_mode());
        let confirm_password_input =
            cx.new(|cx| TextInput::new(cx, "Confirm Password").with_password_mode());

        let _email_sub = cx.observe(&email_input, |_, _, cx| {
            cx.notify();
        });
        let _password_sub = cx.observe(&password_input, |_, _, cx| {
            cx.notify();
        });
        let _confirm_password_sub = cx.observe(&confirm_password_input, |_, _, cx| {
            cx.notify();
        });

        let remote_controller = cx.global::<AppStore>().remote_controller.clone();
        let remote_sub = remote_controller.as_ref().map(|controller| {
            cx.observe(controller, |_this, _controller, cx| {
                cx.notify();
            })
        });

        let toast = cx.new(|_| Toast::new(""));
        let _toast_sub = cx.observe(&toast, |_this, _toast, cx| {
            cx.notify();
        });

        Self {
            email_input,
            password_input,
            confirm_password_input,
            auth_error: None,
            auth_dialog: None,
            cloudflared_dialog: None,
            toast,
            _email_sub,
            _password_sub,
            _confirm_password_sub,
            _remote_sub: remote_sub,
            _toast_sub,
        }
    }

    fn start_cloudflared_download(&mut self, cx: &mut Context<Self>) {
        self.cloudflared_dialog = Some(CloudflaredDialog::Downloading);
        cx.notify();

        let weak = cx.entity().downgrade();
        cx.spawn(async move |_, cx| {
            let result = smol::unblock(move || cloudflared::download_and_install()).await;
            let _ = weak.update(cx, |this, cx| {
                match result {
                    Ok(path) => {
                        let controller = cx.update_global(|app: &mut AppStore, _| {
                            app.remote_controller.clone()
                        });
                        if let Some(controller) = controller {
                            let command = path.to_string_lossy().to_string();
                            controller.update(cx, |c, cx| {
                                c.config.cloudflared.command = command;
                                c.set_enabled(true, cx);
                            });
                        }
                        this.cloudflared_dialog = None;
                    }
                    Err(e) => {
                        this.cloudflared_dialog = Some(CloudflaredDialog::Error(e));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl EventEmitter<UserPanelEvent> for UserPanel {}

impl Render for UserPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let auth = cx.global::<AppStore>().auth.clone();
        if auth.is_logged_in() && self.auth_dialog.is_some() {
            self.auth_dialog = None;
        }

        let email_val = self.email_input.read(cx).content().clone();
        let password_val = self.password_input.read(cx).content().clone();
        let is_logging_in = matches!(auth, AuthState::LoggingIn);
        let error_msg: Option<SharedString> = self.auth_error.clone().map(|s| s.into());

        let content = div()
            .id("user-panel-content")
            .flex_1()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .items_center()
            .px_6()
            .py_8()
            .gap_6()
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .child(render_back_button(cx)),
            )
            .child(
                gpui::svg()
                    .path("logo.svg")
                    .size(px(48.))
                    .text_color(rgb(0x6366f1)),
            )
            .child(render_auth_content(self, &auth, cx));

        if let Some(dialog) = self.auth_dialog {
            let confirm_password_val = self.confirm_password_input.read(cx).content().clone();

            let (title, subtitle): (SharedString, SharedString) = match dialog {
                AuthDialog::Login => (
                    "Sign In".into(),
                    "Sign in to sync your agent settings across devices".into(),
                ),
                AuthDialog::Signup => (
                    "Create Account".into(),
                    "Sign up to sync your agent settings across devices".into(),
                ),
            };

            let form_fields = div()
                .w_full()
                .flex()
                .flex_col()
                .gap_3()
                .child(render_email_field(self))
                .child(render_password_field(self))
                .when(dialog == AuthDialog::Signup, |el: gpui::Div| {
                    el.child(render_confirm_password_field(self))
                })
                .when(error_msg.is_some(), |el: gpui::Div| {
                    el.child(
                        div()
                            .text_xs()
                            .text_color(rgb(0xfca5a5))
                            .child(error_msg.unwrap_or_default()),
                    )
                })
                .when(is_logging_in, |el: gpui::Div| {
                    el.child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .px_4()
                            .py_3()
                            .rounded_lg()
                            .bg(rgb(0x4f46e5))
                            .text_color(rgb(0xffffff))
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("Signing in..."),
                    )
                })
                .when(
                    !is_logging_in && dialog == AuthDialog::Login,
                    |el: gpui::Div| {
                        el.child(render_login_button(
                            email_val.clone(),
                            password_val.clone(),
                            cx,
                        ))
                    },
                )
                .when(
                    !is_logging_in && dialog == AuthDialog::Signup,
                    |el: gpui::Div| {
                        el.child(render_signup_submit_button(
                            email_val.clone(),
                            password_val.clone(),
                            confirm_password_val.clone(),
                            cx,
                        ))
                    },
                );

            div()
                .id("user-panel")
                .flex()
                .flex_col()
                .size_full()
                .relative()
                .child(content)
                .child(
                    div()
                        .id("auth-dialog-overlay")
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .bg(gpui::rgba(0x00000099))
                        .flex()
                        .items_center()
                        .justify_center()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.auth_dialog = None;
                            this.auth_error = None;
                            cx.notify();
                        }))
                        .child(
                            div()
                                .id("auth-dialog-card")
                                .mx_8()
                                .w(px(360.))
                                .flex()
                                .flex_col()
                                .gap_4()
                                .px_6()
                                .py_6()
                                .rounded_xl()
                                .bg(rgb(0x1f1f1f))
                                .border_1()
                                .border_color(rgb(0x333333))
                                .on_click(|_, _, cx| {
                                    cx.stop_propagation();
                                })
                                .child(
                                    div()
                                        .text_xl()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(rgb(0xe0e0e0))
                                        .child(title),
                                )
                                .child(div().text_sm().text_color(rgb(0x888888)).child(subtitle))
                                .child(form_fields)
                                .child(
                                    div()
                                        .id("auth-dialog-close-btn")
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .px_4()
                                        .py_2()
                                        .rounded_lg()
                                        .bg(rgb(0x252525))
                                        .border_1()
                                        .border_color(rgb(0x444444))
                                        .cursor_pointer()
                                        .text_color(rgb(0x888888))
                                        .text_sm()
                                        .hover(|style| style.bg(rgb(0x333333)))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.auth_dialog = None;
                                            this.auth_error = None;
                                            cx.notify();
                                        }))
                                        .child("Cancel"),
                                )
                                .when(!is_logging_in && dialog == AuthDialog::Login, |el| {
                                    el.child(
                                        div()
                                            .id("switch-to-signup")
                                            .w_full()
                                            .flex()
                                            .flex_row()
                                            .justify_end()
                                            .cursor_pointer()
                                            .text_color(rgb(0x6366f1))
                                            .text_xs()
                                            .hover(|style| style.text_color(rgb(0x818cf8)))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.auth_error = None;
                                                this.auth_dialog = Some(AuthDialog::Signup);
                                                cx.notify();
                                            }))
                                            .child("Create Account"),
                                    )
                                })
                                .when(!is_logging_in && dialog == AuthDialog::Signup, |el| {
                                    el.child(
                                        div()
                                            .id("switch-to-login")
                                            .w_full()
                                            .flex()
                                            .flex_row()
                                            .justify_end()
                                            .cursor_pointer()
                                            .text_color(rgb(0x6366f1))
                                            .text_xs()
                                            .hover(|style| style.text_color(rgb(0x818cf8)))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.auth_error = None;
                                                this.auth_dialog = Some(AuthDialog::Login);
                                                cx.notify();
                                            }))
                                            .child("Sign In"),
                                    )
                                }),
                        ),
                )
                .when(self.cloudflared_dialog.is_some(), |this| {
                    this.child(render_cloudflared_dialog(self, cx))
                })
                .when(self.toast.read(cx).visible, |this| {
                    this.child(render_toast_overlay(self, cx))
                })
        } else {
            div()
                .id("user-panel")
                .flex()
                .flex_col()
                .size_full()
                .relative()
                .child(content)
                .when(self.cloudflared_dialog.is_some(), |this| {
                    this.child(render_cloudflared_dialog(self, cx))
                })
                .when(self.toast.read(cx).visible, |this| {
                    this.child(render_toast_overlay(self, cx))
                })
        }
    }
}

fn render_toast_overlay(panel: &UserPanel, _cx: &mut Context<UserPanel>) -> impl IntoElement {
    div()
        .absolute()
        .top(px(48.))
        .left(px(0.))
        .right(px(0.))
        .flex()
        .flex_row()
        .items_center()
        .justify_center()
        .child(panel.toast.clone())
}

fn render_cloudflared_dialog(
    panel: &mut UserPanel,
    cx: &mut Context<UserPanel>,
) -> impl IntoElement {
    let state = panel.cloudflared_dialog.clone().unwrap();
    let (title, body, primary_label, is_downloading, error_msg): (
        SharedString,
        SharedString,
        SharedString,
        bool,
        Option<String>,
    ) = match &state {
        CloudflaredDialog::Prompt => (
            "Cloudflared required".into(),
            "Remote control needs the cloudflared tunnel binary. Download it to the app data folder now?".into(),
            "Download & Start".into(),
            false,
            None,
        ),
        CloudflaredDialog::Downloading => (
            "Downloading cloudflared".into(),
            "Downloading and installing cloudflared...".into(),
            "Downloading...".into(),
            true,
            None,
        ),
        CloudflaredDialog::Error(e) => (
            "Download failed".into(),
            "Could not download cloudflared.".into(),
            "Retry".into(),
            false,
            Some(e.clone()),
        ),
    };

    div()
        .id("cloudflared-dialog-overlay")
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .bg(gpui::rgba(0x00000099))
        .flex()
        .items_center()
        .justify_center()
        .on_click(cx.listener(|this, _, _, cx| {
            this.cloudflared_dialog = None;
            cx.notify();
        }))
        .child(
            div()
                .id("cloudflared-dialog-card")
                .mx_8()
                .w(px(360.))
                .flex()
                .flex_col()
                .gap_4()
                .px_6()
                .py_6()
                .rounded_xl()
                .bg(rgb(0x1f1f1f))
                .border_1()
                .border_color(rgb(0x333333))
                .on_click(|_, _, cx| {
                    cx.stop_propagation();
                })
                .child(
                    div()
                        .text_xl()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(0xe0e0e0))
                        .child(title),
                )
                .child(div().text_sm().text_color(rgb(0x888888)).child(body))
                .when_some(error_msg, |this, err| {
                    this.child(
                        div()
                            .text_xs()
                            .text_color(rgb(0xfca5a5))
                            .child(err),
                    )
                })
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_3()
                        .child(
                            div()
                                .id("cloudflared-download-btn")
                                .flex_1()
                                .flex()
                                .items_center()
                                .justify_center()
                                .px_4()
                                .py_2()
                                .rounded_lg()
                                .bg(if is_downloading {
                                    rgb(0x333333)
                                } else {
                                    rgb(0x4f46e5)
                                })
                                .cursor_pointer()
                                .text_color(if is_downloading {
                                    rgb(0x888888)
                                } else {
                                    rgb(0xffffff)
                                })
                                .text_sm()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .when(!is_downloading, |s| {
                                    s.hover(|style| style.bg(rgb(0x6366f1)))
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.start_cloudflared_download(cx);
                                        }))
                                })
                                .child(primary_label),
                        )
                        .child(
                            div()
                                .id("cloudflared-cancel-btn")
                                .flex_1()
                                .flex()
                                .items_center()
                                .justify_center()
                                .px_4()
                                .py_2()
                                .rounded_lg()
                                .bg(rgb(0x252525))
                                .border_1()
                                .border_color(rgb(0x444444))
                                .cursor_pointer()
                                .text_color(rgb(0x888888))
                                .text_sm()
                                .hover(|style| style.bg(rgb(0x333333)))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.cloudflared_dialog = None;
                                    cx.notify();
                                }))
                                .child("Cancel"),
                        ),
                ),
        )
}

fn render_remote_control_section(cx: &mut Context<UserPanel>) -> impl IntoElement {
    let Some(controller) = cx.global::<AppStore>().remote_controller.clone() else {
        return div();
    };

    let c = controller.read(cx);
    let enabled = c.is_enabled();
    let status = c.status.clone();
    let tunnel_url = c.tunnel_url.clone();
    let tunnel_log = c.tunnel_log.clone();
    let error_message = c.error_message.clone();
    let is_starting = c.is_starting();
    let is_reconnecting = c.is_reconnecting();
    let is_busy = is_starting || is_reconnecting;

    let status_text: SharedString = match &status {
        RemoteStatus::Disabled => "Off".into(),
        RemoteStatus::Starting => "Starting...".into(),
        RemoteStatus::Running => "Connected".into(),
        RemoteStatus::Reconnecting => "Reconnecting...".into(),
        RemoteStatus::Error(e) => format!("Error: {}", e).into(),
    };
    let status_color = match &status {
        RemoteStatus::Running => rgb(0x22c55e),
        RemoteStatus::Error(_) => rgb(0xef4444),
        RemoteStatus::Reconnecting => rgb(0xf59e0b),
        _ => rgb(0x888888),
    };

    let mut section = div()
        .w_full()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(rgb(0x888888))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child("REMOTE CONTROL"),
        )
        .child(
            div()
                .id("remote-toggle-row")
                .w_full()
                .flex()
                .flex_row()
                .items_center()
                .gap_3()
                .px_4()
                .py_3()
                .rounded_lg()
                .bg(rgb(0x252525))
                .child(
                    div()
                        .flex_1()
                        .text_sm()
                        .text_color(rgb(0xe0e0e0))
                        .child("Enable remote control"),
                )
                .child(
                    div()
                        .id("remote-toggle")
                        .w(px(44.))
                        .h(px(24.))
                        .rounded_full()
                        .bg(if enabled {
                            rgb(0x6366f1)
                        } else {
                            rgb(0x444444)
                        })
                        .when(!is_busy, |s| s.cursor_pointer())
                        .when(is_busy, |s| s.opacity(0.6))
                        .child(
                            div()
                                .id("remote-toggle-knob")
                                .size(px(20.))
                                .rounded_full()
                                .bg(rgb(0xffffff))
                                .when(enabled, |s| s.ml(px(22.)))
                                .when(!enabled, |s| s.ml(px(2.)))
                                .mt(px(2.)),
                        )
                        .when(!is_busy, |s| {
                            s.on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(controller) =
                                    cx.global::<AppStore>().remote_controller.clone()
                                {
                                    let enabled = controller.read(cx).is_enabled();
                                    if !enabled
                                        && !cloudflared::app_data_cloudflared_path().exists()
                                    {
                                        this.cloudflared_dialog =
                                            Some(CloudflaredDialog::Prompt);
                                        cx.notify();
                                        return;
                                    }
                                    controller
                                        .update(cx, |c, cx| c.set_enabled(!enabled, cx));
                                }
                            }))
                        }),
                ),
        )
        .child(
            div()
                .id("remote-status-row")
                .w_full()
                .flex()
                .flex_row()
                .items_center()
                .gap_3()
                .px_4()
                .py_2()
                .rounded_lg()
                .bg(rgb(0x252525))
                .child(div().text_xs().text_color(rgb(0x888888)).child("Status"))
                .child(div().flex_1())
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(div().text_xs().text_color(status_color).child(status_text))
                        .when_some(tunnel_log.clone(), |this, log: TunnelLog| {
                            let icon_color = match log.level.as_str() {
                                "ERR" => rgb(0xfca5a5),
                                "WRN" => rgb(0xfbbf24),
                                _ => rgb(0x888888),
                            };
                            let tooltip_text = format!("[{}] {}", log.level, log.message);
                            this.child(
                                div()
                                    .id("remote-status-log-icon")
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .size(px(16.))
                                    .child(
                                        gpui::svg()
                                            .path("exclamation.svg")
                                            .size(px(14.))
                                            .text_color(icon_color),
                                    )
                                    .tooltip(move |_, cx| {
                                        cx.new(|_| StatusLogTooltip {
                                            text: tooltip_text.clone().into(),
                                        })
                                        .into()
                                    }),
                            )
                        }),
                ),
        );

    if let Some(tunnel) = tunnel_url {
        let qr = qr_image_source(&tunnel);
        let tunnel_for_text_copy = tunnel.clone();
        let tunnel_for_display = tunnel.clone();
        let pi_commander_url = "https://pi.raven-ai.one/".to_string();
        let pi_commander_for_open = pi_commander_url.clone();
        section = section.child(
            div()
                .id("remote-qr-card")
                .w_full()
                .flex()
                .flex_col()
                .items_center()
                .gap_3()
                .px_4()
                .py_4()
                .rounded_lg()
                .bg(rgb(0x252525))
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_center()
                        .gap_1()
                        .text_xs()
                        .text_color(rgb(0x888888))
                        .child("Scan with ")
                        .child(
                            div()
                                .id("pi-commander-link")
                                .text_color(rgb(0x6366f1))
                                .cursor_pointer()
                                .hover(|style| style.text_color(rgb(0x818cf8)))
                                .on_click(cx.listener(move |_this, _, _, cx| {
                                    cx.open_url(&pi_commander_for_open);
                                }))
                                .child("pi-commander"),
                        ),
                )
                .child(
                    div()
                        .id("remote-qr-code")
                        .when_some(qr, |this, source| {
                            this.child(gpui::img(source).size(px(160.)))
                        }),
                )
                .child(
                    div()
                        .id("remote-tunnel-url")
                        .w_full()
                        .px_3()
                        .py_2()
                        .rounded_lg()
                        .bg(rgb(0x1f1f1f))
                        .border_1()
                        .border_color(rgb(0x333333))
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(0x2a2a2a)).border_color(rgb(0x444444)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(
                                tunnel_for_text_copy.clone(),
                            ));
                            this.toast.update(cx, |toast, cx| {
                                toast.set_message("URL copied to clipboard");
                                toast.show_for(Duration::from_secs(3), cx);
                            });
                            cx.notify();
                        }))
                        .child(
                            div()
                                .w_full()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_center()
                                .gap_2()
                                .child(
                                    gpui::svg()
                                        .path("clipboard.svg")
                                        .size(px(14.))
                                        .text_color(rgb(0x888888)),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .overflow_x_hidden()
                                        .whitespace_nowrap()
                                        .text_ellipsis()
                                        .text_xs()
                                        .text_color(rgb(0xcccccc))
                                        .child(tunnel_for_display),
                                ),
                        ),
                ),
        );
    }

    if let Some(err) = error_message {
        section = section.child(
            div()
                .id("remote-error-card")
                .w_full()
                .flex()
                .flex_col()
                .gap_2()
                .px_4()
                .py_3()
                .rounded_lg()
                .bg(rgb(0x2a1a1a))
                .border_1()
                .border_color(rgb(0x7f1d1d))
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(0xfca5a5))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child("Remote control failed"),
                )
                .child(div().text_xs().text_color(rgb(0xfca5a5)).child(err)),
        );
    }

    section
}

fn render_back_button(cx: &mut Context<UserPanel>) -> impl IntoElement {
    div()
        .id("back-button")
        .flex()
        .items_center()
        .justify_center()
        .size(px(32.))
        .rounded_full()
        .bg(rgb(0x252525))
        .cursor_pointer()
        .child(
            gpui::svg()
                .path("arrow-left.svg")
                .size(px(16.))
                .text_color(rgb(0x888888)),
        )
        .hover(|style| style.bg(rgb(0x333333)))
        .on_click(cx.listener(|_this, _, _, cx| {
            cx.emit(UserPanelEvent::BackPressed);
        }))
}

fn render_auth_content(
    _panel: &UserPanel,
    auth: &AuthState,
    cx: &mut Context<UserPanel>,
) -> impl IntoElement {
    match auth {
        AuthState::LoggedIn(user) => {
            let initials: String = user
                .email
                .chars()
                .next()
                .map(|c| c.to_uppercase().to_string())
                .unwrap_or_else(|| "?".to_string());
            let threads_count = cx
                .global::<AppStore>()
                .store
                .list_threads()
                .map(|t| t.len())
                .unwrap_or(0);
            let sync_status = cx.global::<AppStore>().sync_status.clone();
            let sync_label: SharedString = match &sync_status {
                settings_sync::SyncStatus::Idle => "Not synced".into(),
                settings_sync::SyncStatus::Syncing => "Syncing...".into(),
                settings_sync::SyncStatus::Synced => "Synced".into(),
                settings_sync::SyncStatus::Error(e) => format!("Error: {}", e).into(),
            };

            div()
                .w_full()
                .flex()
                .flex_col()
                .items_center()
                .gap_6()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(80.))
                        .rounded_full()
                        .bg(rgb(0x6366f1))
                        .border_3()
                        .border_color(rgb(0x4f46e5))
                        .text_color(rgb(0xffffff))
                        .text_size(px(28.))
                        .font_weight(gpui::FontWeight::BOLD)
                        .child(initials),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap_1()
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(0xe0e0e0))
                                .overflow_x_hidden()
                                .child(user.email.clone()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(0x888888))
                                .child("Authenticated"),
                        ),
                )
                .child(
                    div().w_full().flex().flex_row().gap_3().child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap_1()
                            .px_4()
                            .py_3()
                            .rounded_lg()
                            .bg(rgb(0x252525))
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(rgb(0xe0e0e0))
                                    .child(threads_count.to_string()),
                            )
                            .child(div().text_xs().text_color(rgb(0x888888)).child("Threads")),
                    ),
                )
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .px_2()
                                .py_1()
                                .text_xs()
                                .text_color(rgb(0x888888))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child("SYNC"),
                        )
                        .child(sync_row("Agent Settings", &sync_label))
                        .child(render_sync_button(cx)),
                )
                .child(render_remote_control_section(cx))
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .px_2()
                                .py_1()
                                .text_xs()
                                .text_color(rgb(0x888888))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child("SETTINGS"),
                        )
                        .child(settings_row("Account", "account.svg"))
                        .child(settings_row("Notifications", "notifications.svg"))
                        .child(settings_row("Appearance", "appearance.svg"))
                        .child(settings_row("Keyboard Shortcuts", "keyboard.svg"))
                        .child(settings_row("About", "about.svg")),
                )
                .child(render_logout_button(cx))
        }
        _ => div()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .gap_6()
            .child(
                div()
                    .id("login-dialog-btn")
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .px_4()
                    .py_3()
                    .rounded_lg()
                    .bg(rgb(0x6366f1))
                    .cursor_pointer()
                    .text_color(rgb(0xffffff))
                    .text_sm()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .hover(|style| style.bg(rgb(0x4f46e5)))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.auth_dialog = Some(AuthDialog::Login);
                        cx.notify();
                    }))
                    .child(
                        gpui::svg()
                            .path("login.svg")
                            .size(px(16.))
                            .text_color(rgb(0xffffff)),
                    )
                    .child("Sign In"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(0x888888))
                    .child("Sign in to sync your agent settings"),
            )
            .child(render_remote_control_section(cx))
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .text_xs()
                            .text_color(rgb(0x888888))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("SETTINGS"),
                    )
                    .child(settings_row("Notifications", "notifications.svg"))
                    .child(settings_row("Appearance", "appearance.svg"))
                    .child(settings_row("Keyboard Shortcuts", "keyboard.svg"))
                    .child(settings_row("About", "about.svg")),
            ),
    }
}

fn render_email_field(panel: &UserPanel) -> impl IntoElement {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_xs()
                .text_color(rgb(0x888888))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child("EMAIL"),
        )
        .child(
            div()
                .w_full()
                .px_3()
                .py_2()
                .rounded_lg()
                .bg(rgb(0x252525))
                .border_1()
                .border_color(rgb(0x444444))
                .child(panel.email_input.clone()),
        )
}

fn render_password_field(panel: &UserPanel) -> impl IntoElement {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_xs()
                .text_color(rgb(0x888888))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child("PASSWORD"),
        )
        .child(
            div()
                .w_full()
                .px_3()
                .py_2()
                .rounded_lg()
                .bg(rgb(0x252525))
                .border_1()
                .border_color(rgb(0x444444))
                .child(panel.password_input.clone()),
        )
}

fn render_login_button(
    email_val: SharedString,
    password_val: SharedString,
    cx: &mut Context<UserPanel>,
) -> impl IntoElement {
    div()
        .id("login-button")
        .w_full()
        .flex()
        .items_center()
        .justify_center()
        .px_4()
        .py_3()
        .rounded_lg()
        .bg(rgb(0x6366f1))
        .cursor_pointer()
        .text_color(rgb(0xffffff))
        .text_sm()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .hover(|style| style.bg(rgb(0x4f46e5)))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.auth_error = None;
            let email = email_val.to_string();
            let password = password_val.to_string();
            if email.is_empty() || password.is_empty() {
                this.auth_error = Some("Email and password are required".to_string());
                cx.notify();
                return;
            }
            cx.update_global(|app: &mut AppStore, _| {
                app.auth = AuthState::LoggingIn;
            });
            cx.notify();
            let store = cx.global::<AppStore>().store.clone();
            cx.spawn(async move |weak, cx| {
                let result = smol::unblock(move || supabase::login(&email, &password)).await;
                let _ = weak.update(cx, |this, cx| {
                    match result {
                        Ok(session) => {
                            let _ = state::save_session(&store, &session);
                            let user = session.user.clone();
                            cx.update_global(|app: &mut AppStore, _| {
                                app.auth = AuthState::LoggedIn(user);
                                app.session = Some(session);
                            });
                            cx.emit(UserPanelEvent::AuthStateChanged);
                        }
                        Err(e) => {
                            this.auth_error = Some(e.to_string());
                            cx.update_global(|app: &mut AppStore, _| {
                                app.auth = AuthState::LoggedOut;
                            });
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
        }))
        .child("Sign In")
}

fn render_confirm_password_field(panel: &UserPanel) -> impl IntoElement {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_xs()
                .text_color(rgb(0x888888))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child("CONFIRM PASSWORD"),
        )
        .child(
            div()
                .w_full()
                .px_3()
                .py_2()
                .rounded_lg()
                .bg(rgb(0x252525))
                .border_1()
                .border_color(rgb(0x444444))
                .child(panel.confirm_password_input.clone()),
        )
}

fn render_signup_submit_button(
    email_val: SharedString,
    password_val: SharedString,
    confirm_password_val: SharedString,
    cx: &mut Context<UserPanel>,
) -> impl IntoElement {
    div()
        .id("signup-submit-button")
        .w_full()
        .flex()
        .items_center()
        .justify_center()
        .px_4()
        .py_3()
        .rounded_lg()
        .bg(rgb(0x6366f1))
        .cursor_pointer()
        .text_color(rgb(0xffffff))
        .text_sm()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .hover(|style| style.bg(rgb(0x4f46e5)))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.auth_error = None;
            let email = email_val.to_string();
            let password = password_val.to_string();
            let confirm = confirm_password_val.to_string();
            if email.is_empty() || password.is_empty() {
                this.auth_error = Some("Email and password are required".to_string());
                cx.notify();
                return;
            }
            if password != confirm {
                this.auth_error = Some("Passwords do not match".to_string());
                cx.notify();
                return;
            }
            cx.update_global(|app: &mut AppStore, _| {
                app.auth = AuthState::LoggingIn;
            });
            cx.notify();
            let store = cx.global::<AppStore>().store.clone();
            cx.spawn(async move |weak, cx| {
                let result = smol::unblock(move || supabase::signup(&email, &password)).await;
                let _ = weak.update(cx, |this, cx| {
                    match result {
                        Ok(session) => {
                            let _ = state::save_session(&store, &session);
                            let user = session.user.clone();
                            cx.update_global(|app: &mut AppStore, _| {
                                app.auth = AuthState::LoggedIn(user);
                                app.session = Some(session);
                            });
                            cx.emit(UserPanelEvent::AuthStateChanged);
                        }
                        Err(e) => {
                            this.auth_error = Some(e.to_string());
                            cx.update_global(|app: &mut AppStore, _| {
                                app.auth = AuthState::LoggedOut;
                            });
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
        }))
        .child("Create Account")
}

fn render_logout_button(cx: &mut Context<UserPanel>) -> impl IntoElement {
    div()
        .id("logout-button")
        .w_full()
        .flex()
        .items_center()
        .justify_center()
        .px_4()
        .py_3()
        .mb_6()
        .rounded_lg()
        .bg(rgb(0x7f1d1d))
        .cursor_pointer()
        .text_color(rgb(0xfca5a5))
        .text_sm()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .hover(|style| style.bg(rgb(0x991b1b)))
        .on_click(cx.listener(|this, _, _, cx| {
            this.auth_error = None;
            let session = cx.global::<AppStore>().session.clone();
            let store = cx.global::<AppStore>().store.clone();
            if let Some(s) = session {
                let _ = supabase::logout(&s.access_token);
            }
            let _ = state::clear_session(&store);
            cx.update_global(|app: &mut AppStore, _| {
                app.auth = AuthState::LoggedOut;
                app.session = None;
            });
            cx.emit(UserPanelEvent::AuthStateChanged);
            cx.notify();
        }))
        .child("Sign Out")
}

fn render_sync_button(cx: &mut Context<UserPanel>) -> impl IntoElement {
    let sync_status = cx.global::<AppStore>().sync_status.clone();
    let is_syncing = sync_status == settings_sync::SyncStatus::Syncing;
    let label: SharedString = if is_syncing {
        "Syncing...".into()
    } else {
        "Sync Now".into()
    };

    div()
        .id("sync-button")
        .w_full()
        .flex()
        .items_center()
        .justify_center()
        .px_4()
        .py_3()
        .rounded_lg()
        .bg(if is_syncing {
            rgb(0x333333)
        } else {
            rgb(0x4f46e5)
        })
        .cursor_pointer()
        .text_color(if is_syncing {
            rgb(0x888888)
        } else {
            rgb(0xffffff)
        })
        .text_sm()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .when(!is_syncing, |el| el.hover(|style| style.bg(rgb(0x4338ca))))
        .on_click(cx.listener(|_this, _, _, cx| {
            let session = cx.global::<AppStore>().session.clone();
            if let Some(s) = session {
                cx.update_global(|app: &mut AppStore, _| {
                    app.sync_status = settings_sync::SyncStatus::Syncing;
                });
                cx.notify();
                let access_token = s.access_token.clone();
                let user_id = s.user.id.clone();
                let initial_meta = cx.global::<AppStore>().sync_meta.clone();
                cx.spawn(async move |_, cx| {
                    let result =
                        smol::unblock(move || settings_sync::sync_changes(&access_token, &user_id, initial_meta))
                            .await;
                    let _ = cx.update_global(|app: &mut AppStore, _| match result {
                        Ok(meta) => {
                            let _ = settings_sync::save_sync_meta(&app.store, &meta);
                            app.sync_meta = meta;
                            app.sync_status = settings_sync::SyncStatus::Synced;
                        }
                        Err(e) => {
                            app.sync_status = settings_sync::SyncStatus::Error(e);
                        }
                    });
                })
                .detach();
            }
        }))
        .child(label)
}

fn sync_row(label: impl Into<SharedString>, status_label: &SharedString) -> impl IntoElement {
    let label: SharedString = label.into();
    let status_color = if status_label.as_ref() == "Synced" {
        rgb(0x22c55e)
    } else if status_label.as_ref() == "Syncing..." {
        rgb(0xeab308)
    } else if status_label.starts_with("Error") {
        rgb(0xef4444)
    } else {
        rgb(0x888888)
    };
    div()
        .id(SharedString::from(format!(
            "sync-{}",
            label.to_lowercase().replace(" ", "-")
        )))
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .px_4()
        .py_3()
        .rounded_lg()
        .bg(rgb(0x252525))
        .child(
            div()
                .flex_1()
                .flex()
                .flex_row()
                .items_center()
                .child(div().text_sm().text_color(rgb(0xe0e0e0)).child(label)),
        )
        .child(
            div()
                .text_xs()
                .text_color(status_color)
                .child(status_label.clone()),
        )
}

fn settings_row(
    label: impl Into<SharedString>,
    icon_path: impl Into<SharedString>,
) -> impl IntoElement {
    let label: SharedString = label.into();
    let icon_path: SharedString = icon_path.into();
    div()
        .id(SharedString::from(format!(
            "settings-{}",
            label.to_lowercase().replace(" ", "-")
        )))
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .px_4()
        .py_3()
        .rounded_lg()
        .bg(rgb(0x252525))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(0x333333)))
        .child(
            div()
                .size(px(20.))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    gpui::svg()
                        .path(icon_path)
                        .size(px(18.))
                        .text_color(rgb(0x888888)),
                ),
        )
        .child(
            div()
                .flex_1()
                .text_sm()
                .text_color(rgb(0xe0e0e0))
                .child(label),
        )
        .child(
            gpui::svg()
                .path("chevron-right.svg")
                .size(px(16.))
                .text_color(rgb(0x666666)),
        )
}
