# Reimplementing `@` Mention Autocomplete

A guide derived from Zed's `agent_ui` crate.

---

## 1. Architecture Overview

The system has **four layers**:

| Layer | What | Where |
|-------|------|-------|
| **Trigger & Parse** | Detects `@` in the text buffer, extracts mode and query | `completion_provider.rs` |
| **Completion Provider** | Implements the editor's `CompletionProvider` trait, responds with results | `completion_provider.rs` |
| **Search** | Fuzzy-matches files/dirs/symbols/threads/skills against queries | `completion_provider.rs` |
| **Confirmation & UI** | Replaces typed text with a collapsed inline button ("crease"), loads content async | `mention_set.rs`, `ui/mention_crease.rs` |

```
MessageEditor
  ├── Editor (GPUI rich-text editor)
  │     └── set_completion_provider(PromptCompletionProvider)
  │           ├── delegate: PromptCompletionProviderDelegate (for config)
  │           ├── mention_set: MentionSet (stores active mentions)
  │           └── workspace: Workspace (for file search)
  └── MentionSet (Entity)
        ├── mentions: HashMap<CreaseId, (MentionUri, MentionTask)>
        └── insert_crease_for_mention() → Crease::Inline + FoldPlaceholder
              └── renders MentionCrease (GPUI element)
```

---

## 2. Trigger Detection & Parsing

### 2a. When Does It Fire?

The editor calls `CompletionProvider::is_completion_trigger()` on every keystroke:

```rust
fn is_completion_trigger(
    &self,
    buffer: &Entity<Buffer>,
    position: Anchor,
    _text: &str,
    _trigger_in_words: bool,
    cx: &mut Context<Editor>,
) -> bool {
    // Get the current line text from the buffer
    let buffer = buffer.read(cx);
    let position = position.to_point(buffer);
    let line_start = Point::new(position.row, 0);
    let offset_to_line = buffer.point_to_offset(line_start);
    let line = buffer.text_for_range(line_start..position).lines().next()?;

    // Parse and check if cursor is within the mention range
    PromptCompletion::try_parse(line, offset_to_line, &self.source.supported_modes(cx))
        .filter(|c| c.source_range().start <= offset + col && offset + col <= c.source_range().end)
        .is_some()
}
```

### 2b. The Mention Parser

`MentionCompletion::try_parse(line, offset_to_line, supported_modes)`:

1. Find the **rightmost `@`** that has a **word boundary** before it:
   - Start of line
   - Preceded by whitespace
   - Preceded by `(`, `[`, or `{`
2. Ensure **no whitespace immediately after `@`** (reject `@ foo`)
3. Split the rest on whitespace:
   - `@file main.rs` → `mode = Some(File)`, `argument = Some("main.rs")`
   - `@main` → `mode = None`, `argument = Some("main")` (bare mention, implicit file search)
   - `@` → `mode = None`, `argument = None` (open mode picker)
4. Compute `source_range`: the byte range that will be replaced on accept

### 2c. The `PromptCompletion` Enum

```rust
enum PromptCompletion {
    SlashCommand(SlashCommandCompletion),  // /command
    Mention(MentionCompletion),            // @file query, @symbol query, etc.
}
```

### 2d. `PromptContextType` — Known Mention Modes

| Keyword | Type | Description |
|---------|------|-------------|
| `file` | `PromptContextType::File` | Searches files and directories |
| `symbol` | `PromptContextType::Symbol` | Searches LSP workspace symbols |
| `fetch` | `PromptContextType::Fetch` | Fetch URL content |
| `thread` | `PromptContextType::Thread` | Search conversation threads |
| `skill` | `PromptContextType::Skill` | Search available skills |
| `diagnostics` | `PromptContextType::Diagnostics` | Project diagnostics |
| `branch diff` | `PromptContextType::BranchDiff` | Current branch diff |

A bare `@` without a mode keyword triggers **implicit file search** plus a mode-picker with entries.

---

## 3. The `CompletionProvider` Trait

The editor framework defines a trait. You implement these methods:

### 3a. Required Methods

```rust
impl CompletionProvider for PromptCompletionProvider<T> {
    fn completions(
        &self,
        buffer: &Entity<Buffer>,
        buffer_position: Anchor,
        _trigger: CompletionContext,
        window: &mut Window,
        cx: &mut Context<Editor>,
    ) -> Task<Result<Vec<CompletionResponse>>>;

    fn is_completion_trigger(
        &self,
        buffer: &Entity<Buffer>,
        position: Anchor,
        _text: &str,
        _trigger_in_words: bool,
        cx: &mut Context<Editor>,
    ) -> bool;

    fn sort_completions(&self) -> bool { false }  // Disable built-in sort
    fn filter_completions(&self) -> bool { false } // Disable built-in filter
}
```

