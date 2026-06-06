mod core;
mod data;
mod rpc;
mod config;
mod ui;
mod views;
mod utils;

use std::{path::PathBuf, sync::Arc};

use gpui::{
    App, AppContext, Application, Bounds, KeyBinding, px, size,
};

use crate::core::actions::Quit;
use crate::core::app::{AppStore, custom_window_options};
use crate::core::assets::Assets;
use crate::data::store::Store;
use crate::views::thread_list::ThreadList;

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
