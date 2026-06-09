use gpui::{
    AppContext, BorrowAppContext, Context, EventEmitter, InteractiveElement, IntoElement,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window, div, px, rgb,
    prelude::FluentBuilder,
};

use crate::auth::state::{self, AuthState};
use crate::auth::supabase;
use crate::core::app::AppStore;
use crate::sync::settings_sync;
use crate::ui::input::TextInput;

#[derive(Clone)]
pub enum UserPanelEvent {
    BackPressed,
    AuthStateChanged,
}

pub struct UserPanel {
    pub email_input: gpui::Entity<TextInput>,
    pub password_input: gpui::Entity<TextInput>,
    pub auth_error: Option<String>,
    pub sync_status: settings_sync::SyncStatus,
    pub _email_sub: gpui::Subscription,
    pub _password_sub: gpui::Subscription,
}

impl UserPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let email_input = cx.new(|cx| TextInput::new(cx, "Email"));
        let password_input = cx.new(|cx| TextInput::new(cx, "Password").with_password_mode());

        let _email_sub = cx.observe(&email_input, |_, _, cx| {
            cx.notify();
        });
        let _password_sub = cx.observe(&password_input, |_, _, cx| {
            cx.notify();
        });

        Self {
            email_input,
            password_input,
            auth_error: None,
            sync_status: settings_sync::SyncStatus::Idle,
            _email_sub,
            _password_sub,
        }
    }
}

impl EventEmitter<UserPanelEvent> for UserPanel {}

impl Render for UserPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let auth = cx.global::<AppStore>().auth.clone();
        let email_val = self.email_input.read(cx).content().clone();
        let password_val = self.password_input.read(cx).content().clone();

        div()
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
            .child(render_auth_content(self, &auth, &email_val, &password_val, cx))
    }
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
    panel: &UserPanel,
    auth: &AuthState,
    email_val: &SharedString,
    password_val: &SharedString,
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
            let threads_count = cx.global::<AppStore>().store.list_threads().map(|t| t.len()).unwrap_or(0);
            let sync_label: SharedString = match &panel.sync_status {
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
                    div()
                        .w_full()
                        .flex()
                        .flex_row()
                        .gap_3()
                        .child(
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
                        .child(sync_row("Agent Settings", &sync_label)),
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
        _ => {
            let is_logging_in = matches!(auth, AuthState::LoggingIn);
            let error_msg: Option<SharedString> = panel.auth_error.clone().map(|s| s.into());

            div()
                .w_full()
                .flex()
                .flex_col()
                .items_center()
                .gap_4()
                .child(
                    div()
                        .text_xl()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(0xe0e0e0))
                        .child("Welcome to Mini Pi"),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(0x888888))
                        .child("Sign in to sync your agent settings"),
                )
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(render_email_field(panel))
                        .child(render_password_field(panel))
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
                        .when(!is_logging_in, |el: gpui::Div| {
                            el.child(render_login_button(email_val.clone(), password_val.clone(), cx))
                                .child(render_signup_button(email_val.clone(), password_val.clone(), cx))
                        }),
                )
        }
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
            cx.spawn(async move |weak, cx| {
                let result = smol::unblock(move || supabase::login(&email, &password)).await;
                let _ = weak.update(cx, |this, cx| {
                    match result {
                        Ok(session) => {
                            let _ = state::save_session(&session);
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

fn render_signup_button(
    email_val: SharedString,
    password_val: SharedString,
    cx: &mut Context<UserPanel>,
) -> impl IntoElement {
    div()
        .id("signup-button")
        .w_full()
        .flex()
        .items_center()
        .justify_center()
        .px_4()
        .py_3()
        .rounded_lg()
        .bg(rgb(0x252525))
        .border_1()
        .border_color(rgb(0x444444))
        .cursor_pointer()
        .text_color(rgb(0xcccccc))
        .text_sm()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .hover(|style| style.bg(rgb(0x333333)))
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
            cx.spawn(async move |weak, cx| {
                let result = smol::unblock(move || supabase::signup(&email, &password)).await;
                let _ = weak.update(cx, |this, cx| {
                    match result {
                        Ok(session) => {
                            let _ = state::save_session(&session);
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
            if let Some(s) = session {
                let _ = supabase::logout(&s.access_token);
            }
            let _ = state::clear_session();
            cx.update_global(|app: &mut AppStore, _| {
                app.auth = AuthState::LoggedOut;
                app.session = None;
            });
            cx.emit(UserPanelEvent::AuthStateChanged);
            cx.notify();
        }))
        .child("Sign Out")
}

fn sync_row(
    label: impl Into<SharedString>,
    status_label: &SharedString,
) -> impl IntoElement {
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
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(0xe0e0e0))
                        .child(label),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(status_color)
                        .child(status_label.clone()),
                ),
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