use std::{collections::VecDeque, ops::Range};

use gpui::{
    Context, FontStyle, FontWeight, HighlightStyle, IntoElement, ParentElement, Render,
    SharedString, StrikethroughStyle, Styled, StyledText, UnderlineStyle, Window, div, prelude::*,
    px, rgb,
};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

static OPTIONS: Options = Options::from_bits_truncate(
    Options::ENABLE_TABLES.bits()
        | Options::ENABLE_STRIKETHROUGH.bits()
        | Options::ENABLE_TASKLISTS.bits()
        | Options::ENABLE_SMART_PUNCTUATION.bits()
        | Options::ENABLE_FOOTNOTES.bits()
        | Options::ENABLE_GFM.bits(),
);

pub struct MarkdownRenderer {
    content: SharedString,
}

impl MarkdownRenderer {
    pub fn new(content: impl Into<SharedString>) -> Self {
        Self {
            content: content.into(),
        }
    }

    pub fn set_content(&mut self, content: impl Into<SharedString>) {
        self.content = content.into();
    }
}

// ---- AST types ----

#[derive(Debug, Clone)]
enum InlineNode {
    Text { text: SharedString },
    Code { code: SharedString },
    Emphasis { children: Vec<InlineNode> },
    Strong { children: Vec<InlineNode> },
    Strikethrough { children: Vec<InlineNode> },
    Link { children: Vec<InlineNode> },
    Image { alt: SharedString },
    SoftBreak,
    HardBreak,
    InlineHtml { html: SharedString },
    TaskMarker { checked: bool },
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
                            alt: dest_url.to_string().into(),
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
                        if let BlockContext::List { items, .. } = ctx {
                            let block = BlockNode::List {
                                ordered: is_ordered,
                                start: None,
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
                        if let BlockContext::Link { children, .. } = ctx {
                            push_inline(
                                &mut inline_buffer,
                                &mut block_stack,
                                InlineNode::Link { children },
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
) {
    if children.is_empty() {
        return;
    }

    append_inline_padding(text, highlights, style);
    collect_styled_inlines(children, style, text, highlights);
    append_inline_padding(text, highlights, style);
}

fn collect_styled_inlines(
    inlines: &[InlineNode],
    active_style: HighlightStyle,
    text: &mut String,
    highlights: &mut Vec<(Range<usize>, HighlightStyle)>,
) {
    for inline in inlines {
        match inline {
            InlineNode::Text { text: inline_text } => {
                append_highlighted_text(text, highlights, inline_text, active_style);
            }
            InlineNode::Code { code } => {
                let style = active_style.highlight(HighlightStyle {
                    color: Some(rgb(0xe5c07b).into()),
                    background_color: Some(rgb(0x333333).into()),
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
                );
            }
            InlineNode::Strong { children } => {
                append_padded_children(
                    text,
                    highlights,
                    children,
                    active_style.highlight(FontWeight(700.0).into()),
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
                );
            }
            InlineNode::Link { children } => {
                append_padded_children(
                    text,
                    highlights,
                    children,
                    active_style.highlight(HighlightStyle {
                        color: Some(rgb(0x60a5fa).into()),
                        underline: Some(UnderlineStyle {
                            thickness: px(1.),
                            color: Some(rgb(0x60a5fa).into()),
                            wavy: false,
                        }),
                        ..Default::default()
                    }),
                );
            }
            InlineNode::Image { alt } => {
                let image_text = format!("[Image: {alt}]");
                append_padded_segment(
                    text,
                    highlights,
                    &image_text,
                    active_style.highlight(HighlightStyle::color(rgb(0x888888).into())),
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
                        rgb(0x3b82f6).into()
                    } else {
                        rgb(0x888888).into()
                    })),
                );
            }
        }
    }
}

fn build_styled_inline_text(
    inlines: &[InlineNode],
) -> (String, Vec<(Range<usize>, HighlightStyle)>) {
    let mut text = String::new();
    let mut highlights = Vec::new();
    collect_styled_inlines(
        inlines,
        HighlightStyle::default(),
        &mut text,
        &mut highlights,
    );
    (text, highlights)
}

fn render_styled_inlines(inlines: &[InlineNode]) -> StyledText {
    let (text, highlights) = build_styled_inline_text(inlines);
    StyledText::new(text).with_highlights(highlights)
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
            InlineNode::Link { children } => {
                result.push_str(&render_inlines_text(children));
            }
            InlineNode::Image { alt } => {
                result.push_str("[Image: ");
                result.push_str(alt);
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

fn render_blocks(blocks: &[BlockNode]) -> Vec<gpui::AnyElement> {
    blocks.iter().map(|block| render_block(block)).collect()
}

fn render_block(block: &BlockNode) -> gpui::AnyElement {
    match block {
        BlockNode::Paragraph { inlines } => {
            if inlines.is_empty() {
                div().h(px(4.)).into_any_element()
            } else {
                div()
                    .w_full()
                    .text_left()
                    .child(render_styled_inlines(inlines))
                    .into_any_element()
            }
        }
        BlockNode::Heading { level, inlines } => {
            let font_size = match level {
                HeadingLevel::H1 => px(22.),
                HeadingLevel::H2 => px(20.),
                HeadingLevel::H3 => px(18.),
                HeadingLevel::H4 => px(16.),
                HeadingLevel::H5 => px(15.),
                HeadingLevel::H6 => px(14.),
            };
            div()
                .w_full()
                .mt_4()
                .mb_2()
                .font_weight(FontWeight(700.0))
                .text_size(font_size)
                .text_color(rgb(0xf0f0f0))
                .child(render_styled_inlines(inlines))
                .into_any_element()
        }
        BlockNode::CodeBlock { language, code } => {
            let lang_label = language
                .clone()
                .unwrap_or_else(|| SharedString::from("code"));
            div()
                .flex()
                .flex_col()
                .w_full()
                .my_2()
                .rounded_md()
                .border_1()
                .border_color(rgb(0x404040))
                .bg(rgb(0x1e1e1e))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .px_3()
                        .py_1()
                        .border_b_1()
                        .border_color(rgb(0x404040))
                        .bg(rgb(0x2a2a2a))
                        .rounded_t_md()
                        .text_xs()
                        .text_color(rgb(0x888888))
                        .child(lang_label),
                )
                .child(
                    div()
                        .p_3()
                        .text_xs()
                        .text_color(rgb(0xe5e5e5))
                        .font_family("Menlo, Monaco, 'Courier New', monospace")
                        .child(code.clone()),
                )
                .into_any_element()
        }
        BlockNode::BlockQuote { children } => div()
            .border_l_4()
            .border_color(rgb(0x444444))
            .text_color(rgb(0xaaaaaa))
            .flex()
            .flex_col()
            .gap_1()
            .children(render_blocks(children))
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
                    .child(div().text_color(rgb(0x888888)).min_w(px(16.)).child(marker))
                    .child(div().flex_1().children(render_blocks(item_blocks)))
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
            .border_color(rgb(0x404040))
            .child(render_table_row(headers, true, alignments))
            .children(
                rows.iter()
                    .map(|row| render_table_row(row, false, alignments)),
            )
            .into_any_element(),
        BlockNode::Rule => div()
            .w_full()
            .h(px(1.))
            .bg(rgb(0x444444))
            .my_3()
            .into_any_element(),
    }
}

fn render_table_row(
    cells: &[Vec<InlineNode>],
    is_header: bool,
    alignments: &[pulldown_cmark::Alignment],
) -> gpui::AnyElement {
    div()
        .flex()
        .flex_row()
        .w_full()
        .when(is_header, |this| {
            this.border_b_1()
                .border_color(rgb(0x444444))
                .bg(rgb(0x252525))
        })
        .children(cells.iter().enumerate().map(|(i, cell)| {
            let text = render_inlines_text(cell);
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
                        .text_color(rgb(0xf0f0f0))
                })
                .when(!is_header, |this| this.text_color(rgb(0xe5e5e5)))
                .border_r_1()
                .border_color(rgb(0x404040))
                .child(SharedString::from(text))
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
        let blocks = parse_markdown(&self.content);

        div()
            .flex()
            .flex_col()
            .gap_1()
            .w_full()
            .children(render_blocks(&blocks))
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

        let (text, _) = build_styled_inline_text(&inlines);

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

        let (text, _) = build_styled_inline_text(&inlines);

        assert_eq!(
            text,
            format!("before{INLINE_HORIZONTAL_PADDING}italic{INLINE_HORIZONTAL_PADDING}.")
        );
    }
}
