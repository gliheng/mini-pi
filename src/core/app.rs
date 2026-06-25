use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use gpui::{
    AnyWindowHandle, Bounds, Entity, Global, TitlebarOptions, WindowBackgroundAppearance,
    WindowBounds, WindowOptions, point, px,
};

use crate::auth::state::{AuthState, SupabaseSession};
use crate::config::app_config::AppConfig;
use crate::config::model_config::ModelInfo;
use crate::core::session_manager::SessionManager;
use crate::data::store::Store;
use crate::remote::RemoteController;
use crate::rpc::pi_rpc::PiBridge;
use crate::sync::settings_sync::{SyncMeta, SyncStatus};

pub struct AppStore {
    pub store: Arc<Store>,
    pub config: AppConfig,
    pub thread_windows: HashMap<String, AnyWindowHandle>,
    pub auth: AuthState,
    pub session: Option<SupabaseSession>,
    pub sync_meta: SyncMeta,
    pub sync_status: SyncStatus,
    pub user_panel_active: bool,
    pub pi_bridge: Option<Arc<PiBridge>>,
    pub session_manager: SessionManager,
    pub streaming_thread_ids: HashSet<String>,
    pub remote_controller: Option<Entity<RemoteController>>,
    pub models: Vec<ModelInfo>,
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
        #[cfg(target_os = "linux")]
        window_background: gpui::WindowBackgroundAppearance::Transparent,
        #[cfg(target_os = "linux")]
        window_decorations: Some(gpui::WindowDecorations::Client),
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    }
}
