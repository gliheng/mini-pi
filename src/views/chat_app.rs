use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    Context, Entity, FocusHandle, Focusable, IntoElement, ParentElement, PathPromptOptions, Render,
    SharedString, Window, div, px,
};
use gpui::{MouseButton, prelude::*};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{ActiveTheme, Icon, Root, Sizable as _, TitleBar};

use crate::data::store::{Store, ThreadMeta};
use crate::views::chat_window::ChatWindow;

pub struct ChatApp {
    chat_window: Entity<ChatWindow>,
    pinned: bool,
    title: SharedString,
    _chat_subscription: gpui::Subscription,
}

impl ChatApp {
    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        thread: Option<&ThreadMeta>,
        store: Arc<Store>,
    ) -> Self {
        let chat_window = cx.new(|cx| ChatWindow::new(window, cx, thread, store));
        let title = chat_window.read(cx).title.clone();
        let _chat_subscription = cx.observe(&chat_window, |this, chat_window, cx| {
            this.title = chat_window.read(cx).title.clone();
            cx.notify();
        });

        Self {
            chat_window,
            pinned: false,
            title,
            _chat_subscription,
        }
    }
}

impl Render for ChatApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let chat_window = self.chat_window.clone();
        let title = self.title.clone();
        let pinned = self.pinned;

        let export_chat = chat_window.clone();
        let open_workspace_chat = chat_window.clone();

        let dialog_layer = Root::render_dialog_layer(window, cx);
        let notification_layer = Root::render_notification_layer(window, cx);
        let sheet_layer = Root::render_sheet_layer(window, cx);

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .font_family(theme.font_family.clone())
            .child(
                TitleBar::new()
                    .child(
                        div().flex().items_center().gap_2().child(
                            div()
                                .text_size(px(13.0))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(title),
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .pr_2()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(
                                Button::new("pin")
                                    .with_size(gpui_component::Size::Small)
                                    .ghost()
                                    .icon(
                                        Icon::empty()
                                            .path(if pinned { "unpin.svg" } else { "pin.svg" })
                                            .text_color(if pinned {
                                                gpui::rgb(0x4f46e5)
                                            } else {
                                                gpui::rgb(0x888888)
                                            }),
                                    )
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.pinned = !this.pinned;
                                        crate::views::title_bar::set_window_level(
                                            window,
                                            this.pinned,
                                        );
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("open-workspace")
                                    .with_size(gpui_component::Size::Small)
                                    .ghost()
                                    .icon(
                                        Icon::empty()
                                            .path("folder.svg")
                                            .text_color(gpui::rgb(0x888888)),
                                    )
                                    .on_click(move |_, _, cx| {
                                        let selected_id = open_workspace_chat
                                            .read(cx)
                                            .selected_workspace_id
                                            .clone();
                                        let workspace_dir: Option<PathBuf> =
                                            selected_id.and_then(|id| {
                                                open_workspace_chat
                                                    .read(cx)
                                                    .workspaces
                                                    .iter()
                                                    .find(|ws| ws.id == id)
                                                    .map(|ws| PathBuf::from(&ws.path))
                                            });
                                        if let Some(dir) = workspace_dir {
                                            cx.reveal_path(&dir);
                                        }
                                    }),
                            )
                            .child(
                                Button::new("export-html")
                                    .with_size(gpui_component::Size::Small)
                                    .ghost()
                                    .icon(
                                        Icon::empty()
                                            .path("export.svg")
                                            .text_color(gpui::rgb(0x888888)),
                                    )
                                    .on_click(move |_, _, cx| {
                                        let rx = cx.prompt_for_paths(PathPromptOptions {
                                            files: false,
                                            directories: true,
                                            multiple: false,
                                            prompt: Some(
                                                "Choose a folder to export the session HTML".into(),
                                            ),
                                        });
                                        let session = export_chat.read(cx).session.clone();
                                        let session_file =
                                            export_chat.read(cx).session_file.clone();
                                        cx.spawn(async move |cx| {
                                            if let Ok(Ok(Some(paths))) = rx.await
                                                && let Some(dir) = paths.first()
                                            {
                                                let file_name = session_file
                                                    .rsplit_once('.')
                                                    .map(|(name, _)| format!("{}.html", name))
                                                    .unwrap_or_else(|| "session.html".to_string());
                                                let output_path = dir.join(&file_name);
                                                let path_str =
                                                    output_path.to_string_lossy().to_string();
                                                if let Some(ref s) = session {
                                                    let _ = s.update(cx, |session, _cx| {
                                                        session.export_html(&path_str);
                                                    });
                                                }
                                            }
                                        })
                                        .detach();
                                    }),
                            ),
                    ),
            )
            .child(self.chat_window.clone())
            .children(dialog_layer)
            .children(notification_layer)
            .children(sheet_layer)
    }
}

/// Convenience constructor used by callers: creates a `ChatApp` wrapped in a `gpui_component::Root`.
pub fn open_chat_window(
    cx: &mut gpui::App,
    thread: Option<&ThreadMeta>,
    store: Arc<Store>,
    window_options: gpui::WindowOptions,
) -> gpui::WindowHandle<Root> {
    cx.open_window(window_options, |window, cx| {
        let app = cx.new(|cx| ChatApp::new(window, cx, thread, store));
        let focus_handle: FocusHandle = app
            .read(cx)
            .chat_window
            .read(cx)
            .chat_input
            .read(cx)
            .focus_handle(cx)
            .clone();
        window.focus(&focus_handle, cx);
        cx.new(|cx| Root::new(app, window, cx))
    })
    .expect("failed to open the chat window")
}
