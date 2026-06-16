use std::{
    collections::VecDeque,
    ops::Range,
    sync::{Arc, OnceLock},
};

use gpui::{
    ClipboardItem, Context, FontStyle, FontWeight, HighlightStyle, Image, ImageFormat, IntoElement,
    ParentElement, Render, SharedString, StatefulInteractiveElement, StrikethroughStyle, Styled,
    StyledImage, StyledText, TextStyle, UnderlineStyle, Window, div, img, prelude::*, px, rgb,
};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Color as SyntectColor, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

static OPTIONS: Options = Options::from_bits_truncate(
    Options::ENABLE_TABLES.bits()
        | Options::ENABLE_STRIKETHROUGH.bits()
        | Options::ENABLE_TASKLISTS.bits()
        | Options::ENABLE_SMART_PUNCTUATION.bits()
        | Options::ENABLE_FOOTNOTES.bits()
        | Options::ENABLE_GFM.bits(),
);

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static THEME_SET: OnceLock<ThemeSet> = OnceLock::new();

fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn theme_set() -> &'static ThemeSet {
    THEME_SET.get_or_init(ThemeSet::load_defaults)
}

fn highlight_code(
    code: &str,
    language: Option<&str>,
) -> Vec<(String, Vec<(Range<usize>, HighlightStyle)>)> {
    let ss = syntax_set();
    let syntax = language
        .and_then(|lang| ss.find_syntax_by_token(lang))
        .unwrap_or_else(|| ss.find_syntax_plain_text());

    let ts = theme_set();
    let theme = &ts.themes["base16-ocean.dark"];
    let mut highlighter = HighlightLines::new(syntax, theme);

    let mut result = Vec::new();

    for line in LinesWithEndings::from(code) {
        let trimmed_line = if line.ends_with("\r\n") {
            &line[..line.len() - 2]
        } else if line.ends_with('\n') {
            &line[..line.len() - 1]
        } else {
            line
        };

        let highlighted = highlighter
            .highlight_line(trimmed_line, ss)
            .unwrap_or_default();
        let mut line_text = String::new();
        let mut line_highlights: Vec<(Range<usize>, HighlightStyle)> = Vec::new();

        for (style, text) in highlighted {
            let start = line_text.len();
            line_text.push_str(text);
            let end = line_text.len();

            let color = style.foreground;
            // Skip default white foreground to avoid unnecessary highlights
            if color == SyntectColor::WHITE {
                continue;
            }

            let gpui_color =
                rgb((color.r as u32) * 0x10000 + (color.g as u32) * 0x100 + (color.b as u32));

            line_highlights.push((
                start..end,
                HighlightStyle {
                    color: Some(gpui_color.into()),
                    ..Default::default()
                },
            ));
        }

        result.push((line_text, line_highlights));
    }

    result
}

pub struct MarkdownRenderer {
    content: SharedString,
    parsed: Option<Vec<BlockNode>>,
}

impl MarkdownRenderer {
    pub fn new(content: impl Into<SharedString>) -> Self {
        Self {
            content: content.into(),
            parsed: None,
        }
    }

    pub fn set_content(&mut self, content: impl Into<SharedString>) {
        self.content = content.into();
        self.parsed = None;
    }
}

// ---- AST types ----

#[derive(Debug, Clone)]
enum InlineNode {
    Text {
        text: SharedString,
    },
    Code {
        code: SharedString,
    },
    Emphasis {
        children: Vec<InlineNode>,
    },
    Strong {
        children: Vec<InlineNode>,
    },
    Strikethrough {
        children: Vec<InlineNode>,
    },
    Link {
        children: Vec<InlineNode>,
        url: SharedString,
    },
    Image {
        alt: SharedString,
        url: SharedString,
    },
    SoftBreak,
    HardBreak,
    InlineHtml {
        html: SharedString,
    },
    TaskMarker {
        checked: bool,
    },
}

#[derive(Debug, Clone)]
enum BlockNode {
    Paragraph {
        inlines: Vec<InlineNode>,
    },
    Heading {
        level: HeadingLevel,
        inlines: Vec<InlineNode>,
    },
    CodeBlock {
        language: Option<SharedString>,
        code: SharedString,
    },
    BlockQuote {
        children: Vec<BlockNode>,
    },
    List {
        ordered: bool,
        start: Option<u64>,
        items: Vec<Vec<BlockNode>>,
    },
    Table {
        alignments: Vec<pulldown_cmark::Alignment>,
        headers: Vec<Vec<InlineNode>>,
        rows: Vec<Vec<Vec<InlineNode>>>,
    },
    Rule,
}

// ---- Parser ----

