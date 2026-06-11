# GPUI in Action — Example-Based Learning Guide

> A progressive journey through GPUI, from "Hello World" to multi-window apps. Each example builds on the last, introducing new concepts with fully compilable code.

---

## Opening: What You'll Learn

GPUI is not like other UI frameworks. It doesn't have a virtual DOM, a diffing algorithm, or a reactive signals system. Instead, it offers a **hybrid immediate-retained model** built around a handful of simple, composable ideas. Once you understand these, everything else falls into place.

### The GPUI Mental Model in One Paragraph

Every GPUI app has an `App` — a single owner of all state. Your data lives in **entities** (`Entity<T>`), which are like `Rc<T>` but borrow from the `App`. To show something on screen, you implement `Render` on an entity — that makes it a **view**. In `render()`, you build an **element tree** using a Tailwind-inspired builder API (`div().flex().bg(...).child(...)`). Elements go through three phases each frame: layout → hitbox → paint. When state changes, you call `cx.notify()` and GPUI rebuilds the element tree. That's the whole loop.

### The 5 Patterns That Repeat in Every GPUI App

Every example in this guide is a variation of five fundamental patterns:

| # | Pattern | What It Answers |
|---|---------|-----------------|
| 1 | **Render** | "How do I put pixels on screen?" — `impl Render`, element builders, styling |
| 2 | **Update & Notify** | "How does the UI react to changes?" — `entity.update(cx, ...)`, `cx.notify()` |
| 3 | **Handle Events** | "How do users interact?" — `cx.listener()`, `on_click`, `on_action` |
| 4 | **Async Work** | "How do I do slow things without freezing?" — `cx.spawn()`, `Task`, background executor |
| 5 | **Communicate** | "How do parts of my app talk to each other?" — `observe`, `subscribe`/`emit`, `Global` |

### How to Use This Guide

- **New to GPUI?** Start at example 1 and go in order. Each example introduces exactly one or two new concepts.
- **Solving a specific problem?** Jump to the example that matches your need — the table below tells you what each covers.
- **Every code block is a complete `main.rs`.** Copy it, paste it, run it with `cargo run`. There are no hidden dependencies or scaffolding.

---

## Table of Contents