Return `false` for both `sort_completions` and `filter_completions` — the provider does its own ranking and deduplication.

### 3b. The Delegate Pattern

Rather than hard-coding mode support, Zed uses a **delegate trait** so the same provider can serve different contexts (agent panel, inline assistant, etc.):

```rust
pub trait PromptCompletionProviderDelegate: Send + Sync + 'static {
    fn supported_modes(&self, cx: &App) -> Vec<PromptContextType>;
    fn supports_images(&self, cx: &App) -> bool;
    fn available_commands(&self, cx: &App) -> Vec<AvailableCommand>;
    fn available_skills(&self, cx: &App) -> Vec<AvailableSkill>;
    fn confirm_command(&self, cx: &mut App);
}
```

The `PromptCompletionProvider<T>` is generic over `T: PromptCompletionProviderDelegate`.

### 3c. The `completions()` Method Flow

1. Re-parse the current line using `PromptCompletion::try_parse()`
2. Compute `source_range` as buffer anchors (the text that will be replaced)
3. **For mentions**: call `self.search_mentions(mode, query, ...)` which returns `Task<Vec<Match>>`
4. Convert each `Match` into a `Completion` struct:
   - `replace_range`: buffer anchors for the `@...` text
   - `new_text`: the inserted markdown link, e.g. `[@main.rs](file:///path/to/main.rs)`
   - `label`: a `CodeLabel` (file name + truncated directory path)
   - `icon_path`: file/symbol/thread icon
   - `confirm`: callback invoked on accept (creates the crease)
   - `group`: optional `CompletionGroup` for section headers ("Recent", "Context")
5. Wrap in `CompletionResponse` with `dynamic_width: true`

---

## 4. Search & Filtering

### 4a. File Search (`search_files()`)

**Empty query** (`@file `):
- Returns recent navigation history (last 10 paths)
- Returns all visible worktree entries (files and dirs)
- Recent items are marked `is_recent = true` and sorted first

**Non-empty query** (`@file main`):
- Builds `PathMatchCandidateSet` for each visible worktree
- Calls `fuzzy::match_path_sets()` which performs sub-path fuzzy matching
- Parameters: `smart_case: false`, `max_results: 100`, `cancellation_flag`
- Path format: `prefix/path` where `prefix` is the worktree root name (if >1 worktree)

### 4b. Symbol Search (`search_symbols()`)

- Calls `project.symbols(&query)` → LSP workspace/symbol request
- Filters with `fuzzy::match_strings()` on each symbol's filter text
- Splits into **in-project** vs **external** symbols; in-project shown first
- Max 100 matches total
- For Rust path-style queries (`::`), only the last segment is matched

### 4c. Session/Thread Search

- Reads `ThreadMetadataStore` for recent non-archived threads
- Sorted by `updated_at` descending
- Fuzzy-matched on thread titles

### 4d. Skill Search

- `fuzzy::match_strings()` on available skill names from the delegate

### 4e. Bare `@` (No Mode, No Query)

Shows a combined view:
1. **Recent entries**: last 4 navigation history items, current thread, last 2 threads — excluding already-mentioned items
2. **Mode entries**: `@file`, `@symbol`, `@thread`, `@skill`, `@fetch`, `@selection` (if applicable), `@diagnostics` (if errors/warnings), `@branch diff` (if supported)
3. Ordered: recent → context entries → branch diff

### 4f. Bare `@` (No Mode, With Query)

Fuzzy-string-filters the mode entry keywords (`"file"`, `"symbol"`, etc.) and merges results with file search results. Sorted by score descending.

---

## 5. Completion Construction

Each search `Match` variant becomes a `Completion` struct:

### 5a. The `Completion` Struct

```rust
struct Completion {
    replace_range: Range<Anchor>,     // Text to replace on accept
    new_text: String,                  // Inserted text (markdown link)
    label: CodeLabel,                 // Rendered label (file name + directory)
    icon_path: Option<SharedString>,  // Icon in the menu
    documentation: Option<CompletionDocumentation>,
    confirm: Option<ConfirmCallback>, // Invoked on accept
    group: Option<CompletionGroup>,   // Section header grouping
    insert_text_mode: Option<...>,
    snippet_deduplication_key: Option<...>,
    // ...
}
```

### 5b. `new_text` Format — Markdown Links via `MentionUri`

When a completion is accepted, the `@...` text is replaced with a markdown link:

