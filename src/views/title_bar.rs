use gpui::Window;
#[cfg(target_os = "macos")]
use objc::{msg_send, sel, sel_impl};

/// Set the platform window level so the chat window can be pinned on top.
#[cfg(target_os = "macos")]
pub fn set_window_level(window: &Window, pinned: bool) {
    use objc::runtime::Object;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    const NSFLOATING_WINDOW_LEVEL: isize = 3;
    const NSNORMAL_WINDOW_LEVEL: isize = 0;

    if let Ok(handle) = HasWindowHandle::window_handle(window)
        && let RawWindowHandle::AppKit(appkit) = handle.as_raw()
    {
        let ns_view = appkit.ns_view.as_ptr() as *mut Object;
        #[allow(unexpected_cfgs)]
        unsafe {
            let ns_window: *mut Object = msg_send![ns_view, window];
            let level = if pinned {
                NSFLOATING_WINDOW_LEVEL
            } else {
                NSNORMAL_WINDOW_LEVEL
            };
            let () = msg_send![ns_window, setLevel: level];
        }
    }
}

#[cfg(target_os = "windows")]
pub fn set_window_level(window: &Window, pinned: bool) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    type HWND = *mut std::ffi::c_void;
    const HWND_TOPMOST: HWND = -1isize as HWND;
    const HWND_NOTOPMOST: HWND = -2isize as HWND;
    const SWP_NOSIZE: u32 = 0x0001;
    const SWP_NOMOVE: u32 = 0x0002;
    const SWP_SHOWWINDOW: u32 = 0x0040;

    unsafe extern "system" {
        fn SetWindowPos(
            hwnd: HWND,
            hwnd_insert_after: HWND,
            x: i32,
            y: i32,
            cx: i32,
            cy: i32,
            u_flags: u32,
        ) -> i32;
    }

    if let Ok(handle) = HasWindowHandle::window_handle(window) {
        if let RawWindowHandle::Win32(win32) = handle.as_raw() {
            let hwnd = win32.hwnd.get() as *mut std::ffi::c_void;
            unsafe {
                SetWindowPos(
                    hwnd,
                    if pinned { HWND_TOPMOST } else { HWND_NOTOPMOST },
                    0,
                    0,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOMOVE | SWP_SHOWWINDOW,
                );
            }
        }
    }
}

#[cfg(target_os = "linux")]
pub fn set_window_level(_window: &Window, pinned: bool) {
    // Best-effort X11 support via wmctrl. Wayland has no standard always-on-top protocol.
    let _ = std::process::Command::new("wmctrl")
        .args([
            "-r",
            ":ACTIVE:",
            "-b",
            if pinned { "add,above" } else { "remove,above" },
        ])
        .spawn();
}
