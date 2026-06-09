use std::{collections::HashMap, sync::Arc};

use gpui::{
    point, px, AnyWindowHandle, Bounds, Global, TitlebarOptions, WindowBackgroundAppearance,
    WindowBounds, WindowDecorations, WindowOptions,
};

use crate::auth::state::{AuthState, SupabaseSession};
use crate::config::app_config::AppConfig;
use crate::data::store::Store;
use crate::sync::settings_sync::SyncMeta;

pub struct AppStore {
    pub store: Arc<Store>,
    pub config: AppConfig,
    pub thread_windows: HashMap<i64, AnyWindowHandle>,
    pub auth: AuthState,
    pub session: Option<SupabaseSession>,
    pub sync_meta: SyncMeta,
}

impl Global for AppStore {}

pub fn custom_window_options(bounds: Option<Bounds<gpui::Pixels>>) -> WindowOptions {
    WindowOptions {
        window_bounds: bounds.map(WindowBounds::Windowed),
        titlebar: Some(TitlebarOptions {
            title: None,
            appears_transparent: true,
            traffic_light_position: Some(point(px(9.0), px(9.0))),
        }),
        window_decorations: if cfg!(target_os = "macos") {
            None
        } else {
            Some(WindowDecorations::Client)
        },
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    }
}
