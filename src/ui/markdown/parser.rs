use std::collections::{HashMap, HashSet};
use std::ops::Range;

use gpui::SharedString;
use linkify::LinkFinder;
use pulldown_cmark::{
    Alignment, BlockQuoteKind, CowStr, HeadingLevel, LinkType, MetadataBlockKind, Options, Parser,
};
pub use pulldown_cmark::TagEnd as MarkdownTagEnd;

/// Default parse options for mini-pi markdown.
pub const PARSE_OPTIONS: Options = Options::ENABLE_TABLES
    .union(Options::ENABLE_FOOTNOTES)
    .union(Options::ENABLE_STRIKETHROUGH)
    .union(Options::ENABLE_TASKLISTS)
    .union(Options::ENABLE_SMART_PUNCTUATION)
    .union(Options::ENABLE_HEADING_ATTRIBUTES)
    .union(Options::ENABLE_GFM);

/// Options controlling what the parser extracts.
#[derive(Clone, Copy, Debug, Default)]
pub struct ParseOptions {
    #[allow(dead_code)]
    pub parse_html: bool,
    pub parse_heading_slugs: bool,
    pub parse_metadata_blocks: bool,
}

impl ParseOptions {
    #[cfg(test)]
    pub fn all() -> Self {
        Self {
            parse_html: true,
            parse_heading_slugs: true,
            parse_metadata_blocks: true,
        }
    }
}

/// Result of parsing a markdown document.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParsedMarkdown {
    pub source: SharedString,
    pub events: Vec<(Range<usize>, MarkdownEvent)>,
    pub language_names: HashSet<SharedString>,
    pub heading_slugs: HashMap<SharedString, usize>,
    pub footnote_definitions: HashMap<SharedString, usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MarkdownEvent {
    /// Start of a tagged element. Events between this and its matching `End`
    /// are children of the element.
    Start(MarkdownTag),
    /// End of a tagged element.
    End(MarkdownTagEnd),
    /// Text that matches the associated source range exactly.
    Text,
    /// Text that differs from the source range, e.g. after smart-punctuation
    /// substitution or HTML entity decoding.
    SubstitutedText(String),
    /// Inline code.
    Code,
    /// Block or inline HTML.
    Html,
    /// Inline HTML.
    InlineHtml,
    /// A reference to a footnote label.
    FootnoteReference(SharedString),
    /// A soft line break.
    SoftBreak,
    /// A hard line break.
    HardBreak,
    /// A horizontal rule.
    Rule,
    /// A task-list checkbox marker.
    TaskListMarker(bool),
    /// Start of a top-level block.
    RootStart,
    /// End of a top-level block, carrying the block index.
    RootEnd(usize),
}

#[derive(Clone, Debug, PartialEq)]
pub enum MarkdownTag {
    Paragraph,
    Heading {
        level: HeadingLevel,
        id: Option<SharedString>,
        classes: Vec<SharedString>,
        attrs: Vec<(SharedString, Option<SharedString>)>,
    },
    BlockQuote(Option<BlockQuoteKind>),
    CodeBlock {
        kind: CodeBlockKind,
        metadata: CodeBlockMetadata,
    },
    HtmlBlock,
    List(Option<u64>),
    Item,
    FootnoteDefinition(SharedString),
    Table(Vec<Alignment>),
    TableHead,
    TableRow,
    TableCell,
    Emphasis,
    Strong,
    Strikethrough,
    DefinitionList,
    DefinitionListTitle,
    DefinitionListDefinition,
    Link {
        link_type: LinkType,
        dest_url: SharedString,
        title: SharedString,
        id: SharedString,
    },
    Image {
        link_type: LinkType,
        dest_url: SharedString,
        title: SharedString,
        id: SharedString,
    },
    MetadataBlock(MetadataBlockKind),
}

