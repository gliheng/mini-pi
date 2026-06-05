mod actions;
mod app;
mod assets;
mod chat_window;
mod dropdown;
mod input;
mod model_config;
mod models;
mod pi_rpc;
mod store;
mod thread_list;
mod title_bar;
mod user_panel;
mod utils;

use std::{path::PathBuf, sync::Arc};

use gpui::{
    App, AppContext, Application, Bounds, KeyBinding, px, size,
};

use crate::actions::Quit;
use crate::app::{AppStore, custom_window_options};
use crate::assets::Assets;
use crate::store::Store;
use crate::thread_list::ThreadList;

fn quit(_: &Quit, cx: &mut App) {
    cx.quit();
}

fn main() {
    let store = Arc::new(Store::open().expect("failed to open database"));
    let assets_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");

    Application::new()
        .with_assets(Assets { base: assets_dir })
        .run(|cx: &mut App| {
            cx.set_global(AppStore(store));

            cx.on_action(quit);
            cx.bind_keys([
                KeyBinding::new("ctrl-w", actions::CloseWindow, None),
                KeyBinding::new("cmd-w", actions::CloseWindow, None),
                KeyBinding::new("cmd-q", Quit, None),
                KeyBinding::new("enter", actions::SendMessage, None),
                KeyBinding::new("backspace", input::Backspace, None),
                KeyBinding::new("delete", input::Delete, None),
                KeyBinding::new("left", input::Left, None),
                KeyBinding::new("right", input::Right, None),
                KeyBinding::new("shift-left", input::SelectLeft, None),
                KeyBinding::new("shift-right", input::SelectRight, None),
                KeyBinding::new("ctrl-f", input::Forward, None),
                KeyBinding::new("ctrl-b", input::Backward, None),
                KeyBinding::new("cmd-a", input::SelectAll, None),
                KeyBinding::new("cmd-v", input::Paste, None),
                KeyBinding::new("cmd-c", input::CopyText, None),
                KeyBinding::new("cmd-x", input::Cut, None),
                KeyBinding::new("home", input::Home, None),
                KeyBinding::new("end", input::End, None),
            ]);

            cx.on_window_closed(|cx| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();

            let store = cx.global::<AppStore>().0.clone();
            let bounds = Bounds::centered(None, size(px(420.0), px(600.0)), cx);
            cx.open_window(
                custom_window_options(Some(bounds)),
                |window, cx| {
                    cx.new(|cx| {
                        let list = ThreadList::new(cx, store);
                        list.focus_handle.focus(window);
                        list
                    })
                },
            )
            .unwrap();

            cx.activate(true);
        });
}