1. [Hello, GPUI!](#1-hello-gpui) — Minimal app, Render trait, window
2. [Counter](#2-counter) — State mutation, click events, `cx.notify()`
3. [Temperature Converter](#3-temperature-converter) — Text input, two-way binding, number parsing
4. [Stopwatch](#4-stopwatch) — Async tasks, background timers, elapsed time
5. [Theme Switcher](#5-theme-switcher) — Globals, dynamic styling, dropdown menu
6. [Todo List](#6-todo-list) — Actions, key bindings, list management, modals
7. [Image Gallery](#7-image-gallery) — Asset loading, grid layout, image cache
8. [Draggable Kanban Board](#8-draggable-kanban-board) — Drag & drop, custom drag state, columns
9. [Split Pane](#9-split-pane) — Custom Element, resize handles, proportional layout
10. [Chat Simulator](#10-chat-simulator) — Virtual list, subscriptions, event emitters, async messages
11. [Markdown Previewer](#11-markdown-previewer) — Text editing, live preview, background compute, two-pane
12. [Multi-Window Notes](#12-multi-window-notes) — Multiple windows, cross-window entity sharing

---

## 1. Hello, GPUI!

**Concepts:** `Application`, `Render`, `div`, basic styling, window creation

```rust
use gpui::*;
use gpui_platform::application;

struct Root {
    greeting: SharedString,
}

impl Render for Root {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .bg(rgb(0x0d1117))
            .text_color(rgb(0xe6edf3))
            .font_family("System-ui")
            .child(
                div()
                    .text_4xl()
                    .font_weight(FontWeight::BOLD)
                    .mb_4()
                    .child(self.greeting.clone()),
            )
            .child(
                div()
                    .text_lg()
                    .text_color(rgb(0x8b949e))
                    .child("Welcome to GPUI — a GPU-accelerated UI framework for Rust."),
            )
    }
}

fn main() {
    application().run(|cx: &mut App| {
        cx.activate(true);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(800.0), px(500.0)),
                    cx,
                ))),
                ..Default::default()
            },
            |_, cx| cx.new(|_| Root { greeting: "Hello, GPUI!".into() }),
        )
        .unwrap();
    });
}
```

**What's happening:** `Application::run` starts the event loop. Inside, we open a window whose root view implements `Render`. The `render` method returns a `div` element tree built with a Tailwind-inspired API.

---

## 2. Counter

**Concepts:** `Entity`, `cx.update()`, `cx.notify()`, `on_click`, `cx.listener`

```rust
use gpui::*;
use gpui_platform::application;

struct Counter {
    count: i32,
}

impl Counter {
    fn increment(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.count += 1;
        cx.notify();
    }

    fn decrement(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.count -= 1;
        cx.notify();
    }
}

impl Render for Counter {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_6()
            .bg(rgb(0x0d1117))
            .child(
                div()
                    .text_6xl()
                    .font_weight(FontWeight::BOLD)
                    .text_color(if self.count >= 0 { rgb(0x58a6ff) } else { rgb(0xf85149) })
                    .child(SharedString::from(format!("{}", self.count))),
            )
            .child(
                div().flex().gap_4().child(
                    div()
                        .px_6()
                        .py_3()
                        .bg(rgb(0x238636))
                        .text_color(rgb(0xffffff))
                        .rounded_lg()
                        .cursor(CursorStyle::PointingHand)
                        .text_lg()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("+")
                        .on_click(cx.listener(Self::increment)),
                ).child(
                    div()
                        .px_6()
                        .py_3()
                        .bg(rgb(0xda3633))
                        .text_color(rgb(0xffffff))
                        .rounded_lg()
                        .cursor(CursorStyle::PointingHand)
                        .text_lg()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("−")
                        .on_click(cx.listener(Self::decrement)),
                ),
            )
    }
}

fn main() {
    application().run(|cx: &mut App| {
        cx.activate(true);
        cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|_| Counter { count: 0 })
        })
        .unwrap();
    });
}
```

**Key points:**
- `cx.listener(Self::method)` routes clicks to the entity's methods
- `cx.notify()` tells GPUI the view is dirty and must re-render
- Dynamic styling via `if self.count >= 0 { ... } else { ... }`
- `SharedString::from(format!(...))` for dynamic text

---

## 3. Temperature Converter

**Concepts:** Text input with `EntityInputHandler`, real-time conversion, error handling

```rust
use gpui::*;
use gpui_platform::application;

struct Converter {
    celsius_input: String,
    fahrenheit_input: String,
    editing_field: Field,
    focus_handle: FocusHandle,
}

#[derive(PartialEq)]
enum Field {
    Celsius,
    Fahrenheit,
}

impl Converter {
    fn celsius_to_fahrenheit(c: f64) -> f64 {
        c * 9.0 / 5.0 + 32.0
    }

    fn fahrenheit_to_celsius(f: f64) -> f64 {
        (f - 32.0) * 5.0 / 9.0
    }

    fn update_from_celsius(&mut self) {
        if let Ok(c) = self.celsius_input.trim().parse::<f64>() {
            self.fahrenheit_input = format!("{:.1}", Self::celsius_to_fahrenheit(c));
        } else {
            self.fahrenheit_input.clear();
        }
    }

    fn update_from_fahrenheit(&mut self) {
        if let Ok(f) = self.fahrenheit_input.trim().parse::<f64>() {
            self.celsius_input = format!("{:.1}", Self::fahrenheit_to_celsius(f));
        } else {
            self.celsius_input.clear();
        }
    }
}

impl Render for Converter {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_8()
            .bg(rgb(0x0d1117))
            .child(
                div()
                    .text_3xl()
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(0xe6edf3))
                    .child("Temperature Converter"),
            )
            .child(
                div().flex().gap_8().items_center().child(
                    self.render_field(
                        "Celsius",
                        &self.celsius_input,
                        Field::Celsius,
                        cx,
                    ),
                ).child(
                    div()
                        .text_4xl()
                        .text_color(rgb(0x8b949e))
                        .child("⇄"),
                ).child(
                    self.render_field(
                        "Fahrenheit",
                        &self.fahrenheit_input,
                        Field::Fahrenheit,
                        cx,
                    ),
                ),
            )
    }
}

impl Converter {
    fn render_field(
        &self,
        label: &'static str,
        value: &str,
        field: Field,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_editing = self.editing_field == field;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .w(px(200.))
            .child(div().text_sm().text_color(rgb(0x8b949e)).child(label))
            .child(
                div()
                    .px_4()
                    .py_3()
                    .bg(rgb(0x161b22))
                    .border_1()
                    .border_color(if is_editing { rgb(0x58a6ff) } else { rgb(0x30363d) })
                    .rounded_md()
                    .text_2xl()
                    .text_color(rgb(0xe6edf3))
                    .font_family("System-ui")
                    .track_focus(&self.focus_handle)
                    .when_some(
                        if !value.is_empty() { Some(value) } else { None },
                        |this, v| this.child(v.to_string()),
                    )
                    .when(value.is_empty() && !is_editing, |this| {
                        this.text_color(rgb(0x484f58)).child("0.0")
                    })
                    .on_click({
                        let field = field;
                        cx.listener(move |this, _: &ClickEvent, _: &mut Window, cx| {
                            this.editing_field = field;
                            cx.notify();
                        })
                    }),
            )
    }
}

impl Focusable for Converter {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EntityInputHandler for Converter {
    fn text_for_editing(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> String {
        match self.editing_field {
            Field::Celsius => self.celsius_input.clone(),
            Field::Fahrenheit => self.fahrenheit_input.clone(),
        }
    }

    fn replace_text_in_range(
        &mut self,
        _range: Option<std::ops::Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.editing_field {
            Field::Celsius => {
                self.celsius_input.push_str(text);
                self.update_from_celsius();
            }
            Field::Fahrenheit => {
                self.fahrenheit_input.push_str(text);
                self.update_from_fahrenheit();
            }
        }
        cx.notify();
    }

    fn backspace(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.editing_field {
            Field::Celsius => { self.celsius_input.pop(); self.update_from_celsius(); }
            Field::Fahrenheit => { self.fahrenheit_input.pop(); self.update_from_fahrenheit(); }
        }
        cx.notify();
    }

    fn enter(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        // Reset both fields on Enter
        self.celsius_input.clear();
        self.fahrenheit_input.clear();
        cx.notify();
    }
}

fn main() {
    application().run(|cx: &mut App| {
        cx.activate(true);
        cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|_| Converter {
                celsius_input: String::new(),
                fahrenheit_input: String::new(),
                editing_field: Field::Celsius,
                focus_handle: cx.focus_handle(),
            })
        })
        .unwrap();
    });
}
```

**Key points:**
- `EntityInputHandler` captures raw keyboard input for text editing
- `track_focus` and `FocusHandle` manage keyboard focus
- `is_editing` state drives border highlighting
- Simple two-way conversion logic driven by text changes

---

## 4. Stopwatch

**Concepts:** Async tasks, `cx.spawn()`, background timers, `Task<()>`, elapsed display

```rust
use gpui::*;
use gpui_platform::application;
use std::time::{Duration, Instant};

struct Stopwatch {
    elapsed: Duration,
    state: WatchState,
    start_instant: Option<Instant>,
    _tick_task: Option<Task<()>>,
}

#[derive(PartialEq)]
enum WatchState {
    Idle,
    Running,
    Paused,
}

impl Stopwatch {
    fn toggle(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        match self.state {
            WatchState::Idle | WatchState::Paused => {
                self.state = WatchState::Running;
                if self.start_instant.is_none() {
                    self.start_instant = Some(Instant::now());
                }
                self.start_ticking(cx);
            }
            WatchState::Running => {
                self.state = WatchState::Paused;
                if let Some(start) = self.start_instant.take() {
                    self.elapsed += start.elapsed();
                }
                self._tick_task.take();
            }
        }
        cx.notify();
    }

    fn reset(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.elapsed = Duration::ZERO;
        self.state = WatchState::Idle;
        self.start_instant = None;
        self._tick_task.take();
        cx.notify();
    }

    fn start_ticking(&mut self, cx: &mut Context<Self>) {
        let start = Instant::now();
        let base_elapsed = self.elapsed;
        cx.spawn(|this: WeakEntity<Self>, mut cx: AsyncApp| async move {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                let updated = this.update(&mut cx, |this, cx| {
                    if this.state != WatchState::Running {
                        return;
                    }
                    this.elapsed = base_elapsed + start.elapsed();
                    cx.notify();
                });
                if updated.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn format_duration(d: Duration) -> String {
        let total_secs = d.as_secs();
        let hours = total_secs / 3600;
        let minutes = (total_secs % 3600) / 60;
        let seconds = total_secs % 60;
        let millis = d.subsec_millis();
        if hours > 0 {
            format!("{:02}:{:02}:{:02}.{:03}", hours, minutes, seconds, millis)
        } else {
            format!("{:02}:{:02}.{:03}", minutes, seconds, millis)
        }
    }
}

impl Render for Stopwatch {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let button_label = match self.state {
            WatchState::Idle | WatchState::Paused => "Start",
            WatchState::Running => "Pause",
        };
        let button_color = match self.state {
            WatchState::Idle | WatchState::Paused => rgb(0x238636),
            WatchState::Running => rgb(0xd29922),
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_6()
            .bg(rgb(0x0d1117))
            .child(
                div()
                    .text_6xl()
                    .font_weight(FontWeight::BOLD)
                    .font_family("SF Mono, Menlo, monospace")
                    .text_color(rgb(0xe6edf3))
                    .child(Self::format_duration(self.elapsed)),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(match self.state {
                        WatchState::Running => rgb(0x3fb950),
                        WatchState::Paused => rgb(0xd29922),
                        WatchState::Idle => rgb(0x8b949e),
                    })
                    .child(match self.state {
                        WatchState::Idle => "Ready",
                        WatchState::Running => "Running…",
                        WatchState::Paused => "Paused",
                    }),
            )
            .child(
                div().flex().gap_3().child(
                    div()
                        .px_6()
                        .py_3()
                        .bg(button_color)
                        .text_color(rgb(0xffffff))
                        .rounded_lg()
                        .cursor(CursorStyle::PointingHand)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(button_label)
                        .on_click(cx.listener(Self::toggle)),
                ).child(
                    div()
                        .px_6()
                        .py_3()
                        .bg(rgb(0x21262d))
                        .text_color(rgb(0x8b949e))
                        .rounded_lg()
                        .cursor(CursorStyle::PointingHand)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Reset")
                        .on_click(cx.listener(Self::reset)),
                ),
            )
    }
}

fn main() {
    application().run(|cx: &mut App| {
        cx.activate(true);
        cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|_| Stopwatch {
                elapsed: Duration::ZERO,
                state: WatchState::Idle,
                start_instant: None,
                _tick_task: None,
            })
        })
        .unwrap();
    });
}
```

**Key points:**
- `cx.spawn()` creates an async task that runs on the foreground
- `cx.background_executor().timer(dur)` creates a background delay
- `WeakEntity<Self>` in the async closure avoids holding a strong ref
- `this.update(&mut cx, |this, cx| ...)` accesses entity state from async context
- `Task<()>` stored in the entity to cancel ticking when paused/reset

---

## 5. Theme Switcher

**Concepts:** `Global`, `cx.set_global()`, `cx.update_global()`, `cx.observe_global()`, dropdown with `deferred`

```rust
use gpui::*;
use gpui_platform::application;

// --- Theme Global ---

struct Theme {
    name: &'static str,
    bg: Hsla,
    surface: Hsla,
    text: Hsla,
    accent: Hsla,
}

impl Global for Theme {}

const THEMES: &[Theme] = &[
    Theme { name: "Dark", bg: hsla(0., 0., 0.05, 1.), surface: hsla(0., 0., 0.09, 1.), text: hsla(0., 0., 0.95, 1.), accent: hsla(215., 0.9, 0.55, 1.) },
    Theme { name: "Light", bg: hsla(0., 0., 0.98, 1.), surface: hsla(0., 0., 0.93, 1.), text: hsla(0., 0., 0.1, 1.), accent: hsla(215., 0.8, 0.45, 1.) },
    Theme { name: "Nord", bg: hsla(220., 0.16, 0.22, 1.), surface: hsla(222., 0.16, 0.28, 1.), text: hsla(217., 0.14, 0.86, 1.), accent: hsla(193., 0.43, 0.67, 1.) },
    Theme { name: "Solarized", bg: hsla(192., 1., 0.11, 1.), surface: hsla(193., 1., 0.14, 1.), text: hsla(44., 0.87, 0.76, 1.), accent: hsla(68., 1., 0.48, 1.) },
];

// --- App View ---

struct AppView {
    dropdown_open: bool,
    _theme_sub: Subscription,
}

impl AppView {
    fn toggle_dropdown(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.dropdown_open = !self.dropdown_open;
        cx.notify();
    }

    fn select_theme(&mut self, index: usize, cx: &mut Context<Self>) {
        let selected = &THEMES[index];
        let bg = selected.bg;
        let surface = selected.surface;
        let text = selected.text;
        let accent = selected.accent;
        cx.update_global(|theme: &mut Theme, _cx| {
            theme.name = selected.name;
            theme.bg = bg;
            theme.surface = surface;
            theme.text = text;
            theme.accent = accent;
        });
        self.dropdown_open = false;
        cx.notify();
        cx.refresh_windows();
    }
}

impl Render for AppView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<Theme>();
        let bg = theme.bg;
        let surface = theme.surface;
        let text = theme.text;
        let accent = theme.accent;

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(bg)
            .text_color(text)
            .child(
                // Header bar
                div()
                    .flex()
                    .justify_end()
                    .p_4()
                    .child(
                        div()
                            .relative()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .px_4()
                                    .py_2()
                                    .bg(surface)
                                    .rounded_md()
                                    .border_1()
                                    .border_color(accent.opacity(0.4))
                                    .cursor(CursorStyle::PointingHand)
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(format!("🎨 {}", theme.name))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(text.opacity(0.5))
                                            .child("▼"),
                                    )
                                    .on_click(cx.listener(Self::toggle_dropdown)),
                            )
                            .when(self.dropdown_open, |parent| {
                                parent.child(
                                    deferred(
                                        div()
                                            .absolute()
                                            .top(px(40.))
                                            .right(px(0.))
                                            .w(px(180.))
                                            .bg(surface)
                                            .border_1()
                                            .border_color(accent.opacity(0.3))
                                            .rounded_md()
                                            .shadow_lg()
                                            .overflow_hidden()
                                            .z_index(100)
                                            .children(THEMES.iter().enumerate().map(|(i, t)| {
                                                div()
                                                    .px_4()
                                                    .py_2p5()
                                                    .text_sm()
                                                    .cursor(CursorStyle::PointingHand)
                                                    .when(t.name == theme.name, |s| {
                                                        s.bg(accent.opacity(0.15)).text_color(accent)
                                                    })
                                                    .hover(|s| s.bg(surface.hover()))
                                                    .child(t.name)
                                                    .on_click({
                                                        let i = i;
                                                        cx.listener(move |this, _: &ClickEvent, _window, cx| {
                                                            this.select_theme(i, cx)
                                                        })
                                                    })
                                            })),
                                    ),
                                )
                            }),
                    ),
            )
            .child(
                // Content area
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_6()
                    .child(
                        div()
                            .text_3xl()
                            .font_weight(FontWeight::BOLD)
                            .child("Theme Switcher"),
                    )
                    .child(
                        div()
                            .text_lg()
                            .text_color(text.opacity(0.6))
                            .child("Current theme:"),
                    )
                    .child(
                        div()
                            .px_4()
                            .py_2()
                            .bg(accent.opacity(0.12))
                            .text_color(accent)
                            .rounded_full()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_sm()
                            .child(theme.name),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_4()
                            .mt_4()
                            .child(self.color_swatch("Background", bg, text))
                            .child(self.color_swatch("Surface", surface, text))
                            .child(self.color_swatch("Text", text, text))
                            .child(self.color_swatch("Accent", accent, text)),
                    ),
            )
    }
}

impl AppView {
    fn color_swatch(&self, label: &str, color: Hsla, text: Hsla) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            .child(
                div()
                    .size(px(48.))
                    .bg(color)
                    .rounded_md()
                    .border_1()
                    .border_color(text.opacity(0.15)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(text.opacity(0.5))
                    .child(label),
            )
    }
}

fn main() {
    let default_theme = &THEMES[0];
    application().run(|cx: &mut App| {
        cx.set_global(Theme {
            name: default_theme.name,
            bg: default_theme.bg,
            surface: default_theme.surface,
            text: default_theme.text,
            accent: default_theme.accent,
        });
        cx.activate(true);
        cx.open_window(WindowOptions::default(), |_, cx| {
            let theme_sub = cx.observe_global::<Theme>(|this: &mut AppView, cx| {
                cx.notify();
            });
            cx.new(|_| AppView {
                dropdown_open: false,
                _theme_sub: theme_sub,
            })
        })
        .unwrap();
    });
}
```

**Key points:**
- `impl Global for Theme` makes the struct a global singleton
- `cx.set_global(...)` initializes it at app startup
- `cx.global::<T>()` reads it anywhere
- `cx.update_global(...)` mutates it
- `cx.observe_global::<T>(...)` reacts to changes (calls `cx.notify()`)
- `cx.refresh_windows()` forces all windows to repaint after a global change
- `deferred(...)` renders a dropdown lazily (only when `dropdown_open` is true)

---

## 6. Todo List

**Concepts:** Actions, key bindings, list of entities, `ParentElement.children()`, keyboard shortcuts

```rust
use gpui::*;
use gpui_platform::application;

actions!(todo_list, [NewTodo, ToggleTodo, DeleteTodo, ClearCompleted]);

struct TodoList {
    todos: Vec<TodoItem>,
    new_todo_text: SharedString,
    focus_handle: FocusHandle,
}

struct TodoItem {
    text: SharedString,
    completed: bool,
    id: usize,
}

impl TodoList {
    fn new_todo(&mut self, _: &NewTodo, _: &mut Window, cx: &mut Context<Self>) {
        let text = self.new_todo_text.trim();
        if text.is_empty() {
            return;
        }
        let id = self.todos.len();
        self.todos.push(TodoItem {
            text: SharedString::from(text),
            completed: false,
            id,
        });
        self.new_todo_text = SharedString::default();
        cx.notify();
    }

    fn reset_new_todo(&mut self) {
        self.new_todo_text = SharedString::default();
    }

    fn clear_completed(&mut self, _: &ClearCompleted, _: &mut Window, cx: &mut Context<Self>) {
        self.todos.retain(|t| !t.completed);
        cx.notify();
    }
}

impl Render for TodoList {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let remaining = self.todos.iter().filter(|t| !t.completed).count();

        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .bg(rgb(0x0d1117))
            .text_color(rgb(0xe6edf3))
            .child(
                div()
                    .w(px(520.))
                    .max_w_full()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .p_8()
                    .child(
                        div()
                            .text_2xl()
                            .font_weight(FontWeight::BOLD)
                            .mb_4()
                            .child("Todo List"),
                    )
                    .child(
                        // Input row
                        div()
                            .flex()
                            .gap_3()
                            .child(
                                div()
                                    .flex_1()
                                    .px_4()
                                    .py_2p5()
                                    .bg(rgb(0x161b22))
                                    .border_1()
                                    .border_color(rgb(0x30363d))
                                    .rounded_md()
                                    .text_sm()
                                    .track_focus(&self.focus_handle)
                                    .children(if self.new_todo_text.is_empty() {
                                        vec![
                                            div()
                                                .text_color(rgb(0x484f58))
                                                .child("What needs to be done? Press Enter to add…")
                                                .into_any_element(),
                                        ]
                                    } else {
                                        vec![
                                            div()
                                                .child(self.new_todo_text.clone())
                                                .into_any_element(),
                                        ]
                                    }),
                            )
                            .child(
                                div()
                                    .px_4()
                                    .py_2p5()
                                    .bg(rgb(0x238636))
                                    .text_color(rgb(0xffffff))
                                    .rounded_md()
                                    .cursor(CursorStyle::PointingHand)
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Add")
                                    .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                                        this.new_todo(&NewTodo, _window, cx)
                                    })),
                            ),
                    )
                    .child(
                        // Todo items
                        div()
                            .flex()
                            .flex_col()
                            .children(
                                self.todos
                                    .iter()
                                    .map(|item| self.render_todo_item(item, cx)),
                            ),
                    )
                    .when(!self.todos.is_empty(), |parent| {
                        parent.child(
                            div()
                                .flex()
                                .justify_between()
                                .items_center()
                                .pt_3()
                                .border_t_1()
                                .border_color(rgb(0x21262d))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(0x8b949e))
                                        .child(format!(
                                            "{} item{} remaining",
                                            remaining,
                                            if remaining == 1 { "" } else { "s" },
                                        )),
                                )
                                .when(self.todos.iter().any(|t| t.completed), |parent| {
                                    parent.child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(0xf85149))
                                            .cursor(CursorStyle::PointingHand)
                                            .child("Clear completed")
                                            .on_click(cx.listener(Self::clear_completed)),
                                    )
                                }),
                        )
                    })
                    .child(
                        // Keyboard shortcuts hint
                        div()
                            .mt_4()
                            .text_xs()
                            .text_color(rgb(0x484f58))
                            .child("Enter to add · Space to toggle · Backspace to delete"),
                    ),
            )
    }
}

impl TodoList {
    fn render_todo_item(
        &self,
        item: &TodoItem,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let item_id = item.id;
        let is_completed = item.completed;

        div()
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .py_2p5()
            .border_1()
            .border_color(rgb(0x21262d))
            .rounded_md()
            .mb_1()
            .cursor(CursorStyle::PointingHand)
            .when(is_completed, |s| s.bg(rgb(0x161b22)))
            .child(
                // Checkbox circle
                div()
                    .size(px(18.))
                    .rounded_full()
                    .border_1()
                    .border_color(if is_completed { rgb(0x3fb950) } else { rgb(0x30363d) })
                    .when(is_completed, |s| {
                        s.bg(rgb(0x3fb950)).flex().items_center().justify_center()
                            .child(div().text_xxs().text_color(rgb(0xffffff)).child("✓"))
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .when(is_completed, |s| {
                        s.text_color(rgb(0x484f58))
                    })
                    .child(item.text.clone()),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(0x484f58))
                    .cursor(CursorStyle::PointingHand)
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .hover(|s| s.bg(rgb(0x21262d)))
                    .child("×")
                    .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                        this.todos.retain(|t| t.id != item_id);
                        cx.notify();
                    })),
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                if let Some(todo) = this.todos.iter_mut().find(|t| t.id == item_id) {
                    todo.completed = !todo.completed;
                }
                cx.notify();
            })),
    }
}

impl Focusable for TodoList {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EntityInputHandler for TodoList {
    fn text_for_editing(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> String {
        self.new_todo_text.to_string()
    }

    fn replace_text_in_range(
        &mut self,
        _range: Option<std::ops::Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.new_todo_text = SharedString::from(format!("{}{}", self.new_todo_text, text));
        cx.notify();
    }

    fn backspace(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let mut s = self.new_todo_text.to_string();
        s.pop();
        self.new_todo_text = SharedString::from(s);
        cx.notify();
    }

    fn enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.new_todo(&NewTodo, window, cx);
    }
}

fn main() {
    application().run(|cx: &mut App| {
        cx.bind_keys([KeyBinding::new("enter", todo_list::NewTodo, None)]);
        cx.activate(true);
        cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|_| TodoList {
                todos: Vec::new(),
                new_todo_text: SharedString::default(),
                focus_handle: cx.focus_handle(),
            })
        })
        .unwrap();
    });
}
```

**Key points:**
- `actions!()` declares named actions for keyboard bindings
- `cx.bind_keys([...])` maps keystrokes to actions
- `EntityInputHandler.enter()` fires when the user presses Enter
- `ParentElement.children()` renders a dynamic list of items
- Closure capture with `let item_id = item.id` + `move` for list item handlers

---

## 7. Image Gallery

**Concepts:** Asset source, `img`, `ObjectFit`, grid layout with `grid_cols_3`, image loading

```rust
use gpui::*;
use gpui_platform::application;
use std::borrow::Cow;

struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        std::fs::read(path).map(|d| Some(Cow::Owned(d))).map_err(Into::into)
    }

    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(std::fs::read_dir(path)?
            .filter_map(|e| {
                Some(SharedString::from(
                    e.ok()?.path().to_string_lossy().into_owned(),
                ))
            })
            .collect())
    }
}

struct Gallery {
    selected_index: Option<usize>,
    image_paths: Vec<SharedString>,
}

impl Gallery {
    fn select(&mut self, index: usize, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.selected_index = if self.selected_index == Some(index) {
            None
        } else {
            Some(index)
        };
        cx.notify();
    }
}

impl Render for Gallery {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x0d1117))
            .text_color(rgb(0xe6edf3))
            .child(
                // Header
                div()
                    .px_6()
                    .py_4()
                    .border_b_1()
                    .border_color(rgb(0x21262d))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!("Image Gallery ({})", self.image_paths.len())),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x8b949e))
                            .child("Click an image to view it full-size"),
                    ),
            )
            .child(
                // Grid
                div()
                    .flex_1()
                    .overflow_y_scroll()
                    .p_4()
                    .child(
                        div()
                            .grid()
                            .grid_cols_3()
                            .gap_3()
                            .children(self.image_paths.iter().enumerate().map(|(i, path)| {
                                let is_selected = self.selected_index == Some(i);
                                div()
                                    .aspect_square()
                                    .bg(rgb(0x161b22))
                                    .rounded_lg()
                                    .overflow_hidden()
                                    .border_2()
                                    .border_color(if is_selected { rgb(0x58a6ff) } else { rgb(0x21262d) })
                                    .cursor(CursorStyle::PointingHand)
                                    .child(
                                        img(path.clone())
                                            .size_full()
                                            .object_fit(ObjectFit::Cover),
                                    )
                                    .child(
                                        // Label overlay
                                        div()
                                            .absolute()
                                            .bottom_0()
                                            .left_0()
                                            .right_0()
                                            .px_2()
                                            .py_1p5()
                                            .bg(rgb(0x0d1117).opacity(0.85))
                                            .text_xs()
                                            .text_color(rgb(0x8b949e))
                                            .truncate()
                                            .child(path.clone()),
                                    )
                                    .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                                        this.select(i, _, _window, cx)
                                    }))
                            })),
                    ),
            )
            .when_some(self.selected_index, |parent, index| {
                parent.child(
                    // Full-size overlay
                    div()
                        .absolute()
                        .inset_0()
                        .bg(rgb(0x0d1117).opacity(0.95))
                        .flex()
                        .items_center()
                        .justify_center()
                        .z_index(50)
                        .cursor(CursorStyle::PointingHand)
                        .child(
                            div()
                                .max_w(px(700.))
                                .max_h(px(500.))
                                .rounded_xl()
                                .overflow_hidden()
                                .shadow_2xl()
                                .child(
                                    img(self.image_paths[index].clone())
                                        .size_full()
                                        .object_fit(ObjectFit::Contain),
                                ),
                        )
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .right_0()
                                .p_6()
                                .text_2xl()
                                .text_color(rgb(0x8b949e))
                                .cursor(CursorStyle::PointingHand)
                                .hover(|s| s.text_color(rgb(0xe6edf3)))
                                .child("✕"),
                        )
                        .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                            this.selected_index = None;
                            cx.notify();
                        })),
                )
            })
    }
}

fn main() {
    application()
        .with_assets(Assets)
        .run(|cx: &mut App| {
            cx.activate(true);
            cx.open_window(WindowOptions::default(), |_, cx| {
                cx.new(|_| Gallery {
                    selected_index: None,
                    image_paths: vec![
                        // Replace with paths to actual images on your system
                        SharedString::from("/path/to/image1.jpg"),
                        SharedString::from("/path/to/image2.jpg"),
                        SharedString::from("/path/to/image3.jpg"),
                        SharedString::from("/path/to/image4.jpg"),
                        SharedString::from("/path/to/image5.jpg"),
                        SharedString::from("/path/to/image6.jpg"),
                    ],
                })
            })
            .unwrap();
        });
}
```

**Key points:**
- `impl AssetSource` provides custom asset loading from the filesystem
- `application().with_assets(Assets)` registers the asset source
- `img(path)` loads and renders images
- `ObjectFit::Cover` fills the container while preserving aspect ratio
- `grid_cols_3()` for a 3-column responsive grid
- Overlay pattern for full-size image viewer with `absolute().inset_0()`

---

## 8. Draggable Kanban Board

**Concepts:** Drag & drop, `on_drag`, `on_drag_move`, `on_drop`, custom drag state, columns

```rust
use gpui::*;
use gpui_platform::application;

#[derive(Clone)]
struct Card {
    id: usize,
    title: SharedString,
    column: usize,
}

struct DragCard {
    card_id: usize,
    source_column: usize,
    position: Point<Pixels>,
}

impl Render for DragCard {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .absolute()
            .top(self.position.y)
            .left(self.position.x)
            .w(px(220.))
            .px_3()
            .py_3()
            .bg(rgb(0x1c2128))
            .border_1()
            .border_color(rgb(0x58a6ff))
            .rounded_md()
            .shadow_2xl()
            .opacity(0.9)
            .child("Dragging…")
    }
}

struct Board {
    columns: Vec<Vec<Card>>,
    column_names: Vec<&'static str>,
    next_id: usize,
    drag_card: Option<(DragCard, Task<()>)>,
}

impl Board {
    fn new() -> Self {
        let mut board = Board {
            columns: vec![vec![], vec![], vec![]],
            column_names: vec!["To Do", "In Progress", "Done"],
            next_id: 0,
            drag_card: None,
        };
        // Add some initial cards
        board.add_card(0, "Design landing page".into());
        board.add_card(0, "Set up CI pipeline".into());
        board.add_card(1, "Implement auth module".into());
        board.add_card(2, "Write unit tests".into());
        board
    }

    fn add_card(&mut self, column: usize, title: SharedString) {
        self.columns[column].push(Card {
            id: self.next_id,
            title,
            column,
        });
        self.next_id += 1;
    }
}

impl Render for Board {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x0d1117))
            .text_color(rgb(0xe6edf3))
            .child(
                div()
                    .px_6()
                    .py_4()
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .border_b_1()
                    .border_color(rgb(0x21262d))
                    .child("Kanban Board"),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .gap_0()
                    .overflow_x_auto()
                    .p_4()
                    .children((0..3).map(|col_idx| {
                        let column = &self.columns[col_idx];
                        let column_name = self.column_names[col_idx];
                        self.render_column(col_idx, column, column_name, cx)
                    })),
            )
    }
}

impl Board {
    fn render_column(
        &self,
        col_idx: usize,
        column: &Vec<Card>,
        name: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let col_idx_for_drop = col_idx;

        div()
            .flex_1()
            .flex()
            .flex_col()
            .gap_3()
            .min_w(px(240.))
            .child(
                div()
                    .px_3()
                    .py_2()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(0x8b949e))
                    .child(format!("{} ({})", name, column.len())),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_2()
                    .bg(rgb(0x161b22))
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(0x21262d))
                    .min_h(px(200.))
                    .children(column.iter().map(move |card| {
                        let card_id = card.id;
                        let source_column = card.column;

                        div()
                            .px_3()
                            .py_3()
                            .bg(rgb(0x21262d))
                            .border_1()
                            .border_color(rgb(0x30363d))
                            .rounded_md()
                            .cursor(CursorStyle::Grab)
                            .text_sm()
                            .hover(|s| s.bg(rgb(0x272e36)))
                            .child(card.title.clone())
                            .on_drag(
                                cx.listener(
                                    move |this: &mut Board,
                                          _: &MouseDownEvent,
                                          _window,
                                          cx| {
                                        this.drag_card.take();
                                        let drag = cx.new(|_| DragCard {
                                            card_id,
                                            source_column,
                                            position: Point::default(),
                                        });
                                        this.drag_card = Some((drag, Task::ready(())));
                                        cx.notify();
                                    },
                                ),
                            )
                            .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                                // Quick delete card on click
                                this.columns[col_idx_for_drop].retain(|c| c.id != card_id);
                                cx.notify();
                            }))
                    })),
            )
            .on_drop::<(DragCard, Task<()>)>(
                cx.listener(
                    move |this: &mut Board,
                          _: &MouseUpEvent,
                          _bounds,
                          drag: impl Into<Option<(DragCard, Task<()>)>>,
                          _window: &mut Window,
                          cx: &mut Context<Self>| {
                        if let Some((drag_card, _)) = drag.into() {
                            let card_id = drag_card.card_id;
                            // Remove from all columns
                            for col in &mut this.columns {
                                col.retain(|c| c.id != card_id);
                            }
                            // Add to target column
                            this.columns[col_idx_for_drop].push(Card {
                                id: card_id,
                                title: SharedString::from(format!(
                                    "Card {}",
                                    card_id
                                )),
                                column: col_idx_for_drop,
                            });
                            this.drag_card.take();
                            cx.notify();
                        }
                    },
                ),
            )
    }
}

fn main() {
    application().run(|cx: &mut App| {
        cx.activate(true);
        cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|_| Board::new())
        })
        .unwrap();
    });
}
```

**Key points:**
- `on_drag::<T>()` begins a drag, creating a drag-state entity
- The dragged entity implements `Render` — it renders as a floating element
- `on_drop::<T>()` receives the drag state on the target element
- Columns are drop targets for cards
- Drag state flows through the entity system for clean type safety

---

## 9. Split Pane

**Concepts:** Custom `Element`, proportional splits, resize handles, hitbox management

A custom split pane that gives two child views a resizable proportional layout.

```rust
use gpui::*;
use gpui_platform::application;
use smallvec::SmallVec;

// ---- Custom Element ----

struct SplitPane {
    children: SmallVec<[AnyElement; 2]>,
    split_ratio: f32, // 0.0 = all left, 1.0 = all right
    dragging: bool,
}

impl SplitPane {
    fn new(left: impl IntoElement, right: impl IntoElement) -> Self {
        Self {
            children: SmallVec::from_buf([
                left.into_any_element(),
                right.into_any_element(),
            ]),
            split_ratio: 0.5,
            dragging: false,
        }
    }

    fn split_ratio(mut self, ratio: f32) -> Self {
        self.split_ratio = ratio.clamp(0.1, 0.9);
        self
    }
}

impl Element for SplitPane {
    type RequestLayoutState = (LayoutId, LayoutId);
    type PrepaintState = (Option<FocusHandle>, Option<FocusHandle>);

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        // Root: full-size flex row
        let root_id = window.request_layout(
            Style {
                display: Display::Flex.into(),
                flex_direction: FlexDirection::Row.into(),
                size: size(relative(1.), relative(1.)).into(),
                ..Default::default()
            },
            None,
            cx,
        );

        // Left pane
        let left_id = window.request_layout(
            Style {
                size: size(relative(self.split_ratio), relative(1.)).into(),
                ..Default::default()
            },
            Some(root_id),
            cx,
        );
        self.children[0].request_layout(window, cx);

        // Resize handle
        let handle_style = Style {
            size: size(px(4.), relative(1.)).into(),
            cursor: Some(CursorStyle::ColResize),
            ..Default::default()
        };
        let _handle_id = window.request_layout(handle_style, Some(root_id), cx);

        // Right pane
        let right_id = window.request_layout(
            Style {
                size: size(relative(1.0 - self.split_ratio), relative(1.)).into(),
                ..Default::default()
            },
            Some(root_id),
            cx);
        self.children[1].request_layout(window, cx);

        (root_id, (left_id, right_id))
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let (left_id, right_id) = *request_layout;

        // Left pane bounds
        let left_bounds = window.layout_bounds(left_id);
        let handle_x = left_bounds.origin.x + left_bounds.size.width;
        let handle_bounds = Bounds {
            origin: point(handle_x, bounds.origin.y),
            size: size(px(4.), bounds.size.height),
        };

        // Insert the resize handle hitbox
        window.insert_hitbox(handle_bounds, HitboxBehavior::default());
        window.set_cursor_style(CursorStyle::ColResize, false);

        // Drag detection
        if self.dragging {
            // Already handled on mouse move
        }

        let left_focus = self.children[0].prepaint(window, cx);
        let right_focus = self.children[1].prepaint(window, cx);

        (left_focus, right_focus)
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (left_id, right_id) = *request_layout;

        let left_bounds = window.layout_bounds(left_id);
        // Paint resize handle
        let handle_x = left_bounds.origin.x + left_bounds.size.width;
        let handle_rect = Bounds {
            origin: point(handle_x, bounds.origin.y),
            size: size(px(4.), bounds.size.height),
        };
        window.paint_quad(fill(
            handle_rect,
            if self.dragging {
                rgb(0x58a6ff)
            } else {
                rgb(0x30363d)
            },
        ));

        self.children[0].paint(window, cx);
        self.children[1].paint(window, cx);
    }
}

impl IntoElement for SplitPane {
    type Element = Self;
    fn into_element(self) -> Self::Element { self }
}

impl ParentElement for SplitPane {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

// ---- App View ----

struct SplitApp {
    left_counter: i32,
    right_counter: i32,
    split: f32,
}

impl Render for SplitApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        SplitPane::new(
            // Left panel
            div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_4()
                .bg(rgb(0x161b22))
                .border_r_1()
                .border_color(rgb(0x21262d))
                .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child("Left Panel"))
                .child(div().text_4xl().font_weight(FontWeight::BOLD).child(format!("{}", self.left_counter)))
                .child(
                    div()
                        .px_4()
                        .py_2()
                        .bg(rgb(0x238636))
                        .text_color(rgb(0xffffff))
                        .rounded_md()
                        .cursor(CursorStyle::PointingHand)
                        .child("Increment")
                        .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                            this.left_counter += 1;
                            cx.notify();
                        })),
                ),
            // Right panel
            div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_4()
                .bg(rgb(0x0d1117))
                .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child("Right Panel"))
                .child(div().text_4xl().font_weight(FontWeight::BOLD).child(format!("{}", self.right_counter)))
                .child(
                    div()
                        .px_4()
                        .py_2()
                        .bg(rgb(0x1f6feb))
                        .text_color(rgb(0xffffff))
                        .rounded_md()
                        .cursor(CursorStyle::PointingHand)
                        .child("Increment")
                        .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                            this.right_counter += 1;
                            cx.notify();
                        })),
                ),
        )
        .split_ratio(0.5)
        .into_any_element()
    }
}

fn main() {
    application().run(|cx: &mut App| {
        cx.activate(true);
        cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|_| SplitApp {
                left_counter: 0,
                right_counter: 0,
                split: 0.5,
            })
        })
        .unwrap();
    });
}
```

**Key points:**
- Custom `impl Element` gives full control over layout, hitbox, and paint
- `window.request_layout(style, parent_id, cx)` adds children to Taffy
- `window.layout_bounds(layout_id)` queries computed sizes
- `window.insert_hitbox(bounds, behavior)` sets up hit testing
- `window.paint_quad(fill(bounds, color))` draws colored rectangles
- `ParentElement` enables `.child()` API on the custom element
- `relative(0.5)` means 50% of the parent's available space

---

## 10. Chat Simulator

**Concepts:** Virtual list, `uniform_list`, event emitters, subscriptions, async messages, self-scrolling

```rust
use gpui::*;
use gpui_platform::application;
use std::time::Duration;

#[derive(Clone)]
struct Message {
    id: usize,
    sender: SharedString,
    content: SharedString,
    timestamp: SharedString,
    is_bot: bool,
}

struct MessageEvent {
    message: Message,
}

struct Chat {
    messages: Vec<Message>,
    input: SharedString,
    focus_handle: FocusHandle,
    next_id: usize,
    auto_scroll: bool,
}

impl EventEmitter<MessageEvent> for Chat {}

impl Chat {
    fn send_message(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let content = self.input.trim();
        if content.is_empty() {
            return;
        }

        let msg = Message {
            id: self.next_id,
            sender: "You".into(),
            content: SharedString::from(content),
            timestamp: "Just now".into(),
            is_bot: false,
        };
        self.next_id += 1;
        self.messages.push(msg);
        self.input = SharedString::default();
        self.auto_scroll = true;
        cx.notify();

        // Simulate bot responding after a delay
        let bot_content = SharedString::from(format!("Thanks for saying '{}'!", content));
        cx.spawn(|this: WeakEntity<Self>, mut cx: AsyncApp| async move {
            cx.background_executor()
                .timer(Duration::from_millis(800 + (bot_content.len() as u64 * 30)))
                .await;

            this.update(&mut cx, |this, cx| {
                let bot_msg = Message {
                    id: this.next_id,
                    sender: "Bot 🤖".into(),
                    content: bot_content,
                    timestamp: "Just now".into(),
                    is_bot: true,
                };
                this.next_id += 1;
                this.messages.push(bot_msg);
                this.auto_scroll = true;
                cx.notify();
                cx.emit(MessageEvent {
                    message: this.messages.last().unwrap().clone(),
                });
            })
            .ok();
        })
        .detach();
    }
}

impl Render for Chat {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x0d1117))
            .text_color(rgb(0xe6edf3))
            .child(
                // Header
                div()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(rgb(0x21262d))
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Chat Simulator"),
            )
            .child(
                // Messages area
                div()
                    .flex_1()
                    .overflow_y_scroll()
                    .px_4()
                    .py_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .id("chat-messages")
                    .children(self.messages.iter().map(|msg| self.render_message(msg))),
            )
            .child(
                // Input bar
                div()
                    .px_4()
                    .py_3()
                    .border_t_1()
                    .border_color(rgb(0x21262d))
                    .flex()
                    .gap_3()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .px_3()
                            .py_2()
                            .bg(rgb(0x161b22))
                            .border_1()
                            .border_color(rgb(0x30363d))
                            .rounded_full()
                            .track_focus(&self.focus_handle)
                            .when(self.input.is_empty(), |s| {
                                s.child(
                                    div()
                                        .text_color(rgb(0x484f58))
                                        .text_sm()
                                        .child("Type a message…"),
                                )
                            })
                            .when(!self.input.is_empty(), |s| {
                                s.child(div().text_sm().child(self.input.clone()))
                            }),
                    )
                    .child(
                        div()
                            .size(px(34.))
                            .rounded_full()
                            .bg(rgb(0x238636))
                            .text_color(rgb(0xffffff))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor(CursorStyle::PointingHand)
                            .child("↑")
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.send_message(window, cx)
                            })),
                    ),
            )
    }
}

impl Chat {
    fn render_message(&self, msg: &Message) -> impl IntoElement {
        let align = if msg.is_bot {
            AlignItems::Start
        } else {
            AlignItems::End
        };

        div()
            .flex()
            .flex_col()
            .items(align)
            .child(
                div()
                    .text_xxs()
                    .text_color(rgb(0x8b949e))
                    .px_1()
                    .child(format!("{} · {}", msg.sender, msg.timestamp)),
            )
            .child(
                div()
                    .max_w(px(360.))
                    .px_3()
                    .py_2()
                    .bg(if msg.is_bot { rgb(0x161b22) } else { rgb(0x1f6feb) })
                    .rounded_xl()
                    .mt_1()
                    .text_sm()
                    .child(msg.content.clone()),
            )
    }
}

impl Focusable for Chat {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EntityInputHandler for Chat {
    fn text_for_editing(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> String {
        self.input.to_string()
    }

    fn replace_text_in_range(
        &mut self,
        _range: Option<std::ops::Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.input = SharedString::from(format!("{}{}", self.input, text));
        cx.notify();
    }

    fn backspace(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let mut s = self.input.to_string();
        s.pop();
        self.input = SharedString::from(s);
        cx.notify();
    }

    fn enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.send_message(window, cx);
    }
}

fn main() {
    application().run(|cx: &mut App| {
        cx.activate(true);
        cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|_| Chat {
                messages: Vec::new(),
                input: SharedString::default(),
                focus_handle: cx.focus_handle(),
                next_id: 0,
                auto_scroll: false,
            })
        })
        .unwrap();
    });
}
```

**Key points:**
- `cx.spawn()` for simulated async bot responses with `cx.background_executor().timer(dur)`
- `EventEmitter<MessageEvent>` + `cx.emit()` to notify subscribers of new messages
- `cx.subscribe()` pattern to react to events (e.g., scroll to bottom on new message)
- `EntityInputHandler.enter()` for sending with Enter key
- Conditional styling for user vs. bot message bubbles
- `.id("chat-messages")` for stable element identity across frames

---

## 11. Markdown Previewer

**Concepts:** Two-pane layout, live editing, async text processing, split view

```rust
use gpui::*;
use gpui_platform::application;

struct MarkdownPreviewer {
    source: String,
    html_output: String,
    focus_handle: FocusHandle,
    preview_visible: bool,
}

impl MarkdownPreviewer {
    fn process_markdown(&mut self, cx: &mut Context<Self>) {
        // Simple markdown-to-HTML conversion (simplified for the example)
        let mut html = String::from("<div style='font-family: system-ui; padding: 16px;'>");

        for line in self.source.lines() {
            if line.starts_with("# ") {
                html.push_str(&format!(
                    "<h1 style='font-size:24px;font-weight:700;margin:16px 0 8px;color:#e6edf3;'>{}</h1>",
                    &line[2..]
                ));
            } else if line.starts_with("## ") {
                html.push_str(&format!(
                    "<h2 style='font-size:20px;font-weight:600;margin:12px 0 6px;color:#e6edf3;'>{}</h2>",
                    &line[3..]
                ));
            } else if line.starts_with("```") {
                html.push_str("<pre style='background:#161b22;border:1px solid #30363d;border-radius:6px;padding:12px;margin:8px 0;font-family:monospace;font-size:13px;color:#e6edf3;'>");
            } else if line == "```" {
                html.push_str("</pre>");
            } else if line.starts_with("- ") {
                html.push_str(&format!(
                    "<div style='padding-left:16px;margin:2px 0;color:#8b949e;'>• {}</div>",
                    &line[2..]
                ));
            } else if line.starts_with("> ") {
                html.push_str(&format!(
                    "<blockquote style='border-left:3px solid #7c3aed;padding:4px 12px;margin:8px 0;background:#161b22;border-radius:0 6px 6px 0;color:#8b949e;'>{}</blockquote>",
                    &line[2..]
                ));
            } else if !line.is_empty() {
                html.push_str(&format!(
                    "<p style='margin:4px 0;color:#c9d1d9;line-height:1.6;'>{}</p>",
                    line
                ));
            }
        }

        html.push_str("</div>");
        self.html_output = html;
        cx.notify();
    }
}

impl Render for MarkdownPreviewer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x0d1117))
            .text_color(rgb(0xe6edf3))
            .child(
                // Toolbar
                div()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(rgb(0x21262d))
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Markdown Previewer"),
                    )
                    .child(
                        div()
                            .px_3()
                            .py_1p5()
                            .bg(if self.preview_visible { rgb(0x1f6feb) } else { rgb(0x21262d) })
                            .text_color(if self.preview_visible { rgb(0xffffff) } else { rgb(0x8b949e) })
                            .rounded_md()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .cursor(CursorStyle::PointingHand)
                            .child(if self.preview_visible { "Hide Preview" } else { "Show Preview" })
                            .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                                this.preview_visible = !this.preview_visible;
                                if this.preview_visible {
                                    this.process_markdown(cx);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(
                // Main area
                div()
                    .flex_1()
                    .flex()
                    .child(
                        // Editor pane
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .border_r_1()
                            .border_color(rgb(0x21262d))
                            .child(
                                div()
                                    .px_3()
                                    .py_2()
                                    .text_xs()
                                    .text_color(rgb(0x8b949e))
                                    .border_b_1()
                                    .border_color(rgb(0x21262d))
                                    .child("Editor"),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .p_4()
                                    .font_family("SF Mono, Menlo, monospace")
                                    .text_sm()
                                    .track_focus(&self.focus_handle)
                                    .when(self.source.is_empty(), |s| {
                                        s.text_color(rgb(0x484f58)).child(
                                            "# Start typing markdown here…\n\n- List item\n- Another item\n\n> A blockquote\n\n```rust\nlet x = 42;\n```",
                                        )
                                    })
                                    .when(!self.source.is_empty(), |s| {
                                        s.child(self.source.clone())
                                    }),
                            ),
                    )
                    .when(self.preview_visible, |parent| {
                        parent.child(
                            // Preview pane
                            div()
                                .w(px(400.))
                                .flex()
                                .flex_col()
                                .border_l_1()
                                .border_color(rgb(0x21262d))
                                .child(
                                    div()
                                        .px_3()
                                        .py_2()
                                        .text_xs()
                                        .text_color(rgb(0x8b949e))
                                        .border_b_1()
                                        .border_color(rgb(0x21262d))
                                        .child("Preview"),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .overflow_y_scroll()
                                        .p_0()
                                        .text_sm()
                                        .child(self.html_output.clone()),
                                ),
                        )
                    }),
            )
            .child(
                // Status bar
                div()
                    .px_4()
                    .py_1p5()
                    .border_t_1()
                    .border_color(rgb(0x21262d))
                    .text_xs()
                    .text_color(rgb(0x484f58))
                    .child(format!(
                        "{} lines · {} characters{}",
                        self.source.lines().count(),
                        self.source.len(),
                        if self.preview_visible { " · Preview active" } else { "" },
                    )),
            )
    }
}

impl Focusable for MarkdownPreviewer {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EntityInputHandler for MarkdownPreviewer {
    fn text_for_editing(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> String {
        self.source.clone()
    }

    fn replace_text_in_range(
        &mut self,
        _range: Option<std::ops::Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.source.push_str(text);
        if self.preview_visible {
            self.process_markdown(cx);
        }
        cx.notify();
    }

    fn backspace(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.source.pop();
        if self.preview_visible {
            self.process_markdown(cx);
        }
        cx.notify();
    }

    fn enter(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.source.push('\n');
        if self.preview_visible {
            self.process_markdown(cx);
        }
        cx.notify();
    }
}

fn main() {
    application().run(|cx: &mut App| {
        cx.activate(true);
        cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|_| MarkdownPreviewer {
                source: String::new(),
                html_output: String::new(),
                focus_handle: cx.focus_handle(),
                preview_visible: false,
            })
        })
        .unwrap();
    });
}
```

**Key points:**
- Two-pane layout with conditional rendering via `.when(self.preview_visible, ...)`
- Text editing via `EntityInputHandler` for both typing and Enter key
- Live document stats in the status bar
- Toggle button with dynamic label ("Show Preview" / "Hide Preview")
- Simple string-based markdown processing with inline HTML styling

---

## 12. Multi-Window Notes

**Concepts:** Multiple windows, `cx.open_window()`, cross-window entity sharing, `cx.spawn_in()`

```rust
use gpui::*;
use gpui_platform::application;
use std::sync::atomic::{AtomicUsize, Ordering};

// --- Shared Note Entity ---

#[derive(Clone)]
struct Note {
    id: usize,
    title: SharedString,
    content: SharedString,
}

struct NotesChanged;

struct Notebook {
    notes: Vec<Note>,
    selected_index: Option<usize>,
    next_id: usize,
}

impl EventEmitter<NotesChanged> for Notebook {}

impl Notebook {
    fn add_note(&mut self, title: SharedString, cx: &mut Context<Self>) {
        let id = self.next_id;
        self.next_id += 1;
        self.notes.push(Note {
            id,
            title,
            content: SharedString::default(),
        });
        self.selected_index = Some(self.notes.len() - 1);
        cx.notify();
        cx.emit(NotesChanged);
    }

    fn selected_note(&self) -> Option<&Note> {
        self.selected_index.and_then(|i| self.notes.get(i))
    }
}

// --- Main Window (Notebook List) ---

struct NotebookView {
    notebook: Entity<Notebook>,
    _notes_sub: Subscription,
    new_note_title: SharedString,
    focus_handle: FocusHandle,
}

impl NotebookView {
    fn open_note_window(&self, note_id: usize, cx: &mut Context<Self>) {
        let notebook = self.notebook.clone();
        cx.spawn(|_, mut cx: AsyncApp| async move {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: point(px(200.0), px(150.0)),
                        size: size(px(500.0), px(400.0)),
                    })),
                    ..Default::default()
                },
                |_, cx| {
                    cx.new(move |_| NoteWindow {
                        notebook: notebook.clone(),
                        note_id,
                        _notes_sub: cx.subscribe(&notebook, |this, _notebook, _: &NotesChanged, cx| {
                            cx.notify();
                        }),
                    })
                },
            )
            .ok();
        })
        .detach();
    }
}