```rust
impl MentionUri {
    fn as_link(&self) -> MentionLink { MentionLink(self) }
}
impl fmt::Display for MentionLink {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "[@{}]({})", self.0.name(), self.0.to_uri())
    }
}
```

For example: `[@main.rs](file:///Users/juju/Develop/mini-pi/src/main.rs)`

The URI schemes used:
- `file:///abs/path` — files
- `file:///abs/path/` — directories (trailing slash)
- `file:///abs/path?symbol=Name#L10:20` — symbols
- `zed:///agent/thread/{id}?name=...` — threads
- `zed:///agent/skill?name=...&source=...&path=...` — skills
- `http(s)://...` — fetch URLs
- `zed:///agent/diagnostics?...` — diagnostics
- `zed:///agent/git-diff?base=main` — branch diff

### 5c. `label` Format — `CodeLabel` with Truncation

```rust
fn build_code_label_for_path(
    file: &str,
    directory: Option<&str>,
    line_number: Option<u32>,
    label_max_chars: usize,
    cx: &App,
) -> CodeLabel {
    let mut label = CodeLabelBuilder::default();
    label.push_str(file, None);
    label.push_str(" ", None);
    if let Some(dir) = directory {
        // Truncate directory from the front to fit within available space
        let dir_max = label_max_chars.saturating_sub(file.chars().count() + 1);
        let truncated = truncate_and_remove_front(dir, dir_max.max(5));
        label.push_str(&truncated, variable_highlight_id);
    }
    if let Some(line) = line_number {
        label.push_str(&format!(" L{}", line), variable_highlight_id);
    }
    label.build()
}
```

The `label_max_chars` is computed from:
- `COMPLETION_MENU_MAX_WIDTH` minus padding (3 × `DynamicSpacing::Base06`) minus icon width
- Divided by `em_width` for the current font (TextSize::Small) at the current `rem_size`

### 5d. Icons

Each match type gets an icon via `MentionUri::icon_path(cx)`:
- `File` → resolved by `FileIcons::get_icon(abs_path, cx)` (respects file type)
- `Directory` → `FileIcons::get_folder_icon(false, abs_path, cx)`
- `Symbol` → `IconName::Code`
- `Thread` → `IconName::Thread`
- `Skill` → `IconName::Sparkle`
- `Fetch` → `IconName::ToolWeb`
- `Diagnostics` → `IconName::Warning`
- Recent items override with `IconName::HistoryRerun`

### 5e. Section Headers (Groups)

When the user types a bare `@` (no mode, no query), results are grouped:
```rust
CompletionGroup { key: "recent".into(), label: None }    // Recent files/threads
CompletionGroup { key: "context".into(), label: None }   // Mode entries
```

The editor renders group separators between sections.

### 5f. The `confirm` Callback

```rust
type ConfirmCallback = Arc<dyn Fn(CompletionIntent, &mut Window, &mut App) -> bool + Send + Sync>;

fn confirm_completion_callback(...) -> ConfirmCallback {
    Arc::new(move |_, window, cx| {
        window.defer(cx, move |window, cx| {
            mention_set.update(cx, |mention_set, cx| {
                mention_set.confirm_mention_completion(
                    crease_text, start, content_len, mention_uri,
                    supports_images, editor, workspace, window, cx,
                ).detach();
            });
        });
        false  // Don't keep the menu open
    })
}
```

Key details:
- `window.defer()` is used to avoid modifying the editor during the completion acceptance handshake
- Returns `false` → the completion menu closes after selection
- For mode entries (e.g. selecting `@file`), returns `true` → menu stays open for the next query

---

## 6. Confirmation & the MentionSet

### 6a. `MentionSet::confirm_mention_completion()`

1. Compute buffer `Anchor`s for the inserted text range
2. Call `insert_crease_for_mention()` to create a **crease** (foldable inline span)
3. Create a `MentionTask` (async task) to load the actual content:
   - `File` → open the buffer in the project, read full text
   - `Directory` → returns `Mention::Link` immediately (then lazily expands)
   - `Symbol` → open the buffer, read the line range
   - `Skill` → read SKILL.md from disk (or built-in content)
   - `Thread` → fetch via native agent server
   - `Fetch` → HTTP GET the URL, convert HTML to Markdown
   - `Diagnostics` → collect from the project
   - `GitDiff` → `git diff {base_ref}` from active repository
4. Store in `self.mentions: HashMap<CreaseId, (MentionUri, MentionTask)>`
5. Run **disambiguation**: if multiple mentions share the same name, apply `util::disambiguate::compute_disambiguation_details()` to append parent path components

### 6b. The `insert_crease_for_mention()` Function