#[derive(Clone, Debug, PartialEq)]
pub enum CodeBlockKind {
    Indented,
    Fenced,
    FencedLang(SharedString),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CodeBlockMetadata {
    pub content_range: Range<usize>,
    pub line_count: usize,
}

#[derive(Default)]
struct ParseState {
    events: Vec<(Range<usize>, MarkdownEvent)>,
    root_block_starts: Vec<usize>,
    depth: usize,
}

impl ParseState {
    fn push_event(&mut self, range: Range<usize>, event: MarkdownEvent) {
        match &event {
            MarkdownEvent::Start(_) => {
                if self.depth == 0 {
                    self.root_block_starts.push(range.start);
                    self.events.push((range.clone(), MarkdownEvent::RootStart));
                }
                self.depth += 1;
                self.events.push((range, event));
            }
            MarkdownEvent::End(_) => {
                self.events.push((range.clone(), event));
                if self.depth > 0 {
                    self.depth -= 1;
                    if self.depth == 0 {
                        let root_block_index = self.root_block_starts.len() - 1;
                        self.events
                            .push((range, MarkdownEvent::RootEnd(root_block_index)));
                    }
                }
            }
            MarkdownEvent::Rule => {
                if self.depth == 0 && !range.is_empty() {
                    self.root_block_starts.push(range.start);
                    let root_block_index = self.root_block_starts.len() - 1;
                    self.events.push((range.clone(), MarkdownEvent::RootStart));
                    self.events.push((range.clone(), event));
                    self.events
                        .push((range, MarkdownEvent::RootEnd(root_block_index)));
                } else {
                    self.events.push((range, event));
                }
            }
            _ => {
                self.events.push((range, event));
            }
        }
    }
}

/// Parse markdown with default options.
pub fn parse_markdown(source: &str) -> ParsedMarkdown {
    parse_markdown_with_options(source, ParseOptions::default())
}

/// Parse markdown with the supplied options, returning source-mapped events.
pub fn parse_markdown_with_options(source: &str, options: ParseOptions) -> ParsedMarkdown {
    let mut state = ParseState::default();
    let mut language_names = HashSet::default();
    let mut within_link = false;
    let mut within_code_block = false;
    let mut within_metadata = false;

    let parse_options = if options.parse_metadata_blocks {
        PARSE_OPTIONS.union(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS)
    } else {
        PARSE_OPTIONS
    };

    let mut parser = Parser::new_ext(source, parse_options)
        .into_offset_iter()
        .peekable();

    while let Some((pulldown_event, range)) = parser.next() {
        if within_metadata && !options.parse_metadata_blocks {
            if let pulldown_cmark::Event::End(pulldown_cmark::TagEnd::MetadataBlock(_)) =
                pulldown_event
            {
                within_metadata = false;
            }
            continue;
        }

        match pulldown_event {
            pulldown_cmark::Event::Start(tag) => {
                let tag = match tag {
                    pulldown_cmark::Tag::Link {
                        link_type,
                        dest_url,
                        title,
                        id,
                    } => {
                        within_link = true;
                        MarkdownTag::Link {
                            link_type,
                            dest_url: SharedString::from(dest_url.into_string()),
                            title: SharedString::from(title.into_string()),
                            id: SharedString::from(id.into_string()),
                        }
                    }
                    pulldown_cmark::Tag::MetadataBlock(kind) => {
                        within_metadata = true;
                        MarkdownTag::MetadataBlock(kind)
                    }
                    pulldown_cmark::Tag::CodeBlock(pulldown_cmark::CodeBlockKind::Indented) => {
                        within_code_block = true;
                        MarkdownTag::CodeBlock {
                            kind: CodeBlockKind::Indented,
                            metadata: CodeBlockMetadata {
                                content_range: range.clone(),
                                line_count: 1,
                            },
                        }
                    }
                    pulldown_cmark::Tag::CodeBlock(pulldown_cmark::CodeBlockKind::Fenced(info)) => {
                        within_code_block = true;
                        let info = info.trim();
                        let kind = if info.is_empty() {
                            CodeBlockKind::Fenced
                        } else {
                            let language = SharedString::from(info.to_string());
                            language_names.insert(language.clone());
                            CodeBlockKind::FencedLang(language)
                        };

                        let content_range =
                            extract_code_block_content_range(&source[range.clone()]);
                        let content_range =
                            range.start + content_range.start..range.start + content_range.end;
                        let line_count = source[content_range.clone()]
                            .bytes()
                            .filter(|b| *b == b'\n')
                            .count();

                        MarkdownTag::CodeBlock {
                            kind,
                            metadata: CodeBlockMetadata {
                                content_range,
                                line_count,
                            },
                        }
                    }
                    pulldown_cmark::Tag::Paragraph => MarkdownTag::Paragraph,
                    pulldown_cmark::Tag::Heading {
                        level,
                        id,
                        classes,
                        attrs,
                    } => {
                        let id = id.map(|id| SharedString::from(id.into_string()));
                        let classes = classes
                            .into_iter()
                            .map(|c| SharedString::from(c.into_string()))
                            .collect();
                        let attrs = attrs
                            .into_iter()
                            .map(|(key, value)| {
                                (
                                    SharedString::from(key.into_string()),
                                    value.map(|v| SharedString::from(v.into_string())),
                                )
                            })
                            .collect();
                        MarkdownTag::Heading {
                            level,
                            id,
                            classes,
                            attrs,
                        }
                    }
                    pulldown_cmark::Tag::BlockQuote(kind) => MarkdownTag::BlockQuote(kind),
                    pulldown_cmark::Tag::List(start_number) => MarkdownTag::List(start_number),
                    pulldown_cmark::Tag::Item => MarkdownTag::Item,
                    pulldown_cmark::Tag::FootnoteDefinition(label) => {
                        MarkdownTag::FootnoteDefinition(SharedString::from(label.to_string()))
                    }
                    pulldown_cmark::Tag::Table(alignments) => MarkdownTag::Table(alignments),
                    pulldown_cmark::Tag::TableHead => MarkdownTag::TableHead,
                    pulldown_cmark::Tag::TableRow => MarkdownTag::TableRow,
                    pulldown_cmark::Tag::TableCell => MarkdownTag::TableCell,
                    pulldown_cmark::Tag::Emphasis => MarkdownTag::Emphasis,
                    pulldown_cmark::Tag::Strong => MarkdownTag::Strong,
                    pulldown_cmark::Tag::Strikethrough => MarkdownTag::Strikethrough,
                    pulldown_cmark::Tag::Image {
                        link_type,
                        dest_url,
                        title,
                        id,
                    } => MarkdownTag::Image {
                        link_type,
                        dest_url: SharedString::from(dest_url.into_string()),
                        title: SharedString::from(title.into_string()),
                        id: SharedString::from(id.into_string()),
                    },
                    pulldown_cmark::Tag::HtmlBlock => MarkdownTag::HtmlBlock,
                    pulldown_cmark::Tag::DefinitionList => MarkdownTag::DefinitionList,
                    pulldown_cmark::Tag::DefinitionListTitle => MarkdownTag::DefinitionListTitle,
                    pulldown_cmark::Tag::DefinitionListDefinition => {
                        MarkdownTag::DefinitionListDefinition
                    }
                };
                state.push_event(range, MarkdownEvent::Start(tag));
            }
            pulldown_cmark::Event::End(tag) => {
                if let pulldown_cmark::TagEnd::Link = tag {
                    within_link = false;
                } else if let pulldown_cmark::TagEnd::CodeBlock = tag {
                    within_code_block = false;
                } else if let pulldown_cmark::TagEnd::MetadataBlock(_) = tag {
                    within_metadata = false;
                    if !options.parse_metadata_blocks {
                        continue;
                    }
                }
                state.push_event(range, MarkdownEvent::End(tag));
            }
            pulldown_cmark::Event::Text(parsed) => {
                fn event_for(
                    text: &str,
                    range: Range<usize>,
                    rendered: &str,
                ) -> (Range<usize>, MarkdownEvent) {
                    if rendered == &text[range.clone()] {
                        (range, MarkdownEvent::Text)
                    } else {
                        (range, MarkdownEvent::SubstitutedText(rendered.to_owned()))
                    }
                }

                if within_code_block {
                    let (range, event) = event_for(source, range, &parsed);
                    state.push_event(range, event);
                    continue;
                }

                #[derive(Debug)]
                struct TextRange<'a> {
                    source_range: Range<usize>,
                    merged_range: Range<usize>,
                    parsed: CowStr<'a>,
                }

                let mut last_len = parsed.len();
                let mut ranges = vec![TextRange {
                    source_range: range.clone(),
                    merged_range: 0..last_len,
                    parsed,
                }];

                while matches!(parser.peek(), Some((pulldown_cmark::Event::Text(_), _))) {
                    let Some((next_event, next_range)) = parser.next() else {
                        unreachable!()
                    };
                    let next_text = match next_event {
                        pulldown_cmark::Event::Text(text) => text,
                        _ => unreachable!(),
                    };
                    let next_len = last_len + next_text.len();
                    ranges.push(TextRange {
                        source_range: next_range.clone(),
                        merged_range: last_len..next_len,
                        parsed: next_text,
                    });
                    last_len = next_len;
                }

                let mut merged_text =
                    String::with_capacity(ranges.last().unwrap().merged_range.end);
                for range in &ranges {
                    merged_text.push_str(&range.parsed);
                }

                let mut ranges = ranges.into_iter().peekable();

                if !within_link && !within_code_block {
                    let mut finder = LinkFinder::new();
                    finder.kinds(&[linkify::LinkKind::Url]);

                    for link in finder.links(&merged_text) {
                        let link_start_in_merged = link.start();
                        let link_end_in_merged = link.end();

                        while ranges.peek().is_some_and(|range| {
                            range.merged_range.end <= link_start_in_merged
                        }) {
                            let range = ranges.next().unwrap();
                            let (event_range, event) =
                                event_for(source, range.source_range, &range.parsed);
                            state.push_event(event_range, event);
                        }

                        let Some(range) = ranges.peek_mut() else {
                            continue;
                        };
                        let prefix_len = link_start_in_merged - range.merged_range.start;
                        if prefix_len > 0 {
                            let (head, tail) = range.parsed.split_at(prefix_len);
                            let (event_range, event) = event_for(
                                source,
                                range.source_range.start..range.source_range.start + prefix_len,
                                head,
                            );
                            state.push_event(event_range, event);
                            range.parsed = CowStr::Boxed(tail.into());
                            range.merged_range.start += prefix_len;
                            range.source_range.start += prefix_len;
                        }

                        let link_start_in_source = range.source_range.start;
                        let mut link_end_in_source = range.source_range.end;
                        let mut link_events = Vec::new();

                        while ranges.peek().is_some_and(|range| {
                            range.merged_range.end <= link_end_in_merged
                        }) {
                            let range = ranges.next().unwrap();
                            link_end_in_source = range.source_range.end;
                            link_events.push(event_for(source, range.source_range, &range.parsed));
                        }

                        if let Some(range) = ranges.peek_mut() {
                            let prefix_len = link_end_in_merged - range.merged_range.start;
                            if prefix_len > 0 {
                                let (head, tail) = range.parsed.split_at(prefix_len);
                                link_events.push(event_for(
                                    source,
                                    range.source_range.start
                                        ..range.source_range.start + prefix_len,
                                    head,
                                ));
                                range.parsed = CowStr::Boxed(tail.into());
                                range.merged_range.start += prefix_len;
                                range.source_range.start += prefix_len;
                                link_end_in_source = range.source_range.start;
                            }
                        }
                        let link_range = link_start_in_source..link_end_in_source;

                        state.push_event(
                            link_range.clone(),
                            MarkdownEvent::Start(MarkdownTag::Link {
                                link_type: LinkType::Autolink,
                                dest_url: SharedString::from(link.as_str().to_string()),
                                title: SharedString::default(),
                                id: SharedString::default(),
                            }),
                        );
                        for (event_range, event) in link_events {
                            state.push_event(event_range, event);
                        }
                        state.push_event(
                            link_range,
                            MarkdownEvent::End(MarkdownTagEnd::Link),
                        );
                    }
                }

                for range in ranges {
                    let (event_range, event) = event_for(source, range.source_range, &range.parsed);
                    state.push_event(event_range, event);
                }
            }
            pulldown_cmark::Event::Code(_) => {
                let content_range = extract_code_content_range(&source[range.clone()]);
                let content_range =
                    range.start + content_range.start..range.start + content_range.end;
                state.push_event(content_range, MarkdownEvent::Code);
            }
            pulldown_cmark::Event::Html(_) => state.push_event(range, MarkdownEvent::Html),
            pulldown_cmark::Event::InlineHtml(_) => {
                state.push_event(range, MarkdownEvent::InlineHtml)
            }
            pulldown_cmark::Event::FootnoteReference(label) => state.push_event(
                range,
                MarkdownEvent::FootnoteReference(SharedString::from(label.to_string())),
            ),
            pulldown_cmark::Event::SoftBreak => state.push_event(range, MarkdownEvent::SoftBreak),
            pulldown_cmark::Event::HardBreak => state.push_event(range, MarkdownEvent::HardBreak),
            pulldown_cmark::Event::Rule => state.push_event(range, MarkdownEvent::Rule),
            pulldown_cmark::Event::TaskListMarker(checked) => {
                state.push_event(range, MarkdownEvent::TaskListMarker(checked))
            }
            pulldown_cmark::Event::InlineMath(_) | pulldown_cmark::Event::DisplayMath(_) => {}
        }
    }

