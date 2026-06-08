use std::ops::Range;
use std::path::PathBuf;

use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, Element, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, GlobalElementId, KeyDownEvent,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point,
    ShapedLine, SharedString, Style, TextRun, UTF16Selection, UnderlineStyle, Window, actions,
    div, fill, hsla, point, prelude::*, px, relative, rgba, size,
};
use unicode_segmentation::*;

use crate::utils::file_scanner;

actions!(
    chat_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        Forward,
        Backward,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        ShowCharacterPalette,
        Paste,
        Cut,
        CopyText,
    ]
);

#[derive(Clone, Debug)]
pub struct MentionItem {
    pub name: String,
    pub relative_path: String,
    pub absolute_path: PathBuf,
    pub is_dir: bool,
}

#[derive(Clone, Debug)]
pub struct AtMentionParse {
    pub query: String,
    pub replace_range: Range<usize>,
}

#[derive(Clone, Debug)]
pub enum ChatInputEvent {
    Change,
    AtMentionChanged,
}

pub struct ChatInput {
    pub focus_handle: FocusHandle,
    pub content: SharedString,
    pub placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,

    pub at_mention_active: bool,
    at_mention_query: String,
    at_mention_replace_range: Range<usize>,
    pub at_mention_highlighted: usize,
    pub mention_items: Vec<MentionItem>,
    file_cache: Vec<MentionItem>,
    file_cache_loaded: bool,
    file_cache_loading: bool,
    pub workspace_dir: Option<PathBuf>,
    pub workspace_name: Option<String>,
    pub just_selected_mention: bool,
    cached_workspace_id: Option<i64>,
}

impl EventEmitter<ChatInputEvent> for ChatInput {}

impl ChatInput {
    pub fn new(cx: &mut Context<Self>, placeholder: impl Into<SharedString>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content: "".into(),
            placeholder: placeholder.into(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            is_selecting: false,
            at_mention_active: false,
            at_mention_query: String::new(),
            at_mention_replace_range: 0..0,
            at_mention_highlighted: 0,
            mention_items: Vec::new(),
            file_cache: Vec::new(),
            file_cache_loaded: false,
            file_cache_loading: false,
            workspace_dir: None,
            workspace_name: None,
            just_selected_mention: false,
            cached_workspace_id: None,
        }
    }

    pub fn content(&self) -> &SharedString {
        &self.content
    }

    pub fn is_popup_visible(&self) -> bool {
        self.at_mention_active && !self.mention_items.is_empty()
    }

    pub fn popup_items(&self) -> &[MentionItem] {
        &self.mention_items
    }

    pub fn popup_highlighted(&self) -> usize {
        self.at_mention_highlighted
    }

    pub fn is_just_selected_mention(&self) -> bool {
        self.just_selected_mention
    }

    pub fn clear_just_selected_mention(&mut self) {
        self.just_selected_mention = false;
    }

    pub fn set_workspace(
        &mut self,
        id: i64,
        dir: PathBuf,
        name: String,
        cx: &mut Context<Self>,
    ) {
        if self.cached_workspace_id == Some(id) {
            return;
        }
        self.cached_workspace_id = Some(id);
        self.workspace_dir = Some(dir);
        self.workspace_name = Some(name);
        self.file_cache_loaded = false;
        self.file_cache_loading = false;
        self.file_cache.clear();
        self.load_file_cache(cx);
    }

    fn load_file_cache(&mut self, cx: &mut Context<Self>) {
        if self.file_cache_loading || self.file_cache_loaded {
            return;
        }
        let Some(ref dir) = self.workspace_dir else {
            return;
        };
        let dir = dir.clone();
        self.file_cache_loading = true;
        cx.spawn(async move |this, cx| {
            let entries = smol::unblock(move || file_scanner::scan_directory(&dir)).await;
            let mention_items: Vec<MentionItem> = entries
                .into_iter()
                .map(|e| MentionItem {
                    name: e.name,
                    relative_path: e.relative_path,
                    absolute_path: e.path,
                    is_dir: e.is_dir,
                })
                .collect();
            this.update(cx, |input, cx| {
                input.file_cache = mention_items;
                input.file_cache_loaded = true;
                input.file_cache_loading = false;
                input.update_at_mention(cx);
            })
            .ok();
        })
        .detach();
    }

