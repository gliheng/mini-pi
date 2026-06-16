use std::fs;

use gpui::{
    App, Application, Bounds, Context, Entity, IntoElement, KeyBinding, ParentElement, Render,
    Window, actions, div, prelude::*, px, rgb, size,
};
use mini_pi::core::assets::Assets;
use mini_pi::ui::markdown::MarkdownRenderer;

actions!(markdown_example, [Quit]);

#[derive(Clone)]
struct SplitterDrag;

struct EmptyDragGhost;

impl Render for EmptyDragGhost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().w(px(0.)).h(px(0.))
    }
}

struct MarkdownExample {
    renderer: Entity<MarkdownRenderer>,
    source: String,
    split_ratio: f32,
}

impl Render for MarkdownExample {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let left_width = px(self.split_ratio * 1400.0);
        let right_width = px((1.0 - self.split_ratio) * 1400.0 - 4.0);

        let reload_button = div()
            .px_3()
            .rounded_md()
            .bg(rgb(0x3b82f6))
            .text_color(rgb(0xffffff))
            .text_sm()
            .cursor_pointer()
            .child("Reload from file");

        div()
            .id("main-container")
            .flex()
            .flex_row()
            .w_full()
            .h_full()
            .bg(rgb(0x0d0d0d))
            .on_drag_move({
                let weak = cx.entity().downgrade();
                move |e: &gpui::DragMoveEvent<SplitterDrag>, _window, cx| {
                    if let Some(this) = weak.upgrade() {
                        let window_width = e.bounds.size.width;
                        if window_width > px(0.0) {
                            let relative_x = e.event.position.x - e.bounds.left();
                            let new_ratio = (relative_x / window_width).clamp(0.2, 0.8);
                            this.update(cx, |this, _cx| {
                                this.split_ratio = new_ratio;
                            });
                        }
                    }
                }
            })
            .child(
                div()
                    .w(left_width)
                    .flex()
                    .flex_col()
                    .h_full()
                    .min_w(px(200.))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .px_3()
                            .h(px(36.))
                            .bg(rgb(0x1a1a1a))
                            .border_b_1()
                            .border_color(rgb(0x333333))
                            .text_color(rgb(0xe5e5e5))
                            .child("Raw Markdown")
                            .child(reload_button),
                    )
                    .child(
                        div()
                            .id("raw-panel")
                            .flex_1()
                            .p_4()
                            .overflow_scroll()
                            .font_family("Menlo, Monaco, 'Courier New', monospace")
                            .text_size(px(13.))
                            .text_color(rgb(0xe5e5e5))
                            .child(self.source.clone()),
                    ),
            )
            .child(
                // Draggable splitter
                div()
                    .id("splitter")
                    .w(px(4.))
                    .h_full()
                    .bg(rgb(0x333333))
                    .cursor_col_resize()
                    .on_drag(
                        SplitterDrag,
                        |_payload: &SplitterDrag,
                         _position: gpui::Point<gpui::Pixels>,
                         _window: &mut Window,
                         cx: &mut App| { cx.new(|_cx| EmptyDragGhost) },
                    ),
            )
            .child(
                div()
                    .w(right_width)
                    .flex()
                    .flex_col()
                    .h_full()
                    .min_w(px(200.))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .px_3()
                            .h(px(36.))
                            .bg(rgb(0x1a1a1a))
                            .border_b_1()
                            .border_color(rgb(0x333333))
                            .text_color(rgb(0xe5e5e5))
                            .child("Rendered Output")
                            .child(div()),
                    )
                    .child(
                        div()
                            .id("rendered-panel")
                            .flex_1()
                            .p_4()
                            .overflow_y_scroll()
                            .child(self.renderer.clone()),
                    ),
            )
    }
}

fn quit(_: &Quit, cx: &mut App) {
    cx.quit();
}

fn main() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let md_path = std::path::PathBuf::from(manifest_dir).join("examples/markdown_test.md");
    let source = fs::read_to_string(&md_path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", md_path.display(), e));

    let assets_dir = std::path::PathBuf::from(manifest_dir).join("assets");

    Application::new()
        .with_assets(Assets { base: assets_dir })
        .run(move |cx: &mut App| {
            cx.on_action(quit);
            cx.bind_keys([
                KeyBinding::new("cmd-q", Quit, None),
                KeyBinding::new("ctrl-w", Quit, None),
            ]);

            let bounds = Bounds::centered(None, size(px(1400.0), px(900.0)), cx);
            cx.open_window(
                gpui::WindowOptions {
                    window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |_window, cx| {
                    let renderer = cx.new(|_cx| MarkdownRenderer::new(source.clone()));

                    cx.new(|_cx| MarkdownExample {
                        renderer,
                        source: source.clone(),
                        split_ratio: 0.5,
                    })
                },
            )
            .unwrap();

            cx.on_window_closed(|cx| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();

            cx.activate(true);
        });
}
