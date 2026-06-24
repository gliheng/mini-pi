use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use gpui::{
    App, Application, Bounds, FontWeight, KeyBinding, Menu, MenuItem, SharedString, Window,
    WindowBounds, WindowDecorations, WindowOptions, px, size,
};
use gpui::{MouseButton, prelude::*};
use gpui_component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_component::theme::{Theme, ThemeRegistry};
use gpui_component::{ActiveTheme, Icon, Root, Sizable as _, TitleBar};

use crate::auth::state::{self, AuthState};
use crate::config::app_config::AppConfig;
use crate::config::model_config;
use crate::core::actions::Quit;
use crate::core::app::AppStore;
use crate::core::assets::Assets;
use crate::core::session_manager::SessionManager;
use crate::data::store::Store;
use crate::remote::RemoteController;
use crate::rpc::pi_rpc::PiBridge;
use crate::sync::settings_sync;
use crate::views::thread_list::ThreadList;
use crate::views::user_panel::{UserPanel, UserPanelEvent};

pub fn run() {
    let store = Arc::new(Store::open().expect("failed to open database"));
    let mut config = AppConfig::load();
    // Remote control is always off on app startup.
    config.remote_control.enabled = false;
    if let Err(e) = config.save() {
        eprintln!("[remote] failed to save startup config: {}", e);
    }
    let assets_dir = crate::utils::paths::app_root().join("assets");

    let (auth, session) = match state::load_session(&store) {
        Some(session) => {
            if session.is_expired() {
                match crate::auth::supabase::refresh_session(&session.refresh_token) {
                    Ok(new_session) => {
                        let user = new_session.user.clone();
                        let _ = state::save_session(&store, &new_session);
                        (AuthState::LoggedIn(user), Some(new_session))
                    }
                    Err(_) => {
                        let _ = state::clear_session(&store);
                        (AuthState::LoggedOut, None)
                    }
                }
            } else {
                match crate::auth::supabase::get_user(&session.access_token) {
                    Ok(user) => (AuthState::LoggedIn(user), Some(session)),
                    Err(_) => {
                        let _ = state::clear_session(&store);
                        (AuthState::LoggedOut, None)
                    }
                }
            }
        }
        None => (AuthState::LoggedOut, None),
    };

    let sync_meta = settings_sync::load_sync_meta(&store);
    let initial_sync_meta = sync_meta.clone();

    let pi_bridge = match PiBridge::spawn() {
        Ok(bridge) => {
            eprintln!("[mini-pi] pi SDK bridge connected");
            Some(bridge)
        }
        Err(e) => {
            eprintln!("[mini-pi] failed to start pi SDK bridge: {}", e);
            None
        }
    };

    Application::with_platform(gpui_platform::current_platform(false))
        .with_assets(Assets { base: assets_dir })
        .run(move |cx: &mut App| {
            gpui_component::init(cx);

            let themes_dir = crate::utils::paths::app_root()
                .join("assets")
                .join("themes");
            let theme_store = store.clone();
            if let Err(err) = ThemeRegistry::watch_dir(themes_dir, cx, move |cx| {
                let theme_name = theme_store
                    .theme_name()
                    .unwrap_or_else(|| "Ayu Dark".to_string());
                let theme_name = SharedString::from(theme_name);
                if let Some(theme) = ThemeRegistry::global(cx).themes().get(&theme_name).cloned() {
                    let mode = theme.mode;
                    let global_theme = Theme::global_mut(cx);
                    if mode.is_dark() {
                        global_theme.dark_theme = theme;
                    } else {
                        global_theme.light_theme = theme;
                    }
                    Theme::change(mode, None, cx);
                    cx.refresh_windows();
                }
            }) {
                eprintln!("[theme] failed to watch themes directory: {}", err);
            }

            let models = pi_bridge
                .as_ref()
                .map(|bridge| match model_config::load_models(bridge) {
                    Ok(models) => models,
                    Err(e) => {
                        eprintln!("[mini-pi] failed to load model list: {}", e);
                        Vec::new()
                    }
                })
                .unwrap_or_default();
            eprintln!("[mini-pi] loaded {} models", models.len());
            for m in &models {
                eprintln!(
                    "[mini-pi]   model: id={} name={} thinking_level_map={:?}",
                    m.id, m.name, m.thinking_level_map
                );
            }

            let remote_controller =
                cx.new(|cx| RemoteController::new(cx, config.remote_control.clone()));

            cx.set_global(AppStore {
                store: store.clone(),
                config: config.clone(),
                thread_windows: HashMap::new(),
                auth: auth.clone(),
                session: session.clone(),
                sync_meta,
                sync_status: settings_sync::SyncStatus::Idle,
                user_panel_active: false,
                pi_bridge: pi_bridge.clone(),
                session_manager: SessionManager::new(),
                streaming_thread_ids: HashSet::new(),
                remote_controller: Some(remote_controller),
                models,
            });

            if auth.is_logged_in() {
                if let Some(ref sess) = session {
                    let _ = state::agent_dir();
                    let access_token = sess.access_token.clone();
                    let user_id = sess.user.id.clone();
                    let initial_meta = initial_sync_meta.clone();
                    cx.spawn(async move |cx| {
                        let result = smol::unblock(move || {
                            settings_sync::sync_changes(&access_token, &user_id, initial_meta)
                        })
                        .await;
                        let _ = cx.update_global(|app: &mut AppStore, _| {
                            app.sync_status = match result {
                                Ok(meta) => {
                                    let _ = settings_sync::save_sync_meta(&app.store, &meta);
                                    app.sync_meta = meta;
                                    settings_sync::SyncStatus::Synced
                                }
                                Err(e) => settings_sync::SyncStatus::Error(e),
                            };
                        });
                    })
                    .detach();
                }
            }

            cx.on_action(|_: &Quit, cx: &mut App| cx.quit());
            cx.bind_keys([
                KeyBinding::new("ctrl-w", crate::core::actions::CloseWindow, None),
                KeyBinding::new("cmd-w", crate::core::actions::CloseWindow, None),
                KeyBinding::new("cmd-q", Quit, None),
                KeyBinding::new("enter", crate::core::actions::SendMessage, None),
            ]);

            cx.set_menus(vec![Menu {
                name: "Mini Pi".into(),
                items: vec![MenuItem::action("Quit", Quit)],
                disabled: false,
            }]);

            cx.on_window_closed(|cx: &mut App, _window_id| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();

            let bounds = Bounds::centered(None, size(px(420.0), px(600.0)), cx);
            let window_options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitleBar::title_bar_options()),
                window_decorations: if cfg!(target_os = "macos") {
                    None
                } else {
                    Some(WindowDecorations::Client)
                },
                ..Default::default()
            };

            cx.open_window(window_options, |window, cx| {
                let app = cx.new(|cx| MiniPiApp::new(window, cx));
                let focus_handle = app.read(cx).thread_list.read(cx).focus_handle.clone();
                window.focus(&focus_handle, cx);
                cx.new(|cx| Root::new(app, window, cx))
            })
            .expect("failed to open the Mini Pi window");

            cx.activate(true);
        });
}

