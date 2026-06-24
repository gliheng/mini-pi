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

---

## 🚧 Remaining work

### Phase 3 — Chat input (`TextArea`) without losing `@` / `/`
**Goal:** Replace the base text editor in `src/ui/text_area.rs` while preserving autocomplete.

1. Introduce a new `ChatInput` component backed by `gpui_component::input::InputState` with `.multi_line(true)` and `.submit_on_enter(true)`.
2. Re-implement the `@` mention popup as a `gpui_component::popover::Popover` containing a `gpui_component::list::List` (or a simple styled list for a first pass).
3. Re-implement the `/` slash palette the same way.
4. Keep the existing `file_scanner` cache and mention insertion logic (`[@name](path)`); only the rendering layer changes.
5. Delete `src/ui/text_area.rs` and remove the `TextArea` global key bindings from `src/app.rs`.
6. Update `src/views/chat_window.rs` (`chat_input`, `inline_edit_input`) and any other callers.

**Files likely to change:**
- `src/ui/text_area.rs` — delete
- `src/ui/mod.rs` — remove `text_area`
- `src/app.rs` — remove `TextArea` key bindings
- `src/views/chat_window.rs` — adopt new `ChatInput`
- Possibly new file `src/ui/chat_input.rs` or `src/views/chat_input.rs`

### Phase 4 — Markdown renderer and modals (highest risk)
**Goal:** Align the richest custom components.

1. **Markdown**
   - Evaluate `gpui_component::text::{TextView, TextViewState}` against `src/ui/markdown/`.
   - Prototype one assistant message with `TextView::markdown(...)` and compare rendering/syntax highlighting.
   - If acceptable, replace `MarkdownRenderer` and delete `src/ui/markdown/`.
   - If critical features are missing (custom block types, exact `syntect` theme), defer or wrap `TextView` with custom plugins.
2. **Modals**
   - Replace `src/views/workspace_manager.rs` inline modal with `gpui_component::dialog::*`.
   - Replace `src/views/pi_agent_import.rs` import prompt with `gpui_component::dialog::*` if desired.
3. **Reasoning**
   - Replace `src/views/reasoning.rs` with `gpui_component::collapsible::*`.

**Files likely to change:**
- `src/ui/markdown/` — evaluate/delete
- `src/views/workspace_manager.rs`
- `src/views/reasoning.rs`
- `src/views/pi_agent_import.rs`

### Optional future — Tabs
- If the multi-window thread model is ever consolidated into a single window, consider `gpui_component::tab::{Tab, TabBar}`.
- This is **not** part of the current migration unless explicitly requested.

---

## Handy API reminders for the next session

- `gpui-component` is now the GitHub `0.5.2` stream; it may differ slightly from the old crates.io `0.5.1`.
- Traits must be imported to use their methods:
  - `gpui_component::Sizable as _`
  - `gpui_component::Disableable as _`
  - `gpui_component::button::ButtonVariants as _`
- `Button` default variant is `Secondary`; there is **no** `.secondary()` method.
- `Icon::empty().path("assets/relative.svg")` works with mini-pi’s asset loader; `IconName` bundled icons do not.
- Run tests with: `NO_PROXY=localhost,127.0.0.1 cargo test`