    let heading_slugs = if options.parse_heading_slugs {
        build_heading_slugs(source, &state.events)
    } else {
        HashMap::default()
    };
    let footnote_definitions = build_footnote_definitions(&state.events);

    ParsedMarkdown {
        source: SharedString::from(source.to_string()),
        events: state.events,
        language_names,
        heading_slugs,
        footnote_definitions,
    }
}

fn build_heading_slugs(
    source: &str,
    events: &[(Range<usize>, MarkdownEvent)],
) -> HashMap<SharedString, usize> {
    let mut slugs = HashMap::default();
    let mut slug_counts: HashMap<String, usize> = HashMap::default();
    let mut inside_heading = false;
    let mut heading_text = String::new();
    let mut heading_source_start: Option<usize> = None;

    for (range, event) in events {
        match event {
            MarkdownEvent::Start(MarkdownTag::Heading { .. }) => {
                inside_heading = true;
                heading_text.clear();
                heading_source_start = None;
            }
            MarkdownEvent::End(MarkdownTagEnd::Heading(_)) => {
                if inside_heading {
                    let source_offset = heading_source_start.unwrap_or(range.start);
                    let base_slug = generate_heading_slug(&heading_text);
                    let count = slug_counts.entry(base_slug.clone()).or_insert(0);
                    let mut slug = if *count == 0 {
                        base_slug.clone()
                    } else {
                        format!("{base_slug}-{count}")
                    };
                    *count += 1;
                    while slugs.contains_key(slug.as_str()) {
                        let Some(count) = slug_counts.get_mut(&base_slug) else {
                            slug.clear();
                            break;
                        };
                        slug = format!("{base_slug}-{count}");
                        *count += 1;
                    }
                    if !slug.is_empty() {
                        slugs.insert(SharedString::from(slug), source_offset);
                    }
                    inside_heading = false;
                }
            }
            MarkdownEvent::Text if inside_heading => {
                if heading_source_start.is_none() {
                    heading_source_start = Some(range.start);
                }
                heading_text.push_str(&source[range.clone()]);
            }
            MarkdownEvent::SubstitutedText(substituted) if inside_heading => {
                if heading_source_start.is_none() {
                    heading_source_start = Some(range.start);
                }
                heading_text.push_str(substituted);
            }
            _ => {}
        }
    }

    slugs
}