struct MiniPiApp {
    thread_list: gpui::Entity<ThreadList>,
    user_panel: gpui::Entity<UserPanel>,
    _user_panel_subscription: gpui::Subscription,
}

impl MiniPiApp {
    fn new(window: &mut Window, cx: &mut gpui::Context<Self>) -> Self {
        let store = cx.global::<AppStore>().store.clone();
        let thread_list = cx.new(|cx| ThreadList::new(window, cx, store));
        let user_panel = cx.new(|cx| UserPanel::new(window, cx));

        let _user_panel_subscription =
            cx.subscribe(&user_panel, move |_this, _, event: &UserPanelEvent, cx| {
                cx.update_global(|app: &mut AppStore, _| {
                    app.user_panel_active = false;
                });
                match event {
                    UserPanelEvent::AuthStateChanged => {
                        let auth = cx.global::<AppStore>().auth.clone();
                        if let AuthState::LoggedIn(_) = &auth {
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
                                    let result = smol::unblock(move || {
                                        settings_sync::sync_changes(
                                            &access_token,
                                            &user_id,
                                            initial_meta,
                                        )
                                    })
                                    .await;
                                    let _ =
                                        cx.update_global(|app: &mut AppStore, _| match result {
                                            Ok(meta) => {
                                                let _ = settings_sync::save_sync_meta(
                                                    &app.store, &meta,
                                                );
                                                app.sync_meta = meta;
                                                app.sync_status = settings_sync::SyncStatus::Synced;
                                            }
                                            Err(e) => {
                                                app.sync_status =
                                                    settings_sync::SyncStatus::Error(e);
                                            }
                                        });
                                })
                                .detach();
                            }
                        }
                    }
                    UserPanelEvent::BackPressed => {}
                }
                cx.notify();
            });

        Self {
            thread_list,
            user_panel,
            _user_panel_subscription,
        }
    }
}