impl Render for NotebookView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let notebook = self.notebook.read(cx);

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x0d1117))
            .text_color(rgb(0xe6edf3))
            .child(
                // Header
                div()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(rgb(0x21262d))
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Notebook"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x8b949e))
                            .child("Click a note to open in a new window"),
                    ),
            )
            .child(
                // Create new note
                div()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(rgb(0x21262d))
                    .flex()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .px_3()
                            .py_2()
                            .bg(rgb(0x161b22))
                            .border_1()
                            .border_color(rgb(0x30363d))
                            .rounded_md()
                            .track_focus(&self.focus_handle)
                            .when(self.new_note_title.is_empty(), |s| {
                                s.child(
                                    div()
                                        .text_color(rgb(0x484f58))
                                        .text_xs()
                                        .child("New note title…"),
                                )
                            })
                            .when(!self.new_note_title.is_empty(), |s| {
                                s.child(div().text_xs().child(self.new_note_title.clone()))
                            }),
                    )
                    .child(
                        div()
                            .px_3()
                            .py_2()
                            .bg(rgb(0x238636))
                            .text_color(rgb(0xffffff))
                            .rounded_md()
                            .text_xs()
                            .cursor(CursorStyle::PointingHand)
                            .child("+ Add")
                            .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                                if !this.new_note_title.trim().is_empty() {
                                    let title =
                                        SharedString::from(this.new_note_title.trim());
                                    this.notebook.update(cx, |nb, cx| {
                                        nb.add_note(title, cx)
                                    });
                                    this.new_note_title = SharedString::default();
                                    cx.notify();
                                }
                            })),
                    ),
            )
            .child(
                // Notes list
                div()
                    .flex_1()
                    .overflow_y_scroll()
                    .children(notebook.notes.iter().map(|note| {
                        let note_id = note.id;
                        let is_selected = notebook.selected_index
                            .map(|i| notebook.notes[i].id == note_id)
                            .unwrap_or(false);

                        div()
                            .px_4()
                            .py_3()
                            .flex()
                            .items_center()
                            .justify_between()
                            .border_b_1()
                            .border_color(rgb(0x21262d))
                            .when(is_selected, |s| s.bg(rgb(0x161b22)))
                            .hover(|s| s.bg(rgb(0x161b22)))
                            .cursor(CursorStyle::PointingHand)
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_0p5()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(note.title.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(0x8b949e))
                                            .child(format!(
                                                "{} characters",
                                                note.content.len()
                                            )),
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(0x484f58))
                                    .child("Open →"),
                            )
                            .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                                this.open_note_window(note_id, cx)
                            }))
                    })),
            )
    }
}

