# mini-pi → gpui-component migration plan

> Updated: 2026-06-24
> This plan covers what has already been done and what remains, so the next session can pick up cleanly.

## Context

`mini-pi` is migrating away from hand-rolled UI components to `gpui-component`.
The project now depends on the GitHub versions so that `gpui` and `gpui-component` stay in lockstep:

```toml
# Cargo.toml (current)
gpui = { git = "https://github.com/zed-industries/zed", rev = "1d217ee39d381ac101b7cf49d3d22451ac1093fe" }
gpui_platform = { git = "https://github.com/zed-industries/zed", rev = "1d217ee39d381ac101b7cf49d3d22451ac1093fe", features = ["font-kit", "x11", "wayland", "runtime_shaders"] }
gpui-component = { git = "https://github.com/longbridge/gpui-component.git", rev = "7315e07" }
```

Other notes:
- The custom `loader` (`src/ui/loader.rs`) was kept because `gpui_component::Spinner` could not load its bundled icon through mini-pi’s asset loader.
- Tests must be run with `NO_PROXY=localhost,127.0.0.1 cargo test` to avoid localhost proxy 502 errors.

---

## ✅ Completed

### Phase 1 — Low-risk mechanical replacements
- `src/views/title_bar.rs`: stripped to just the platform `set_window_level` helper.
- `src/ui/loader.rs`: deleted and restored (kept custom loader).
- Replaced inline `div(...).on_click(...)` buttons with `gpui_component::button::Button` in:
  - `src/views/chat_window.rs`
  - `src/views/thread_list.rs`
  - `src/views/user_panel.rs`
  - `src/views/workspace_manager.rs`
  - `src/views/pi_agent_import.rs`
- Kept custom SVG icons via `Icon::empty().path("...")` because `gpui-component`’s bundled Lucide icons do not resolve through mini-pi’s asset loader.

### Phase 1.5 — Unify gpui / gpui-component versions
- Switched from crates.io `gpui 0.2.2` + `gpui-component 0.5.1` to the GitHub versions shown above.
- Fixed GPUI API differences (`Application::with_platform`, `Menu`, `on_window_closed`, `BoxShadow` `inset`, `window.focus(cx)`, `max_offset().y`, etc.).

### Phase 2 — Input / Dropdown / Toast
- Deleted:
  - `src/ui/input.rs`
  - `src/ui/dropdown.rs`
  - `src/ui/toast.rs`
- Updated `src/ui/mod.rs`.
- Removed no-context `TextInput` key bindings from `src/app.rs`.
- Migrated usage sites:
  - `src/views/thread_list.rs` search box → `gpui_component::input::Input` / `InputState`
  - `src/views/user_panel.rs` auth fields → `Input` / `InputState`
  - `src/views/chat_window.rs` model/thinking selectors → `gpui_component::select::Select` / `SelectState` / `SelectItem` / `SearchableVec`
  - `src/views/chat_window.rs` + `src/views/user_panel.rs` toast → `gpui_component::notification::Notification` via `window.push_notification(...)`

### Phase 3 — Chat input (`TextArea`) without losing `@` / `/`
- Deleted `src/ui/text_area.rs`.
- Added `src/ui/chat_input.rs` wrapping `gpui_component::input::InputState` (`.multi_line(true)`, `.auto_grow(1, 8)`, `.submit_on_enter(true)`).
- Kept the `@` mention and `/` slash popups rendered inline with simple styled lists.
- Preserved `file_scanner` cache and `[@name](path)` mention insertion logic.
- Removed `TextArea` key bindings from `src/app.rs`.
- Updated `src/views/chat_window.rs` (`chat_input`, `inline_edit_input`) and `src/core/session_handle.rs` (`CommandItem` import).
- Popup keyboard navigation (`up`/`down`/`enter`/`tab`) is handled by an `on_key_down` listener on the input container; verify interactively that this correctly intercepts keys before `InputState` consumes them.

### Phase 4 — Markdown renderer, modals, and reasoning
- **Markdown:**
  - Deleted `src/ui/markdown/` and removed `pulldown-cmark` + `syntect` from `Cargo.toml`.
  - Replaced `MarkdownRenderer` with `gpui_component::text::{TextView, TextViewState}`.
  - Updated `src/views/chat_window.rs` to render assistant messages with `TextView::new(&text_view_state).w_full()`.
  - Updated `examples/markdown_renderer.rs` and `examples/markdown_test.md` to use `TextViewState::markdown(...)`.
- **Modals:**
  - `src/views/workspace_manager.rs` no longer implements `Render`; it builds dialog content via `render_dialog_content(...)` and is shown through `window.open_dialog(...)` in `src/views/chat_window.rs`.
- **Reasoning:**
  - `src/views/reasoning.rs` now uses `gpui_component::collapsible::Collapsible` for thinking/reasoning blocks.

---

## 🚧 Remaining work

The major component migration is complete. Remaining items are verification and optional polish:

1. **Manual GUI verification**
   - Confirm `TextView` renders assistant markdown (paragraphs, lists, code blocks, links) correctly in a live chat window.
   - Confirm the `Dialog`-based workspace manager opens, adds, and deletes workspaces correctly.
   - Confirm the `Collapsible` reasoning block expands/collapses and preserves its toggle state.
2. **Optional: import prompt dialog**
   - `src/views/pi_agent_import.rs` still renders inline. If desired, migrate it to `gpui_component::dialog::*` for consistency with the workspace manager.
3. **Optional: autocomplete popovers**
   - Consider migrating the inline `@` mention and `/` slash popups to `gpui_component::popover::Popover` + `gpui_component::list::List` for more integrated focus management.
4. **Optional: tabs**
   - If the multi-window thread model is ever consolidated into a single window, consider `gpui_component::tab::{Tab, TabBar}`.

---

## Handy API reminders for the next session

- `gpui-component` is now the GitHub `0.5.2` stream; it may differ slightly from the old crates.io `0.5.1`.
- Traits must be imported to use their methods:
  - `gpui_component::Sizable as _`
  - `gpui_component::Disableable as _`
  - `gpui_component::button::ButtonVariants as _`
  - `gpui_component::WindowExt as _` (for `window.open_dialog(...)` / `window.close_dialog(cx)`)
- `Button` default variant is `Secondary`; there is **no** `.secondary()` method.
- `Icon::empty().path("assets/relative.svg")` works with mini-pi’s asset loader; `IconName` bundled icons do not.
- `gpui_component::text::TextView` and `TextViewState` live in the `text` submodule; `Collapsible` lives in `gpui_component::collapsible::Collapsible`; `Dialog` APIs live in `gpui_component::dialog`.
- Run tests with: `NO_PROXY=localhost,127.0.0.1 cargo test`