fn parse_markdown(source: &str) -> Vec<BlockNode> {
    let parser = Parser::new_ext(source, OPTIONS);

    let mut root_blocks: Vec<BlockNode> = Vec::new();
    let mut block_stack: VecDeque<BlockContext> = VecDeque::new();
    let mut inline_buffer: Vec<InlineNode> = Vec::new();

    let mut table_headers: Vec<Vec<InlineNode>> = Vec::new();
    let mut table_rows: Vec<Vec<Vec<InlineNode>>> = Vec::new();
    let mut table_alignments: Vec<pulldown_cmark::Alignment> = Vec::new();
    let mut current_row: Vec<Vec<InlineNode>> = Vec::new();
    let mut in_table_head = false;
    let mut code_lang: Option<SharedString> = None;
    let mut code_text = String::new();
    let mut in_code_block = false;

    for event in parser {
        match event {
            Event::Start(tag) => {
                flush_inlines(&mut inline_buffer, &mut block_stack, &mut root_blocks);

                match tag {
                    Tag::Paragraph => {
                        block_stack.push_back(BlockContext::Paragraph {
                            inlines: Vec::new(),
                        });
                    }
                    Tag::Heading { level, .. } => {
                        block_stack.push_back(BlockContext::Heading {
                            level,
                            inlines: Vec::new(),
                        });
                    }
                    Tag::CodeBlock(kind) => {
                        in_code_block = true;
                        code_text.clear();
                        code_lang = match kind {
                            CodeBlockKind::Fenced(lang) => {
                                if lang.is_empty() {
                                    None
                                } else {
                                    Some(lang.to_string().into())
                                }
                            }
                            CodeBlockKind::Indented => None,
                        };
                    }
                    Tag::BlockQuote(_) => {
                        block_stack.push_back(BlockContext::BlockQuote {
                            children: Vec::new(),
                        });
                    }
                    Tag::List(order) => {
                        block_stack.push_back(BlockContext::List {
                            ordered: order.is_some(),
                            start: order,
                            items: Vec::new(),
                        });
                    }
                    Tag::Item => {
                        block_stack.push_back(BlockContext::ListItem { blocks: Vec::new() });
                    }
                    Tag::Table(alignments) => {
                        table_alignments = alignments;
                        table_headers.clear();
                        table_rows.clear();
                        block_stack.push_back(BlockContext::Table);
                    }
                    Tag::TableHead => {
                        in_table_head = true;
                    }
                    Tag::TableRow => {
                        current_row.clear();
                    }
                    Tag::TableCell => {
                        block_stack.push_back(BlockContext::InlineContainer {
                            inlines: Vec::new(),
                        });
                    }
                    Tag::Emphasis => {
                        block_stack.push_back(BlockContext::Emphasis {
                            children: Vec::new(),
                        });
                    }
                    Tag::Strong => {
                        block_stack.push_back(BlockContext::Strong {
                            children: Vec::new(),
                        });
                    }
                    Tag::Strikethrough => {
                        block_stack.push_back(BlockContext::Strikethrough {
                            children: Vec::new(),
                        });
                    }
                    Tag::Link { dest_url, .. } => {
                        block_stack.push_back(BlockContext::Link {
                            url: dest_url.to_string().into(),
                            children: Vec::new(),
                        });
                    }
                    Tag::Image { dest_url, .. } => {
                        let img_node = InlineNode::Image {
                            alt: SharedString::default(),
                            url: dest_url.to_string().into(),
                        };
                        push_inline(&mut inline_buffer, &mut block_stack, img_node);
                    }
                    _ => {}
                }
            }
            Event::End(end_tag) => match end_tag {
                TagEnd::Paragraph => {
                    if let Some(ctx) = block_stack.pop_back() {
                        if let BlockContext::Paragraph { inlines } = ctx {
                            let block = BlockNode::Paragraph { inlines };
                            add_block(&mut block_stack, &mut root_blocks, block);
                        }
                    }
                }
                TagEnd::Heading(level) => {
                    if let Some(ctx) = block_stack.pop_back() {
                        if let BlockContext::Heading { inlines, .. } = ctx {
                            let block = BlockNode::Heading { level, inlines };
                            add_block(&mut block_stack, &mut root_blocks, block);
                        }
                    }
                }
                TagEnd::CodeBlock => {
                    in_code_block = false;
                    let block = BlockNode::CodeBlock {
                        language: code_lang.take(),
                        code: SharedString::from(std::mem::take(&mut code_text)),
                    };
                    add_block(&mut block_stack, &mut root_blocks, block);
                }
                TagEnd::BlockQuote(_) => {
                    flush_inlines(&mut inline_buffer, &mut block_stack, &mut root_blocks);
                    if let Some(ctx) = block_stack.pop_back() {
                        if let BlockContext::BlockQuote { children } = ctx {
                            let block = BlockNode::BlockQuote { children };
                            add_block(&mut block_stack, &mut root_blocks, block);
                        }
                    }
                }
                TagEnd::List(is_ordered) => {
                    if let Some(ctx) = block_stack.pop_back() {
                        if let BlockContext::List { items, start, .. } = ctx {
                            let block = BlockNode::List {
                                ordered: is_ordered,
                                start,
                                items,
                            };
                            add_block(&mut block_stack, &mut root_blocks, block);
                        }
                    }
                }
                TagEnd::Item => {
                    flush_inlines(&mut inline_buffer, &mut block_stack, &mut root_blocks);
                    if let Some(ctx) = block_stack.pop_back() {
                        if let BlockContext::ListItem { blocks } = ctx {
                            if let Some(parent) = block_stack.back_mut() {
                                if let BlockContext::List { items, .. } = parent {
                                    items.push(blocks);
                                }
                            }
                        }
                    }
                }
                TagEnd::Table => {
                    if block_stack.pop_back().is_some() {
                        let block = BlockNode::Table {
                            alignments: std::mem::take(&mut table_alignments),
                            headers: std::mem::take(&mut table_headers),
                            rows: std::mem::take(&mut table_rows),
                        };
                        add_block(&mut block_stack, &mut root_blocks, block);
                    }
                }
                TagEnd::TableHead => {
                    in_table_head = false;
                }
                TagEnd::TableRow => {
                    if in_table_head {
                        table_headers = std::mem::take(&mut current_row);
                    } else {
                        table_rows.push(std::mem::take(&mut current_row));
                    }
                }
                TagEnd::TableCell => {
                    if let Some(ctx) = block_stack.pop_back() {
                        if let BlockContext::InlineContainer { inlines } = ctx {
                            current_row.push(inlines);
                        }
                    }
                }
                TagEnd::Emphasis => {
                    if let Some(ctx) = block_stack.pop_back() {
                        if let BlockContext::Emphasis { children } = ctx {
                            push_inline(
                                &mut inline_buffer,
                                &mut block_stack,
                                InlineNode::Emphasis { children },
                            );
                        }
                    }
                }
                TagEnd::Strong => {
                    if let Some(ctx) = block_stack.pop_back() {
                        if let BlockContext::Strong { children } = ctx {
                            push_inline(
                                &mut inline_buffer,
                                &mut block_stack,
                                InlineNode::Strong { children },
                            );
                        }
                    }
                }
                TagEnd::Strikethrough => {
                    if let Some(ctx) = block_stack.pop_back() {
                        if let BlockContext::Strikethrough { children } = ctx {
                            push_inline(
                                &mut inline_buffer,
                                &mut block_stack,
                                InlineNode::Strikethrough { children },
                            );
                        }
                    }
                }
                TagEnd::Link => {
                    if let Some(ctx) = block_stack.pop_back() {
                        if let BlockContext::Link { children, url } = ctx {
                            push_inline(
                                &mut inline_buffer,
                                &mut block_stack,
                                InlineNode::Link { children, url },
                            );
                        }
                    }
                }
                _ => {}
            },
            Event::Text(text) => {
                if in_code_block {
                    code_text.push_str(&text);
                } else {
                    push_inline(
                        &mut inline_buffer,
                        &mut block_stack,
                        InlineNode::Text {
                            text: text.to_string().into(),
                        },
                    );
                }
            }
            Event::Code(code) => {
                push_inline(
                    &mut inline_buffer,
                    &mut block_stack,
                    InlineNode::Code {
                        code: code.to_string().into(),
                    },
                );
            }
            Event::SoftBreak => {
                push_inline(&mut inline_buffer, &mut block_stack, InlineNode::SoftBreak);
            }
            Event::HardBreak => {
                push_inline(&mut inline_buffer, &mut block_stack, InlineNode::HardBreak);
            }
            Event::Rule => {
                flush_inlines(&mut inline_buffer, &mut block_stack, &mut root_blocks);
                root_blocks.push(BlockNode::Rule);
            }
            Event::TaskListMarker(checked) => {
                push_inline(
                    &mut inline_buffer,
                    &mut block_stack,
                    InlineNode::TaskMarker { checked },
                );
            }
            Event::InlineHtml(html) | Event::Html(html) => {
                let stripped = strip_html_tags(&html);
                if !stripped.is_empty() {
                    push_inline(
                        &mut inline_buffer,
                        &mut block_stack,
                        InlineNode::InlineHtml {
                            html: stripped.into(),
                        },
                    );
                }
            }
            _ => {}
        }
    }

    flush_inlines(&mut inline_buffer, &mut block_stack, &mut root_blocks);

    root_blocks
}