impl Focusable for NotebookView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle { self.focus_handle.clone() }
}

impl EntityInputHandler for NotebookView {
    fn text_for_editing(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> String {
        self.new_note_title.to_string()
    }
    fn replace_text_in_range(&mut self, _: Option<std::ops::Range<usize>>, text: &str, _: &mut Window, cx: &mut Context<Self>) {
        self.new_note_title = SharedString::from(format!("{}{}", self.new_note_title, text));
        cx.notify();
    }
    fn backspace(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let mut s = self.new_note_title.to_string(); s.pop();
        self.new_note_title = SharedString::from(s);
        cx.notify();
    }
    fn enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.new_note_title.trim().is_empty() {
            let title = SharedString::from(self.new_note_title.trim());
            self.notebook.update(cx, |nb, cx| nb.add_note(title, cx));
            self.new_note_title = SharedString::default();
            cx.notify();
        }
    }
}

// --- Note Window (Individual Note Editor) ---

struct NoteWindow {
    notebook: Entity<Notebook>,
    note_id: usize,
    _notes_sub: Subscription,
}

impl Render for NoteWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let notebook = self.notebook.read(cx);
        let note_title = notebook
            .notes
            .iter()
            .find(|n| n.id == self.note_id)
            .map(|n| n.title.clone())
            .unwrap_or_else(|| SharedString::from("(deleted)"));

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x0d1117))
            .text_color(rgb(0xe6edf3))
            .child(
                div()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(rgb(0x21262d))
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(note_title),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x8b949e))
                            .child(format!("Note #{}", self.note_id)),
                    ),
            )
            .child(
                // Note content (read-only display for simplicity)
                div()
                    .flex_1()
                    .p_4()
                    .child(
                        div()
                            .size_full()
                            .bg(rgb(0x161b22))
                            .border_1()
                            .border_color(rgb(0x21262d))
                            .rounded_lg()
                            .p_4()
                            .text_sm()
                            .when_some(
                                notebook
                                    .notes
                                    .iter()
                                    .find(|n| n.id == self.note_id)
                                    .map(|n| n.content.clone()),
                                |this, content| {
                                    this.child(content)
                                },
                            )
                            .when(
                                !notebook
                                    .notes
                                    .iter()
                                    .any(|n| n.id == self.note_id),
                                |this| {
                                    this.flex()
                                        .items_center()
                                        .justify_center()
                                        .text_color(rgb(0x484f58))
                                        .child("This note has been deleted.")
                                },
                            ),
                    ),
            )
    }
}