fn build_footnote_definitions(
    events: &[(Range<usize>, MarkdownEvent)],
) -> HashMap<SharedString, usize> {
    let mut definitions = HashMap::default();
    let mut current_label: Option<SharedString> = None;

    for (range, event) in events {
        match event {
            MarkdownEvent::Start(MarkdownTag::FootnoteDefinition(label)) => {
                current_label = Some(label.clone());
            }
            MarkdownEvent::End(MarkdownTagEnd::FootnoteDefinition) => {
                current_label = None;
            }
            MarkdownEvent::Text if current_label.is_some() => {
                if let Some(label) = current_label.take() {
                    definitions.entry(label).or_insert(range.start);
                }
            }
            _ => {}
        }
    }

    definitions
}

fn generate_heading_slug(text: &str) -> String {
    let mut slug = String::with_capacity(text.len());
    let mut prev_hyphen = true;
    for ch in text.to_lowercase().chars() {
        if ch.is_alphanumeric() {
            slug.push(ch);
            prev_hyphen = false;
        } else if !prev_hyphen {
            slug.push('-');
            prev_hyphen = true;
        }
    }
    if slug.ends_with('-') {
        slug.pop();
    }
    slug
}

fn extract_code_content_range(text: &str) -> Range<usize> {
    let text_len = text.len();
    if text_len == 0 {
        return 0..0;
    }

    let start_ticks = text.chars().take_while(|&c| c == '`').count();
    if start_ticks == 0 || start_ticks > text_len {
        return 0..text_len;
    }

    let end_ticks = text.chars().rev().take_while(|&c| c == '`').count();
    if end_ticks != start_ticks || text_len < start_ticks + end_ticks {
        return 0..text_len;
    }

    start_ticks..text_len - end_ticks
}

