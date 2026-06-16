use std::ops::Range;

use gpui::{
    AnyElement, ClipboardItem, FontStyle, FontWeight, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, StrikethroughStyle, Styled, StyledImage, StyledText, TextRun,
    TextStyle, TextStyleRefinement, UnderlineStyle, div, img, prelude::*, px,
};
use pulldown_cmark::{Alignment, HeadingLevel};

use super::{
    MarkdownTheme, heading_style, highlight_code, image_source_from_url, strip_html_tags,
};
use crate::ui::markdown::parser::{
    CodeBlockKind, MarkdownEvent, MarkdownTag, MarkdownTagEnd, ParsedMarkdown,
};

/// A mapping from a rendered text byte range back to the source markdown byte
/// range. Used for selection, click-to-link, and copy-as-markdown.
#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct SourceMapping {
    pub source_range: Range<usize>,
    pub rendered_start: usize,
    pub rendered_end: usize,
    /// True when the rendered text differs from the source text (smart
    /// punctuation, entity decoding). In that case source positions inside the
    /// run are approximate.
    pub is_substituted: bool,
}

/// A single styled run inside a pending line of rendered text.
#[derive(Clone, Debug, Default)]
pub struct PendingRun {
    pub style: TextStyleRefinement,
    pub start: usize,
    pub end: usize,
}

/// A line of text being accumulated before it is emitted as a GPUI element.
#[derive(Clone, Debug, Default)]
pub struct PendingLine {
    pub text: String,
    pub runs: Vec<PendingRun>,
    pub source_mappings: Vec<SourceMapping>,
}

/// Zed-style builder that turns `(Range<usize>, MarkdownEvent)` into GPUI
/// elements.
pub struct MarkdownElementBuilder<'a> {
    theme: &'a MarkdownTheme,
    base_text_style: TextStyle,
    text_style_stack: Vec<TextStyleRefinement>,
    combined_style: TextStyleRefinement,
    container_stack: Vec<Container>,
    pending_line: PendingLine,
    root_blocks: Vec<AnyElement>,
    image: Option<ImageState>,
    link_url: Option<SharedString>,
    source_mappings: Vec<SourceMapping>,
    in_html_block: bool,
    in_metadata_block: bool,
    in_table_head: bool,
    table_alignments: Vec<Alignment>,
}

#[derive(Clone, Debug, Default)]
struct ImageState {
    url: SharedString,
    alt: String,
}

enum Container {
    Paragraph {
        children: Vec<AnyElement>,
    },
    Heading {
        level: HeadingLevel,
        children: Vec<AnyElement>,
    },
    BlockQuote {
        children: Vec<AnyElement>,
    },
    List {
        ordered: bool,
        start: Option<u64>,
        items: Vec<Vec<AnyElement>>,
    },
    ListItem {
        children: Vec<AnyElement>,
    },
    Table {
        header_rows: Vec<AnyElement>,
        body_rows: Vec<AnyElement>,
    },
    TableRow {
        cells: Vec<AnyElement>,
        is_header: bool,
    },
    TableCell {
        children: Vec<AnyElement>,
    },
    CodeBlock {
        language: Option<SharedString>,
        code: String,
    },
    FootnoteDefinition {
        label: SharedString,
        children: Vec<AnyElement>,
    },
    HtmlBlock,
}

impl<'a> MarkdownElementBuilder<'a> {
    pub fn build(parsed: &ParsedMarkdown, theme: &'a MarkdownTheme) -> AnyElement {
        Self::build_with_mappings(parsed, theme).0
    }

    pub fn build_with_mappings(
        parsed: &ParsedMarkdown,
        theme: &'a MarkdownTheme,
    ) -> (AnyElement, Vec<SourceMapping>) {
        let mut builder = Self::new(theme);
        let source = parsed.source.as_ref();
        for (range, event) in &parsed.events {
            builder.handle_event(range.clone(), event, source);
        }
        builder.finish_with_mappings()
    }