    pub fn update_at_mention(&mut self, _cx: &mut Context<Self>) {
        if self.workspace_dir.is_none() {
            self.at_mention_active = false;
            self.mention_items.clear();
            return;
        }

        let cursor = self.cursor_offset();
        if let Some(parse) = parse_at_mention(&self.content, cursor) {
            self.at_mention_query = parse.query.clone();
            self.at_mention_replace_range = parse.replace_range.clone();

            if self.file_cache_loaded {
                let filtered =
                    filter_mention_items(&self.file_cache, &parse.query);
                self.at_mention_active = !filtered.is_empty();
                self.mention_items = filtered;
            } else {
                self.at_mention_active = false;
                self.mention_items.clear();
            }

            if self.at_mention_highlighted >= self.mention_items.len() {
                self.at_mention_highlighted = 0;
            }
        } else {
            self.at_mention_active = false;
            self.at_mention_query.clear();
            self.mention_items.clear();
        }
    }

    pub fn navigate_popup(&mut self, direction: i32, cx: &mut Context<Self>) {
        if !self.mention_items.is_empty() {
            let len = self.mention_items.len();
            if direction > 0 {
                self.at_mention_highlighted =
                    (self.at_mention_highlighted + 1) % len;
            } else if direction < 0 {
                self.at_mention_highlighted = if self.at_mention_highlighted == 0 {
                    len - 1
                } else {
                    self.at_mention_highlighted - 1
                };
            }
        }
        cx.notify();
    }

    pub fn select_highlighted_mention(&mut self, cx: &mut Context<Self>) {
        if let Some(item) = self.mention_items.get(self.at_mention_highlighted) {
            let suffix = if item.is_dir { "/" } else { "" };
            let insertion = format!(
                "[@{}]({}{})",
                item.name, item.absolute_path.to_string_lossy(), suffix
            );
            let range = self.at_mention_replace_range.clone();
            if range.start <= self.content.len() && range.end <= self.content.len() {
                self.replace_range(range, &insertion, cx);
            } else {
                let start = range.start.min(self.content.len());
                self.replace_range(start..self.content.len(), &insertion, cx);
            }
            self.at_mention_active = false;
            self.mention_items.clear();
            self.just_selected_mention = true;
            self.update_at_mention(cx);
        } else {
            self.at_mention_active = false;
            self.mention_items.clear();
        }
        cx.notify();
    }

    pub fn select_mention_at(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.mention_items.len() {
            self.at_mention_highlighted = index;
            self.select_highlighted_mention(cx);
        }
    }

    pub fn close_popup(&mut self, cx: &mut Context<Self>) {
        self.at_mention_active = false;
        self.mention_items.clear();
        cx.notify();
    }