```rust
fn insert_crease_for_mention(
    anchor: Anchor, content_len: usize,
    crease_label: SharedString, crease_icon: SharedString,
    crease_tooltip: Option<SharedString>,
    mention_uri: Option<MentionUri>,
    workspace: Option<WeakEntity<Workspace>>,
    image: Option<Shared<Task<Result<Arc<Image>, String>>>>,
    editor: Entity<Editor>,
    window: &mut Window, cx: &mut App,
) -> Option<(CreaseId, postage::barrier::Sender, Option<Entity<LoadingContext>>)>
```

This function:

1. Creates a `postage::barrier` channel — the sender is held until loading completes
2. Creates a `FoldPlaceholder` whose `render` closure returns a `LoadingContext` entity
3. Creates a `Crease::Inline` with the placeholder, metadata (label + icon), and the buffer range
4. Calls `editor.insert_creases(vec![crease])` and `editor.fold_creases(vec![crease], false, ...)`
5. Returns the `CreaseId`, barrier sender, and `LoadingContext` entity

**The barrier pattern**: `render_mention_fold_button` spawns a task that `await`s `loading_finished.recv()`. When the sender is dropped (content loaded or failed), the task clears `self.loading` and calls `cx.notify()` — which stops the pulsating animation.

### 6c. `MentionTask` — Shared Async Tasks

```rust
type MentionTask = Shared<Task<Result<Mention, String>>>;
```

Using `Task::shared()` allows multiple consumers (e.g., the crease renderer and the content assembler) to observe the same result without re-executing.

### 6d. Error Handling

If mention loading fails:
1. The barrier sender is dropped → loading animation stops but the crease stays
2. `window.defer` is used to edit the editor and **remove the failed mention text**
3. The crease and mention entries are removed from `MentionSet`

---

## 7. GPUI UI: The `MentionCrease` Component

### 7a. The Struct

```rust
#[derive(IntoElement)]
pub struct MentionCrease {
    id: ElementId,
    icon: SharedString,
    label: SharedString,
    mention_uri: Option<MentionUri>,
    workspace: Option<WeakEntity<Workspace>>,
    is_toggled: bool,
    is_loading: bool,
    tooltip: Option<SharedString>,
    image_preview: Option<Box<dyn Fn(&mut Window, &mut App) -> AnyView + 'static>>,
}
```

`#[derive(IntoElement)]` auto-implements `IntoElement`, allowing the struct to be used directly as a child of `div()` etc.

### 7b. `RenderOnce` Implementation — The Visual Structure

```
┌─────────────────────────────────────────┐
│  ButtonLike (Outlined, Compact)         │
│  ┌───┬──────────────────────────────┐   │
│  │   │ h_flex (gap-1)               │   │
│  │ 🖹 │ Icon (XSmall, Muted)        │   │
│  │   │ Label text (buffer_font)     │◄──│ Pulsating opacity when loading
│  └───┴──────────────────────────────┘   │
│  + Tooltip (absolute path) / ImageHover │
└─────────────────────────────────────────┘
```

Key GPUI styling calls:
```rust
ButtonLike::new(self.id)
    .style(ButtonStyle::Outlined)
    .size(ButtonSize::Compact)
    .height(DefiniteLength::Absolute(AbsoluteLength::Pixels(
        px(window.line_height().into()) - px(1.),
    )))
    .selected_style(ButtonStyle::Tinted(TintColor::Accent))
    .toggle_state(self.is_toggled)
    .on_click(move |_event, window, cx| {
        open_mention_uri(mention_uri.clone(), &workspace, window, cx);
    })
```

The crease height matches the editor line height minus 1px, ensuring it sits flush within the text.

### 7c. Pulsating Loading Animation

```rust
// In LoadingContext::render():
if is_loading {
    this.with_animation(
        "loading-context-crease",
        Animation::new(Duration::from_secs(2))
            .repeat()
            .with_easing(pulsating_between(0.4, 0.8)),
        |label, delta| label.opacity(delta),
    )
    .into_any()
}
```

When `is_loading` is `true`, the crease's opacity oscillates between 0.4 and 0.8 over a 2-second cycle. The animation stops when the barrier sender is dropped.

### 7d. Tooltips and Image Previews

- **Standard tooltip**: `Tooltip::text(absolute_path)` shown on hover
- **Image hover** (for raster images): a custom `ImageHover` element rendered in a `hoverable_tooltip`, showing the image at `max_w_96` with rounded corners, behind an `elevation_2` shadow

### 7e. Click-to-Open

