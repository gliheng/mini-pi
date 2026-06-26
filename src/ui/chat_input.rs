use std::ops::Range;
use std::path::PathBuf;

use gpui::{
    AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, SharedString, Subscription,
    Window,
};

use gpui_component::input::{InputEvent, InputState, RopeExt as _};

use crate::utils::file_scanner;

#[derive(Clone, Debug)]
pub struct MentionItem {
    pub name: String,
    pub relative_path: String,
    pub absolute_path: PathBuf,
    pub is_dir: bool,
}

#[derive(Clone, Debug)]
pub struct CommandItem {
    pub name: String,
    pub description: Option<String>,
    pub source: String,
}

#[derive(Clone, Debug)]
pub struct AtMentionParse {
    pub query: String,
    pub replace_range: Range<usize>,
}

#[derive(Clone, Debug)]
pub enum ChatInputEvent {
    Change,
}

pub struct ChatInput {
    pub focus_handle: FocusHandle,
    pub input_state: Entity<InputState>,
    _input_subscription: Subscription,

    pub enable_at_mention: bool,
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
    cached_workspace_id: Option<String>,

    pub enable_slash_commands: bool,
    pub slash_command_active: bool,
    slash_command_query: String,
    slash_command_replace_range: Range<usize>,
    pub slash_command_highlighted: usize,
    pub slash_command_items: Vec<CommandItem>,
    available_commands: Vec<CommandItem>,
}

impl EventEmitter<ChatInputEvent> for ChatInput {}

impl ChatInput {
    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        placeholder: impl Into<SharedString>,
    ) -> Self {
        let placeholder = placeholder.into();
        let input_state = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(placeholder)
                .auto_grow(1, 8)
                .submit_on_enter(true)
        });

        let input_subscription = cx.subscribe_in(
            &input_state,
            window,
            |this, _state, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.update_popups(cx);
                    cx.emit(ChatInputEvent::Change);
                    cx.notify();
                }
            },
        );

        Self {
            focus_handle: input_state.focus_handle(cx),
            input_state,
            _input_subscription: input_subscription,
            enable_at_mention: true,
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
            enable_slash_commands: true,
            slash_command_active: false,
            slash_command_query: String::new(),
            slash_command_replace_range: 0..0,
            slash_command_highlighted: 0,
            slash_command_items: Vec::new(),
            available_commands: Vec::new(),
        }
    }

    pub fn with_at_mention(mut self, enabled: bool) -> Self {
        self.enable_at_mention = enabled;
        self
    }

    pub fn with_slash_commands(mut self, enabled: bool) -> Self {
        self.enable_slash_commands = enabled;
        self
    }

    pub fn content(&self, cx: &gpui::App) -> SharedString {
        self.input_state.read(cx).value()
    }

    pub fn is_popup_visible(&self) -> bool {
        self.is_at_popup_visible() || self.is_command_popup_visible()
    }

    pub fn is_at_popup_visible(&self) -> bool {
        self.at_mention_active && !self.mention_items.is_empty()
    }

    pub fn is_command_popup_visible(&self) -> bool {
        self.slash_command_active && !self.slash_command_items.is_empty()
    }

    pub fn popup_items(&self) -> &[MentionItem] {
        &self.mention_items
    }

    pub fn popup_highlighted(&self) -> usize {
        self.at_mention_highlighted
    }

    pub fn slash_command_items(&self) -> &[CommandItem] {
        &self.slash_command_items
    }

    pub fn slash_command_highlighted(&self) -> usize {
        self.slash_command_highlighted
    }

    pub fn is_just_selected_mention(&self) -> bool {
        self.just_selected_mention
    }

    pub fn clear_just_selected_mention(&mut self) {
        self.just_selected_mention = false;
    }

    pub fn set_workspace(
        &mut self,
        id: String,
        dir: PathBuf,
        name: String,
        cx: &mut Context<Self>,
    ) {
        if self.cached_workspace_id == Some(id.clone()) {
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
                input.update_popups(cx);
            })
            .ok();
        })
        .detach();
    }

    pub fn set_commands(&mut self, commands: Vec<CommandItem>, cx: &mut Context<Self>) {
        self.available_commands = commands;
        self.update_popups(cx);
    }

    pub fn update_popups(&mut self, _cx: &mut Context<Self>) {
        let cursor = self.input_state.read(_cx).cursor();
        let content = self.input_state.read(_cx).value().to_string();

        // First check slash command
        if self.enable_slash_commands {
            if let Some(parse) = parse_slash_command(&content, cursor) {
                self.slash_command_query = parse.query.clone();
                self.slash_command_replace_range = parse.replace_range.clone();

                let filtered = filter_command_items(&self.available_commands, &parse.query);
                self.slash_command_active = !filtered.is_empty();
                self.slash_command_items = filtered;

                if self.slash_command_highlighted >= self.slash_command_items.len() {
                    self.slash_command_highlighted = 0;
                }

                // Close at mention
                self.at_mention_active = false;
                self.mention_items.clear();
                return;
            } else {
                self.slash_command_active = false;
                self.slash_command_items.clear();
            }
        }

        // Then check at mention
        if !self.enable_at_mention || self.workspace_dir.is_none() {
            self.at_mention_active = false;
            self.mention_items.clear();
            return;
        }

        if let Some(parse) = parse_at_mention(&content, cursor) {
            self.at_mention_query = parse.query.clone();
            self.at_mention_replace_range = parse.replace_range.clone();

            if self.file_cache_loaded {
                let filtered = filter_mention_items(&self.file_cache, &parse.query);
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
                self.at_mention_highlighted = (self.at_mention_highlighted + 1) % len;
            } else if direction < 0 {
                self.at_mention_highlighted = if self.at_mention_highlighted == 0 {
                    len - 1
                } else {
                    self.at_mention_highlighted - 1
                };
            }
        } else if !self.slash_command_items.is_empty() {
            let len = self.slash_command_items.len();
            if direction > 0 {
                self.slash_command_highlighted = (self.slash_command_highlighted + 1) % len;
            } else if direction < 0 {
                self.slash_command_highlighted = if self.slash_command_highlighted == 0 {
                    len - 1
                } else {
                    self.slash_command_highlighted - 1
                };
            }
        }
        cx.notify();
    }

    pub fn select_highlighted_mention(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(item) = self.mention_items.get(self.at_mention_highlighted).cloned() {
            let suffix = if item.is_dir { "/" } else { "" };
            let insertion = format!(
                "[@{}]({}{})",
                item.name,
                item.absolute_path.to_string_lossy(),
                suffix
            );
            let range = self.at_mention_replace_range.clone();
            self.replace_range(range, &insertion, window, cx);
            self.at_mention_active = false;
            self.mention_items.clear();
            self.just_selected_mention = true;
            self.update_popups(cx);
        } else {
            self.at_mention_active = false;
            self.mention_items.clear();
        }
        cx.notify();
    }

    pub fn select_mention_at(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index < self.mention_items.len() {
            self.at_mention_highlighted = index;
            self.select_highlighted_mention(window, cx);
        }
    }

    pub fn select_highlighted_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(item) = self
            .slash_command_items
            .get(self.slash_command_highlighted)
            .cloned()
        {
            let insertion = format!("/{} ", item.name);
            let range = self.slash_command_replace_range.clone();
            self.replace_range(range, &insertion, window, cx);
            self.slash_command_active = false;
            self.slash_command_items.clear();
        } else {
            self.slash_command_active = false;
            self.slash_command_items.clear();
        }
        cx.notify();
    }

    pub fn select_command_at(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index < self.slash_command_items.len() {
            self.slash_command_highlighted = index;
            self.select_highlighted_command(window, cx);
        }
    }

    pub fn close_popup(&mut self, cx: &mut Context<Self>) {
        self.at_mention_active = false;
        self.mention_items.clear();
        self.slash_command_active = false;
        self.slash_command_items.clear();
        cx.notify();
    }

    fn replace_range(
        &mut self,
        range: Range<usize>,
        replacement: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let content = self.input_state.read(cx).value();
        let range_start = range.start.min(content.len());
        let range_end = range.end.min(content.len());
        let new_content = content[0..range_start].to_string() + replacement + &content[range_end..];
        let new_cursor = range_start + replacement.len();

        self.input_state.update(cx, |state, cx| {
            state.set_value(new_content, window, cx);
            let position = state.text().offset_to_position(new_cursor);
            state.set_cursor_position(position, window, cx);
        });
        self.update_popups(cx);
    }

    pub fn set_content(
        &mut self,
        content: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let content = content.into();
        self.input_state.update(cx, |state, cx| {
            state.set_value(content, window, cx);
        });
        self.update_popups(cx);
        cx.emit(ChatInputEvent::Change);
        cx.notify();
    }

    pub fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input_state.update(cx, |state, cx| {
            state.set_value("", window, cx);
        });
        self.at_mention_active = false;
        self.mention_items.clear();
        self.slash_command_active = false;
        self.slash_command_items.clear();
        self.just_selected_mention = false;
        cx.emit(ChatInputEvent::Change);
        cx.notify();
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.input_state.update(cx, |state, cx| {
            state.focus(window, cx);
        });
    }
}