    fn replace_range(
        &mut self,
        range: Range<usize>,
        replacement: &str,
        cx: &mut Context<Self>,
    ) {
        let range_start = range.start.min(self.content.len());
        let range_end = range.end.min(self.content.len());
        let end = range_start + replacement.len();
        self.content = (self.content[0..range_start].to_owned()
            + replacement
            + &self.content[range_end..])
            .into();
        self.selected_range = end..end;
        self.selection_reversed = false;
        self.marked_range = None;
        self.last_layout = None;
        self.last_bounds = None;
        self.is_selecting = false;
        cx.notify();
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx)
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx)
        }
    }

    fn backward(&mut self, _: &Backward, _window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx)
        }
    }

    fn forward(&mut self, _: &Forward, _window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx)
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.selected_range.end), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx)
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let prev = self.previous_boundary(self.cursor_offset());
            if self.cursor_offset() == prev {
                return;
            }
            self.select_to(prev, cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let next = self.next_boundary(self.selected_range.end);
            if self.cursor_offset() == next {
                return;
            }
            self.select_to(next, cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle);
        window.prevent_default();
        self.is_selecting = true;

        if event.modifiers.shift {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        } else {
            self.move_to(self.index_for_mouse_position(event.position), cx)
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn show_character_palette(
        &mut self,
        _: &ShowCharacterPalette,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.show_character_palette();
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &text.replace("\n", " "), window, cx);
        }
    }

    fn copy(&mut self, _: &CopyText, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx)
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        cx.notify()
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }

        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.content.len();
        }
        line.closest_index_for_x(position.x - bounds.left())
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset
        } else {
            self.selected_range.end = offset
        };
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify()
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;

        for ch in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }

        utf8_offset.min(self.content.len())
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;

        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }

        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start.min(self.content.len()))
            ..self.offset_to_utf16(range.end.min(self.content.len()))
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        let start = self.offset_from_utf16(range_utf16.start);
        let end = self.offset_from_utf16(range_utf16.end);
        start.min(self.content.len())..end.min(self.content.len())
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
    }

    pub fn set_content(&mut self, content: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.content = content.into();
        self.selected_range = self.content.len()..self.content.len();
        self.selection_reversed = false;
        self.marked_range = None;
        self.last_layout = None;
        self.last_bounds = None;
        self.is_selecting = false;
        self.update_at_mention(cx);
        cx.notify();
    }

    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.content = "".into();
        self.selected_range = 0..0;
        self.selection_reversed = false;
        self.marked_range = None;
        self.last_layout = None;
        self.last_bounds = None;
        self.is_selecting = false;
        self.at_mention_active = false;
        self.mention_items.clear();
        self.just_selected_mention = false;
        cx.notify();
    }
}

impl EntityInputHandler for ChatInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        let start = range.start.min(self.content.len());
        let end = range.end.min(self.content.len());
        actual_range.replace(self.range_to_utf16(&(start..end)));
        if start >= end {
            Some(String::new())
        } else {
            Some(self.content[start..end].to_string())
        }
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());

        let start = range.start.min(self.content.len());
        let end = range.end.min(self.content.len());

        self.content =
            (self.content[0..start].to_owned() + new_text + &self.content[end..])
                .into();
        self.selected_range = start + new_text.len()..start + new_text.len();
        self.marked_range.take();
        self.update_at_mention(cx);
        cx.emit(ChatInputEvent::Change);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());

        let start = range.start.min(self.content.len());
        let end = range.end.min(self.content.len());

        self.content =
            (self.content[0..start].to_owned() + new_text + &self.content[end..])
                .into();
        if !new_text.is_empty() {
            self.marked_range = Some(start..start + new_text.len());
        } else {
            self.marked_range = None;
        }
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .map(|new_range| (new_range.start + start).min(self.content.len())..(new_range.end + end).min(self.content.len()))
            .unwrap_or_else(|| start + new_text.len()..start + new_text.len());

        self.update_at_mention(cx);
        cx.emit(ChatInputEvent::Change);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let last_layout = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        Some(Bounds::from_corners(
            point(
                bounds.left() + last_layout.x_for_index(range.start),
                bounds.top(),
            ),
            point(
                bounds.left() + last_layout.x_for_index(range.end),
                bounds.bottom(),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let last_bounds = self.last_bounds?;
        let last_layout = self.last_layout.as_ref()?;
        let line_point = last_bounds.localize(&point)?;
        let utf8_index = last_layout.index_for_x(point.x - line_point.x)?;
        Some(self.offset_to_utf16(utf8_index))
    }
}

struct ChatInputElement {
    input: Entity<ChatInput>,
}

struct PrepaintState {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
}

impl IntoElement for ChatInputElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for ChatInputElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let content = input.content.clone();
        let selected_range = input.selected_range.clone();
        let cursor = input.cursor_offset();
        let style = window.text_style();

        let (display_text, text_color) = if content.is_empty() {
            (input.placeholder.clone(), hsla(0., 0., 1., 0.3))
        } else {
            (content, hsla(0., 0., 1., 1.))
        };

        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = if let Some(marked_range) = input.marked_range.as_ref() {
            vec![
                TextRun {
                    len: marked_range.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked_range.end - marked_range.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: display_text.len() - marked_range.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect()
        } else {
            vec![run]
        };

        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(display_text, font_size, &runs, None);

        let cursor_pos = line.x_for_index(cursor);
        let (selection, cursor) = if selected_range.is_empty() {
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(bounds.left() + cursor_pos, bounds.top()),
                        size(px(2.), bounds.bottom() - bounds.top()),
                    ),
                    hsla(200. / 360., 0.8, 0.7, 1.),
                )),
            )
        } else {
            (
                Some(fill(
                    Bounds::from_corners(
                        point(
                            bounds.left() + line.x_for_index(selected_range.start),
                            bounds.top(),
                        ),
                        point(
                            bounds.left() + line.x_for_index(selected_range.end),
                            bounds.bottom(),
                        ),
                    ),
                    rgba(0x3311ff30),
                )),
                None,
            )
        };
        PrepaintState {
            line: Some(line),
            cursor,
            selection,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection)
        }
        let line = prepaint.line.take().unwrap();
        line.paint(bounds.origin, window.line_height(), window, cx)
            .unwrap();

        if focus_handle.is_focused(window) && let Some(cursor) = prepaint.cursor.take() {
            window.paint_quad(cursor);
        }

        self.input.update(cx, |input, _cx| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
        });
    }
}