    fn new(theme: &'a MarkdownTheme) -> Self {
        let base_text_style = TextStyle {
            color: theme.text_primary,
            font_size: px(14.).into(),
            line_height: (px(14.) * 1.5).into(),
            ..Default::default()
        };
        Self {
            theme,
            base_text_style,
            text_style_stack: Vec::new(),
            combined_style: TextStyleRefinement::default(),
            container_stack: Vec::new(),
            pending_line: PendingLine::default(),
            root_blocks: Vec::new(),
            image: None,
            link_url: None,
            source_mappings: Vec::new(),
            in_html_block: false,
            in_metadata_block: false,
            in_table_head: false,
            table_alignments: Vec::new(),
        }
    }

    fn finish_with_mappings(mut self) -> (AnyElement, Vec<SourceMapping>) {
        self.flush_pending_line();
        while let Some(container) = self.container_stack.pop() {
            let element = self.finalize_container(container);
            self.push_child(element);
        }
        let element = div()
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .gap_1()
            .children(self.root_blocks)
            .into_any_element();
        (element, self.source_mappings)
    }

    fn handle_event(&mut self, range: Range<usize>, event: &MarkdownEvent, source: &str) {
        match event {
            MarkdownEvent::Start(tag) => self.start_tag(range, tag),
            MarkdownEvent::End(tag) => self.end_tag(range, tag),
            MarkdownEvent::Text => self.push_text(&source[range.clone()], range),
            MarkdownEvent::SubstitutedText(text) => self.push_substituted_text(text, range),
            MarkdownEvent::Code => self.push_inline_code(&source[range.clone()], range),
            MarkdownEvent::Html => {
                if !self.in_html_block {
                    let stripped = strip_html_tags(&source[range.clone()]);
                    if !stripped.is_empty() {
                        self.push_text(&stripped, range);
                    }
                }
            }
            MarkdownEvent::InlineHtml => {
                let stripped = strip_html_tags(&source[range.clone()]);
                if !stripped.is_empty() {
                    self.push_text(&stripped, range);
                }
            }
            MarkdownEvent::FootnoteReference(label) => {
                self.push_text(&format!("[^{label}]"), range);
            }
            MarkdownEvent::SoftBreak => self.push_text(" ", range.clone()),
            MarkdownEvent::HardBreak => self.push_text("\n", range.clone()),
            MarkdownEvent::Rule => {
                self.flush_pending_line();
                let rule = div()
                    .w_full()
                    .h(px(1.))
                    .bg(self.theme.rule_color)
                    .my_3()
                    .into_any_element();
                self.push_child(rule);
            }
            MarkdownEvent::TaskListMarker(checked) => {
                let marker = if *checked { "[x] " } else { "[ ] " };
                let color = if *checked {
                    self.theme.task_checked
                } else {
                    self.theme.task_unchecked
                };
                self.push_text_style(TextStyleRefinement {
                    color: Some(color),
                    ..Default::default()
                });
                self.push_text(marker, range);
                self.pop_text_style();
            }
            MarkdownEvent::RootStart | MarkdownEvent::RootEnd(_) => {
                self.flush_pending_line();
            }
        }
    }

