use std::collections::HashSet;

use anaquel_catalog::Heading;
use anaquel_directive::{Node, parse_blocks};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use unicode_normalization::UnicodeNormalization;

/// Markdown features of Anaquel content: `CommonMark` plus GFM tables, task
/// lists, strikethrough and footnotes. Wiki links stay off on purpose.
pub(crate) fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
}

fn level_number(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// Byte offsets of every `\n`, to turn an offset into a line number.
pub(crate) struct Lines(Vec<usize>);

impl Lines {
    pub(crate) fn new(text: &str) -> Self {
        Self(text.match_indices('\n').map(|(i, _)| i).collect())
    }

    /// 1-based line that holds byte `offset`.
    pub(crate) fn at(&self, offset: usize) -> u32 {
        let before = self.0.partition_point(|&newline| newline < offset);
        u32::try_from(before + 1).unwrap_or(u32::MAX)
    }
}

/// Makes the `id` of each heading: readable, stable, unique within a page.
#[derive(Default)]
pub(crate) struct Slugger {
    used: HashSet<String>,
}

impl Slugger {
    pub(crate) fn anchor(&mut self, text: &str) -> String {
        let base = slugify(text);
        let mut candidate = base.clone();
        let mut n = 0u32;
        while !self.used.insert(candidate.clone()) {
            n += 1;
            candidate = format!("{base}-{n}");
        }
        candidate
    }
}

/// Lowercase letters and digits of any script joined by single dashes.
pub(crate) fn slugify(text: &str) -> String {
    let mut out = String::new();
    let mut pending_dash = false;
    for c in text.nfc().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(c);
        } else {
            pending_dash = true;
        }
    }
    if out.is_empty() {
        "section".to_owned()
    } else {
        out
    }
}

/// Lists the headings of a Markdown body, with anchors and lines counted from
/// 1 within the body. Headings inside code blocks are not headings.
///
/// The Markdown between `:::` directives is read piece by piece, in the same order the page is
/// composed, so a heading inside a block is listed and a directive line never joins a heading.
pub fn headings_of(body: &str) -> Vec<Heading> {
    let lines = Lines::new(body);
    let mut slugger = Slugger::default();
    let mut found = Vec::new();
    let parsed = parse_blocks(body);
    let mut pieces = Vec::new();
    collect_markdown(&parsed.nodes, &mut pieces);
    for (base, text) in pieces {
        let mut current: Option<(u8, usize, String)> = None;
        for (event, range) in Parser::new_ext(text, options()).into_offset_iter() {
            match event {
                Event::Start(Tag::Heading { level, .. }) => {
                    current = Some((level_number(level), base + range.start, String::new()));
                }
                Event::Text(t) | Event::Code(t) => {
                    if let Some((_, _, text)) = current.as_mut() {
                        text.push_str(&t);
                    }
                }
                Event::End(TagEnd::Heading(_)) => {
                    if let Some((level, start, text)) = current.take() {
                        let text = text.trim().to_owned();
                        found.push(Heading {
                            level,
                            anchor: slugger.anchor(&text),
                            text,
                            line: lines.at(start),
                        });
                    }
                }
                _ => {}
            }
        }
    }
    found
}

/// Every piece of Markdown of `nodes`, with its offset, in page order.
fn collect_markdown<'a>(nodes: &[Node<'a>], out: &mut Vec<(usize, &'a str)>) {
    for node in nodes {
        match node {
            Node::Markdown { start, text } => out.push((*start, text)),
            Node::Block(block) => collect_markdown(&block.children, out),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_readable_and_unicode_aware() {
        assert_eq!(slugify("Hello, World!"), "hello-world");
        assert_eq!(slugify("  Árbol  de  decisión "), "árbol-de-decisión");
        assert_eq!(slugify("日本語 テスト"), "日本語-テスト");
        assert_eq!(slugify("!!!"), "section");
    }

    #[test]
    fn repeated_headings_get_numbered_anchors() {
        let hs = headings_of("# A\n\n## Same\n\n## Same\n\n### Same\n");
        let anchors: Vec<_> = hs.iter().map(|h| h.anchor.as_str()).collect();
        assert_eq!(anchors, ["a", "same", "same-1", "same-2"]);
    }

    #[test]
    fn lines_and_levels() {
        let hs = headings_of("text\n\n## Two\n\n### Three `code`\n");
        assert_eq!((hs[0].level, hs[0].line), (2, 3));
        assert_eq!(
            (hs[1].level, hs[1].line, hs[1].text.as_str()),
            (3, 5, "Three code")
        );
    }

    #[test]
    fn code_blocks_hide_headings() {
        assert_eq!(headings_of("```\n# not a heading\n```\n").len(), 0);
    }
}