fn main() {
    application().run(|cx: &mut App| {
        cx.activate(true);

        // Create the shared notebook entity
        let notebook = cx.new(|_| Notebook {
            notes: vec![
                Note { id: 0, title: "Welcome".into(), content: "Start writing notes! Each note opens in its own window.".into() },
                Note { id: 1, title: "Shopping List".into(), content: "Milk\nEggs\nBread\nApples".into() },
                Note { id: 2, title: "Ideas".into(), content: "Build something cool with GPUI!".into() },
            ],
            selected_index: Some(0),
            next_id: 3,
        });

        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(420.0), px(600.0)),
                    cx,
                ))),
                ..Default::default()
            },
            |_, cx| {
                let _notes_sub =
                    cx.subscribe(&notebook, |this: &mut NotebookView, _nb, _: &NotesChanged, cx| {
                        cx.notify();
                    });
                cx.new(move |_| NotebookView {
                    notebook: notebook.clone(),
                    _notes_sub,
                    new_note_title: SharedString::default(),
                    focus_handle: cx.focus_handle(),
                })
            },
        )
        .unwrap();
    });
}
```

**Key points:**
- The `Notebook` entity is shared across multiple windows
- `cx.open_window()` in an async task creates a new window
- Each window reads from the same `Entity<Notebook>` for real-time sync
- `EventEmitter<NotesChanged>` + `cx.emit()` + `cx.subscribe()` propagates changes across windows
- `cx.spawn()` + `cx.open_window()` for opening windows from event handlers
- Read-only display in the note detail window keeps the example focused on cross-window concepts

---

---

## Key Takeaways: What You Should Remember

After working through these 12 examples, here's what matters most.

### 1. State lives in entities, not in the UI

There is no `useState` in GPUI. Your data lives in struct fields on entities. The UI reads from entities and writes back to them via callbacks. The element tree is **derived** from state — it's a pure function of your data at render time.

```rust
// ❌ Don't think: "I need to update the counter label"
// ✅ Think:    "I update the count and call cx.notify()"
counter.update(cx, |this, cx| {
    this.count += 1;
    cx.notify();  // <-- this triggers re-render
});
```

### 2. `cx` is your universal remote

Every context type (`App`, `Context<T>`, `AsyncApp`, `Window`) gives you access to a specific set of services. `Context<T>` is the one you'll use most: it derefs to `App` (so you get everything `App` has) *plus* entity-level operations like `notify`, `emit`, `observe`, and `subscribe`.

| If you need to… | Use… |
|---|---|
| Create entities or windows | `App` or `Context<T>` |
| Update your own entity's state | `Context<T>` |
| React to another entity's changes | `Context<T>.observe()` |
| Do async work without blocking UI | `Context<T>.spawn()` → `AsyncApp` |
| Dispatch an action or show a tooltip | `Window` |

### 3. Call `cx.notify()` after every state change

GPUI doesn't track dependencies automatically. If you change data that affects the UI, you **must** call `cx.notify()`. Forgetting this is the #1 bug in GPUI apps. If your UI isn't updating, check for missing `cx.notify()` calls.

```rust
// ✅ Correct
this.name = new_name;
cx.notify();

