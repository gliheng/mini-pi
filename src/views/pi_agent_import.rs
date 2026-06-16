use gpui::{
    Context, EventEmitter, InteractiveElement, IntoElement, MouseButton, ParentElement, Render,
    StatefulInteractiveElement, Styled, div, prelude::FluentBuilder, px, rgb, svg,
};

use crate::auth::state;

#[derive(Clone)]
pub enum PiAgentImportEvent {
    ImportRequested,
    SkipRequested,
}

pub struct PiAgentImport {
    files: Vec<(String, std::path::PathBuf)>,
    import_result: Option<Result<usize, String>>,
}

impl Default for PiAgentImport {
    fn default() -> Self {
        Self::new()
    }
}

impl PiAgentImport {
    pub fn new() -> Self {
        Self {
            files: state::list_pi_agent_json_files(),
            import_result: None,
        }
    }

    pub fn run_import(&mut self) {
        match state::import_from_pi_agent() {
            Ok(count) => {
                self.import_result = Some(Ok(count));
            }
            Err(e) => {
                self.import_result = Some(Err(e.to_string()));
            }
        }
    }

    pub fn has_files(&self) -> bool {
        !self.files.is_empty()
    }
}

impl EventEmitter<PiAgentImportEvent> for PiAgentImport {}

impl Render for PiAgentImport {
    fn render(&mut self, _window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let file_names: String = self
            .files
            .iter()
            .map(|(name, _)| format!("  • {}", name))
            .collect::<Vec<_>>()
            .join("\n");

        div()
            .id("import-prompt-overlay")
            .absolute()
            .top(px(0.))
            .left(px(0.))
            .w_full()
            .h_full()
            .child(
                div()
                    .absolute()
                    .top(px(0.))
                    .left(px(0.))
                    .w_full()
                    .h_full()
                    .bg(gpui::hsla(0.0, 0.0, 0.0, 0.65))
                    .on_mouse_down(MouseButton::Left, cx.listener(|_this, _, _window, cx| {
                        cx.emit(PiAgentImportEvent::SkipRequested);
                    })),
            )
            .child(
                div()
                    .absolute()
                    .top(px(0.))
                    .left(px(0.))
                    .w_full()
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .child(
                        div()
                            .id("import-prompt")
                            .w(px(360.))
                            .px_5()
                            .py_5()
                            .rounded_lg()
                            .bg(rgb(0x1e1e1e))
                            .border_1()
                            .border_color(rgb(0x333333))
                            .flex()
                            .flex_col()
                            .gap_4()
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        svg()
                                            .path("folder.svg")
                                            .size(px(20.))
                                            .text_color(rgb(0x818cf8)),
                                    )
                                    .child(
                                        div()
                                            .text_base()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(rgb(0xe0e0e0))
                                            .child("Import from Pi"),
                                    ),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0x888888))
                                    .child(format!(
                                        "Detected settings from ~/.pi/agent/.\n\nFound {} JSON file(s):\n{}",
                                        self.files.len(),
                                        file_names
                                    )),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap_2()
                                    .child(
                                        div()
                                            .id("import-btn")
                                            .flex_1()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .px_4()
                                            .py_2()
                                            .rounded_md()
                                            .bg(rgb(0x4f46e5))
                                            .cursor_pointer()
                                            .text_color(rgb(0xffffff))
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .hover(|style| style.bg(rgb(0x6366f1)))
                                            .on_click(cx.listener(|this: &mut Self, _, _, cx| {
                                                this.run_import();
                                                cx.emit(PiAgentImportEvent::ImportRequested);
                                                cx.notify();
                                            }))
                                            .child("Import"),
                                    )
                                    .child(
                                        div()
                                            .id("skip-import-btn")
                                            .flex_1()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .px_4()
                                            .py_2()
                                            .rounded_md()
                                            .bg(rgb(0x333333))
                                            .cursor_pointer()
                                            .text_color(rgb(0x888888))
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .hover(|style| style.bg(rgb(0x444444)))
                                            .on_click(cx.listener(|_this: &mut Self, _, _, cx| {
                                                cx.emit(PiAgentImportEvent::SkipRequested);
                                            }))
                                            .child("Skip"),
                                    ),
                            )
                            .when(self.import_result.is_some(), |el: gpui::Stateful<gpui::Div>| {
                                let msg = match self.import_result.as_ref().unwrap() {
                                    Ok(count) => format!("Imported {} file(s) successfully", count),
                                    Err(e) => format!("Import failed: {}", e),
                                };
                                let color = if self.import_result.as_ref().unwrap().is_ok() {
                                    rgb(0x22c55e)
                                } else {
                                    rgb(0xef4444)
                                };
                                el.child(
                                    div()
                                        .text_sm()
                                        .text_color(color)
                                        .child(msg),
                                )
                            }),
                    ),
            )
    }
}
