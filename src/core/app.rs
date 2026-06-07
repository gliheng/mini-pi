use std::sync::Arc;

use gpui::{
    Bounds, Global, TitlebarOptions, WindowBackgroundAppearance, WindowBounds, WindowDecorations,
    WindowOptions, point, px,
};

use crate::data::store::Store;

pub struct AppStore(pub Arc<Store>);

impl Global for AppStore {}

pub fn custom_window_options(bounds: Option<Bounds<gpui::Pixels>>) -> WindowOptions {
    WindowOptions {
        window_bounds: bounds.map(WindowBounds::Windowed),
        titlebar: Some(TitlebarOptions {
            title: None,
            appears_transparent: true,
            traffic_light_position: Some(point(px(9.0), px(9.0))),
        }),
        window_decorations: if cfg!(target_os = "linux") {
            Some(WindowDecorations::Client)
        } else {
            None
        },
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    }
}
