use std::{
    ops::Range,
    sync::{Arc, OnceLock},
};

use gpui::{
    Context, FontWeight, HighlightStyle, Image, ImageFormat, IntoElement, ParentElement, Render,
    SharedString, Styled, Window, div, rgb,
};
use pulldown_cmark::HeadingLevel;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Color as SyntectColor, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

pub(crate) mod builder;
pub mod parser;

use parser::{ParsedMarkdown, parse_markdown};

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static THEME_SET: OnceLock<ThemeSet> = OnceLock::new();

fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn theme_set() -> &'static ThemeSet {
    THEME_SET.get_or_init(ThemeSet::load_defaults)
}

type HighlightedLines = Vec<(String, Vec<(Range<usize>, HighlightStyle)>)>;

pub(crate) fn highlight_code(code: &str, language: Option<&str>) -> HighlightedLines {
    let ss = syntax_set();
    let syntax = language
        .and_then(|lang| ss.find_syntax_by_token(lang))
        .unwrap_or_else(|| ss.find_syntax_plain_text());

    let ts = theme_set();
    let theme = &ts.themes["base16-ocean.dark"];
    let mut highlighter = HighlightLines::new(syntax, theme);

    let mut result = Vec::new();

    for line in LinesWithEndings::from(code) {
        let trimmed_line = line
            .strip_suffix("\r\n")
            .or_else(|| line.strip_suffix('\n'))
            .unwrap_or(line);

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
    parsed: Option<ParsedMarkdown>,
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

#[allow(dead_code)]
pub(crate) struct MarkdownTheme {
    pub text_primary: gpui::Hsla,
    pub text_muted: gpui::Hsla,
    pub code_bg: gpui::Hsla,
    pub code_header_bg: gpui::Hsla,
    pub code_border: gpui::Hsla,
    pub code_text: gpui::Hsla,
    pub code_inline_bg: gpui::Hsla,
    pub code_inline_text: gpui::Hsla,
    pub link_color: gpui::Hsla,
    pub link_underline: gpui::Hsla,
    pub heading_color: gpui::Hsla,
    pub quote_border: gpui::Hsla,
    pub quote_text: gpui::Hsla,
    pub list_marker: gpui::Hsla,
    pub table_border: gpui::Hsla,
    pub table_header_bg: gpui::Hsla,
    pub table_header_text: gpui::Hsla,
    pub table_row_text: gpui::Hsla,
    pub rule_color: gpui::Hsla,
    pub task_checked: gpui::Hsla,
    pub task_unchecked: gpui::Hsla,
    pub image_text: gpui::Hsla,
    pub diff_add_bg: gpui::Hsla,
    pub diff_remove_bg: gpui::Hsla,
    pub diff_hunk_bg: gpui::Hsla,
    pub button_bg: gpui::Hsla,
    pub button_text: gpui::Hsla,
}

pub(crate) fn default_theme() -> MarkdownTheme {
    MarkdownTheme {
        text_primary: rgb(0xe5e5e5).into(),
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

pub(crate) fn heading_style(level: HeadingLevel) -> (f32, FontWeight) {
    match level {
        HeadingLevel::H1 => (26.0, FontWeight(700.0)),
        HeadingLevel::H2 => (22.0, FontWeight(700.0)),
        HeadingLevel::H3 => (19.0, FontWeight(700.0)),
        HeadingLevel::H4 => (16.0, FontWeight(600.0)),
        HeadingLevel::H5 => (14.0, FontWeight(600.0)),
        HeadingLevel::H6 => (13.0, FontWeight(600.0)),
    }
}

pub(crate) fn decode_data_url_image(url: &str) -> Option<Arc<Image>> {
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

pub(crate) fn image_source_from_url(url: &str) -> Option<gpui::ImageSource> {
    if url.starts_with("data:") {
        let image = decode_data_url_image(url)?;
        Some(gpui::ImageSource::Image(image))
    } else {
        Some(url.into())
    }
}

pub(crate) fn strip_html_tags(html: &str) -> String {
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
        let parsed = self
            .parsed
            .get_or_insert_with(|| parse_markdown(&self.content));
        let theme = default_theme();
        let rendered = builder::MarkdownElementBuilder::build(parsed, &theme);

        div()
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .text_color(theme.text_primary)
            .child(rendered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn syntax_highlighting_fallback_for_unknown_language() {
        let code = "some random text\nanother line";
        let lines = highlight_code(code, Some("nonexistent-language-xyz"));

        assert_eq!(lines.len(), 2, "expected two lines");
        assert_eq!(lines[0].0, "some random text");
        assert_eq!(lines[1].0, "another line");
    }
}