    fn start_tag(&mut self, _range: Range<usize>, tag: &MarkdownTag) {
        self.flush_pending_line();
        match tag {
            MarkdownTag::Paragraph => {
                self.container_stack.push(Container::Paragraph {
                    children: Vec::new(),
                });
            }
            MarkdownTag::Heading { level, .. } => {
                let (size, weight) = heading_style(*level);
                self.push_text_style(TextStyleRefinement {
                    font_size: Some(px(size).into()),
                    font_weight: Some(weight),
                    color: Some(self.theme.heading_color),
                    ..Default::default()
                });
                self.container_stack.push(Container::Heading {
                    level: *level,
                    children: Vec::new(),
                });
            }
            MarkdownTag::BlockQuote(_) => {
                self.push_text_style(TextStyleRefinement {
                    color: Some(self.theme.quote_text),
                    ..Default::default()
                });
                self.container_stack.push(Container::BlockQuote {
                    children: Vec::new(),
                });
            }
            MarkdownTag::List(start) => {
                self.container_stack.push(Container::List {
                    ordered: start.is_some(),
                    start: *start,
                    items: Vec::new(),
                });
            }
            MarkdownTag::Item => {
                self.container_stack.push(Container::ListItem {
                    children: Vec::new(),
                });
            }
            MarkdownTag::CodeBlock { kind, .. } => {
                let language = match kind {
                    CodeBlockKind::FencedLang(lang) => Some(lang.clone()),
                    _ => None,
                };
                self.container_stack.push(Container::CodeBlock {
                    language,
                    code: String::new(),
                });
            }
            MarkdownTag::HtmlBlock => {
                self.in_html_block = true;
                self.container_stack.push(Container::HtmlBlock);
            }
            MarkdownTag::Table(alignments) => {
                self.table_alignments = alignments.clone();
                self.container_stack.push(Container::Table {
                    header_rows: Vec::new(),
                    body_rows: Vec::new(),
                });
            }
            MarkdownTag::TableHead => self.in_table_head = true,
            MarkdownTag::TableRow => {
                self.container_stack.push(Container::TableRow {
                    cells: Vec::new(),
                    is_header: self.in_table_head,
                });
            }
            MarkdownTag::TableCell => {
                if self.in_table_head {
                    self.push_text_style(TextStyleRefinement {
                        color: Some(self.theme.table_header_text),
                        font_weight: Some(FontWeight(600.0)),
                        ..Default::default()
                    });
                }
                self.container_stack.push(Container::TableCell {
                    children: Vec::new(),
                });
            }
            MarkdownTag::FootnoteDefinition(label) => {
                self.container_stack.push(Container::FootnoteDefinition {
                    label: label.clone(),
                    children: Vec::new(),
                });
            }
            MarkdownTag::Emphasis => {
                self.push_text_style(TextStyleRefinement {
                    font_style: Some(FontStyle::Italic),
                    ..Default::default()
                });
            }
            MarkdownTag::Strong => {
                self.push_text_style(TextStyleRefinement {
                    font_weight: Some(FontWeight(700.0)),
                    ..Default::default()
                });
            }
            MarkdownTag::Strikethrough => {
                self.push_text_style(TextStyleRefinement {
                    strikethrough: Some(StrikethroughStyle::default()),
                    ..Default::default()
                });
            }
            MarkdownTag::Link { dest_url, .. } => {
                self.link_url = Some(dest_url.clone());
                self.push_text_style(TextStyleRefinement {
                    color: Some(self.theme.link_color),
                    underline: Some(UnderlineStyle {
                        thickness: px(1.),
                        color: Some(self.theme.link_underline),
                        wavy: false,
                    }),
                    ..Default::default()
                });
            }
            MarkdownTag::Image { dest_url, .. } => {
                self.flush_pending_line();
                self.image = Some(ImageState {
                    url: dest_url.clone(),
                    alt: String::new(),
                });
            }
            MarkdownTag::MetadataBlock(_) => {
                self.in_metadata_block = true;
            }
            _ => {}
        }
    }

