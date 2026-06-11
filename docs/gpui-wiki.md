# GPUI — The Zed UI Framework

GPUI is a hybrid immediate-and-retained-mode, GPU-accelerated UI framework for Rust, built to power the [Zed](https://zed.dev) code editor. It runs on macOS (Metal), Linux (Vulkan), and Windows (Vulkan/DirectX), with experimental WebAssembly support.

> **Status:** Pre-1.0, active development. Breaking changes between versions are common. Requires the latest stable Rust.

---

## Table of Contents

1. [Getting Started](#getting-started)
2. [Core Concepts](#core-concepts)
3. [Entities & State Management](#entities--state-management)
4. [Context Types](#context-types)
5. [Elements & Rendering](#elements--rendering)
6. [Views (Renderable Entities)](#views-renderable-entities)
7. [Styling API](#styling-api)
8. [Interactivity & Events](#interactivity--events)
9. [Actions & Key Bindings](#actions--key-bindings)
10. [Concurrency & Async](#concurrency--async)
11. [Observability & Subscriptions](#observability--subscriptions)
12. [Windows & Platform](#windows--platform)
13. [Layout System](#layout-system)
14. [Built-in Elements](#built-in-elements)
15. [Globals](#globals)
16. [Testing](#testing)
17. [Architecture Deep Dive](#architecture-deep-dive)
18. [Custom Elements](#custom-elements)

---

## Getting Started

A GPUI application starts with a call to `gpui_platform::application()`, which returns an `Application` builder. You configure it—optionally setting an asset source, HTTP client, or quit mode—then call `.run()` with a callback. That callback receives `&mut App`, the root context from which you open windows, create entities, and set global state. Everything flows from there.

Below is a minimal "Hello World" that opens a single centered window with a styled greeting:

```rust
use gpui::*;
use gpui_platform::application;

struct HelloWorld {
    text: SharedString,
}

impl Render for HelloWorld {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .bg(rgb(0x505050))
            .size(px(500.0))
            .justify_center()
            .items_center()
            .text_xl()
            .text_color(rgb(0xffffff))
            .child(format!("Hello, {}!", &self.text))
    }
}

fn main() {
    application().run(|cx: &mut App| {
        cx.open_window(
            WindowOptions::default(),
            |_, cx| cx.new(|_| HelloWorld { text: "World".into() }),
        )
        .unwrap();
        cx.activate(true);
    });
}
```

Run with: `cargo run -p gpui --example hello_world`

Key points in this example:

- `application().run(...)` boots the platform event loop and hands you `&mut App`.
- `cx.open_window(...)` creates a native OS window. The second argument is a closure that returns the *root view* for the window—an `Entity<T>` where `T: Render`.
- `cx.new(|_| HelloWorld { ... })` creates an entity whose state GPUI owns. You get back an `Entity<HelloWorld>` handle.
- `cx.activate(true)` brings the application to the foreground.
- The `impl Render` block describes what the window should draw. We return a `div()` styled with a Tailwind-inspired fluent API.

### System Dependencies

- **macOS:** Xcode + Xcode CLT (`xcode-select --install`)
- **Linux:** Vulkan drivers, `libxcb`, `libxkbcommon`, `libfontconfig`, `libssl`
- **Windows:** Vulkan SDK or DirectX

---

## Core Concepts

GPUI is unusual among Rust UI frameworks because it blends *immediate-mode* element construction with *retained-mode* state management. The element tree is rebuilt from scratch every frame (like immediate mode), but application state lives in long-lived `Entity<T>` objects that persist across frames (like retained mode). This hybrid design gives you the simplicity of declaring your UI as a pure function of state while avoiding the performance pitfalls of rebuilding heavyweight data structures.

The framework provides **three registers** depending on what you're building:

| Register | Primary Trait / Type | Purpose |
|---|---|---|
| **State** | `Entity<T>` | Application state, shared ownership, inter-entity communication |
| **Declarative UI** | `Render`, `RenderOnce` | High-level views, element trees, Tailwind-style styling |
| **Imperative UI** | `Element` | Full control over layout and painting (custom editors, virtual lists) |

Most applications will use all three: entities to hold state, `Render` to declare views, and occasionally `Element` for performance-critical custom drawing.

The ownership model is also distinctive. Every entity is ultimately owned by `App`, not by any handle. `Entity<T>` is a reference-counted smart pointer that borrows from `App` to access state—you never hold a `&mut T` directly, only through a closure passed to `.read()` or `.update()`. This single-owner design eliminates whole classes of borrow-checker headaches, while the ref-counting enables shared references between entities.

---

## Entities & State Management

Entities are the backbone of GPUI's state system. They are analogous to an `Rc<RefCell<T>>` with one critical difference: the actual `T` lives inside `App`, not inside the handle. This means:

- There is exactly one mutable reference to any entity's state at a time (enforced at runtime by `App`).
- You cannot accidentally hold a `&mut T` across an `await` point, because you never hold one at all—you only get one inside a synchronous closure.
- Entities can freely reference each other via `Entity<T>` handles without the borrow checker fighting you.

### Creating, Reading, and Updating Entities

You create an entity with `cx.new()`, passing a closure that constructs the initial state. The closure receives a `&mut Context<T>` scoped to the new entity, which you can use to immediately set up observers and subscriptions before the entity is even returned.

```rust
struct Counter {
    count: usize,
}

// Creating an entity:
// cx.new(|cx| ...) allocates state inside App and returns an Entity<Counter> handle.
let counter: Entity<Counter> = cx.new(|_cx| Counter { count: 0 });

// Reading — borrows state immutably. The closure receives &T and &App.
counter.read(cx, |counter, _cx| {
    println!("count is {}", counter.count);
});

// read_with — same as read but returns the closure's value.
let count = counter.read_with(cx, |counter: &Counter, _cx: &App| counter.count);

// Updating — borrows state mutably. The closure receives &mut T and &mut Context<T>.
// Context<T> derefs to App, so you have full access to all app services.
counter.update(cx, |counter: &mut Counter, cx: &mut Context<Counter>| {
    counter.count += 1;
    cx.notify(); // Tell observers state changed — critical for UI updates
});
```

**Important:** Never try to `.update()` an entity that is already being updated. GPUI tracks which entities have active borrows and will panic if you re-enter. If you need to update a different entity from within an `update` closure, that's fine—the restriction only applies to re-borrowing the *same* entity.

### Entity Handles

GPUI provides four handle types that vary in strength and type-erasure:

| Handle | Description |
|---|---|
| `Entity<T>` | Strong handle — increments the reference count. As long as any `Entity<T>` exists, the entity stays alive. |
| `WeakEntity<T>` | Weak handle — does not prevent release. `.read()` and `.update()` return `Result` and fail if the entity is gone. |
| `AnyEntity` | Dynamically-typed strong handle. Useful for collections of heterogeneous entities. |
| `AnyWeakEntity` | Dynamically-typed weak handle. |

Weak handles are the primary tool for avoiding reference cycles. If entity A holds an `Entity<B>` and entity B holds an `Entity<A>`, neither will ever be dropped. Use `WeakEntity` for one direction of the cycle:

```rust
// Avoiding cycles with weak handles:
let weak: WeakEntity<Counter> = counter.downgrade();
if let Some(entity) = weak.upgrade() {
    // upgrade() returns Option<Entity<T>> — None if the entity has been dropped
    entity.update(cx, |counter, cx| counter.count += 1);
}
```

### Entity IDs

Every entity has a globally unique `EntityId`, available before the entity is even created (via `Reservation<T>`). These are used internally for tracking, and you can use them to build your own lookup tables:

```rust
let id: EntityId = counter.entity_id();
```

### Reservation\<T\> — Pre-allocated Entity IDs

Sometimes you need an entity's ID before the entity exists—for example, when the entity wants to store its own ID in a field, or when two entities need to reference each other on construction. `cx.reserve_entity()` returns a `Reservation<T>` with a pre-assigned `EntityId`:

```rust
// Reserve an ID, then insert the entity later.
let reservation = cx.reserve_entity::<Foo>();
let id = reservation.entity_id();       // Available immediately
let entity = cx.insert_entity(reservation, |cx| Foo::new(id, cx));
```

### EventEmitter — Typed Events Between Entities

The `notify`/`observe` pattern signals "something changed." For richer communication—where you want to say *what* changed and include data—GPUI provides `emit`/`subscribe` with typed events.

First, declare that an entity can emit a certain event type by implementing `EventEmitter<E>` for it. The `impl` block is empty—it's purely a marker trait:

```rust
struct CounterChangeEvent {
    increment: usize,
}

// Marker impl: "Counter can emit CounterChangeEvent"
impl EventEmitter<CounterChangeEvent> for Counter {}
```

Now emit the event from inside an `update` closure and subscribe to it from another entity:

```rust
// Emitting:
counter.update(cx, |counter, cx| {
    counter.count += 1;
    cx.emit(CounterChangeEvent { increment: 1 });
});

// Subscribing from another entity's context:
cx.subscribe(
    &counter,
    |this: &mut Observer, counter, event: &CounterChangeEvent, cx| {
        this.total += event.increment;
    },
).detach();
```

The distinction between `observe` and `subscribe`:

- **`observe`**: fires on `cx.notify()`. No payload—just a signal that the observed entity changed. Use for simple "please re-render" or "please re-check state" patterns.
- **`subscribe`**: fires on `cx.emit(event)`. Carries a typed payload. Use when the consumer needs to know *what* changed, not just *that* something changed.

---

## Context Types

GPUI uses context types to control what capabilities are available at any point in the code. This is a deliberate design choice: by narrowing the context type, GPUI prevents you from accidentally doing things that don't make sense in that scope (e.g., calling `cx.notify()` when you're not inside an entity update, or holding a reference across an await point).

All contexts implement the `AppContext` trait, which gives access to core operations like creating entities, reading/writing globals, and spawning tasks. More specific contexts add their own capabilities.

### `App` — The Root Context

`App` is the top-level context, handed to you in `application().run(...)`. It has no associated entity—it represents the application itself. From `App` you can:

- Open and close windows
- Read and write global state
- Access platform services (clipboard, URLs, keychain, displays)
- Create entities (though typically you do this inside a `Context<T>` that binds them to a parent)

```rust
application().run(|cx: &mut App| {
    cx.set_global(MyGlobal::new());
    cx.open_window(WindowOptions::default(), |_, cx| {
        cx.new(|_| MyView { ... })
    });
});
```

### `Context<T>` — Entity-Scoped Context

`Context<T>` is what you receive inside `Entity::update()` closures and `Render::render()`. It wraps an `&mut App` (and derefs to it) but also carries the identity of the entity you're operating on. This enables entity-scoped operations:

- `cx.notify()` — mark this entity as changed (triggers observer callbacks and re-renders)
- `cx.emit(event)` — emit a typed event from this entity
- `cx.entity()` / `cx.weak_entity()` — get a handle to this entity
- `cx.observe(...)` / `cx.subscribe(...)` — observe other entities from this one's perspective

```rust
impl Counter {
    fn increment(&mut self, cx: &mut Context<Self>) {
        self.count += 1;
        cx.notify();     // Only makes sense because we're inside Counter's context
    }
}
```

### `AsyncApp` — Async-Safe Context

`AsyncApp` is the context you receive in `cx.spawn()` futures. It has a `'static` lifetime so it can be held across `.await` points. Internally it holds a `Weak<AppCell>`, which means:

- Methods on `AsyncApp` will panic if the `App` has been dropped by the time you call them.
- In practice this doesn't happen in foreground tasks because the executor checks whether the app is alive before polling each task.
- You must call `.update()` on entity handles to access state—you cannot hold a borrow across an await.

```rust
cx.spawn(|this: WeakEntity<MyView>, mut cx: AsyncApp| async move {
    let data = fetch_data().await;          // Network I/O, doesn't touch GPUI state
    this.update(&mut cx, |view, cx| {       // Re-acquire borrow after await
        view.data = data;
        cx.notify();
    }).log_err();
}).detach();
```

### `Window` — Window-Scoped Context

`Window` appears in function signatures *before* `cx` (as in `fn render(&mut self, window: &mut Window, cx: &mut Context<Self>)`). It provides window-specific operations:

- Focus management, cursor style, and input dispatch
- Drawing primitives (`paint_quad`, `paint_path`)
- Layout queries (`request_layout`, `layout_bounds`)
- Window geometry (`window_bounds`, `scale_factor`, `content_mask`)

The distinction between `Window` and `App`/`Context<T>` matters: everything on `Window` is scoped to one window. `App` operations affect the entire application.

### Context Relationships

```
AppContext (trait)        ── core operations (new entity, spawn, globals)
├── App                   ── the root, no entity association
│   └── dereffed by Context<T>
├── AsyncApp              ── static lifetime, held across await points
└── VisualContext (trait) ── requires a window
    ├── Window            ── window-specific operations
    └── TestAppContext    ── testing variant
```

---

## Elements & Rendering

Elements are the fundamental UI building blocks. Conceptually, GPUI's rendering is *immediate mode*: every frame, the element tree is built from scratch by calling `Render::render()` on views, and the tree is discarded after painting. But thanks to view caching, only views that called `cx.notify()` actually re-execute their render logic—the rest reuse their previous layout and paint data.

### The Element Lifecycle

Each element goes through three phases per frame:

1. **`request_layout()`** — The element describes its desired size and layout properties to Taffy, GPUI's layout engine. It returns a `LayoutId` used to query computed bounds later.

2. **`prepaint()`** — After Taffy computes the layout, the element receives its final `Bounds<Pixels>`. This is where hitboxes are inserted, accessibility nodes are pushed, and focus is tracked. Hitboxes registered here determine what the user can click on.

3. **`paint()`** — The element issues draw commands to the `Scene` (colored quads, glyphs, paths, shadows, images). These are batched and sent to the GPU at the end of the frame.

```rust
pub trait Element: 'static + IntoElement {
    type RequestLayoutState: 'static;
    type PrepaintState: 'static;

    fn id(&self) -> Option<ElementId>;
    fn request_layout(&mut self, ...) -> (LayoutId, Self::RequestLayoutState);
    fn prepaint(&mut self, ...) -> Self::PrepaintState;
    fn paint(&mut self, ...);
}
```

The associated types `RequestLayoutState` and `PrepaintState` allow elements to carry data between phases—for example, a `div` element stores child layout IDs in `RequestLayoutState` so it can position children during `prepaint`.

### `IntoElement`

`IntoElement` is the trait that connects Rust types to the element system. Anything that implements it can be passed to `.child()` or returned from `Render::render()`. `Entity<T>` automatically implements `IntoElement` when `T: Render`—this is what makes views work as children.

```rust
pub trait IntoElement: Sized {
    type Element: Element;
    fn into_element(self) -> Self::Element;
}
```

### `RenderOnce` — Stateless Components

`RenderOnce` is for reusable UI components that don't have their own persistent state. Unlike `Render`, which borrows `&mut self` (because the view entity owns the state), `RenderOnce` takes `self` by value. This makes components lightweight: you construct them, call `.render()`, and they're consumed.

`#[derive(IntoElement)]` on a `RenderOnce` type lets you use the component directly as a child without calling `.render()` manually:

```rust
#[derive(IntoElement)]
struct MyButton {
    label: SharedString,
    on_click: Arc<dyn Fn(&ClickEvent, &mut Window, &mut App)>,
}

impl RenderOnce for MyButton {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .px_4().py_2()
            .bg(rgb(0x2563eb))
            .rounded_md()
            .text_color(white())
            .child(self.label)
            .on_click(self.on_click)
    }
}

// Usage — MyButton is used directly as a child:
div().child(MyButton { label: "Save".into(), on_click: save_handler })
```

### `ParentElement` — Adding Children

Containers implement `ParentElement` to receive child elements. `div` does this out of the box; for custom containers, you implement `extend()` to collect `AnyElement` instances:

```rust
pub trait ParentElement {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>);
    fn child(mut self, child: impl IntoElement) -> Self;
    fn children(mut self, children: impl IntoIterator<Item = impl IntoElement>) -> Self;
}
```

### `Drawable<E>` and `AnyElement`

`Drawable<E>` is the internal wrapper that manages an element's lifecycle phases (starting → layout requested → layout computed → prepainted → painted). You never interact with it directly unless you're implementing `Element`.

`AnyElement` is the type-erased element handle—it stores a `Drawable<dyn ElementObject>` allocated in GPUI's per-frame arena. This is how heterogeneous element trees work: a `div` can hold children of different concrete element types because they're all stored as `AnyElement`.

```rust
let mut element: AnyElement = div().child("Hello").into_any_element();
element.request_layout(window, cx);
element.prepaint(window, cx);
element.paint(window, cx);
```

---

## Views (Renderable Entities)

A **view** is an `Entity<T>` where `T: Render`. Views are the primary way to build UI: you store application state in the entity, and `Render::render()` declares what that state should look like on screen.

### `Render` Trait

```rust
pub trait Render: 'static + Sized {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement;
}
```

The `render` method receives `&mut self` (so you can read state fields), `&mut Window` (for window queries), and `&mut Context<Self>` (for registering event listeners via `cx.listener(...)`). You return any type that implements `IntoElement`—typically a `div()` with children.

### View Caching

GPUI automatically caches view output. Here's how it works:

1. When `AnyView.prepaint()` runs, it checks whether `cx.notify()` was called on this entity since the last frame.
2. It also checks whether the view's bounds, content mask, and text style match the previous frame.
3. If nothing changed, GPUI *skips* calling `Render::render()` and reuses the previous layout and paint commands.
4. If something changed, it re-renders and stores the new output for future caching.

This means you should call `cx.notify()` whenever your entity's state changes in a way that affects its visual appearance. Without it, the UI will appear frozen even though the data changed.

For even more aggressive caching, `AnyView::cached(style)` allows you to specify a `StyleRefinement` that gets applied to the cached output, letting GPUI skip rendering even when layout properties (like padding or color) change:

```rust
AnyView::cached(style: StyleRefinement)
```

### `AnyView` — Type-Erased Views

Windows don't know the concrete type of their root view—they operate on `AnyView`, which stores:
- An `AnyEntity` handle to the underlying entity
- A function pointer to `<V as Render>::render` (for re-rendering)

When you use `Entity<V>` as an element (which works because `Entity<V>: IntoElement` for `V: Render`), GPUI wraps it in an `AnyView` and calls `Render::render()` through the stored function pointer.

---

## Styling API

GPUI's styling is inspired by Tailwind CSS: a fluent builder API where each method call sets one style property. The `Styled` trait provides these methods, and the underlying data is a `StyleRefinement`—a sparse set of overrides that get merged with a default `Style`.

The design philosophy: styles are *declarative* and *composable*. You build up a style by chaining method calls. The order of calls doesn't usually matter because each targets a specific property. Conditional styles use `.when(cond, |style| ...)` which applies refinements only when a condition holds.

### Layout

These control the display mode and flex behavior of an element:

```rust
.flex()          // display: flex
.flex_col()      // flex-direction: column
.flex_row()      // flex-direction: row
.grid()          // display: grid
.flex_1()        // flex: 1 (grow and shrink equally)
.flex_none()     // flex: none (don't grow or shrink)
.flex_grow()     // flex-grow: 1
.flex_shrink()   // flex-shrink: 1
```

### Size & Spacing

GPUI uses strongly-typed units. `px(200.0)` is `Pixels`, `rems(1.5)` is relative to font size, and `percentage(50.0)` is a fraction of the parent. Methods like `.p_4()` use `rems` under the hood (1 unit = 0.25rem = 4px at default font size):

```rust
.w(px(200.))       // width in pixels
.h(px(100.))       // height in pixels
.size(px(50.))     // both width and height
.size_full()       // 100% width and height
.w_full()          // 100% width
.h_full()          // 100% height
.p_4()             // padding: 1rem on all sides
.px_2()            // padding-left and padding-right: 0.5rem
.py_3()            // padding-top and padding-bottom: 0.75rem
.pt(px(10.))       // padding-top in explicit pixels
.m_2()             // margin: 0.5rem
.gap_2()           // gap between children: 0.5rem
```

### Alignment

Flexbox and grid alignment properties:

```rust
.justify_center()   // justify-content: center
.justify_between()  // justify-content: space-between
.justify_around()   // justify-content: space-around
.items_center()     // align-items: center
.items_start()      // align-items: flex-start
.items_end()        // align-items: flex-end
.self_center()      // align-self: center
.self_stretch()     // align-self: stretch
```

### Colors & Background

Colors in GPUI are represented as `Hsla` (hue, saturation, lightness, alpha). Constructors exist for common formats:

```rust
.bg(rgb(0xff0000))              // background: red (alpha = 1.0)
.bg(rgba(0xff000080))           // background: red at 50% opacity
.bg(hsla(0.0, 1.0, 0.5, 1.0))  // background: red via HSL
.text_color(rgb(0x333333))      // text color
.bg(gpui::red())                // predefined named color

// Pseudo-class states via closures:
.hover(|style| style.bg(gpui::blue()))     // background on hover
.active(|style| style.bg(gpui::green()))   // background while pressed
.focus(|style| style.border_color(gpui::blue()))  // when focused
```

Available predefined colors: `gpui::red()`, `gpui::blue()`, `gpui::green()`, `gpui::white()`, `gpui::black()`, `gpui::yellow()`, `gpui::transparent_black()`, and more.

### Borders

```rust
.border_1()         // 1px solid border on all sides
.border_2()         // 2px border
.border_color(rgb(0xcccccc))
.border_dashed()    // dashed style instead of solid
.rounded_md()       // medium border-radius
.rounded_full()     // fully rounded (for circles/pills)
.rounded_none()     // no border-radius
```

### Text Styling

GPUI renders text through its own text system (not the system font renderer). Font size steps follow a scale:

```rust
.text_xs()          // extra small
.text_sm()          // small
.text_base()        // base (default)
.text_lg()          // large
.text_xl()          // extra large
.text_2xl()         // 2x large
.font_weight(FontWeight::BOLD)
.text_center()      // text-align: center
.text_left()        // text-align: left
.text_right()       // text-align: right
.truncate()         // single-line: overflow hidden + ellipsis
.line_clamp(3)      // multi-line clamp to 3 lines + overflow hidden
.whitespace_nowrap()
```

### Other Style Properties

```rust
.shadow_md()          // medium box shadow
.shadow_lg()          // large box shadow
.shadow_none()        // no shadow
.opacity(0.5)         // 50% opacity
.visible()            // visibility: visible
.invisible()          // visibility: hidden (still occupies layout space)
.overflow_hidden()    // overflow: hidden
.overflow_scroll()    // overflow: scroll
.overflow_y_scroll()  // vertical scroll only
.cursor(CursorStyle::PointingHand)  // change cursor
```

### Conditional Styling with `when` / `when_some`

These methods apply style refinements only when a condition is true or an `Option` is `Some`:

```rust
div()
    .when(is_active, |this| this.bg(gpui::blue()))
    .when_some(tooltip_text, |this, text| {
        this.tooltip(move |_window, cx| { Tooltip::text(text.clone(), cx) })
    })
```

### Group Styling

Group styling lets a child element change its appearance when a *parent* is hovered:

```rust
div()
    .group("my-group")   // declare a named group
    .child(
        div()
            .group_hover("my-group", |style| style.bg(gpui::blue()))
            .child("I turn blue when the parent is hovered")
    )
```

### Available Units

- `px(10.0)` — device-independent pixels
- `rems(1.5)` — relative to the current font size (1rem = 16px at default)
- `percentage(50.0)` — percentage of the parent's size in that axis
- `relative(0.5)` — fraction of available space (used in flex/grid)

### Color Constructors

- `rgb(0xRRGGBB)` — 24-bit color, alpha = 1.0
- `rgba(0xRRGGBBAA)` — 32-bit color with alpha
- `hsla(h, s, l, a)` — hue (0–1), saturation, lightness, alpha
- `transparent_black()` — fully transparent (useful for overlays)
- Predefined: `gpui::red()`, `gpui::blue()`, `gpui::green()`, `gpui::white()`, `gpui::black()`, `gpui::yellow()`, `opaque_grey()`, etc.

---

## Interactivity & Events

GPUI handles input through *hitboxes*. When an element calls `window.insert_hitbox(bounds, ...)` during `prepaint()`, it registers a region that can receive mouse and scroll events. When a mouse event occurs, GPUI does a hit test across hitboxes from front to back (z-order), then dispatches the event to matching listeners.

### Mouse Events

The most common pattern is registering event handlers directly on a `div` using the fluent API from `InteractiveElement`:

```rust
div()
    .on_click(|event: &ClickEvent, window: &mut Window, cx: &mut App| {
        // event contains position, button, modifiers, click count
    })
    .on_mouse_down(|event: &MouseDownEvent, window, cx| { ... })
    .on_mouse_up(|event: &MouseUpEvent, window, cx| { ... })
    .on_mouse_move(|event: &MouseMoveEvent, window, cx| { ... })
    .on_scroll_wheel(|event: &ScrollWheelEvent, window, cx| { ... })
```

A `ClickEvent` is a higher-level event: GPUI synthesizes it from a mouse-down followed by a mouse-up on the same element (without exceeding the drag threshold). If the user presses down, drags, and releases elsewhere, no click is generated.

### Using `cx.listener` for Entity Event Handlers

When you're inside a view's `Render::render()`, you usually want event handlers to update the view's state. `cx.listener()` wraps a method on your entity type so it can be used as an event callback:

```rust
impl Render for MyView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .on_click(cx.listener(|this: &mut Self, event: &ClickEvent, window, cx| {
                // 'this' is &mut MyView — the entity's state
                // 'cx' is &mut Context<MyView> — scoped to this entity
                this.click_count += 1;
                cx.notify();  // trigger re-render
            }))
            .on_mouse_move(cx.listener(Self::on_mouse_move))  // method reference also works
    }
}
```

The `cx.listener` pattern ensures the event handler has access to both the entity's mutable state and the entity-scoped context. Without it, the handler would only receive `&mut App`, which can't call `cx.notify()` for a specific entity.

### Focus Management

Keyboard events are directed to the *focused* element. To participate in focus, an entity must implement `Focusable` and use `.track_focus()` in its render:

```rust
struct MyInput {
    focus_handle: FocusHandle,
}

impl Focusable for MyInput {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for MyInput {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus_handle)  // register this element for keyboard focus
            .when(self.focus_handle.is_focused(cx), |style| {
                style.border_color(gpui::blue())  // visual focus indicator
            })
    }
}
```

Focus can be requested programmatically:
```rust
cx.focus(&my_input_entity);         // from any Context
focus_handle.focus(window, cx);     // from Window context
```

For keyboard-driven tab navigation, elements can declare tab stop positions:

```rust
div()
    .tab_stop(TabStopIndex::first(0))
    .tab_stop(TabStopIndex::last(1))
```

### Drag & Drop

GPUI's drag-and-drop is type-safe: the drag carries a state value of a specific Rust type, and drop targets specify which type they accept.

```rust
// Starting a drag — the closure creates the drag state entity:
div().on_drag::<MyDragState>(|event, window, cx| {
    cx.new(|cx| MyDragState { start_position: event.position })
})

// Receiving drag-move events — fires as the mouse moves over this element:
div()
    .on_drag_move::<MyDragState>(|event: &DragMoveEvent<MyDragState>, window, cx| {
        // event.drag(cx) returns &MyDragState
        // Can return DragMoveResult to indicate acceptance/rejection
    })
    .on_drop::<MyDragState>(|event, window, cx| {
        // Handle the drop
    })
```

The type parameter `MyDragState` is key: a drag creates one state value, and only elements that register handlers for `MyDragState` will receive drag-move and drop events. This prevents accidental cross-type drag interactions.

### Input Handling for Text

For text input fields, GPUI provides the `InputHandler` trait. Instead of listening to individual key events, implement `InputHandler` to receive composed text input (handling IME, dead keys, and platform text editing):

```rust
impl InputHandler for MyTextInput {
    fn text_for_range(&mut self, range: Range<usize>, cx: &mut Context<Self>) -> Option<String> { ... }
    fn replace_text_in_range(&mut self, range: Option<Range<usize>>, text: &str, window: &mut Window, cx: &mut Context<Self>) { ... }
    fn selected_text_range(&mut self, cx: &mut Context<Self>) -> Option<Range<usize>> { ... }
    // ... other methods
}
```

---

## Actions & Key Bindings

Actions are the bridge between keyboard input and application logic. Instead of hard-coding key-to-function mappings, GPUI uses a declarative system: you define action types, bind keys to them (in code or JSON), and register handlers on specific elements.

### Why Actions?

The indirection between keystrokes and behavior enables several important features:

- **Rebindable keys**: users can customize keybindings without changing code.
- **Context-dependent dispatch**: the same keystroke can do different things depending on which view has focus (e.g., `Enter` sends a message in chat but inserts a newline in an editor).
- **Multi-key sequences**: like Vim's `g g` or VS Code's `Ctrl+K Ctrl+Left`.
- **Menu integration**: the same action can be triggered by a menu item or a keyboard shortcut.

### Declaring Actions

Simple actions (no data) use the `actions!` macro. It generates unit structs with the `Action` trait derived:

```rust
actions!(
    editor,
    [
        MoveUp,
        MoveDown,
        SelectAll,
        Cut,
        Copy,
        Paste,
    ]
);
// Creates: editor::MoveUp, editor::MoveDown, etc.
```

For actions that carry data, use `#[derive(Action)]`:

```rust
#[derive(Clone, PartialEq, serde::Deserialize, schemars::JsonSchema, Action)]
#[action(namespace = editor)]
pub struct SelectNext {
    pub replace_newest: bool,
}
```

The `Clone`, `PartialEq`, and serde derives are required by the `Action` derive. Use `#[action(no_json)]` if your action type doesn't need to be deserialized from JSON keymaps.

### Binding Keys to Actions

Key bindings can be set programmatically:

```rust
cx.bind_keys([
    KeyBinding::new("cmd-c", editor::Copy, Some("Editor")),
    KeyBinding::new("cmd-v", editor::Paste, Some("Editor")),
    KeyBinding::new("up", editor::MoveUp, Some("Editor")),
    KeyBinding::new("cmd-k left", pane::SplitLeft, Some("Pane")),  // multi-key sequence
]);
```

The third argument is the `key_context` — a string that must match the `.key_context(...)` set on an element. This is how the same keystroke dispatches different actions in different contexts.

In Zed, most bindings live in `assets/keymaps/default-{platform}.json` and are loaded at startup.

### Handling Actions

Action handlers are methods on your entity that take the action type as their second parameter:

```rust
impl Editor {
    fn copy(&mut self, _: &editor::Copy, _: &mut Window, cx: &mut Context<Self>) {
        // Copy current selection to clipboard
    }
    fn paste(&mut self, _: &editor::Paste, _: &mut Window, cx: &mut Context<Self>) {
        // Paste from clipboard
    }
}
```

Register them on elements with `.on_action(cx.listener(...))`:

```rust
impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus_handle)
            .key_context("Editor")                         // match against key bindings
            .on_action(cx.listener(Editor::copy))          // handle editor::Copy
            .on_action(cx.listener(Editor::paste))         // handle editor::Paste
    }
}
```

### How Dispatch Works

When a key is pressed:

1. GPUI builds a `DispatchTree` during `prepaint()`. Each element that registers key/action handlers gets a node. The tree records parent-child relationships and `key_context` strings.
2. The keystroke is matched against the `Keymap`. Matching considers the current `key_context` of the focused element and all its ancestors.
3. Matches bubble from the focused element upward through ancestors. The deepest match wins—if `Editor` has a binding for `Enter` and `Pane` also has one, `Editor`'s binding fires.
4. The bound action is dispatched to the matching node's action listeners.

### Dispatch Phases

Events propagate in two phases, though for actions only the bubble phase is typically relevant:

- **Capture**: root → target (rarely used for actions; useful for clearing transient state like "pressed" button appearance)
- **Bubble** (default): target → root

```rust
.on_action(DispatchPhase::Bubble, cx.listener(handler))  // explicit phase
```

Action handlers can call `cx.stop_propagation()` to prevent the action from continuing up the tree.

---

## Concurrency & Async

GPUI runs on a single foreground thread for all UI work, but provides async facilities for I/O, background computation, and timers. The key constraint: you can never hold a borrow of entity state across an `.await` point. Instead, you release the borrow, do async work, then re-acquire it.

### Foreground Tasks with `cx.spawn`

Use `cx.spawn()` from within a `Context<T>` update to run an async block on the foreground executor. The closure receives a `WeakEntity<T>` (to safely re-acquire state after await) and `AsyncApp` (the async-safe context):

```rust
cx.spawn(|this: WeakEntity<MyView>, mut cx: AsyncApp| async move {
    // 1. Do async I/O — no GPUI state held here
    let data = fetch_data().await;

    // 2. Re-acquire entity state and update
    this.update(&mut cx, |view, cx| {
        view.data = data;
        cx.notify();
    }).log_err();  // handle the case where the view was dropped
}).detach();
```

`this.update()` returns `Result` — it fails if the entity was dropped while the async work was running. Always handle this (`.log_err()` or `.ok()`) rather than unwrapping.

### Background Tasks with `cx.background_spawn`

For CPU-intensive work that shouldn't block the UI thread, spawn on the background executor from within a foreground task or directly from a sync context:

```rust
cx.spawn(|this: WeakEntity<MyView>, mut cx: AsyncApp| async move {
    let result = cx.background_spawn(async move {
        heavy_computation()  // runs on a thread pool
    }).await;

    this.update(&mut cx, |view, cx| {
        view.result = result;
        cx.notify();
    }).ok();
}).detach();
```

### Managing `Task<R>`

Both `cx.spawn()` and `cx.background_spawn()` return `Task<R>`, a cancelable future:

- **Cancel on drop**: dropping a `Task` cancels the work. This is important—if a view is closed while an async operation is in flight, dropping the task ensures it doesn't try to update a dead view.
- **`.detach()`**: runs the task independently, ignoring cancellation. Use when the task should outlive the current scope (but be aware it may try to update a dead view).
- **`.detach_and_log_err(cx)`**: detach and log any `Err` results.
- **Awaiting**: in another async context, you can `.await` a `Task` to get its result.
- **`Task::ready(value)`**: create an already-completed task.

```rust
// Store a task to cancel it later (e.g., when the view is dropped):
struct MyView {
    _fetch_task: Option<Task<()>>,
}

// Cancel previous fetch before starting a new one:
self._fetch_task = Some(cx.spawn(|this, cx| async move { ... }).detach());
```

### Spawning with Window Access

`cx.spawn_in(window, ...)` is like `cx.spawn()` but provides `AsyncWindowContext`, which has both `AsyncApp` and window-specific methods:

```rust
cx.spawn_in(window, |this: WeakEntity<MyView>, mut cx: AsyncWindowContext| async move {
    // Can call window-specific operations after await
    cx.dispatch_action(SomeAction, &mut cx).await;
}).detach();
```

### Application Shutdown

To run cleanup code when the application quits, use `cx.on_app_quit()`:

```rust
cx.on_app_quit(|this: WeakEntity<MyView>, mut cx: AsyncApp| async move {
    // Save state, close connections, etc.
    // Must complete within SHUTDOWN_TIMEOUT (200ms by default)
}).detach();
```

### Variable Shadowing Pattern

When spawning tasks that capture values, use variable shadowing to scope clones for clarity:

```rust
let task_ran = Rc::new(Cell::new(false));
executor.spawn({
    let task_ran = task_ran.clone();  // shadow — clones into the async scope
    async move {
        *task_ran.borrow_mut() = true;
    }
}).detach();
```

---

## Observability & Subscriptions

GPUI's observability system is how entities communicate state changes without tight coupling. There are two complementary mechanisms:

### `notify` / `observe` — Broad Change Notification

`cx.notify()` is a general-purpose "this entity changed" signal. Any entity that called `cx.observe(&target, callback)` will have its callback invoked. The callback receives a handle to the observed entity and must re-read whatever state it needs.

**When to use `notify`/`observe`:**
- The observer just needs to know that *something* changed and can re-derive what it needs by reading the observed entity.
- Re-rendering a view when its model data changes.
- Simple parent-child relationships where the child should update when the parent changes.

```rust
// Producer:
entity.update(cx, |this, cx| {
    this.data = new_data;
    cx.notify();  // "I changed — observers, check me again"
});

// Consumer:
cx.observe(&other_entity, |this: &mut Observer, other, cx| {
    // other is Entity<OtherType> — a handle, not a reference
    this.latest_value = other.read(cx).data.clone();
}).detach();
```

### `emit` / `subscribe` — Typed Events

For richer communication where the consumer needs to know *what* specifically changed, use typed events. This requires implementing the `EventEmitter<E>` marker trait on the emitting entity.

**When to use `emit`/`subscribe`:**
- The observer needs to react differently depending on what happened (e.g., `ItemAdded` vs `ItemRemoved`).
- The event carries data that the observer needs but isn't stored on the emitting entity.
- You want multiple subscribers to receive the same event without each having to inspect the entity's full state.

```rust
// Define an event type:
struct ItemAddedEvent { index: usize, item: String }
struct ItemRemovedEvent { index: usize }

// Declare that the entity can emit these:
impl EventEmitter<ItemAddedEvent> for MyList {}
impl EventEmitter<ItemRemovedEvent> for MyList {}

// Emit from within an update:
cx.emit(ItemAddedEvent { index: 0, item: "hello".into() });

// Subscribe from another entity:
cx.subscribe(
    &list_entity,
    |this: &mut Observer, list, event: &ItemAddedEvent, cx| {
        log::info!("Item added at {}: {}", event.index, event.item);
    },
).detach();
```

### Global Observers

You can observe changes to `Global` state—application-level singletons:

```rust
cx.observe_global::<Theme>(|this: &mut MyView, cx| {
    let theme = cx.global::<Theme>();
    // React to theme change — e.g., re-render
}).detach();
```

Global observers fire when `cx.set_global()` or `cx.update_global()` is called for the observed type.

### Self-Observation

A view can observe its own notifications—useful for triggering side effects:

```rust
cx.observe_self(|this: &mut MyView, cx: &mut Context<MyView>| {
    // Called whenever cx.notify() is called on this entity
}).detach();
```

### Subscription Management

Every observe/subscribe call returns a `Subscription`. This handle controls the lifetime of the observation:

```rust
struct MyView {
    _subscriptions: Vec<Subscription>,
}

// Store to keep observations alive:
self._subscriptions.push(cx.observe(&other, callback));

// When MyView is dropped, _subscriptions is dropped,
// and all observations are canceled automatically.
```

- **`.detach()`**: the observation continues for the lifetime of the observed entity, regardless of the `Subscription` handle's lifetime.
- **`Subscription::join(a, b)`**: combine two subscriptions into one.
- **Drop**: when a `Subscription` is dropped, it calls its internal unsubscribe function, removing the callback from the observed entity's subscriber list.

---

## Windows & Platform

Windows in GPUI are native OS windows with their own root view. The platform layer abstracts macOS (`NSWindow`), Linux (Wayland/X11), and Windows (Win32) behind a common `Platform` trait.

### Creating Windows

`cx.open_window()` takes `WindowOptions` and a closure that returns the root entity:

```rust
use gpui::{Bounds, WindowOptions, WindowBounds, size, px};

let bounds = Bounds::centered(None, size(px(800.0), px(600.0)), cx);
cx.open_window(
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        ..Default::default()
    },
    |window: &mut Window, cx: &mut App| {
        cx.new(|_cx| MyRootView { ... })
    },
)?;
```

The `WindowOptions` struct controls title bar style, window decorations, focus behavior, and more.

### Window Bounds & Types

```rust
WindowBounds::Windowed(bounds)   // Normal window with explicit bounds
WindowBounds::Maximized          // Maximized
WindowBounds::Fullscreen         // Fullscreen
```

### Window Handle

`WindowHandle<V>` is a typed handle to a window whose root view is `V`. It can be downgraded to `AnyWindowHandle` for storage in heterogeneous collections:

```rust
let handle: WindowHandle<MyRootView> = cx.open_window(...)?;
let any_handle: AnyWindowHandle = handle.into();
```

### Window Services

The `Window` context provides window-specific operations:

- `window.dispatch_action(action, cx)` — programmatically trigger an action
- `window.activate_window()` — bring this window to the foreground
- `window.set_window_title("Title")`
- `window.show_character_palette()` — open the system emoji/symbol picker
- `window.with_content_mask(mask, |window| ...)` — draw outside normal bounds (for popovers)
- `window.window_bounds()` / `window.set_window_bounds()` — get/set window geometry
- `window.play_system_bell()` — auditory/visual alert
- `window.is_fullscreen()` — query fullscreen state
- `window.scale_factor()` — device pixel ratio (1.0 = standard, 2.0 = Retina)
- `window.set_cursor_style(CursorStyle::PointingHand)`

### App-Level Window Operations

```rust
cx.windows()                        // Vec<AnyWindowHandle> — all open windows
cx.active_window()                  // Option<AnyWindowHandle> — currently focused
cx.window_stack()                   // Option<Vec<AnyWindowHandle>> — Z-ordered, front to back
cx.refresh_windows()               // Force all windows to redraw next frame
```

### Platform Services

GPUI exposes platform APIs through `App`:

```rust
// Clipboard
cx.write_to_clipboard(ClipboardItem::new_string("copied text"));
if let Some(item) = cx.read_from_clipboard() {
    let text = item.text();  // Option<String>
}

// URLs & filesystem
cx.open_url("https://example.com");
cx.open_with_system(&path);       // Open with default app
cx.reveal_path(&path);            // Reveal in Finder/Explorer

// Credentials (OS keychain)
cx.write_credentials("https://example.com", "username", b"password").detach();
cx.read_credentials("https://example.com").detach();
cx.delete_credentials("https://example.com").detach();

// Displays
let displays: Vec<Rc<dyn PlatformDisplay>> = cx.displays();
let primary: Option<Rc<dyn PlatformDisplay>> = cx.primary_display();

// File dialogs
let rx = cx.prompt_for_paths(PathPromptOptions::default());
// rx is a oneshot::Receiver<Result<Option<Vec<PathBuf>>>>
```

### Quit Behavior

Control when the application exits:

```rust
application()
    .with_quit_mode(QuitMode::LastWindowClosed)  // Auto-quit when last window closes
    // .with_quit_mode(QuitMode::Explicit)       // Only quit via cx.quit()
    .run(|cx| { ... });

// Programmatic quit:
cx.quit();
```

The default mode is `QuitMode::Default`, which auto-quits on non-macOS platforms and requires explicit quit on macOS.

### Application Menus

Set the menu bar:

```rust
cx.set_menus(vec![
    Menu {
        name: "File".into(),
        items: vec![
            MenuItem::action("New", editor::NewFile),
            MenuItem::action("Open...", workspace::Open),
            MenuItem::separator(),
            MenuItem::action("Quit", app::Quit),
        ],
    },
]);
```

Menu items that reference actions automatically use the keybinding display from the keymap.

---

## Layout System

GPUI delegates layout computation to [Taffy](https://github.com/DioxusLabs/taffy), a pure-Rust implementation of CSS Flexbox, CSS Grid, and block layout. This means GPUI's layout model is essentially the web layout model—if you know CSS flexbox, you already know GPUI layout.

Layout is computed during `request_layout()`. Elements submit a `Style` to Taffy, and Taffy returns a `LayoutId`. After all elements register, Taffy solves the constraint system and assigns each element a position and size. Elements then query their computed bounds with `window.layout_bounds(layout_id)`.

### Flexbox

Flexbox is the most commonly used layout mode in GPUI:

```rust
div()
    .flex()               // display: flex
    .flex_col()           // flex-direction: column (default is row)
    .flex_row()           // flex-direction: row (explicit)
    .flex_wrap()          // flex-wrap: wrap
    .gap_2()              // gap between children
    .justify_center()     // main-axis alignment
    .justify_between()    // space-between
    .justify_around()     // space-around
    .items_center()       // cross-axis alignment
    .items_start()
    .items_end()
    .self_stretch()       // override cross-axis for this child
    .flex_1()             // flex-grow: 1, flex-shrink: 1, flex-basis: 0%
    .flex_grow()          // flex-grow: 1 (grow to fill space)
    .flex_shrink()        // flex-shrink: 1 (shrink if needed)
    .flex_none()          // flex: none (don't grow or shrink)
```

### Grid Layout

For two-dimensional layouts:

```rust
div()
    .grid()               // display: grid
    .grid_cols_2()        // grid-template-columns: repeat(2, 1fr)
    .grid_cols_3()
    .grid_rows_2()
    .gap_2()
    .col_span_2()         // grid-column: span 2
    .row_span_2()         // grid-row: span 2
```

### Positioning

Elements default to the normal flow. Override with:

```rust
div()
    .relative()           // position: relative (offset from normal position)
    .absolute()           // position: absolute (offset from nearest positioned ancestor)
    .top(px(10.))         // offset from top
    .left(px(20.))
    .right(px(5.))
    .bottom(px(0.))
```

### Anchored Positioning

For floating UI (popovers, dropdowns, tooltips), anchored positioning snaps an element to a corner of its parent, with automatic edge detection to keep it on screen:

```rust
div()
    .absolute()
    .anchored()                              // enable anchored positioning
    .anchor(AnchorCorner::BottomRight)       // which corner to anchor to
    .snap_to_window()                        // ensure the element stays on screen
    .snap_to_window_with_margin(px(8.))      // with a margin from the window edge
```

Anchored elements also implement the `deferred` pattern—they only render when they're actually visible, which avoids wasted layout work for closed popovers.

### Overflow & Scrolling

Control how content that exceeds an element's bounds is handled:

```rust
div()
    .overflow_hidden()      // clip content
    .overflow_x_scroll()    // horizontal scrollbar
    .overflow_y_scroll()    // vertical scrollbar
    .overflow_scroll()      // both scrollbars
```

Scrolling in GPUI is GPU-accelerated—the scroll offset is applied as a translation transform during painting, so no re-layout is needed.

### Layout IDs for Manual Queries

In custom elements, you can query computed bounds after Taffy resolves the layout:

```rust
let layout_id = window.request_layout(my_style, None, cx);
// ... (after layout resolution) ...
let bounds: Bounds<Pixels> = window.layout_bounds(layout_id);
```

---

## Built-in Elements

GPUI ships with several element types that cover common use cases. Each is implemented as a struct with `Element` (and often `Styled` and `InteractiveElement`).

### `div` — The Universal Container

`div` is the workhorse element. It combines layout, styling, and interactivity into one type. Most views render a `div` tree:

```rust
div()
    .flex()
    .flex_col()
    .gap_2()
    .p_4()
    .bg(rgb(0xf0f0f0))
    .rounded_md()
    .child("Content")
    .on_click(cx.listener(MyView::on_click))
```

Under the hood, `div` manages child layout, hitboxes for all mouse events, tooltip timing, focus tracking, drag-and-drop state, and accessibility node generation.

### `text` — Styled Text Rendering

`text()` renders a string with text-specific styling. It supports all `Styled` methods plus text-only refinements:

```rust
text("Hello, world!")
    .text_xl()
    .text_color(rgb(0x333333))
    .font_weight(FontWeight::BOLD)
    .text_align(TextAlign::Center)
    .line_clamp(2)
```

### `img` — Image Display

Loads and displays images with configurable sizing behavior:

```rust
img("path/to/image.png")
    .size_full()
    .object_fit(ObjectFit::Contain)    // Contain, Cover, Fill, ScaleDown, or None
    .object_position(Point::default()) // position within bounds
```

Image loading is async—the element shows nothing until the image data arrives, then updates automatically. Caching is managed by GPUI's asset system.

### `svg` — SVG Rendering

Renders SVG files with color overrides:

```rust
svg()
    .size_8()
    .path("/path/to/icon.svg")
    .text_color(gpui::blue())     // Override fill/stroke colors in the SVG
```

### `canvas` — Custom Drawing

For programmatic drawing with arbitrary paths and shapes. The callbacks receive `Bounds<Pixels>` and access to `Window`'s drawing API:

```rust
canvas(
    |bounds, window, cx| { /* prepaint: register hitboxes */ },
    |bounds, prepaint_state, window, cx| {
        // paint: draw paths, quads, etc.
        window.paint_path(path, color);
    },
)
```

### `deferred` — Lazy Rendering

Wraps an element tree that should only be laid out and painted when actually visible. Essential for popovers and dropdowns that are often hidden:

```rust
deferred(
    div().child("This only renders when the deferred is shown")
)
```

A deferred element's contents are skipped during `request_layout` and `prepaint` until something triggers them to display (typically an anchored positioning ancestor deciding it's on-screen).

### `list` — Stateful Virtualized List

A virtualized list with mutable per-item state. Only renders items that are visible in the viewport:

```rust
// list_state stores scroll position and per-item data
let list_state = ListState::new(items.len());
list(list_state, items.len())
    .child(|state, range, window, cx| {
        state.map(move |item_state| {
            div()
                .h(px(30.))
                .child(format!("Item"))
        })
    })
```

### `uniform_list` — Simple Virtualized List

A simpler virtualized list for items with uniform height. No per-item mutable state—just a rendering function:

```rust
uniform_list(cx.view().clone(), "items", items.len(), |_this, range, _window, cx| {
    range.map(|ix| {
        div()
            .h(px(30.))
            .child(format!("Item {}", ix))
            .collect::<Vec<_>>()
    })
})
```

### `animation` — Animated Properties

Animates a numeric property over time with configurable easing:

```rust
svg()
    .with_animation(
        "my_animation",
        Animation::new(Duration::from_secs(2))
            .repeat()                              // loop forever
            .with_easing(bounce(ease_in_out)),     // easing function
        |svg, delta| svg.with_transformation(      // delta is 0.0 → 1.0
            Transformation::rotate(percentage(delta * 360.))
        ),
    )
```

Animations are driven by the frame timer and don't require any async code or manual ticking.

### `surface` — Raw Rendering Surface

For integrations with external rendering pipelines (e.g., embedding a video player or a WebGL canvas). Returns a native surface handle.

---

## Globals

Globals are application-level singletons—state that exists once per `App` and is accessible from anywhere. They're used for things like themes, configuration, and system-level services.

### Defining and Using a Global

```rust
struct Theme {
    background: Hsla,
    foreground: Hsla,
    accent: Hsla,
}

impl Global for Theme {}

// Setting (typically at startup):
cx.set_global(Theme {
    background: white(),
    foreground: black(),
    accent: blue(),
});

// Reading:
let theme = cx.global::<Theme>();
// Panics if Theme hasn't been set — use try_global() for fallible access

// Updating:
cx.update_global(|theme: &mut Theme, _cx| {
    theme.accent = green();
});

// Observing changes (re-render when theme changes):
cx.observe_global::<Theme>(|this: &mut MyView, cx| {
    let theme = cx.global::<Theme>();
    // Update this view's appearance
}).detach();
```

### When to Use Globals vs Entities

- **Globals**: application-wide, one-instance state. Theme, configuration, connection pools, caches.
- **Entities**: per-component, multi-instance state. Document data, view state, model objects.

Don't put state that varies per-window or per-document in globals. Use entities and pass them as handles.

### Built-in Globals

GPUI sets up these globals internally:

- `SystemWindowTabController` — manages window tab groups (macOS native tabs).
- `Colors` / `GlobalColors` — system color scheme, driven by the platform's light/dark mode.
- `DebugBelow` (debug builds only) — enables drawing element outline boxes for visual debugging.

---

## Testing

GPUI has a first-class testing framework with support for deterministic async execution, simulated input, and window-based tests.

### `#[gpui::test]` — The Test Macro

Replace `#[test]` or `#[tokio::test]` with `#[gpui::test]`. The async test function receives a `TestAppContext`:

```rust
#[gpui::test]
async fn test_my_component(mut cx: TestAppContext) {
    // Create a window with a root view
    let window = cx.open_window(WindowOptions::default(), |cx| {
        cx.new(|_| MyComponent::new())
    }).await;

    // Dispatch an action and wait for it to be handled
    window.dispatch_action(SomeAction, &mut cx).await;

    // Simulate a keystroke
    window.dispatch_keystroke("enter", &mut cx).await;

    // Read the view's state and assert
    let view = window.root_view::<MyComponent>(&mut cx).await.unwrap();
    view.read_with(&cx, |view, _cx| {
        assert_eq!(view.value, expected_value);
    });
}
```

### `TestAppContext`

`TestAppContext` is an `App` bound to a deterministic executor. Key operations:

```rust
let mut cx = TestAppContext::new();

// Pump the event loop until no more work is pending:
cx.executor().run_until_parked();

// Timer that integrates with GPUI's scheduler (prefer over smol::Timer):
cx.background_executor().timer(duration).await;

// Access the underlying App:
cx.update(|app| { /* read/write state */ });
```

**Important:** Use `cx.background_executor().timer(duration).await` instead of `smol::Timer::after(duration)` for test timeouts. The GPUI timer is tracked by the scheduler; `smol::Timer` may not be and can cause `run_until_parked()` to exit early.

### Test Parameters

```rust
#[gpui::test(iterations = 100)]   // Run the test 100 times with different seeds
#[gpui::test(seed = 12345)]       // Pin to a specific seed for reproducibility
```

When a test fails with a random seed, the failure output includes the seed so you can reproduce it deterministically.

### `VisualTestContext` — Window-Level Simulation

For tests that need to simulate mouse and keyboard input at specific positions:

```rust
let mut window_cx = VisualTestContext::from_window(window.into(), &mut cx);

// Simulate mouse movement to a position in the window:
window_cx.simulate_mouse_move(point(px(100.0), px(50.0)), &mut cx).await;

// Simulate a click:
window_cx.simulate_click(point(px(100.0), px(50.0)), &mut cx).await;

// Simulate keyboard input:
window_cx.simulate_keystroke("cmd-s", &mut cx).await;
```

`VisualTestContext` drives the window's event handling as if real user input occurred, including hit testing, focus management, and action dispatch.

### Benchmarks

```rust
#[gpui::bench]
fn bench_rendering(cx: &mut BenchAppContext) {
    // Setup and benchmark rendering performance
}

bench_group!(my_benchmarks, bench_rendering);
bench_main!(my_benchmarks);
```

---

## Architecture Deep Dive

This section explains the internal mechanics of GPUI—how the pieces fit together from `main()` to the GPU.

### Application Lifecycle

When you call `application().run(...)`, here's what happens:

```
main()
  └── Application::new(platform)      // provides OS-specific behavior
        ├── Allocates App with EntityMap, Window slots, Globals, Keymap
        ├── Registers platform event handlers (quit, keyboard layout changes, etc.)
        └── .run(|cx: &mut App| {
              ├── User code runs here ——
              │   ├── cx.open_window(...)   // creates Window, root view
              │   ├── cx.set_global(...)     // sets up globals
              │   └── cx.activate(true)      // show app
              └── Platform event loop starts
                    ├── Input events → dispatched to windows
                    ├── Frame requests → window.draw(cx)
                    └── App shutdown → quit observers → exit
            })
```

### Frame Rendering Pipeline

Each frame follows a strict sequence:

```
1. INPUT DISPATCH
   Raw platform events arrive:
   ├── Key events: DispatchTree matches keystrokes → action dispatch → entity updates
   ├── Mouse events: Hit testing → event listener callbacks → entity updates
   └── Window events: Resize, focus, appearance change

2. EFFECT COLLECTION
   Entity updates call cx.notify(), cx.emit(), which queue Effects:
   ├── Notify: schedules observer callbacks for the changed entity
   └── RefreshWindows: marks windows as needing redraw

3. EFFECT FLUSHING (still within App.update())
   ├── Observer callbacks run (may produce more effects)
   └── Windows marked dirty

4. FRAME RENDERING (Window.draw())
   For each dirty window:
   ├── Render::render() called on root view → element tree built
   ├── request_layout() on each element → Taffy computes layout
   ├── prepaint() on each element → hitboxes, a11y nodes, DispatchTree nodes
   ├── paint() on each element → draw commands batched into Scene
   └── Scene submitted to GPU for presentation
```

The element tree exists only for the duration of one frame. After painting, it (and all callbacks registered during layout/prepaint) are dropped. The next frame rebuilds from scratch—but view caching means most of this is a no-op for unchanged views.

### Entity Storage

Entities are stored in a `SlotMap<EntityId, T>` inside `App::entities`. This provides:

- **O(1) lookup** by `EntityId`, with stable indices across insertions/removals.
- **Reference counting**: each `Entity<T>` handle increments a ref count; when all strong handles are dropped, the entity is removed.
- **Weak handles**: `WeakEntity<T>` stores the `EntityId` and a weak ref count; `upgrade()` checks if the entity still exists.

```
App
 ├── EntityMap
 │    ├── SlotMap<EntityId, Box<dyn Any>>    // actual entity state
 │    ├── Reference counts per entity
 │    ├── Observer sets per entity
 │    ├── Event listener sets (EntityId → TypeId → Listener)
 │    └── Release listeners (called on drop)
 │
 ├── Windows
 │    └── SlotMap<WindowId, Option<Box<Window>>>
 │
 ├── Globals
 │    └── HashMap<TypeId, Box<dyn Any>>
 │
 ├── Tracked entities per window
 │    └── HashMap<WindowId, HashSet<EntityId>>
 │         (used for invalidation: which views were rendered in which window)
 │
 └── Window invalidators
      └── HashMap<EntityId, HashMap<WindowId, WindowInvalidator>>
           (tracks dirty views per window)
```

### View Caching Mechanism

View caching is what makes GPUI efficient despite rebuilding the element tree every frame. When `AnyView.prepaint()` is called:

1. Check if `cx.notify()` was called on this entity since the last frame (via `dirty_views` set).
2. Check if `Window::refresh()` was called (forces re-render for all views).
3. Check if the view's bounds, content mask, or text style changed since last frame.
4. If all checks pass: **skip** `Render::render()`. Reuse the previous frame's layout and paint commands from the element state ranges.
5. If any check fails: call `Render::render()`, lay out and paint the result, and store new ranges for future caching.

The `WindowInvalidator` struct tracks which views are dirty:

```rust
struct WindowInvalidatorInner {
    pub dirty: bool,                        // window-level dirty flag
    pub draw_phase: DrawPhase,              // None, Layout, Prepaint, Paint
    pub dirty_views: FxHashSet<EntityId>,   // specific views that called notify()
    pub update_count: usize,                // monotonic counter for change detection
}
```

### Key Dispatch Tree

The `DispatchTree` is built during `prepaint()` by `div` and other interactive elements. It's a flat `Vec<DispatchNode>` with parent indices (not pointers):

```
Window
 └── next_frame.dispatch_tree: DispatchTree
      ├── nodes: Vec<DispatchNode>
      │    ├── key_listeners: Vec<KeyListener>
      │    ├── action_listeners: Vec<DispatchActionListener>
      │    ├── context: Option<KeyContext>       // e.g., "Editor", "Pane"
      │    ├── focus_id: Option<FocusId>
      │    ├── view_id: Option<EntityId>
      │    └── parent: Option<usize>             // index into nodes[]
      │
      ├── context_stack: Vec<KeyContext>         // built during prepaint walk
      ├── view_stack: Vec<EntityId>
      └── focusable_node_ids: HashMap<FocusId, usize>
```

Keyboard dispatch logic:

1. Raw keystroke arrives from the platform.
2. `DispatchTree::dispatch_key()` is called.
3. It iterates through key bindings registered via `cx.bind_keys()`.
4. For each binding, it matches the keystroke sequence and the `KeyContext` against the focused node and its ancestors.
5. The first match (deepest in the tree) wins.
6. The bound action is dispatched via `dispatch_action()`, which calls the action listeners registered on the matching node.

### Element Arena

GPUI uses a per-`App` arena allocator (`RefCell<Arena>`) for elements. This means element allocations are extremely fast (bump allocation) and bulk-freed at the end of each frame. The arena is isolated per `App` instance, which is important for test isolation—multiple concurrent tests each have their own arena.

```rust
element_arena: RefCell<Arena>,   // Per-frame element allocations
event_arena: Arena,              // Event-specific allocations
```

### Event Propagation

Mouse and keyboard events propagate in two phases, mirroring the DOM event model:

1. **Capture phase** (root → target): useful for "clear all pressed states before a new press" patterns. Listener registrations that specify `DispatchPhase::Capture` fire in this phase.
2. **Bubble phase** (target → root): the default. Most event handlers use this phase.

Propagation can be stopped with `cx.stop_propagation()`, but this is generally discouraged during the capture phase—other elements may rely on seeing global mouse-up events to reset their hover/press states.

### GPU Rendering

GPUI renders via its own GPU abstraction (not a retained scene graph). Each frame, the `Scene` collects a flat list of draw primitives:

```
Element::paint()
  ├── window.paint_quad(fill(bounds, color))  →  Quad
  ├── window.paint_glyph(params)              →  RenderGlyphParams
  ├── window.paint_underline(style)           →  Underline
  ├── window.paint_strikethrough(style)       →  Strikethrough
  ├── window.paint_sprite(...)                →  MonochromeSprite / PolychromeSprite
  ├── window.paint_svg(params)                →  RenderSvgParams
  ├── window.paint_image(params)              →  RenderImageParams
  ├── window.paint_shadow(...)                →  Shadow
  └── window.paint_path(path)                 →  Path
```

The `Scene` is submitted to the platform atlas (`PlatformAtlas`), which batches draw calls and uploads them to the GPU. On macOS this uses Metal; on Linux and Windows it uses Vulkan (via `blade`).

The renderer applies scrolling via GPU transforms rather than re-rendering, which is why GPUI scrolling is smooth and efficient.

---

## Custom Elements

Most applications only need `Render` or `RenderOnce`. But for cases requiring manual control over layout, hitbox management, or painting, you can implement `Element` directly.

### `RenderOnce` — Recommended for Stateless Components

Use `#[derive(IntoElement)]` on a struct that implements `RenderOnce`. The component takes ownership of its data and emits an element tree:

```rust
#[derive(IntoElement)]
struct Badge {
    text: SharedString,
    color: Hsla,
}

impl RenderOnce for Badge {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .px_2()
            .py_0p5()
            .rounded_full()
            .bg(self.color)
            .text_xs()
            .text_color(white())
            .child(self.text)
    }
}

// Usage — use the struct directly as a child:
div().child(Badge { text: "New".into(), color: gpui::red() })
```

This is the sweet spot for reusable UI pieces: you get type safety, dot-notation construction, and zero boilerplate for wiring into the element tree.

### Implementing `Element` Directly — Full Control

Only implement `Element` when you need to:

- Use a custom layout algorithm (not Taffy)
- Manage hitboxes with non-rectangular shapes
- Control exactly which children get painted and when
- Implement a virtualized list or code editor

Here's a minimal custom element that draws a fixed-size red rectangle:

```rust
struct RedBlock {
    width: Pixels,
    height: Pixels,
}

impl Element for RedBlock {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(ElementId::Name("red-block".into()))
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let layout_id = window.request_layout(
            Style {
                size: size(self.width, self.height).into(),
                ..Default::default()
            },
            None,
            cx,
        );
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
        window.insert_hitbox(bounds, HitboxBehavior::default());
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        _cx: &mut App,
    ) {
        window.paint_quad(fill(bounds, gpui::red()));
    }
}

impl IntoElement for RedBlock {
    type Element = Self;
    fn into_element(self) -> Self::Element { self }
}
```

### Adding Children to Custom Elements

Implement `ParentElement` to accept child elements:

```rust
struct MyContainer {
    children: SmallVec<[AnyElement; 2]>,
}

impl ParentElement for MyContainer {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}
```

Then in `request_layout` and `prepaint`/`paint`, iterate over `self.children` and call `.request_layout()`, `.prepaint()`, `.paint()` on each. The order of these calls establishes the z-order and sibling relationships in the dispatch tree.

### Element IDs and State Persistence

Elements that set an `ElementId` via `Element::id()` create a `GlobalElementId` (a path from the root element through all named ancestors). GPUI stores per-element state keyed by this global ID, which allows state like scroll position to persist across frames even though the element tree is rebuilt.

---

## Appendix: Type Reference

### Core Types

| Type | Description |
|---|---|
| `App` | Root application context — owns all state |
| `Application` | Builder that configures and launches the app |
| `Entity<T>` | Typed, ref-counted handle to entity state |
| `WeakEntity<T>` | Weak handle — doesn't prevent release |
| `EntityId` | Globally unique integer entity identifier |
| `Window` | Per-window operations and context |
| `WindowHandle<V>` | Typed handle to a specific window |
| `AnyWindowHandle` | Type-erased window handle |
| `AsyncApp` | Static-lifetime context for async code |
| `AsyncWindowContext` | Async context with window access |
| `Context<T>` | Entity-scoped context (derefs to App) |
| `Task<R>` | Cancelable async future |
| `Subscription` | Handle that deregisters a callback on drop |
| `SharedString` | `Arc<str>` — cheap-to-clone string |

### Geometry Types

| Type | Description |
|---|---|
| `Pixels` | Device-independent pixel unit |
| `DevicePixels` | Physical pixel unit (raw monitor pixels) |
| `ScaledPixels` | Scale-factor-aware pixels |
| `Point<T>` | 2D point `{ x: T, y: T }` |
| `Size<T>` | 2D size `{ width: T, height: T }` |
| `Bounds<T>` | Rectangle `{ origin: Point<T>, size: Size<T> }` |
| `Edges<T>` | Per-edge values `{ top, right, bottom, left }` |
| `Corners<T>` | Per-corner radii `{ top_left, top_right, bottom_left, bottom_right }` |
| `Axis` | `Horizontal` or `Vertical` |
| `AvailableSpace` | Definite (`Pixels`) or indefinite (`MinContent`, `MaxContent`) |

### Color Types

| Type | Description |
|---|---|
| `Hsla` | Hue, saturation, lightness, alpha (internal representation) |
| `Rgba` | Red, green, blue, alpha |
| `rgb(0xRRGGBB)` | 24-bit RGB constructor (alpha = 1.0) |
| `rgba(0xRRGGBBAA)` | 32-bit RGBA constructor |
| `hsla(h, s, l, a)` | HSLA constructor (hue, saturation, lightness, alpha) |

### Key Traits Quick Reference

| Trait | Purpose | Key methods |
|---|---|---|
| `Render` | View → element tree | `fn render(&mut self, window, cx) -> impl IntoElement` |
| `RenderOnce` | Component → element | `fn render(self, window, cx) -> impl IntoElement` |
| `Element` | Low-level element | `request_layout()`, `prepaint()`, `paint()` |
| `IntoElement` | Convert type → element | `fn into_element(self) -> Self::Element` |
| `Styled` | Tailwind-style styling | `.flex()`, `.bg()`, `.p_4()`, etc. |
| `InteractiveElement` | Mouse events | `.on_click()`, `.on_mouse_down()`, etc. |
| `StatefulInteractiveElement` | Stateful events + a11y | `.on_drag()`, `.track_focus()`, `.tooltip()`, etc. |
| `ParentElement` | Accept children | `.child()`, `.children()`, `.extend()` |
| `EventEmitter<E>` | Emit typed events | (marker trait, zero methods) |
| `Focusable` | Keyboard focus | `fn focus_handle(&self, cx) -> FocusHandle` |
| `Global` | App singleton | (marker trait) |
| `InputHandler` | Text input | `text_for_range()`, `replace_text_in_range()`, etc. |
| `AppContext` | Unifying context trait | `cx.new()`, `cx.spawn()`, `cx.read_global()`, etc. |
| `VisualContext` | Context + window | `cx.focus()`, `cx.window_handle()`, etc. |

---

## Further Resources

- [Ownership and Data Flow](https://www.gpui.rs/gpui/_ownership_and_data_flow) — official deep-dive on GPUI's ownership model
- [Accessibility Guide](https://www.gpui.rs/gpui/_accessibility) — how GPUI integrates with screen readers
- [Zed Discord](https://zed.dev/community-links) — ask questions, get help
- [GPUI Examples](https://github.com/zed-industries/zed/tree/main/crates/gpui/examples) — 40+ runnable examples
- [Zed Blog](https://zed.dev/blog) — architecture posts and release notes

---

> *This wiki page covers GPUI as of the Zed repository main branch. GPUI is actively developed and APIs may change.*