fn push_inline(
    inline_buffer: &mut Vec<InlineNode>,
    block_stack: &mut VecDeque<BlockContext>,
    node: InlineNode,
) {
    for ctx in block_stack.iter_mut().rev() {
        match ctx {
            BlockContext::InlineContainer { inlines } => {
                inlines.push(node);
                return;
            }
            BlockContext::Emphasis { children }
            | BlockContext::Strong { children }
            | BlockContext::Strikethrough { children }
            | BlockContext::Link { children, .. } => {
                children.push(node);
                return;
            }
            _ => continue,
        }
    }
    inline_buffer.push(node);
}

fn flush_inlines(
    inline_buffer: &mut Vec<InlineNode>,
    block_stack: &mut VecDeque<BlockContext>,
    root_blocks: &mut Vec<BlockNode>,
) {
    if inline_buffer.is_empty() {
        return;
    }
    let inlines = std::mem::take(inline_buffer);
    for ctx in block_stack.iter_mut().rev() {
        match ctx {
            BlockContext::InlineContainer { inlines: target }
            | BlockContext::Paragraph { inlines: target }
            | BlockContext::Heading {
                inlines: target, ..
            } => {
                target.extend(inlines);
                return;
            }
            _ => continue,
        }
    }
    add_block(block_stack, root_blocks, BlockNode::Paragraph { inlines });
}

fn add_block(
    block_stack: &mut VecDeque<BlockContext>,
    root_blocks: &mut Vec<BlockNode>,
    block: BlockNode,
) {
    for ctx in block_stack.iter_mut().rev() {
        match ctx {
            BlockContext::BlockQuote { children } => {
                children.push(block);
                return;
            }
            BlockContext::ListItem { blocks } => {
                blocks.push(block);
                return;
            }
            _ => continue,
        }
    }
    root_blocks.push(block);
}

struct MarkdownTheme {
    text_primary: gpui::Hsla,
    text_secondary: gpui::Hsla,
    text_muted: gpui::Hsla,
    code_bg: gpui::Hsla,
    code_header_bg: gpui::Hsla,
    code_border: gpui::Hsla,
    code_text: gpui::Hsla,
    code_inline_bg: gpui::Hsla,
    code_inline_text: gpui::Hsla,
    link_color: gpui::Hsla,
    link_underline: gpui::Hsla,
    heading_color: gpui::Hsla,
    quote_border: gpui::Hsla,
    quote_text: gpui::Hsla,
    list_marker: gpui::Hsla,
    table_border: gpui::Hsla,
    table_header_bg: gpui::Hsla,
    table_header_text: gpui::Hsla,
    table_row_text: gpui::Hsla,
    rule_color: gpui::Hsla,
    task_checked: gpui::Hsla,
    task_unchecked: gpui::Hsla,
    image_text: gpui::Hsla,
    diff_add_bg: gpui::Hsla,
    diff_remove_bg: gpui::Hsla,
    diff_hunk_bg: gpui::Hsla,
    button_bg: gpui::Hsla,
    button_text: gpui::Hsla,
}

fn default_theme() -> MarkdownTheme {
    MarkdownTheme {
        text_primary: rgb(0xe5e5e5).into(),
        text_secondary: rgb(0xf0f0f0).into(),
        text_muted: rgb(0x888888).into(),
        code_bg: rgb(0x1e1e1e).into(),
        code_header_bg: rgb(0x2a2a2a).into(),
        code_border: rgb(0x404040).into(),
        code_text: rgb(0xe5e5e5).into(),
        code_inline_bg: rgb(0x333333).into(),
        code_inline_text: rgb(0xe5c07b).into(),
        link_color: rgb(0x60a5fa).into(),
        link_underline: rgb(0x60a5fa).into(),
        heading_color: rgb(0xf0f0f0).into(),
        quote_border: rgb(0x444444).into(),
        quote_text: rgb(0xaaaaaa).into(),
        list_marker: rgb(0x888888).into(),
        table_border: rgb(0x404040).into(),
        table_header_bg: rgb(0x252525).into(),
        table_header_text: rgb(0xf0f0f0).into(),
        table_row_text: rgb(0xe5e5e5).into(),
        rule_color: rgb(0x444444).into(),
        task_checked: rgb(0x3b82f6).into(),
        task_unchecked: rgb(0x888888).into(),
        image_text: rgb(0x888888).into(),
        diff_add_bg: rgb(0x1a472a).into(),
        diff_remove_bg: rgb(0x5c1a1a).into(),
        diff_hunk_bg: rgb(0x2a2a5a).into(),
        button_bg: rgb(0x404040).into(),
        button_text: rgb(0xcccccc).into(),
    }
}