```rust
fn open_mention_uri(mention_uri, workspace, window, cx) {
    match mention_uri {
        File { abs_path }      → workspace.open_abs_path(abs_path)
        Directory { abs_path } → emit RevealInProjectPanel event
        Symbol { abs_path, line_range, .. }
            → workspace.open_abs_path(abs_path).at(line_range.start)
        Thread { id, name }    → panel.open_thread(id, name)
        Skill { skill_file_path }
            → open built-in content in read-only buffer, or open_abs_path
        Fetch { url }          → cx.open_url(url)
        // ...
    }
}
```

### 7f. Selection State

The crease tracks whether it's within the current text selection:
```rust
let is_in_text_selection = editor.is_range_selected(&fold_range, cx).unwrap_or_default();
MentionCrease::new(id, icon, label).is_toggled(is_in_text_selection)...
```

When toggled (inside a text selection), the button gets `Tinted(TintColor::Accent)` styling.

---

## 8. GPUI Wiring: Entity Lifecycle

### 8a. `MentionSet` as an Entity

```rust
pub struct MentionSet {
    project: WeakEntity<Project>,
    thread_store: Option<Entity<ThreadStore>>,
    mentions: HashMap<CreaseId, (MentionUri, MentionTask)>,
    crease_entities: HashMap<CreaseId, Entity<LoadingContext>>,
}
```

The `MentionSet` is:
- Created in `MessageEditor::new()` with `cx.new(|_cx| MentionSet::new(project, thread_store))`
- Passed to `PromptCompletionProvider::new(..., mention_set.clone(), ...)`
- Updated on every `EditorEvent::Edited` to call `remove_invalid()` (cleans up stale creases)

### 8b. Subscriptions

In `MessageEditor::new()`:
```rust
cx.subscribe_in(&editor, window, move |this, editor, event, window, cx| {
    if let EditorEvent::Edited { .. } = event {
        let snapshot = editor.snapshot(window, cx);
        this.mention_set.update(cx, |set, _cx| set.remove_invalid(&snapshot));
    }
})
```

### 8c. `LoadingContext` — Long-Lived Render Entity

`LoadingContext` is an `Entity` (not just `RenderOnce`) because it needs to:
1. Exist for the duration of the async load
2. Be notified when loading completes (via `cx.notify()`)
3. Be re-rendered on state change (loading → done)

Its `Render` impl conditionally applies the pulsating animation based on `self.loading.is_some()`.

### 8d. Content Assembly

When the LLM prompt is assembled, `MentionSet::contents()` awaits all `MentionTask`s concurrently, then formats each resolved mention into the prompt text using code-block fences, headings, etc.

---

## 9. Reimplementation Checklist

### Step 1: Define Your Mention URI Types

Create an enum representing each context type you want to support:
```rust
enum MyMentionUri {
    File { path: PathBuf },
    Directory { path: PathBuf },
    // ... add your types
}
```
Implement `name()`, `to_link_text()`, `icon()`, `tooltip()`.

### Step 2: Implement the Parser

Write a function that:
- Scans the current line for `@` with word-boundary rules
- Extracts `@<mode> <query>` or bare `@<query>`
- Computes the `source_range` (byte offsets) that will be replaced

### Step 3: Implement the CompletionProvider

If your platform has a `CompletionProvider` trait, implement:
- `is_completion_trigger()` → call your parser
- `completions()` → search + build Completion objects
- `sort_completions()` → `false` (do it yourself)
- `filter_completions()` → `false` (do it yourself)

If there's no existing framework, build a custom popover:
- Position it at the cursor using text metrics
- Show a list of items with icons, labels, and documentation
- Handle keyboard navigation and mouse click

### Step 4: Implement Search Backends

- **Files**: Use a fuzzy path matcher. For empty queries, show recent + all entries. For non-empty, filter with fuzzy matching.
- **Symbols**: If you have an LSP, use workspace/symbol. Fall back to file content scanning.
- **Other types**: Fuzzy-match against your available items.

### Step 5: Build the Crease UI Component

After a mention is confirmed:
1. Replace the `@...` text with a compact inline representation (a "chip" or "pill")
2. Show icon + label with truncated path info
3. Animate loading state (pulsating opacity)
4. Show tooltip with full path on hover
5. Make it clickable to navigate to the referenced item
6. Handle disambiguation when multiple mentions share names

### Step 6: Wire Into Your Editor

- Register the completion provider on the text input
- Subscribe to edit events to clean up invalid mentions
- Handle paste of mention links (re-create creases from link text)
- Assemble all mention content when sending the prompt to the LLM

### Step 7: Handle Loading and Error States

- Use shared async tasks so multiple views can observe the same result
- Show loading animation while content is being fetched
- On failure: remove the invalid mention text from the buffer and show an error notification
- For directories: defer full content loading until prompt assembly time
