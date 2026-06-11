# Markdown Rendering Improvement Plan

## Current Architecture

The file implements a markdown → GPUI rendering pipeline in 3 passes:

| Pass | Function | What it does |
|------|----------|-------------|
| Parse | `parse_markdown()` | pulldown-cmark events → custom AST (`BlockNode`/`InlineNode` enums) |
| Collect | `collect_styled_inlines()` | Inline AST → flat `(String, Vec<(Range, HighlightStyle)>)` for GPUI's `StyledText` |
| Render | `render_blocks()` | Block AST → GPUI elements (`div()`, `StyledText`, etc.) |

The only consumer is `chat_window.rs`, which creates `Entity<MarkdownRenderer>` per assistant text part and calls `set_content()` on every render.

---

## Issues Found

### Bugs

1. **Table cells lose inline formatting** (`markdown.rs:868`) — table cells use `render_inlines_text()` (plain text) instead of `render_styled_inlines()` (GPUI highlights). Bold, italic, links, and code formatting inside table cells are silently dropped.

2. **Image `alt` field stores the URL, not the alt text** (`markdown.rs:184-186`) — the `Image` variant copies `dest_url` into the `alt` field, losing the actual alt text from the markdown.

3. **List `start` offset always `None`** (`markdown.rs:229-232`) — ordered list start numbers are discarded.

4. **Known memory leak** (documented in `docs/design-review.md:40`) — `markdown_displays` in `chat_window.rs` grows monotonically; old entities are never released.

### Missing Features

| Priority | Feature | Effort |
|----------|---------|--------|
| High | **Syntax highlighting** for code blocks (add `syntect` or `tree-sitter-highlight`) | Medium |
| High | **Clickable links** — links are styled but have no `on_click` handler | Low |
| High | **Copy button** on code blocks | Low |
| Medium | **Horizontal scroll** for wide code blocks (currently they wrap) | Low |
| Medium | **Math/LaTeX** rendering for `$...$` and `$$...$$` blocks | High |
| Medium | **Mermaid/diagram** fenced code blocks (` ```mermaid`) | High |
| Low | **Diff rendering** for ` ```diff` code blocks | Medium |
| Low | **Nested blockquote** visual differentiation | Low |
| Low | **Code block line numbers** | Low |

### Design / Performance

- **Parse on every frame** — `parse_markdown()` is called inside `render()`. For long messages, this does redundant work every frame. Should memoize or cache the parsed AST.
- **Hardcoded colors** — all 10+ color values are raw `rgb()` literals. No dark/light theme support.
- **`strip_html_tags` is naive** — char-by-char filtering doesn't handle nested tags, entities (`&amp;`), or self-closing tags properly.
- **Unused intermediate AST** — the full `BlockNode`/`InlineNode` tree is built only to be immediately walked in `render_blocks()`. Could render directly from pulldown-cmark events to save allocations.
- **`render_inlines_text` recursion** — allocates a new `String` per nesting level. Could use a single buffer.

---

## Implementation Phases

### Phase 1: Quick wins (bugs + low effort)

1. **Fix table cell formatting** — replace `render_inlines_text(cell)` with `render_styled_inlines(cell)` in `render_table_row` (line 868)
2. **Fix image alt/url mixup** — store `alt_text` in the `Image` variant, URL separately
3. **Fix list start offset** — pass `start` from `BlockContext::List` through to `BlockNode::List` (line 231)
4. **Add link click handler** — wrap link text with `on_click(cx.listener(...))` that opens the URL

### Phase 2: Syntax highlighting

5. **Add `syntect` dependency** to `Cargo.toml`
6. **Build a `SyntaxHighlighter` service** that tokenizes code by language and produces `Vec<(String, HighlightStyle)>` per line
7. **Replace plain code block rendering** with tokenized, colorized line rendering in `BlockNode::CodeBlock`

### Phase 3: Code block enhancements

8. **Add copy-to-clipboard button** in the code block header bar
9. **Add horizontal scrolling** via GPUI `overflow_x_scroll()` on the code block container
10. **Optionally add line numbers**

### Phase 4: Performance + design

11. **Memoize parsed AST** — store `Vec<BlockNode>` in `MarkdownRenderer` state, re-parse only if content changed
12. **Extract color constants** — define a `MarkdownTheme` struct with named color fields, pass to render functions
13. **Fix memory leak** in `chat_window.rs` — truncate `markdown_displays` when messages are removed

### Phase 5: Extended features (higher effort)

14. **Math rendering** via `katex-rs` or server-side rendering
15. **Mermaid support** via mermaid.js rendered to SVG in a background process
16. **Diff syntax highlighting** with +/- line backgrounds

---

## Test Examples

See `examples/markdown_test.md` for a comprehensive test document covering all features.