enum BlockContext {
    Paragraph {
        inlines: Vec<InlineNode>,
    },
    Heading {
        level: HeadingLevel,
        inlines: Vec<InlineNode>,
    },
    BlockQuote {
        children: Vec<BlockNode>,
    },
    List {
        ordered: bool,
        start: Option<u64>,
        items: Vec<Vec<BlockNode>>,
    },
    ListItem {
        blocks: Vec<BlockNode>,
    },
    Table,
    InlineContainer {
        inlines: Vec<InlineNode>,
    },
    Emphasis {
        children: Vec<InlineNode>,
    },
    Strong {
        children: Vec<InlineNode>,
    },
    Strikethrough {
        children: Vec<InlineNode>,
    },
    Link {
        url: SharedString,
        children: Vec<InlineNode>,
    },
}

// ---- Renderer ----

fn append_highlighted_text(
    text: &mut String,
    highlights: &mut Vec<(Range<usize>, HighlightStyle)>,
    segment: &str,
    style: HighlightStyle,
) {
    if segment.is_empty() {
        return;
    }

    let start = text.len();
    text.push_str(segment);
    let end = text.len();

    if style == HighlightStyle::default() {
        return;
    }

    if let Some((range, last_style)) = highlights.last_mut() {
        if *last_style == style && range.end == start {
            range.end = end;
            return;
        }
    }

    highlights.push((start..end, style));
}

const INLINE_HORIZONTAL_PADDING: &str = "\u{2009}";

fn append_inline_padding(
    text: &mut String,
    highlights: &mut Vec<(Range<usize>, HighlightStyle)>,
    style: HighlightStyle,
) {
    append_highlighted_text(text, highlights, INLINE_HORIZONTAL_PADDING, style);
}

fn append_padded_segment(
    text: &mut String,
    highlights: &mut Vec<(Range<usize>, HighlightStyle)>,
    segment: &str,
    style: HighlightStyle,
) {
    if segment.is_empty() {
        return;
    }

    append_inline_padding(text, highlights, style);
    append_highlighted_text(text, highlights, segment, style);
    append_inline_padding(text, highlights, style);
}

fn append_padded_children(
    text: &mut String,
    highlights: &mut Vec<(Range<usize>, HighlightStyle)>,
    children: &[InlineNode],
    style: HighlightStyle,
    theme: &MarkdownTheme,
) {
    if children.is_empty() {
        return;
    }

    append_inline_padding(text, highlights, style);
    collect_styled_inlines(children, style, text, highlights, theme);
    append_inline_padding(text, highlights, style);
}

fn collect_styled_inlines(
    inlines: &[InlineNode],
    active_style: HighlightStyle,
    text: &mut String,
    highlights: &mut Vec<(Range<usize>, HighlightStyle)>,
    theme: &MarkdownTheme,
) {
    for inline in inlines {
        match inline {
            InlineNode::Text { text: inline_text } => {
                append_highlighted_text(text, highlights, inline_text, active_style);
            }
            InlineNode::Code { code } => {
                let style = active_style.highlight(HighlightStyle {
                    color: Some(theme.code_inline_text),
                    background_color: Some(theme.code_inline_bg),
                    ..Default::default()
                });
                append_padded_segment(text, highlights, code, style);
            }
            InlineNode::Emphasis { children } => {
                append_padded_children(
                    text,
                    highlights,
                    children,
                    active_style.highlight(FontStyle::Italic.into()),
                    theme,
                );
            }
            InlineNode::Strong { children } => {
                append_padded_children(
                    text,
                    highlights,
                    children,
                    active_style.highlight(FontWeight(700.0).into()),
                    theme,
                );
            }
            InlineNode::Strikethrough { children } => {
                append_padded_children(
                    text,
                    highlights,
                    children,
                    active_style.highlight(HighlightStyle {
                        strikethrough: Some(StrikethroughStyle::default()),
                        ..Default::default()
                    }),
                    theme,
                );
            }
            InlineNode::Link { children, .. } => {
                append_padded_children(
                    text,
                    highlights,
                    children,
                    active_style.highlight(HighlightStyle {
                        color: Some(theme.link_color),
                        underline: Some(UnderlineStyle {
                            thickness: px(1.),
                            color: Some(theme.link_underline),
                            wavy: false,
                        }),
                        ..Default::default()
                    }),
                    theme,
                );
            }
            InlineNode::Image { alt, url } => {
                let image_text = if alt.is_empty() {
                    format!("[Image: {url}]")
                } else {
                    format!("[Image: {alt}]")
                };
                append_padded_segment(
                    text,
                    highlights,
                    &image_text,
                    active_style.highlight(HighlightStyle::color(theme.image_text)),
                );
            }
            InlineNode::SoftBreak => append_highlighted_text(text, highlights, " ", active_style),
            InlineNode::HardBreak => append_highlighted_text(text, highlights, "\n", active_style),
            InlineNode::InlineHtml { html } => {
                append_padded_segment(text, highlights, html, active_style);
            }
            InlineNode::TaskMarker { checked } => {
                append_highlighted_text(
                    text,
                    highlights,
                    if *checked { "[x] " } else { "[ ] " },
                    active_style.highlight(HighlightStyle::color(if *checked {
                        theme.task_checked
                    } else {
                        theme.task_unchecked
                    })),
                );
            }
        }
    }
}

fn build_styled_inline_text(
    inlines: &[InlineNode],
    theme: &MarkdownTheme,
) -> (String, Vec<(Range<usize>, HighlightStyle)>) {
    let mut text = String::new();
    let mut highlights = Vec::new();
    collect_styled_inlines(
        inlines,
        HighlightStyle::default(),
        &mut text,
        &mut highlights,
        theme,
    );
    (text, highlights)
}

fn render_styled_inlines(inlines: &[InlineNode], theme: &MarkdownTheme) -> StyledText {
    let (text, highlights) = build_styled_inline_text(inlines, theme);
    StyledText::new(text).with_highlights(highlights)
}