impl Focusable for ChatInput {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
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
            Some(b' ') | Some(b'\n') | Some(b'\t') | Some(b'(') | Some(b'[') | Some(b'{') => {}
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
    matches.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| a.0.relative_path.cmp(&b.0.relative_path))
    });
    matches.into_iter().take(50).map(|(item, _)| item).collect()
}

fn parse_slash_command(content: &str, cursor: usize) -> Option<AtMentionParse> {
    if cursor == 0 {
        return None;
    }

    let text_before_cursor = &content[..cursor];

    let slash_pos = text_before_cursor.rfind('/')?;

    // '/' must be at the start of a line (string start or after \n or \r)
    if slash_pos > 0 {
        let ch_before = content.as_bytes().get(slash_pos - 1).copied()?;
        if ch_before != b'\n' && ch_before != b'\r' {
            return None;
        }
    }

    let after_slash = &text_before_cursor[slash_pos + 1..];
    if after_slash.starts_with(' ') {
        return None;
    }

    let query = if after_slash.is_empty() {
        String::new()
    } else {
        let end = after_slash
            .find(|c: char| c.is_whitespace())
            .unwrap_or(after_slash.len());
        after_slash[..end].to_string()
    };

    let replace_start = slash_pos;
    let replace_end = slash_pos + 1 + query.len();

    Some(AtMentionParse {
        query,
        replace_range: replace_start..replace_end,
    })
}

fn filter_command_items(items: &[CommandItem], query: &str) -> Vec<CommandItem> {
    if query.is_empty() {
        return items.iter().take(50).cloned().collect();
    }
    let q = query.to_lowercase();
    let mut matches: Vec<(CommandItem, usize)> = items
        .iter()
        .filter_map(|item| {
            let name_lower = item.name.to_lowercase();
            let desc_lower = item.description.as_deref().unwrap_or("").to_lowercase();
            if name_lower.contains(&q) || desc_lower.contains(&q) {
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
    matches.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.name.cmp(&b.0.name)));
    matches.into_iter().take(50).map(|(item, _)| item).collect()
}
