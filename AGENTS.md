# mini-pi

A desktop GUI chat application that wraps the `pi` AI coding agent CLI. Built with Rust and GPUI (the GPU-accelerated UI framework from the Zed editor).

## Project Overview

`mini-pi` provides a native chat-window interface for interacting with the `pi` CLI tool in RPC mode. Users can create chat threads, select AI models, manage workspaces (project directories), and authenticate via Supabase to sync agent configuration across devices.

The application spawns the `pi` binary as a subprocess and communicates with it via JSON Lines over stdin/stdout. Chat sessions are persisted locally in SQLite and as JSONL files, while agent configuration can be synced to a Supabase storage bucket.

On first run, if `~/.pi/agent/` contains JSON files, the app offers to import them into `~/.mini-pi/agent/`.

## Technology Stack

- **Language:** Rust (2024 edition, requires stable Rust >= 1.92)
- **UI Framework:** GPUI 0.2.2 — hybrid immediate/retained mode, GPU-accelerated (Metal on macOS, Vulkan on Linux/Windows)
- **Database:** SQLite via `rusqlite` (bundled), with WAL mode and manual migrations
- **Async:** `smol` + `futures` for background tasks
- **HTTP:** `reqwest` (blocking client for auth, sync, and title generation)
- **Serialization:** `serde` / `serde_json`
- **Markdown:** `pulldown-cmark` (tables, strikethrough, tasklists, smart punctuation, footnotes, GFM) plus `syntect` for code-block syntax highlighting
- **Platform specifics:** `objc` on macOS for native window chrome; `raw-window-handle` for cross-platform window handles; Windows `CREATE_NO_WINDOW` flag

## Repository Layout

```
Cargo.toml              # Package manifest and dependencies
src/main.rs             # App bootstrap, global key bindings, AppStore setup, initial ThreadList window
src/lib.rs              # Public module re-exports (used by examples)
src/core/
  actions.rs            # Global actions: CloseWindow, Quit, SendMessage, Login, Logout, SignUp
  app.rs                # AppStore GPUI Global and custom_window_options()
  assets.rs             # AssetSource implementation that loads SVGs from the assets/ directory
src/config/
  app_config.rs         # ~/.config/mini-pi/config.json (default_model, default_workspace_name)
  model_config.rs       # Hardcoded model list and provider/name helpers
src/data/
  models.rs             # Domain enums: Role, PartState, MessagePart, Message, ChatState
  store.rs              # SQLite connection, migrations, and CRUD for threads/workspaces/user_settings
src/auth/
  state.rs              # AuthState, SupabaseSession, session persistence, agent-dir helpers
  supabase.rs           # Supabase auth and Storage API client (hardcoded URL + anon key)
src/rpc/
  pi_rpc.rs             # PiRpc subprocess bridge, BridgeEvent enum, JSONL parser
src/sync/
  settings_sync.rs      # Two-way sync of ~/.mini-pi/agent/ files with Supabase Storage bucket pi-sync
src/ui/
  input.rs              # TextInput: single-line custom input with cursor, selection, password mode
  text_area.rs          # TextArea: multi-line chat input with @ mention and / slash-command autocomplete
  dropdown.rs           # Reusable searchable dropdown component
  loader.rs             # Animated dot loader and text-loader spinner
  markdown.rs           # Markdown parser → custom AST → GPUI renderer, with syntax highlighting
src/utils/
  file_scanner.rs       # Workspace file-tree scanner used by @ mention autocomplete
  format.rs             # Relative-time formatting and string truncation helpers
  llm.rs                # Cloudflare AI Gateway title generator
src/views/
  thread_list.rs        # Home window showing pinned/unpinned threads
  chat_window.rs        # Per-thread chat window, model dropdown, workspace bar, message rendering
  user_panel.rs         # Account/auth/settings panel
  title_bar.rs          # Custom title bar with pin, export, workspace, and avatar controls
  workspace_manager.rs  # Modal workspace picker
  reasoning.rs          # Collapsible thinking/reasoning display
  pi_agent_import.rs    # First-run import prompt from ~/.pi/agent/
assets/                 # SVG icons loaded at runtime
assets/prompts/         # System prompt files (e.g. title_generator.txt)
docs/                   # Internal reference: GPUI guides, design review, markdown improvement plan, TODO
examples/               # Standalone markdown renderer example and test markdown file
```