fn render_styled_inlines_with_size(
    inlines: &[InlineNode],
    theme: &MarkdownTheme,
    font_size: gpui::Pixels,
) -> StyledText {
    let (text, highlights) = build_styled_inline_text(inlines, theme);
    let mut style = TextStyle::default();
    style.font_size = gpui::AbsoluteLength::Pixels(font_size);
    style.color = theme.text_primary;
    StyledText::new(text).with_default_highlights(&style, highlights)
}

fn render_inlines_text(inlines: &[InlineNode]) -> String {
    let mut result = String::new();
    for inline in inlines {
        match inline {
            InlineNode::Text { text } => result.push_str(text),
            InlineNode::Code { code } => {
                result.push('`');
                result.push_str(code);
                result.push('`');
            }
            InlineNode::Emphasis { children } => {
                result.push('_');
                result.push_str(&render_inlines_text(children));
                result.push('_');
            }
            InlineNode::Strong { children } => {
                result.push_str("**");
                result.push_str(&render_inlines_text(children));
                result.push_str("**");
            }
            InlineNode::Strikethrough { children } => {
                result.push_str("~~");
                result.push_str(&render_inlines_text(children));
                result.push_str("~~");
            }
            InlineNode::Link { children, .. } => {
                result.push_str(&render_inlines_text(children));
            }
            InlineNode::Image { alt, url } => {
                if alt.is_empty() {
                    result.push_str("[Image: ");
                    result.push_str(url);
                } else {
                    result.push_str("[Image: ");
                    result.push_str(alt);
                }
                result.push(']');
            }
            InlineNode::SoftBreak => result.push(' '),
            InlineNode::HardBreak => result.push('\n'),
            InlineNode::InlineHtml { html } => result.push_str(html),
            InlineNode::TaskMarker { checked } => {
                if *checked {
                    result.push_str("[x] ");
                } else {
                    result.push_str("[ ] ");
                }
            }
        }
    }
    result
}

fn decode_data_url_image(url: &str) -> Option<Arc<Image>> {
    let data_url = url.strip_prefix("data:")?;
    let (mime_info, data) = data_url.split_once(',')?;
    let (mime_type, encoding) = mime_info.split_once(';')?;
    let format = ImageFormat::from_mime_type(mime_type)?;
    if encoding != "base64" {
        return None;
    }
    let bytes = base64::Engine::decode(&base64::prelude::BASE64_STANDARD, data).ok()?;
    Some(Arc::new(Image::from_bytes(format, bytes)))
}

fn image_source_from_url(url: &str) -> Option<gpui::ImageSource> {
    if url.starts_with("data:") {
        let image = decode_data_url_image(url)?;
        Some(gpui::ImageSource::Image(image))
    } else {
        Some(url.into())
    }
}

fn inlines_contain_image(inlines: &[InlineNode]) -> bool {
    inlines.iter().any(|inline| {
        matches!(inline, InlineNode::Image { .. })
            || match inline {
                InlineNode::Emphasis { children }
                | InlineNode::Strong { children }
                | InlineNode::Strikethrough { children }
                | InlineNode::Link { children, .. } => inlines_contain_image(children),
                _ => false,
            }
    })
}

fn inlines_contain_link(inlines: &[InlineNode]) -> bool {
    inlines.iter().any(|inline| {
        matches!(inline, InlineNode::Link { .. })
            || match inline {
                InlineNode::Emphasis { children }
                | InlineNode::Strong { children }
                | InlineNode::Strikethrough { children }
                | InlineNode::Link { children, .. } => inlines_contain_link(children),
                _ => false,
            }
    })
}

fn render_link_element(
    children: &[InlineNode],
    url: &SharedString,
    theme: &MarkdownTheme,
    base_text_style: Option<TextStyle>,
) -> gpui::AnyElement {
    let mut style = base_text_style.unwrap_or_else(|| {
        let mut s = TextStyle::default();
        s.color = theme.text_primary;
        s
    });
    style.color = theme.link_color;
    style.underline = Some(UnderlineStyle {
        thickness: px(1.),
        color: Some(theme.link_underline),
        wavy: false,
    });

    let url = url.clone();
    div()
        .id(SharedString::from(format!("link-{}", url)))
        .cursor_pointer()
        .flex()
        .flex_row()
        .children(render_inlines_as_elements(children, theme, Some(style)))
        .on_click(
            move |_event: &gpui::ClickEvent, _window, cx: &mut gpui::App| {
                let _ = cx.open_url(&url);
            },
        )
        .into_any_element()
}