    fn end_tag(&mut self, _range: Range<usize>, tag: &MarkdownTagEnd) {
        match tag {
            MarkdownTagEnd::Paragraph => {
                self.flush_pending_line();
                if let Some(Container::Paragraph { .. }) = self.container_stack.last() {
                    let container = self.container_stack.pop().unwrap();
                    self.push_child(self.finalize_container(container));
                }
            }
            MarkdownTagEnd::Heading(_) => {
                self.flush_pending_line();
                self.pop_text_style();
                if let Some(Container::Heading { .. }) = self.container_stack.last() {
                    let container = self.container_stack.pop().unwrap();
                    self.push_child(self.finalize_container(container));
                }
            }
            MarkdownTagEnd::BlockQuote(_) => {
                self.flush_pending_line();
                self.pop_text_style();
                if let Some(Container::BlockQuote { .. }) = self.container_stack.last() {
                    let container = self.container_stack.pop().unwrap();
                    self.push_child(self.finalize_container(container));
                }
            }
            MarkdownTagEnd::List(_) => {
                self.flush_pending_line();
                if let Some(Container::List { .. }) = self.container_stack.last() {
                    let container = self.container_stack.pop().unwrap();
                    self.push_child(self.finalize_container(container));
                }
            }
            MarkdownTagEnd::Item => {
                self.flush_pending_line();
                if let Some(Container::ListItem { children }) = self.container_stack.pop()
                    && let Some(Container::List { items, .. }) = self.container_stack.last_mut()
                {
                    items.push(children);
                }
            }
            MarkdownTagEnd::CodeBlock => {
                if let Some(Container::CodeBlock { language, code }) = self.container_stack.pop() {
                    self.push_child(self.render_code_block(language, code));
                }
            }
            MarkdownTagEnd::HtmlBlock => {
                self.in_html_block = false;
                if let Some(Container::HtmlBlock) = self.container_stack.last() {
                    self.container_stack.pop();
                }
            }
            MarkdownTagEnd::Table => {
                self.flush_pending_line();
                if let Some(Container::Table { .. }) = self.container_stack.last() {
                    let container = self.container_stack.pop().unwrap();
                    self.push_child(self.finalize_container(container));
                }
                self.in_table_head = false;
                self.table_alignments.clear();
            }
            MarkdownTagEnd::TableHead => self.in_table_head = false,
            MarkdownTagEnd::TableRow => {
                self.flush_pending_line();
                if let Some(Container::TableRow { cells, is_header }) = self.container_stack.pop() {
                    let row = self.render_table_row(cells, is_header);
                    if let Some(Container::Table { header_rows, body_rows, .. }) =
                        self.container_stack.last_mut()
                    {
                        if is_header {
                            header_rows.push(row);
                        } else {
                            body_rows.push(row);
                        }
                    }
                }
            }
            MarkdownTagEnd::TableCell => {
                self.flush_pending_line();
                if self.in_table_head {
                    self.pop_text_style();
                }
                if let Some(Container::TableCell { children }) = self.container_stack.pop() {
                    let cell_index = if let Some(Container::TableRow { cells, .. }) =
                        self.container_stack.last()
                    {
                        cells.len()
                    } else {
                        0
                    };
                    let align = self
                        .table_alignments
                        .get(cell_index)
                        .copied()
                        .unwrap_or(Alignment::None);
                    let cell = self.render_table_cell(children, align);
                    if let Some(Container::TableRow { cells, .. }) = self.container_stack.last_mut()
                    {
                        cells.push(cell);
                    }
                }
            }
            MarkdownTagEnd::FootnoteDefinition => {
                self.flush_pending_line();
                if let Some(Container::FootnoteDefinition { .. }) = self.container_stack.last() {
                    let container = self.container_stack.pop().unwrap();
                    self.push_child(self.finalize_container(container));
                }
            }
            MarkdownTagEnd::Emphasis
            | MarkdownTagEnd::Strong
            | MarkdownTagEnd::Strikethrough => {
                self.pop_text_style();
            }
            MarkdownTagEnd::Link => {
                self.flush_pending_line();
                self.link_url = None;
                self.pop_text_style();
            }
            MarkdownTagEnd::Image => {
                if let Some(ImageState { url, alt }) = self.image.take() {
                    let element = self.render_image(&url, &alt);
                    self.push_child(element);
                }
            }
            MarkdownTagEnd::MetadataBlock(_) => {
                self.in_metadata_block = false;
            }
            _ => {}
        }
    }

    fn push_text(&mut self, text: &str, source_range: Range<usize>) {
        self.push_text_inner(text, source_range, false);
    }

