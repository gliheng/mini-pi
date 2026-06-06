use gpui::{IntoElement, ParentElement, Styled, div, prelude::*, px, rgb};

/// Create a reusable animated loader / spinner.
///
/// Shows a small centered row of three dots. Use it anywhere you need
/// to indicate that something is happening in the background.
///
/// # Example
/// ```rust
/// .child(loader())
/// ```
pub fn loader() -> impl IntoElement {
    loader_with(8.0, 0x888888)
}

/// Create a loader with a custom dot size (in pixels) and colour (24-bit hex).
pub fn loader_with(size: f32, color: u32) -> impl IntoElement {
    let dot_size = px(size);
    let gap = px(size * 1.5);
    let color = rgb(color);

    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_center()
        .gap(gap)
        .child(dot(dot_size, color, "loader-dot-1"))
        .child(dot(dot_size, color, "loader-dot-2"))
        .child(dot(dot_size, color, "loader-dot-3"))
}

fn dot(size: gpui::Pixels, color: gpui::Rgba, id: &'static str) -> impl IntoElement {
    div()
        .id(id)
        .size(size)
        .rounded_full()
        .bg(color)
}