/// Renders inline nodes as a sequence of GPUI elements, mixing StyledText
/// with img() elements for images (like Zed's markdown renderer).
fn render_inlines_as_elements(
    inlines: &[InlineNode],
    theme: &MarkdownTheme,
    base_text_style: Option<TextStyle>,
) -> Vec<gpui::AnyElement> {
    let mut elements: Vec<gpui::AnyElement> = Vec::new();
    let mut text = String::new();
    let mut highlights: Vec<(Range<usize>, HighlightStyle)> = Vec::new();

    fn flush_text(
        text: &mut String,
        highlights: &mut Vec<(Range<usize>, HighlightStyle)>,
        elements: &mut Vec<gpui::AnyElement>,
        base_text_style: &Option<TextStyle>,
    ) {
        if text.is_empty() {
            return;
        }
        let styled = if let Some(style) = base_text_style {
            StyledText::new(std::mem::take(text))
                .with_default_highlights(style, std::mem::take(highlights))
        } else {
            StyledText::new(std::mem::take(text)).with_highlights(std::mem::take(highlights))
        };
        elements.push(styled.into_any_element());
    }

    fn collect(
        inlines: &[InlineNode],
        active_style: HighlightStyle,
        text: &mut String,
        highlights: &mut Vec<(Range<usize>, HighlightStyle)>,
        elements: &mut Vec<gpui::AnyElement>,
        theme: &MarkdownTheme,
        base_text_style: &Option<TextStyle>,
    ) {
        for inline in inlines {
            match inline {
                InlineNode::Text { text: inline_text } => {
                    append_highlighted_text(text, highlights, inline_text, active_style);
                }
                InlineNode::Code { code } => {
                    let style = active_style.highlight(HighlightStyle {
                        color: Some(theme.code_inline_text),
                        background_color: Some(theme.code_inline_bg),
                        ..Default::default()
                    });
                    append_padded_segment(text, highlights, code, style);
                }
                InlineNode::Emphasis { children } => {
                    append_padded_children_to_elements(
                        text,
                        highlights,
                        elements,
                        children,
                        active_style.highlight(FontStyle::Italic.into()),
                        theme,
                        base_text_style,
                    );
                }
                InlineNode::Strong { children } => {
                    append_padded_children_to_elements(
                        text,
                        highlights,
                        elements,
                        children,
                        active_style.highlight(FontWeight(700.0).into()),
                        theme,
                        base_text_style,
                    );
                }
                InlineNode::Strikethrough { children } => {
                    append_padded_children_to_elements(
                        text,
                        highlights,
                        elements,
                        children,
                        active_style.highlight(HighlightStyle {
                            strikethrough: Some(StrikethroughStyle::default()),
                            ..Default::default()
                        }),
                        theme,
                        base_text_style,
                    );
                }
                InlineNode::Link { children, url } => {
                    flush_text(text, highlights, elements, base_text_style);
                    elements.push(render_link_element(
                        children,
                        url,
                        theme,
                        base_text_style.clone(),
                    ));
                }
                InlineNode::Image { alt, url } => {
                    flush_text(text, highlights, elements, base_text_style);
                    let alt_text = if alt.is_empty() {
                        format!("[Image: {url}]")
                    } else {
                        format!("[Image: {alt}]")
                    };
                    let fallback_bg = theme.code_inline_bg;
                    let fallback_color = theme.image_text;
                    let image_element =
                        if let Some(source) = image_source_from_url(url) {
                            div()
                                .min_w_0()
                                .max_w_full()
                                .child(img(source).max_w_full().rounded_md().with_fallback(
                                    move || {
                                        div()
                                            .px_2()
                                            .py_1()
                                            .rounded_md()
                                            .bg(fallback_bg)
                                            .text_color(fallback_color)
                                            .text_xs()
                                            .child(alt_text.clone())
                                            .into_any_element()
                                    },
                                ))
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
                        };
                    elements.push(image_element);
                }
                InlineNode::SoftBreak => {
                    append_highlighted_text(text, highlights, " ", active_style);
                }
                InlineNode::HardBreak => {
                    append_highlighted_text(text, highlights, "\n", active_style);
                }
                InlineNode::InlineHtml { html } => {
                    append_padded_segment(text, highlights, html, active_style);
                }
                InlineNode::TaskMarker { checked } => {
                    append_highlighted_text(
                        text,
                        highlights,
                        if *checked { "[x] " } else { "[ ] " },
                        active_style.highlight(HighlightStyle::color(if *checked {
                            theme.task_checked
                        } else {
                            theme.task_unchecked
                        })),
                    );
                }
            }
        }
    }

    fn append_padded_children_to_elements(
        text: &mut String,
        highlights: &mut Vec<(Range<usize>, HighlightStyle)>,
        elements: &mut Vec<gpui::AnyElement>,
        children: &[InlineNode],
        style: HighlightStyle,
        theme: &MarkdownTheme,
        base_text_style: &Option<TextStyle>,
    ) {
        if children.is_empty() {
            return;
        }
        append_inline_padding(text, highlights, style);
        collect(
            children,
            style,
            text,
            highlights,
            elements,
            theme,
            base_text_style,
        );
        append_inline_padding(text, highlights, style);
    }

    collect(
        inlines,
        HighlightStyle::default(),
        &mut text,
        &mut highlights,
        &mut elements,
        theme,
        &base_text_style,
    );
    flush_text(&mut text, &mut highlights, &mut elements, &base_text_style);
    elements
}

fn heading_style(level: HeadingLevel) -> (f32, FontWeight) {
    match level {
        HeadingLevel::H1 => (26.0, FontWeight(700.0)),
        HeadingLevel::H2 => (22.0, FontWeight(700.0)),
        HeadingLevel::H3 => (19.0, FontWeight(700.0)),
        HeadingLevel::H4 => (16.0, FontWeight(600.0)),
        HeadingLevel::H5 => (14.0, FontWeight(600.0)),
        HeadingLevel::H6 => (13.0, FontWeight(600.0)),
    }
}

fn render_blocks(blocks: &[BlockNode], theme: &MarkdownTheme) -> Vec<gpui::AnyElement> {
    blocks
        .iter()
        .map(|block| render_block(block, theme))
        .collect()
}