    fn push_substituted_text(&mut self, text: &str, source_range: Range<usize>) {
        self.push_text_inner(text, source_range, true);
    }

    fn push_text_inner(&mut self, text: &str, source_range: Range<usize>, is_substituted: bool) {
        if self.in_metadata_block {
            return;
        }
        if let Some(image) = &mut self.image {
            image.alt.push_str(text);
            return;
        }
        if let Some(Container::CodeBlock { code, .. }) = self.container_stack.last_mut() {
            code.push_str(text);
            return;
        }
        if self.in_html_block {
            return;
        }
        if text.is_empty() {
            return;
        }
        let start = self.pending_line.text.len();
        self.pending_line.text.push_str(text);
        let end = self.pending_line.text.len();
        self.pending_line.runs.push(PendingRun {
            style: self.combined_style.clone(),
            start,
            end,
        });
        self.pending_line.source_mappings.push(SourceMapping {
            source_range,
            rendered_start: start,
            rendered_end: end,
            is_substituted,
        });
    }

    fn push_inline_code(&mut self, text: &str, source_range: Range<usize>) {
        if matches!(self.container_stack.last(), Some(Container::CodeBlock { .. })) {
            self.push_text(text, source_range);
            return;
        }
        self.push_text_style(TextStyleRefinement {
            font_family: Some("Menlo, Monaco, 'Courier New', monospace".into()),
            background_color: Some(self.theme.code_inline_bg),
            color: Some(self.theme.code_inline_text),
            ..Default::default()
        });
        self.push_text(text, source_range);
        self.pop_text_style();
    }

    fn push_text_style(&mut self, style: TextStyleRefinement) {
        merge_refinement(&mut self.combined_style, &style);
        self.text_style_stack.push(style);
    }

    fn pop_text_style(&mut self) {
        self.text_style_stack.pop();
        self.combined_style = combine_stack(&self.text_style_stack);
    }

    fn push_child(&mut self, element: AnyElement) {
        let element = if let Some(url) = self.link_url.clone() {
            div()
                .id(SharedString::from(format!("link-{url}")))
                .cursor_pointer()
                .child(element)
                .on_click(move |_event: &gpui::ClickEvent, _window, cx: &mut gpui::App| {
                    cx.open_url(url.as_ref());
                })
                .into_any_element()
        } else {
            element
        };

        if let Some(container) = self.container_stack.last_mut() {
            match container {
                Container::Paragraph { children }
                | Container::Heading { children, .. }
                | Container::BlockQuote { children }
                | Container::ListItem { children }
                | Container::TableCell { children }
                | Container::FootnoteDefinition { children, .. } => children.push(element),
                Container::TableRow { cells, .. } => cells.push(element),
                _ => {}
            }
        } else {
            self.root_blocks.push(element);
        }
    }

    fn flush_pending_line(&mut self) {
        if self.pending_line.text.is_empty() {
            return;
        }
        let text = std::mem::take(&mut self.pending_line.text);
        let runs = std::mem::take(&mut self.pending_line.runs);
        let mappings = std::mem::take(&mut self.pending_line.source_mappings);
        // TODO: attach source mappings to the rendered output so selection,
        // copy-as-markdown, and search highlights can map rendered positions
        // back to source positions. For now the mappings are preserved on the
        // builder and returned by `build_with_mappings`.
        self.source_mappings.extend(mappings);
        let text_runs = self.pending_runs_to_text_runs(&text, &runs);
        let styled = StyledText::new(text).with_runs(text_runs).into_any_element();
        self.push_child(styled);
    }

    fn pending_runs_to_text_runs(
        &self,
        text: &str,
        runs: &[PendingRun],
    ) -> Vec<TextRun> {
        let mut result = Vec::new();
        let mut last_end = 0;
        for run in runs {
            if run.start > last_end {
                let len = run.start - last_end;
                result.push(self.base_text_style.to_run(len));
                last_end = run.start;
            }
            let len = run.end - run.start;
            if len > 0 {
                let style = self.base_text_style.clone().refined(run.style.clone());
                result.push(style.to_run(len));
                last_end = run.end;
            }
        }
        if last_end < text.len() {
            result.push(self.base_text_style.to_run(text.len() - last_end));
        }
        result
    }