fn extract_code_block_content_range(text: &str) -> Range<usize> {
    let mut range = 0..text.len();
    if text.starts_with("```") {
        range.start += 3;
        if let Some(newline_ix) = text[range.clone()].find('\n') {
            range.start += newline_ix + 1;
        }
    }
    if !range.is_empty() && text.ends_with("```") {
        range.end -= 3;
    }
    if range.start > range.end {
        range.end = range.start;
    }
    range
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_paragraph_and_heading() {
        let source = "Hello\n\n# Title";
        let parsed = parse_markdown(source);

        assert_eq!(parsed.events[0].0, 0usize..6);
        assert!(matches!(parsed.events[0].1, MarkdownEvent::RootStart));

        assert_eq!(parsed.events[1].0, 0usize..6);
        assert!(matches!(
            parsed.events[1].1,
            MarkdownEvent::Start(MarkdownTag::Paragraph)
        ));

        assert_eq!(parsed.events[2].0, 0usize..5);
        assert!(matches!(parsed.events[2].1, MarkdownEvent::Text));

        assert_eq!(parsed.events[3].0, 0usize..6);
        assert!(matches!(
            parsed.events[3].1,
            MarkdownEvent::End(MarkdownTagEnd::Paragraph)
        ));

        assert_eq!(parsed.events[4].0, 0usize..6);
        assert!(matches!(parsed.events[4].1, MarkdownEvent::RootEnd(0)));

        assert_eq!(parsed.events[5].0, 7usize..14);
        assert!(matches!(parsed.events[5].1, MarkdownEvent::RootStart));

        assert_eq!(parsed.events[6].0, 7usize..14);
        assert!(matches!(
            parsed.events[6].1,
            MarkdownEvent::Start(MarkdownTag::Heading { .. })
        ));

        assert_eq!(parsed.events[7].0, 9usize..14);
        assert!(matches!(parsed.events[7].1, MarkdownEvent::Text));

        assert_eq!(parsed.events[8].0, 7usize..14);
        assert!(matches!(
            parsed.events[8].1,
            MarkdownEvent::End(MarkdownTagEnd::Heading(_))
        ));

        assert_eq!(parsed.events[9].0, 7usize..14);
        assert!(matches!(parsed.events[9].1, MarkdownEvent::RootEnd(1)));
    }

    #[test]
    fn autolink_detection() {
        let source = "Visit https://example.com here";
        let parsed = parse_markdown(source);

        let has_autolink = parsed.events.iter().any(|(_, event)| matches!(
            event,
            MarkdownEvent::Start(MarkdownTag::Link {
                link_type: LinkType::Autolink,
                ..
            })
        ));
        assert!(has_autolink, "expected autolink event: {parsed:?}");
    }

    #[test]
    fn substituted_text_for_smart_punctuation() {
        let source = "-- dash";
        let parsed = parse_markdown(source);

        assert!(parsed.events.iter().any(|(_, event)| matches!(
            event,
            MarkdownEvent::SubstitutedText(_)
        )));
    }

    #[test]
    fn list_preserves_start_offset() {
        let source = "5. First\n6. Second";
        let parsed = parse_markdown(source);

        assert!(parsed.events.iter().any(|(_, event)| matches!(
            event,
            MarkdownEvent::Start(MarkdownTag::List(Some(5)))
        )));
    }

    #[test]
    fn image_tag_preserves_url() {
        let source = "![alt text](https://example.com/img.png)";
        let parsed = parse_markdown(source);

        assert!(parsed.events.iter().any(|(_, event)| matches!(
            event,
            MarkdownEvent::Start(MarkdownTag::Image {
                dest_url,
                ..
            }) if dest_url.as_ref() == "https://example.com/img.png"
        )));
    }

    #[test]
    fn footnote_reference_and_definition() {
        let source = "text[^1]\n\n[^1]: footnote content";
        let parsed = parse_markdown(source);

        assert!(parsed.events.iter().any(|(_, event)| matches!(
            event,
            MarkdownEvent::FootnoteReference(label) if label.as_ref() == "1"
        )));
        assert!(parsed
            .footnote_definitions
            .contains_key(SharedString::from("1").as_ref()));
    }

    #[test]
    fn table_alignment() {
        let source = "| a | b |\n|---|---:|\n| c | d |";
        let parsed = parse_markdown(source);

        assert!(parsed.events.iter().any(|(_, event)| matches!(
            event,
            MarkdownEvent::Start(MarkdownTag::Table(alignments))
            if matches!(alignments.as_slice(), [Alignment::None, Alignment::Right])
        )));
    }

    #[test]
    fn code_block_metadata() {
        let source = "```rust\nfn main() {}\n```";
        let parsed = parse_markdown(source);

        let code_block = parsed.events.iter().find_map(|(_, event)| match event {
            MarkdownEvent::Start(MarkdownTag::CodeBlock { kind, metadata }) => {
                Some((kind.clone(), metadata.clone()))
            }
            _ => None,
        });
        assert!(
            matches!(code_block, Some((CodeBlockKind::FencedLang(ref lang), _)) if lang.as_ref() == "rust")
        );
        if let Some((_, metadata)) = code_block {
            assert_eq!(metadata.line_count, 1);
            assert_eq!(&source[metadata.content_range.clone()], "fn main() {}\n");
        }
    }

    #[test]
    fn heading_slugs_generated() {
        let source = "# Hello World\n\n## Hello World";
        let parsed = parse_markdown_with_options(source, ParseOptions::all());

        assert_eq!(parsed.heading_slugs.get("hello-world"), Some(&2));
        assert_eq!(parsed.heading_slugs.get("hello-world-1"), Some(&18));
    }
}
