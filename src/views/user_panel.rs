use gpui::{
    Context, EventEmitter, Hsla, InteractiveElement, IntoElement, ParentElement, Render,
    SharedString, StatefulInteractiveElement, Styled, Window, div, px, rgb,
};

#[derive(Clone)]
pub enum UserPanelEvent {
    BackPressed,
}

pub struct UserPanel;

// Mock user data
pub struct UserProfile {
    pub name: SharedString,
    pub email: SharedString,
    pub role: SharedString,
    pub avatar_initials: SharedString,
    pub threads_count: i32,
    pub total_messages: i32,
}

impl Default for UserProfile {
    fn default() -> Self {
        Self {
            name: "John Doe".into(),
            email: "john.doe@example.com".into(),
            role: "Pro Member".into(),
            avatar_initials: "JD".into(),
            threads_count: 24,
            total_messages: 186,
        }
    }
}

impl UserPanel {
    pub fn new() -> Self {
        Self
    }
}

impl EventEmitter<UserPanelEvent> for UserPanel {}

impl Render for UserPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let user = UserProfile::default();

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
            // Back button at top left
            .child(
                div().w_full().flex().flex_row().items_center().child(
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
                        })),
                ),
            )
            // Large avatar
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
                    .child(user.avatar_initials.clone()),
            )
            // User info
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .text_xl()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgb(0xe0e0e0))
                            .child(user.name.clone()),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0x888888))
                            .child(user.email.clone()),
                    )
                    .child(
                        div()
                            .mt_1()
                            .px_3()
                            .py_1()
                            .rounded_full()
                            .bg(Into::<Hsla>::into(rgb(0x4f46e5)).alpha(0.2))
                            .text_color(rgb(0x818cf8))
                            .text_xs()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(user.role.clone()),
                    ),
            )
            // Stats
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
                                    .child(user.threads_count.to_string()),
                            )
                            .child(div().text_xs().text_color(rgb(0x888888)).child("Threads")),
                    )
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
                                    .child(user.total_messages.to_string()),
                            )
                            .child(div().text_xs().text_color(rgb(0x888888)).child("Messages")),
                    ),
            )
            // Settings section
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
    }
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