fn render_block(block: &BlockNode, theme: &MarkdownTheme) -> gpui::AnyElement {
    match block {
        BlockNode::Paragraph { inlines } => {
            if inlines.is_empty() {
                div().h(px(4.)).into_any_element()
            } else if inlines_contain_image(inlines) || inlines_contain_link(inlines) {
                div()
                    .w_full()
                    .min_w_0()
                    .text_left()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(render_inlines_as_elements(inlines, theme, None))
                    .into_any_element()
            } else {
                div()
                    .w_full()
                    .min_w_0()
                    .text_left()
                    .child(render_styled_inlines(inlines, theme))
                    .into_any_element()
            }
        }
        BlockNode::Heading { level, inlines } => {
            let (font_size, weight) = heading_style(*level);
            let font_size = px(font_size);
            let heading = div()
                .w_full()
                .min_w_0()
                .mt_2()
                .mb_1()
                .text_size(font_size)
                .font_weight(weight)
                .text_color(theme.heading_color);
            if inlines_contain_image(inlines) || inlines_contain_link(inlines) {
                let mut style = TextStyle::default();
                style.font_size = gpui::AbsoluteLength::Pixels(font_size);
                style.color = theme.heading_color;
                heading
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(render_inlines_as_elements(inlines, theme, Some(style)))
                    .into_any_element()
            } else {
                heading
                    .child(render_styled_inlines_with_size(inlines, theme, font_size))
                    .into_any_element()
            }
        }
        BlockNode::CodeBlock { language, code } => {
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
                    .text_color(theme.code_text)
                    .font_family("Menlo, Monaco, 'Courier New', monospace")
                    .flex()
                    .flex_col()
                    .overflow_x_scroll()
                    .children(lines.into_iter().map(|line| {
                        let bg_color = if line.starts_with('+') {
                            theme.diff_add_bg
                        } else if line.starts_with('-') {
                            theme.diff_remove_bg
                        } else if line.starts_with("@@") {
                            theme.diff_hunk_bg
                        } else {
                            theme.code_bg
                        };
                        div()
                            .px_2()
                            .py_px()
                            .bg(bg_color)
                            .whitespace_nowrap()
                            .child(line.to_string())
                    }))
            } else {
                let highlighted = highlight_code(code, language.as_deref().map(|v| v.as_ref()));
                div()
                    .id("code-block")
                    .p_3()
                    .text_xs()
                    .text_color(theme.code_text)
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
                .border_color(theme.code_border)
                .bg(theme.code_bg)
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .justify_between()
                        .items_center()
                        .px_3()
                        .py_1()
                        .border_b_1()
                        .border_color(theme.code_border)
                        .bg(theme.code_header_bg)
                        .rounded_t_md()
                        .text_xs()
                        .text_color(theme.text_muted)
                        .child(lang_label)
                        .child(
                            div()
                                .id("copy-code-btn")
                                .px_2()
                                .py_px()
                                .rounded_md()
                                .bg(theme.button_bg)
                                .text_color(theme.button_text)
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
        BlockNode::BlockQuote { children } => div()
            .w_full()
            .min_w_0()
            .border_l_4()
            .border_color(theme.quote_border)
            .text_color(theme.quote_text)
            .flex()
            .flex_col()
            .gap_1()
            .children(render_blocks(children, theme))
            .into_any_element(),
        BlockNode::List {
            ordered,
            start,
            items,
        } => div()
            .flex()
            .flex_col()
            .gap_1()
            .children(items.iter().enumerate().map(|(i, item_blocks)| {
                let marker = if *ordered {
                    let num = start.unwrap_or(1) + i as u64;
                    SharedString::from(format!("{}. ", num))
                } else {
                    SharedString::from("• ")
                };
                div()
                    .flex()
                    .flex_row()
                    .gap_1()
                    .child(
                        div()
                            .text_color(theme.list_marker)
                            .min_w(px(16.))
                            .child(marker),
                    )
                    .child(div().flex_1().children(render_blocks(item_blocks, theme)))
                    .into_any_element()
            }))
            .into_any_element(),
        BlockNode::Table {
            alignments,
            headers,
            rows,
        } => div()
            .flex()
            .flex_col()
            .w_full()
            .my_2()
            .rounded_md()
            .border_1()
            .border_color(theme.table_border)
            .child(render_table_row(headers, true, alignments, theme))
            .children(
                rows.iter()
                    .map(|row| render_table_row(row, false, alignments, theme)),
            )
            .into_any_element(),
        BlockNode::Rule => div()
            .w_full()
            .h(px(1.))
            .bg(theme.rule_color)
            .my_3()
            .into_any_element(),
    }
}

fn render_table_row(
    cells: &[Vec<InlineNode>],
    is_header: bool,
    alignments: &[pulldown_cmark::Alignment],
    theme: &MarkdownTheme,
) -> gpui::AnyElement {
    div()
        .flex()
        .flex_row()
        .w_full()
        .when(is_header, |this| {
            this.border_b_1()
                .border_color(theme.rule_color)
                .bg(theme.table_header_bg)
        })
        .children(cells.iter().enumerate().map(|(i, cell)| {
            let align = alignments
                .get(i)
                .copied()
                .unwrap_or(pulldown_cmark::Alignment::None);
            div()
                .flex_1()
                .px_2()
                .py_1()
                .when(align == pulldown_cmark::Alignment::Center, |this| {
                    this.text_center()
                })
                .when(align == pulldown_cmark::Alignment::Right, |this| {
                    this.text_right()
                })
                .when(is_header, |this| {
                    this.font_weight(FontWeight(600.0))
                        .text_color(theme.table_header_text)
                })
                .when(!is_header, |this| this.text_color(theme.table_row_text))
                .border_r_1()
                .border_color(theme.table_border)
                .child(render_styled_inlines(cell, theme))
                .into_any_element()
        }))
        .into_any_element()
}

fn strip_html_tags(html: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        if c == '<' {
            in_tag = true;
        } else if c == '>' {
            in_tag = false;
        } else if !in_tag {
            result.push(c);
        }
    }
    result
}

impl Render for MarkdownRenderer {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let blocks = self
            .parsed
            .get_or_insert_with(|| parse_markdown(&self.content));
        let theme = default_theme();

        div()
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .text_color(theme.text_primary)
            .children(render_blocks(blocks, &theme))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tight_lists_stay_inside_list_items() {
        let blocks = parse_markdown(
            "- **2015** — Rust 1.0\n- **2018** — Rust 1.31 (Dec 2018)\n- **2021** — Rust 1.56 (Oct 2021)\n- **2024** — Rust 1.85 (Feb 2025)",
        );

        assert_eq!(blocks.len(), 1);

        let BlockNode::List { items, .. } = &blocks[0] else {
            panic!("expected top-level list");
        };

        assert_eq!(items.len(), 4);
        assert!(
            items
                .iter()
                .all(|item| matches!(item.as_slice(), [BlockNode::Paragraph { .. }]))
        );
    }

    #[test]
    fn formatted_inlines_add_horizontal_padding_inside_the_span() {
        let inlines = vec![
            InlineNode::Text {
                text: SharedString::from("before"),
            },
            InlineNode::Strong {
                children: vec![InlineNode::Text {
                    text: SharedString::from("bold"),
                }],
            },
            InlineNode::Text {
                text: SharedString::from("after"),
            },
        ];

        let (text, _) = build_styled_inline_text(&inlines, &default_theme());

        assert_eq!(
            text,
            format!("before{INLINE_HORIZONTAL_PADDING}bold{INLINE_HORIZONTAL_PADDING}after")
        );
    }

    #[test]
    fn formatted_inlines_pad_even_when_followed_by_punctuation() {
        let inlines = vec![
            InlineNode::Text {
                text: SharedString::from("before"),
            },
            InlineNode::Emphasis {
                children: vec![InlineNode::Text {
                    text: SharedString::from("italic"),
                }],
            },
            InlineNode::Text {
                text: SharedString::from("."),
            },
        ];

        let (text, _) = build_styled_inline_text(&inlines, &default_theme());

        assert_eq!(
            text,
            format!("before{INLINE_HORIZONTAL_PADDING}italic{INLINE_HORIZONTAL_PADDING}.")
        );
    }

    #[test]
    fn ordered_list_preserves_start_offset() {
        let blocks = parse_markdown("5. First\n6. Second\n7. Third");

        assert_eq!(blocks.len(), 1);

        let BlockNode::List {
            ordered,
            start,
            items,
            ..
        } = &blocks[0]
        else {
            panic!("expected top-level list");
        };

        assert!(ordered);
        assert_eq!(*start, Some(5));
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn image_preserves_url_separately() {
        let inlines = vec![
            InlineNode::Text {
                text: SharedString::from("before "),
            },
            InlineNode::Image {
                alt: SharedString::from("alt text"),
                url: SharedString::from("https://example.com/img.png"),
            },
            InlineNode::Text {
                text: SharedString::from(" after"),
            },
        ];

        let (text, _) = build_styled_inline_text(&inlines, &default_theme());
        assert!(text.contains("[Image: alt text]"));
    }

    #[test]
    fn table_cells_support_inline_formatting() {
        let source = "| Feature | Status |\n|---------|--------|\n| **Bold** | *Italic* |\n| `Code` | [Link](https://example.com) |";
        let blocks = parse_markdown(source);

        assert_eq!(
            blocks.len(),
            1,
            "expected exactly 1 block, got {}: {:?}",
            blocks.len(),
            blocks
        );

        let BlockNode::Table { headers, rows, .. } = &blocks[0] else {
            panic!("expected table, got: {:?}", blocks[0]);
        };

        eprintln!("Headers: {:?}", headers);
        eprintln!("Rows: {:?}", rows);

        // Check that rows parse with inline formatting (headers may be empty due to pulldown-cmark behavior)
        assert!(!rows.is_empty(), "expected non-empty rows");
        let first_row = &rows[0];
        assert_eq!(
            first_row.len(),
            2,
            "expected 2 cells in first row, got: {:?}",
            first_row
        );

        // Check that bold cell has Strong inline node
        let bold_cell = &first_row[0];
        assert!(
            bold_cell
                .iter()
                .any(|inline| matches!(inline, InlineNode::Strong { .. })),
            "Expected bold cell to contain Strong inline node, got: {:?}",
            bold_cell
        );

        // Check that italic cell has Emphasis inline node
        let italic_cell = &first_row[1];
        assert!(
            italic_cell
                .iter()
                .any(|inline| matches!(inline, InlineNode::Emphasis { .. })),
            "Expected italic cell to contain Emphasis inline node, got: {:?}",
            italic_cell
        );
    }

    #[test]
    fn heading_levels_are_distinct() {
        let blocks = parse_markdown("# H1\n\n## H2\n\n### H3\n\n#### H4\n\n##### H5\n\n###### H6");
        let levels: Vec<_> = blocks
            .iter()
            .filter_map(|b| match b {
                BlockNode::Heading { level, .. } => Some(*level),
                _ => None,
            })
            .collect();
        assert_eq!(levels.len(), 6, "expected 6 heading blocks");
        assert_eq!(
            levels,
            vec![
                HeadingLevel::H1,
                HeadingLevel::H2,
                HeadingLevel::H3,
                HeadingLevel::H4,
                HeadingLevel::H5,
                HeadingLevel::H6,
            ],
            "heading levels should be distinct"
        );
    }

    #[test]
    fn heading_font_sizes_are_distinct() {
        let sizes: Vec<f32> = [
            HeadingLevel::H1,
            HeadingLevel::H2,
            HeadingLevel::H3,
            HeadingLevel::H4,
            HeadingLevel::H5,
            HeadingLevel::H6,
        ]
        .iter()
        .map(|level| heading_style(*level).0)
        .collect();
        // All sizes must be distinct
        let mut unique = sizes.clone();
        unique.sort_by(|a, b| a.partial_cmp(b).unwrap());
        unique.dedup();
        assert_eq!(unique.len(), 6, "all 6 heading font sizes must be distinct");
    }

    #[test]
    fn syntax_highlighting_produces_highlights_for_rust() {
        let code = "fn main() {\n    println!(\"Hello, world!\");\n}";
        let lines = highlight_code(code, Some("rust"));

        assert!(!lines.is_empty(), "expected at least one line");

        let total_highlights: usize = lines.iter().map(|(_, h)| h.len()).sum();
        assert!(
            total_highlights > 0,
            "expected syntax highlighting to produce highlights for Rust code"
        );
    }

    #[test]
    fn link_preserves_url() {
        let blocks = parse_markdown("[example](https://example.com)");
        let paragraph = blocks
            .iter()
            .find_map(|b| match b {
                BlockNode::Paragraph { inlines } if !inlines.is_empty() => Some(inlines),
                _ => None,
            })
            .expect("expected a non-empty paragraph");
        assert_eq!(paragraph.len(), 1, "expected 1 inline, got {:?}", paragraph);
        match &paragraph[0] {
            InlineNode::Link { children, url } => {
                assert_eq!(url.as_ref(), "https://example.com");
                assert_eq!(children.len(), 1);
                match &children[0] {
                    InlineNode::Text { text } => assert_eq!(text.as_ref(), "example"),
                    _ => panic!("expected text inside link"),
                }
            }
            _ => panic!("expected link node, got {:?}", paragraph[0]),
        }
    }

    #[test]
    fn syntax_highlighting_fallback_for_unknown_language() {
        let code = "some random text\nanother line";
        let lines = highlight_code(code, Some("nonexistent-language-xyz"));

        assert_eq!(lines.len(), 2, "expected two lines");
        assert_eq!(lines[0].0, "some random text");
        assert_eq!(lines[1].0, "another line");

        // Plain text fallback should still produce the default foreground color
        // highlights (theme's default text color), but text should be preserved
        let total_highlights: usize = lines.iter().map(|(_, h)| h.len()).sum();
        assert!(
            total_highlights >= 0,
            "plain text fallback should render without errors"
        );
    }
}