    fn finalize_container(&self, container: Container) -> AnyElement {
        match container {
            Container::Paragraph { children } => div()
                .w_full()
                .min_w_0()
                .text_left()
                .flex()
                .flex_col()
                .gap_1()
                .children(children)
                .into_any_element(),
            Container::Heading { level, children } => {
                let (size, weight) = heading_style(level);
                let font_size = px(size);
                div()
                    .w_full()
                    .min_w_0()
                    .mt_2()
                    .mb_1()
                    .text_size(font_size)
                    .font_weight(weight)
                    .text_color(self.theme.heading_color)
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(children)
                    .into_any_element()
            }
            Container::BlockQuote { children } => div()
                .w_full()
                .min_w_0()
                .border_l_4()
                .border_color(self.theme.quote_border)
                .text_color(self.theme.quote_text)
                .flex()
                .flex_col()
                .gap_1()
                .children(children)
                .into_any_element(),
            Container::List {
                ordered,
                start,
                items,
            } => div()
                .flex()
                .flex_col()
                .gap_1()
                .children(items.into_iter().enumerate().map(|(i, item_children)| {
                    let marker = if ordered {
                        let num = start.unwrap_or(1) + i as u64;
                        SharedString::from(format!("{num}. "))
                    } else {
                        SharedString::from("• ")
                    };
                    div()
                        .flex()
                        .flex_row()
                        .gap_1()
                        .child(
                            div()
                                .text_color(self.theme.list_marker)
                                .min_w(px(16.))
                                .child(marker),
                        )
                        .child(div().flex_1().flex().flex_col().gap_1().children(item_children))
                        .into_any_element()
                }))
                .into_any_element(),
            Container::Table {
                header_rows,
                body_rows,
            } => div()
                .flex()
                .flex_col()
                .w_full()
                .my_2()
                .rounded_md()
                .border_1()
                .border_color(self.theme.table_border)
                .children(header_rows)
                .children(body_rows)
                .into_any_element(),
            Container::CodeBlock { language, code } => self.render_code_block(language, code),
            Container::FootnoteDefinition { label, children } => div()
                .flex()
                .flex_col()
                .gap_1()
                .child(format!("[^{label}]"))
                .children(children)
                .into_any_element(),
            Container::HtmlBlock => div().into_any_element(),
            Container::ListItem { .. }
            | Container::TableRow { .. }
            | Container::TableCell { .. } => {
                // These containers are finalized inline, not through this path.
                div().into_any_element()
            }
        }
    }