// ❌ Bug: name changes but UI doesn't update
this.name = new_name;
```

### 4. Use `cx.listener()` inside `render()` to wire up events

Event handlers like `on_click` need a callback. `cx.listener(MyView::my_method)` creates one that routes the event back to your entity. The method signature follows a convention: `&mut self, event, window, cx`.

```rust
fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    div()
        .on_click(cx.listener(Self::handle_click))  // method on Self
        .on_action(cx.listener(Self::handle_action)) // action handler
}
```

### 5. Async work: spawn, don't block

Never run blocking work on the main thread. Use `cx.spawn()` for async operations. Inside the spawn closure, `cx` becomes `AsyncApp`. Use `this.update(&mut cx, ...)` to get back to your entity's state.

```rust
cx.spawn(|this: WeakEntity<Self>, mut cx: AsyncApp| async move {
    let data = fetch_data().await;
    this.update(&mut cx, |this, cx| {
        this.data = data;
        cx.notify();
    }).ok();
}).detach();
```

Key rule: the spawn closure captures `WeakEntity<Self>`, **not** `Entity<Self>`. This prevents reference cycles and ensures your task doesn't keep entities alive after they're dropped.

### 6. Observe, subscribe, or store — choose your communication pattern

Entities need to talk. GPUI gives you three ways:

| Pattern | When to use |
|---|---|
| `cx.observe(&entity, callback)` | "Tell me whenever this entity changes" (broad notification) |
| `cx.subscribe(&entity, callback)` | "Tell me when this specific event happens" (typed, precise) |
| Store `Entity<T>` / `WeakEntity<T>` | "I need to read or update this entity directly" |

For app-wide state, put it in a `Global` and use `cx.observe_global::<T>()`.

### 7. The element tree is rebuilt every frame — and that's fine

GPUI rebuilds the entire element tree from scratch each frame. Don't worry about performance: views are **automatically cached** — if `cx.notify()` hasn't been called, GPUI reuses the previous frame's layout and paint results. For very large lists, use `uniform_list` for virtualized rendering.

### Where to Go From Here

- Read the [GPUI Wiki](gpui-wiki.md) for deep architectural details and API reference
- Study the [40+ official examples](https://github.com/zed-industries/zed/tree/main/crates/gpui/examples) in the Zed repo
- Look at Zed's own source code — it's the largest GPUI app in existence
- Join the [Zed Discord](https://zed.dev/community-links) for help and discussion

---

## Concepts Progression Map

| Example | New Concepts Introduced |
|---|---|
| 1. Hello, GPUI! | `Application`, `Render`, `div`, window creation, basic styling |
| 2. Counter | `Entity`, `cx.notify()`, `cx.listener`, `on_click`, dynamic styling |
| 3. Temperature Converter | `EntityInputHandler`, `FocusHandle`, `track_focus`, text editing |
| 4. Stopwatch | `cx.spawn()`, `AsyncApp`, `cx.background_executor().timer()`, `Task` |
| 5. Theme Switcher | `Global`, `cx.set_global()`, `cx.update_global()`, `cx.observe_global()`, `deferred` |
| 6. Todo List | `actions!()`, `cx.bind_keys()`, `children()`, list management, keyboard shortcuts |
| 7. Image Gallery | `AssetSource`, `img`, `ObjectFit`, `grid_cols_3()`, overlays |
| 8. Kanban Board | `on_drag`, `on_drop`, drag state, cross-column movement |
| 9. Split Pane | `impl Element`, `window.request_layout()`, `layout_bounds()`, `insert_hitbox()`, `paint_quad()` |
| 10. Chat Simulator | Virtual lists, `EventEmitter`, `cx.emit()`, `cx.subscribe()`, timed async responses |
| 11. Markdown Previewer | Two-pane layout, conditional rendering, `EntityInputHandler.enter()` |
| 12. Multi-Window Notes | `cx.open_window()` in async, shared entities across windows, cross-window sync |

---

> **Tip:** Run any example with `cargo run -p gpui --example <name>`. Each example is self-contained — copy it into your own project's `main.rs` and run with `cargo run`.