impl Render for ChatInput {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let popup_active = self.at_mention_active && !self.mention_items.is_empty();
        div()
            .key_context("ChatInput")
            .w_full()
            .h(px(28.))
            .track_focus(&self.focus_handle(cx))
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::backward))
            .on_action(cx.listener(Self::forward))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::show_character_palette))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if popup_active {
                    match event.keystroke.key.as_str() {
                        "up" => {
                            this.navigate_popup(-1, cx);
                            window.prevent_default();
                        }
                        "down" => {
                            this.navigate_popup(1, cx);
                            window.prevent_default();
                        }
                        "enter" | "tab" => {
                            this.select_highlighted_mention(cx);
                            window.prevent_default();
                        }
                        "escape" => {
                            this.close_popup(cx);
                        }
                        _ => {}
                    }
                }
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .line_height(px(20.))
            .text_size(px(14.))
            .child(div().size_full().child(ChatInputElement { input: cx.entity() }))
    }
}

impl Focusable for ChatInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

fn parse_at_mention(content: &str, cursor: usize) -> Option<AtMentionParse> {
    if cursor == 0 {
        return None;
    }

    let text_before_cursor = &content[..cursor];

    let at_pos = text_before_cursor.rfind('@')?;
    if at_pos > 0 {
        let ch_before = content.as_bytes().get(at_pos - 1).copied();
        match ch_before {
            Some(b' ') | Some(b'\n') | Some(b'\t') | Some(b'(') | Some(b'[')
            | Some(b'{') => {}
            _ => return None,
        }
    }

    let after_at = &text_before_cursor[at_pos + 1..];
    if after_at.starts_with(' ') {
        return None;
    }

    let query = if after_at.is_empty() {
        String::new()
    } else {
        let end = after_at
            .find(|c: char| c.is_whitespace())
            .unwrap_or(after_at.len());
        after_at[..end].to_string()
    };

    let replace_start = at_pos;
    let replace_end = at_pos + 1 + query.len();

    Some(AtMentionParse {
        query,
        replace_range: replace_start..replace_end,
    })
}

fn filter_mention_items(items: &[MentionItem], query: &str) -> Vec<MentionItem> {
    if query.is_empty() {
        return items.iter().take(50).cloned().collect();
    }
    let q = query.to_lowercase();
    let mut matches: Vec<(MentionItem, usize)> = items
        .iter()
        .filter_map(|item| {
            let name_lower = item.name.to_lowercase();
            let path_lower = item.relative_path.to_lowercase();
            if name_lower.contains(&q) || path_lower.contains(&q) {
                let score = if name_lower.starts_with(&q) {
                    100
                } else if name_lower.contains(&q) {
                    50
                } else {
                    10
                };
                Some((item.clone(), score))
            } else {
                None
            }
        })
        .collect();
    matches.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.relative_path.cmp(&b.0.relative_path)));
    matches.into_iter().take(50).map(|(item, _)| item).collect()
}