    fn render_code_block(
        &self,
        language: Option<SharedString>,
        code: String,
    ) -> AnyElement {
        let code_for_copy = code.clone();
        let lang_label = language
            .clone()
            .unwrap_or_else(|| SharedString::from("code"));
        let is_diff = language.as_ref().map(|s| s.as_ref()) == Some("diff");

        let code_content = if is_diff {
            let lines: Vec<&str> = code.lines().collect();
            div()
                .id("diff-block")
                .p_3()
                .text_xs()
                .text_color(self.theme.code_text)
                .font_family("Menlo, Monaco, 'Courier New', monospace")
                .flex()
                .flex_col()
                .overflow_x_scroll()
                .children(lines.into_iter().map(|line| {
                    let bg_color = if line.starts_with('+') {
                        self.theme.diff_add_bg
                    } else if line.starts_with('-') {
                        self.theme.diff_remove_bg
                    } else if line.starts_with("@@") {
                        self.theme.diff_hunk_bg
                    } else {
                        self.theme.code_bg
                    };
                    div()
                        .px_2()
                        .py_px()
                        .bg(bg_color)
                        .whitespace_nowrap()
                        .child(line.to_string())
                }))
        } else {
            let highlighted = highlight_code(&code, language.as_deref().map(|v| v.as_ref()));
            div()
                .id("code-block")
                .p_3()
                .text_xs()
                .text_color(self.theme.code_text)
                .font_family("Menlo, Monaco, 'Courier New', monospace")
                .flex()
                .flex_col()
                .overflow_x_scroll()
                .children(highlighted.into_iter().map(|(line_text, line_highlights)| {
                    div()
                        .whitespace_nowrap()
                        .child(StyledText::new(line_text).with_highlights(line_highlights))
                }))
        };

        div()
            .flex()
            .flex_col()
            .w_full()
            .my_2()
            .rounded_md()
            .border_1()
            .border_color(self.theme.code_border)
            .bg(self.theme.code_bg)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .items_center()
                    .px_3()
                    .py_1()
                    .border_b_1()
                    .border_color(self.theme.code_border)
                    .bg(self.theme.code_header_bg)
                    .rounded_t_md()
                    .text_xs()
                    .text_color(self.theme.text_muted)
                    .child(lang_label)
                    .child(
                        div()
                            .id("copy-code-btn")
                            .px_2()
                            .py_px()
                            .rounded_md()
                            .bg(self.theme.button_bg)
                            .text_color(self.theme.button_text)
                            .cursor_pointer()
                            .child("Copy")
                            .on_click(
                                move |_event: &gpui::ClickEvent,
                                      _window,
                                      cx: &mut gpui::App| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        code_for_copy.to_string(),
                                    ));
                                },
                            ),
                    ),
            )
            .child(code_content)
            .into_any_element()
    }

    fn render_table_row(&self, cells: Vec<AnyElement>, is_header: bool) -> AnyElement {
        div()
            .flex()
            .flex_row()
            .w_full()
            .when(is_header, |this| {
                this.border_b_1()
                    .border_color(self.theme.rule_color)
                    .bg(self.theme.table_header_bg)
            })
            .children(cells)
            .into_any_element()
    }

    fn render_table_cell(&self, children: Vec<AnyElement>, align: Alignment) -> AnyElement {
        div()
            .flex_1()
            .px_2()
            .py_1()
            .when(align == Alignment::Center, |this| this.text_center())
            .when(align == Alignment::Right, |this| this.text_right())
            .border_r_1()
            .border_color(self.theme.table_border)
            .children(children)
            .into_any_element()
    }

    fn render_image(&self, url: &SharedString, alt: &str) -> AnyElement {
        let alt_text = if alt.is_empty() {
            format!("[Image: {url}]")
        } else {
            format!("[Image: {alt}]")
        };
        let fallback_bg = self.theme.code_inline_bg;
        let fallback_color = self.theme.image_text;
        let alt_for_fallback = alt_text.clone();
        if let Some(source) = image_source_from_url(url.as_ref()) {
            div()
                .min_w_0()
                .max_w_full()
                .child(
                    img(source)
                        .max_w_full()
                        .rounded_md()
                        .with_fallback(move || {
                            div()
                                .px_2()
                                .py_1()
                                .rounded_md()
                                .bg(fallback_bg)
                                .text_color(fallback_color)
                                .text_xs()
                                .child(alt_for_fallback.clone())
                                .into_any_element()
                        }),
                )
                .into_any_element()
        } else {
            div()
                .px_2()
                .py_1()
                .rounded_md()
                .bg(fallback_bg)
                .text_color(fallback_color)
                .text_xs()
                .child(alt_text)
                .into_any_element()
        }
    }
}

