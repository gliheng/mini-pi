mod auth;
mod config;
mod core;
mod data;
mod rpc;
mod sync;
mod ui;
mod utils;
mod views;

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use gpui::{App, AppContext, Application, Bounds, KeyBinding, px, size};

use crate::auth::state::{self, AuthState};
use crate::config::app_config::AppConfig;
use crate::core::actions::Quit;
use crate::core::app::{AppStore, custom_window_options};
use crate::core::assets::Assets;
use crate::data::store::Store;
use crate::sync::settings_sync;
use crate::views::thread_list::ThreadList;

fn quit(_: &Quit, cx: &mut App) {
    cx.quit();
}

fn main() {
    let store = Arc::new(Store::open().expect("failed to open database"));
    let config = AppConfig::load();
    let assets_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");

    let (auth, session) = match state::load_session() {
        Some(session) => {
            if session.is_expired() {
                match crate::auth::supabase::refresh_session(&session.refresh_token) {
                    Ok(new_session) => {
                        let user = new_session.user.clone();
                        let _ = state::save_session(&new_session);
                        (AuthState::LoggedIn(user), Some(new_session))
                    }
                    Err(_) => {
                        let _ = state::clear_session();
                        (AuthState::LoggedOut, None)
                    }
                }
            } else {
                match crate::auth::supabase::get_user(&session.access_token) {
                    Ok(user) => (AuthState::LoggedIn(user), Some(session)),
                    Err(_) => {
                        let _ = state::clear_session();
                        (AuthState::LoggedOut, None)
                    }
                }
            }
        }
        None => (AuthState::LoggedOut, None),
    };

    let sync_meta = settings_sync::load_sync_meta();

    if auth.is_logged_in() {
        if let Some(ref sess) = session {
            let _ = state::agent_dir();
            let access_token = sess.access_token.clone();
            let user_id = sess.user.id.clone();
            let _ = std::thread::spawn(move || {
                let _ = settings_sync::sync_changes(&access_token, &user_id);
            });
        }
    }

    Application::new()
        .with_assets(Assets { base: assets_dir })
        .run(|cx: &mut App| {
            cx.set_global(AppStore {
                store,
                config,
                thread_windows: HashMap::new(),
                auth,
                session,
                sync_meta,
                user_panel_active: false,
            });

            cx.on_action(quit);
            cx.bind_keys([
                KeyBinding::new("ctrl-w", core::actions::CloseWindow, None),
                KeyBinding::new("cmd-w", core::actions::CloseWindow, None),
                KeyBinding::new("cmd-q", Quit, None),
                KeyBinding::new("enter", core::actions::SendMessage, None),
                KeyBinding::new("backspace", ui::input::Backspace, None),
                KeyBinding::new("delete", ui::input::Delete, None),
                KeyBinding::new("left", ui::input::Left, None),
                KeyBinding::new("right", ui::input::Right, None),
                KeyBinding::new("shift-left", ui::input::SelectLeft, None),
                KeyBinding::new("shift-right", ui::input::SelectRight, None),
                KeyBinding::new("ctrl-f", ui::input::Forward, None),
                KeyBinding::new("ctrl-b", ui::input::Backward, None),
                KeyBinding::new("cmd-a", ui::input::SelectAll, None),
                KeyBinding::new("cmd-v", ui::input::Paste, None),
                KeyBinding::new("cmd-c", ui::input::CopyText, None),
                KeyBinding::new("cmd-x", ui::input::Cut, None),
                KeyBinding::new("home", ui::input::Home, None),
                KeyBinding::new("end", ui::input::End, None),
                KeyBinding::new("ctrl-a", ui::input::Home, None),
                KeyBinding::new("ctrl-e", ui::input::End, None),
                KeyBinding::new("backspace", ui::chat_input::Backspace, Some("ChatInput")),
                KeyBinding::new("delete", ui::chat_input::Delete, Some("ChatInput")),
                KeyBinding::new("left", ui::chat_input::Left, Some("ChatInput")),
                KeyBinding::new("right", ui::chat_input::Right, Some("ChatInput")),
                KeyBinding::new("shift-left", ui::chat_input::SelectLeft, Some("ChatInput")),
                KeyBinding::new("shift-right", ui::chat_input::SelectRight, Some("ChatInput")),
                KeyBinding::new("ctrl-f", ui::chat_input::Forward, Some("ChatInput")),
                KeyBinding::new("ctrl-b", ui::chat_input::Backward, Some("ChatInput")),
                KeyBinding::new("cmd-a", ui::chat_input::SelectAll, Some("ChatInput")),
                KeyBinding::new("cmd-v", ui::chat_input::Paste, Some("ChatInput")),
                KeyBinding::new("cmd-c", ui::chat_input::CopyText, Some("ChatInput")),
                KeyBinding::new("cmd-x", ui::chat_input::Cut, Some("ChatInput")),
                KeyBinding::new("home", ui::chat_input::Home, Some("ChatInput")),
                KeyBinding::new("end", ui::chat_input::End, Some("ChatInput")),
                KeyBinding::new("ctrl-a", ui::chat_input::Home, Some("ChatInput")),
                KeyBinding::new("ctrl-e", ui::chat_input::End, Some("ChatInput")),
            ]);

            cx.on_window_closed(|cx| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();

            let store = cx.global::<AppStore>().store.clone();
            let bounds = Bounds::centered(None, size(px(420.0), px(600.0)), cx);
            cx.open_window(custom_window_options(Some(bounds)), |window, cx| {
                cx.new(|cx| {
                    let list = ThreadList::new(cx, store);
                    list.focus_handle.focus(window);
                    list
                })
            })
            .unwrap();

            cx.activate(true);
        });
}
