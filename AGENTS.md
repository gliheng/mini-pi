# mini-pi

A desktop GUI chat application that wraps the `pi` AI coding agent CLI. Built with Rust and GPUI (the GPU-accelerated UI framework from the Zed editor).

## Project Overview

`mini-pi` provides a native chat-window interface for interacting with the `pi` CLI tool in RPC mode. Users can create chat threads, select AI models, manage workspaces (project directories), and authenticate via Supabase for settings synchronization.

The application spawns the `pi` binary as a subprocess and communicates with it via JSON Lines over stdin/stdout. Chat sessions are persisted locally in SQLite and as JSONL files, while agent configuration can be synced to a Supabase storage bucket.

## Technology Stack

- **Language:** Rust (2024 edition, requires stable Rust >= 1.92)
- **UI Framework:** GPUI 0.2.2 — hybrid immediate/retained mode, GPU-accelerated (Metal on macOS, Vulkan on Linux/Windows)
- **Database:** SQLite via `rusqlite` (bundled), with WAL mode and manual migrations
- **Async:** `smol` + `futures` for background tasks
- **HTTP:** `reqwest` (blocking client for auth and sync)
- **Serialization:** `serde` / `serde_json`
- **Markdown:** `pulldown-cmark` (tables, strikethrough, tasklists, smart punctuation, footnotes, GFM)
- **Platform specifics:** `objc` on macOS for native window chrome

## Project Structure

```
src/
  main.rs              — App bootstrap, global key bindings, initial window
  auth/                — Supabase authentication (signup, login, refresh, session storage)
  config/              — App config (JSON) and hardcoded model list
  core/                — GPUI globals (AppStore), actions, assets, window options
  data/                — SQLite store with migrations, ThreadMeta/WorkspaceMeta, data models
  rpc/                 — PiRpc: subprocess bridge to the `pi` CLI (spawn, send commands, parse events)
  sync/                — Two-way sync of agent config files to Supabase storage
  ui/                  — Reusable components: TextInput, ChatInput, Dropdown, Loader, MarkdownRenderer
  utils/               — File scanner (for @-mention), formatting helpers, LLM title generator
  views/               — Top-level views: ThreadList, ChatWindow, UserPanel, TitleBar, etc.
assets/                — SVG icons loaded at runtime
prompts/               — System prompt files (e.g. title_generator.txt)
docs/                  — GPUI learning guides (gpui-in-action.md, gpui-wiki.md, at-mention-autocomplete.md)
```

## Build and Run

```bash
# Standard cargo workflow
cargo build
cargo run

# Release build
cargo build --release
```

### Prerequisites

1. **Rust stable** (>= 1.92)
2. **Platform toolchain:**
   - **macOS:** Xcode + Xcode Command Line Tools (`xcode-select --install`)
   - **Linux:** Vulkan drivers, `libxcb`, `libxkbcommon`, `libfontconfig`, `libssl`
   - **Windows:** Vulkan SDK or DirectX
3. **`pi` CLI binary** must be installed and available on `PATH`. The app spawns `pi --mode rpc --session <path>`.
4. *(Optional)* **Cloudflare AI Gateway** environment variables for auto-generated thread titles:
   - `CLOUDFLARE_API_KEY`
   - `CLOUDFLARE_ACCOUNT_ID`
   - `CLOUDFLARE_GATEWAY_ID`

## Runtime Architecture

### Data Storage

- **Database:** `~/.mini-pi/mini-pi.db` (SQLite, WAL mode, foreign keys ON)
- **Sessions:** `~/.mini-pi/sessions/*.jsonl` — conversation history files
- **Agent config:** `~/.mini-pi/agent/` — imported from `~/.pi/agent/` on first run
- **App config:** `~/.config/mini-pi/config.json`
- **Auth session:** Stored in the `user_settings` table of `~/.mini-pi/mini-pi.db`
- **Sync metadata:** `~/.mini-pi/sync_meta.json`

### Database Migrations

Migrations are defined as a static slice of `(name, sql)` tuples in `src/data/store.rs` and tracked in a `_migrations` table. Current migrations:

- `001_init` — creates `threads` table
- `002_workspaces` — creates `workspaces` table
- `003_user_settings` — creates `user_settings` key-value table

### Process Model

On launch the app:

1. Opens SQLite and runs migrations.
2. Loads `AppConfig` from disk.
3. Attempts to restore the Supabase session (refresh if expired).
4. If logged in, spawns a background thread to sync agent config changes.
5. Opens the `ThreadList` window.

Each chat thread opens its own window. Inside the window, a `PiRpc` instance spawns the `pi` subprocess and runs a background reader thread that parses JSON Lines from `pi` stdout into `BridgeEvent`s.

### Key Bindings

Global bindings are registered in `main.rs`. Context-sensitive bindings use GPUI's `key_context` system (e.g. `"ChatInput"` context for chat-specific input actions). Common bindings:

- `Cmd/Ctrl + W` — Close window
- `Cmd + Q` — Quit
- `Enter` — Send message
- `Cmd + A` — Select all
- `Cmd + C/V/X` — Copy / Paste / Cut
- Arrow keys with Shift — Selection
- Home/End or `Ctrl+A/E` — Line start/end

## Code Organization Conventions

- **Entities:** All persistent UI state lives in GPUI `Entity<T>` structs. Views implement `Render`.
- **Events:** Components communicate via `EventEmitter<E>` + `cx.subscribe(...)` or `cx.observe(...)`.
- **Actions:** Global actions are declared with the `actions!` macro in `src/core/actions.rs`.
- **Globals:** `AppStore` is a GPUI `Global` holding `Arc<Store>`, config, auth state, and a window handle map.
- **Styling:** Uses GPUI's Tailwind-inspired fluent API (`div().flex().bg(rgb(...)).child(...)`).
- **Text input:** Two custom input implementations exist:
  - `ui::input::TextInput` — basic single-line input with custom `EntityInputHandler`
  - `ui::chat_input::ChatInput` — chat-specific input with `@` mention autocomplete and command palette support

## Testing

Test coverage is minimal. The only tests are three unit tests in `src/ui/markdown.rs` (run with `cargo test`). There are no integration tests and no CI workflows configured for this repository.

```bash
cargo test
```

## Security Considerations

- The Supabase anonymous key and URL are hardcoded in `src/auth/supabase.rs`.
- Auth tokens are stored in plaintext JSON inside the `config` table of the local SQLite database (`~/.mini-pi/mini-pi.db`).
- Agent configuration and chat sessions are stored locally in the user's home directory.
- Cloudflare API credentials are read from environment variables only.

## Dependencies on External CLI

This application is a thin GUI wrapper around the `pi` CLI. Without the `pi` binary on `PATH`, chat functionality will fail at runtime when `PiRpc::spawn` is called. The RPC protocol is documented by the `BridgeEvent` enum in `src/rpc/pi_rpc.rs`.

## Notes for Agents

- `docs/` contains GPUI reference material, not project user documentation.
- The model list is hardcoded in `src/config/model_config.rs`. New models must be added there.
- When adding database changes, append a new migration tuple to `MIGRATIONS` in `src/data/store.rs`.
- The app is primarily developed and tested on macOS. Windows-specific code exists (e.g. `CREATE_NO_WINDOW` flag) but may need verification.