impl gpui::Render for MiniPiApp {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        let theme = cx.theme().clone();
        let user_panel_active = cx.global::<AppStore>().user_panel_active;

        let dialog_layer = Root::render_dialog_layer(window, cx);
        let notification_layer = Root::render_notification_layer(window, cx);
        let sheet_layer = Root::render_sheet_layer(window, cx);

        let title = gpui::div().flex().items_center().gap_2().child(
            gpui::div()
                .text_size(px(13.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child("Mini Pi"),
        );

        let title_bar = if cfg!(target_os = "macos") {
            TitleBar::new().child(title).child(
                gpui::div()
                    .flex()
                    .items_center()
                    .pr_2()
                    .child(Self::user_menu_button(cx)),
            )
        } else {
            // On Windows/Linux the TitleBar children container is marked as a
            // window-drag region, so interactive children would not receive
            // clicks. Keep only the non-interactive title inside the TitleBar
            // and render the user menu as an absolute overlay (see below).
            TitleBar::new().child(title)
        };

        gpui::div()
            .flex()
            .flex_col()
            .size_full()
            .relative()
            .bg(theme.background)
            .text_color(theme.foreground)
            .font_family(theme.font_family.clone())
            .child(title_bar)
            .child(if user_panel_active {
                self.user_panel.clone().into_any_element()
            } else {
                self.thread_list.clone().into_any_element()
            })
            .when(cfg!(not(target_os = "macos")), |this| {
                // Position the user menu just to the left of the client-side
                // window controls (minimize/maximize/close), each 34px wide.
                this.child(
                    gpui::div()
                        .absolute()
                        .top_0()
                        .right(px(102.0))
                        .h(px(34.0))
                        .flex()
                        .items_center()
                        .pr_2()
                        .child(
                            gpui::div()
                                .flex()
                                .items_center()
                                .justify_end()
                                .px_2()
                                .gap_2()
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .child(Self::user_menu_button(cx)),
                        ),
                )
            })
            .children(dialog_layer)
            .children(notification_layer)
            .children(sheet_layer)
    }
}

impl MiniPiApp {
    fn user_menu_button(cx: &mut gpui::Context<Self>) -> impl gpui::IntoElement {
        Button::new("user-menu")
            .with_size(gpui_component::Size::Small)
            .ghost()
            .icon(
                Icon::empty()
                    .path("account.svg")
                    .text_color(gpui::rgb(0x888888)),
            )
            .on_click(cx.listener(|_this, _, _, cx| {
                cx.update_global(|app: &mut AppStore, _| {
                    app.user_panel_active = !app.user_panel_active;
                });
                cx.notify();
            }))
    }
}