fn merge_refinement(base: &mut TextStyleRefinement, overlay: &TextStyleRefinement) {
    if let Some(v) = overlay.color {
        base.color = Some(v);
    }
    if let Some(v) = &overlay.font_family {
        base.font_family = Some(v.clone());
    }
    if let Some(v) = &overlay.font_features {
        base.font_features = Some(v.clone());
    }
    if let Some(v) = &overlay.font_fallbacks {
        base.font_fallbacks = Some(v.clone());
    }
    if let Some(v) = overlay.font_size {
        base.font_size = Some(v);
    }
    if let Some(v) = overlay.line_height {
        base.line_height = Some(v);
    }
    if let Some(v) = overlay.font_weight {
        base.font_weight = Some(v);
    }
    if let Some(v) = overlay.font_style {
        base.font_style = Some(v);
    }
    if let Some(v) = overlay.background_color {
        base.background_color = Some(v);
    }
    if let Some(v) = overlay.underline {
        base.underline = Some(v);
    }
    if let Some(v) = overlay.strikethrough {
        base.strikethrough = Some(v);
    }
    if let Some(v) = overlay.white_space {
        base.white_space = Some(v);
    }
    if let Some(v) = &overlay.text_overflow {
        base.text_overflow = Some(v.clone());
    }
    if let Some(v) = overlay.text_align {
        base.text_align = Some(v);
    }
    if let Some(v) = overlay.line_clamp {
        base.line_clamp = Some(v);
    }
}

fn combine_stack(stack: &[TextStyleRefinement]) -> TextStyleRefinement {
    let mut combined = TextStyleRefinement::default();
    for style in stack {
        merge_refinement(&mut combined, style);
    }
    combined
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::markdown::default_theme;
    use crate::ui::markdown::parser::parse_markdown;

    #[test]
    fn builder_produces_element_for_simple_paragraph() {
        let parsed = parse_markdown("Hello, world!");
        let theme = default_theme();
        // Construction succeeding is the test; the resulting AnyElement is opaque.
        let _element = MarkdownElementBuilder::build(&parsed, &theme);
    }

    #[test]
    fn builder_handles_all_heading_levels() {
        let source = "# H1\n\n## H2\n\n### H3\n\n#### H4\n\n##### H5\n\n###### H6";
        let parsed = parse_markdown(source);
        let theme = default_theme();
        let _element = MarkdownElementBuilder::build(&parsed, &theme);
    }

    #[test]
    fn builder_handles_inline_formatting() {
        let parsed = parse_markdown("Some **bold** and *italic* and `code` and ~~strike~~ text.");
        let theme = default_theme();
        let _element = MarkdownElementBuilder::build(&parsed, &theme);
    }

    #[test]
    fn builder_handles_link_and_image() {
        let parsed = parse_markdown(
            "A [link](https://example.com) and an image ![alt](https://example.com/img.png).",
        );
        let theme = default_theme();
        let _element = MarkdownElementBuilder::build(&parsed, &theme);
    }

    #[test]
    fn builder_handles_list_and_blockquote() {
        let parsed = parse_markdown(
            "> A quote\n\n- first\n- second\n\n5. ordered start\n6. next",
        );
        let theme = default_theme();
        let _element = MarkdownElementBuilder::build(&parsed, &theme);
    }

    #[test]
    fn builder_handles_table() {
        let parsed = parse_markdown("| a | b |\n|---|---:|\n| c | d |");
        let theme = default_theme();
        let _element = MarkdownElementBuilder::build(&parsed, &theme);
    }

    #[test]
    fn builder_handles_code_block() {
        let parsed = parse_markdown("```rust\nfn main() {}\n```");
        let theme = default_theme();
        let _element = MarkdownElementBuilder::build(&parsed, &theme);
    }

    #[test]
    fn builder_handles_rule_and_footnote() {
        let parsed = parse_markdown("text[^1]\n\n---\n\n[^1]: note");
        let theme = default_theme();
        let _element = MarkdownElementBuilder::build(&parsed, &theme);
    }

    #[test]
    fn builder_handles_html_block_and_inline_html() {
        let parsed = parse_markdown("<div>block</div>\n\nSome <br> inline.");
        let theme = default_theme();
        let _element = MarkdownElementBuilder::build(&parsed, &theme);
    }
}