## Build, Run and Test

```bash
# Standard cargo workflow
cargo build
cargo run

# Release build
cargo build --release

# Tests
cargo test
```

### Prerequisites

1. **Rust stable** (>= 1.92)
2. **Platform toolchain:**
   - **macOS:** Xcode + Xcode Command Line Tools (`xcode-select --install`)
   - **Linux:** Vulkan drivers, `libxcb`, `libxkbcommon`, `libfontconfig`, `libssl`
   - **Windows:** Vulkan SDK or DirectX
3. **`pi` CLI binary** must be installed and available on `PATH`. The app spawns `pi --mode rpc --session <path>` (on Windows it uses `pi.cmd`).
4. *(Optional)* **Cloudflare AI Gateway** environment variables for auto-generated thread titles:
   - `CLOUDFLARE_API_KEY`
   - `CLOUDFLARE_ACCOUNT_ID`
   - `CLOUDFLARE_GATEWAY_ID`

## Runtime Architecture

### Data Storage

- **Database:** `~/.mini-pi/mini-pi.db` (SQLite, WAL mode, foreign keys ON)
- **Sessions:** `~/.mini-pi/sessions/*.jsonl` — conversation history files used by the `pi` subprocess
- **Agent config:** `~/.mini-pi/agent/` — imported from `~/.pi/agent/` on first run
- **App config:** `~/.config/mini-pi/config.json`
- **Auth session:** Stored in the `user_settings` table of `~/.mini-pi/mini-pi.db` under key `supabase_session`
- **Sync metadata:** `~/.mini-pi/sync_meta.json`

### Database Migrations

Migrations are defined as a static slice of `(name, sql)` tuples in `src/data/store.rs` and tracked in a `_migrations` table. Current migrations:

- `001_init` — creates `threads` table
- `002_workspaces` — creates `workspaces` table
- `003_user_settings` — creates `user_settings` key-value table
- `004_thinking_level` — adds `thinking_level` column to `threads` table

### Process Model

On launch the app:

1. Opens SQLite and runs migrations.
2. Loads `AppConfig` from disk.
3. Attempts to restore the Supabase session (refresh if expired).
4. If logged in, spawns a background `smol` task to sync agent config changes.
5. Opens the `ThreadList` window.

Each chat thread opens its own window. Inside the window, `ChatWindow::spawn_pi` creates a `PiRpc` instance that:

- Runs `pi [--provider <provider> --model <model>] --mode rpc --session <session_path>`.
- Sets `PI_CODING_AGENT_DIR` and `PI_CODING_AGENT_SESSION_DIR` to the `~/.mini-pi` directories.
- Runs a background OS thread reading JSON Lines from `pi` stdout and forwarding parsed `BridgeEvent`s over an async `futures::channel::mpsc` channel.
- The `ChatWindow` GPUI task consumes that channel and updates messages/reasoning/tool-call state.

### Window Management

- `AppStore.thread_windows: HashMap<i64, AnyWindowHandle>` maps thread IDs to open chat windows to avoid duplicate windows for the same thread. Be aware that stale handles are not currently removed when a window is closed externally.
- All windows use `custom_window_options()` from `src/core/app.rs`: a transparent titlebar on macOS with traffic-light offset, and client-side decorations on other platforms.
- `TitleBar` is custom-rendered and includes platform-specific pin-to-top support (macOS `NSWindow` level, Windows `SetWindowPos`, Linux best-effort `wmctrl`).

### Key Bindings

Global bindings are registered in `main.rs`. Context-sensitive bindings use GPUI's `key_context` system (e.g. `"TextInput"` and `"TextArea"`). Common bindings:

- `Cmd/Ctrl + W` — Close window
- `Cmd + Q` — Quit
- `Enter` — Send message (in chat window)
- `Cmd + A` — Select all
- `Cmd + C/V/X` — Copy / Paste / Cut
- Arrow keys with Shift — Selection
- Home/End or `Ctrl+A/E` — Line start/end
- `Escape` — Close mention/command popups or workspace manager

Dropdowns handle `up`, `down`, `enter`, and `escape` internally.

## Code Organization Conventions

- **Entities:** All persistent UI state lives in GPUI `Entity<T>` structs. Views implement `Render`.
- **Events:** Components communicate via `EventEmitter<E>` + `cx.subscribe(...)` or `cx.observe(...)`.
- **Actions:** Global actions are declared with the `actions!` macro in `src/core/actions.rs`.
- **Globals:** `AppStore` is a GPUI `Global` holding `Arc<Store>`, config, auth state, session, sync state, and a window handle map.
- **Styling:** Uses GPUI's Tailwind-inspired fluent API (`div().flex().bg(rgb(...)).child(...)`).
- **Text input:** Two custom input implementations exist:
  - `ui::input::TextInput` — basic single-line input with custom `EntityInputHandler` (used in auth forms and dropdown search)
  - `ui::text_area::TextArea` — chat-specific multi-line input with `@` mention autocomplete and `/` slash-command palette support
- **Markdown:** `ui::markdown::MarkdownRenderer` parses content into custom `BlockNode` / `InlineNode` ASTs and renders them to GPUI elements. Code blocks are highlighted with `syntect` using the `base16-ocean.dark` theme.

## External CLI Dependency

This application is a thin GUI wrapper around the `pi` CLI. Without the `pi` binary on `PATH`, chat functionality will fail at runtime when `PiRpc::spawn` is called. The RPC protocol is documented by the `BridgeEvent` enum in `src/rpc/pi_rpc.rs`.

## Security Considerations

- The Supabase anonymous key and URL are hardcoded in `src/auth/supabase.rs`.
- Auth tokens are stored in plaintext JSON inside the `user_settings` table of the local SQLite database (`~/.mini-pi/mini-pi.db`).
- Agent configuration and chat sessions are stored locally in the user's home directory.
- Cloudflare API credentials are read from environment variables only.

## Testing

- `cargo test` runs the unit tests in `src/ui/markdown.rs`.
- The current codebase has **10 markdown unit tests**, but one is failing:
  - `ui::markdown::tests::heading_font_sizes_are_distinct` asserts that all six heading font sizes are distinct; the current mapping has a duplicate.
- There are no integration tests and no CI workflows configured for this repository.

## Documentation

- `docs/at-mention-autocomplete.md` — Guide for implementing `@` mention autocomplete, derived from Zed's `agent_ui` crate.
- `docs/design-review.md` — Design review (in Chinese) that lists known architecture issues such as PiRpc process monitoring, sync file locking, stale window handles, and Store `Connection` thread safety.
- `docs/markdown-improvement-plan.md` — Planned markdown renderer improvements and known rendering bugs.
- `docs/TODO.md` — Short checklist of upcoming features.
- `examples/markdown_renderer.rs` + `examples/markdown_test.md` — Standalone markdown renderer example.

## Notes for Agents

- `docs/` contains internal reference material, not project user documentation.
- The model list is hardcoded in `src/config/model_config.rs`. New models must be added there. Model IDs use a `<provider>:<model>` format parsed by `parse_model_id`.
- When adding database changes, append a new migration tuple to `MIGRATIONS` in `src/data/store.rs`.
- Assets are loaded from the source tree at runtime via `core::assets::Assets`. Running the binary outside the repository requires the `assets/` directory to be present at the expected path.
- The app is primarily developed and tested on macOS. Windows-specific and Linux-specific code exists (e.g. `CREATE_NO_WINDOW`, client-side titlebar controls, `wmctrl`) but may need verification.
- Several known issues are documented in `docs/design-review.md`; review it before making large changes to process management, sync, or window lifecycle.
