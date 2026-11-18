use std::collections::HashMap;
use std::fmt::Write as _;
use std::ops::Range;

use anaquel_catalog::{Catalog, EntryId};
use anaquel_compose::markdown_options;
use anaquel_directive::{Node, parse_blocks};
use anaquel_schema::parse_page;
use pulldown_cmark::{Event, LinkType, Parser, Tag, TagEnd};

pub(super) struct Problem {
    pub(super) line: u32,
    pub(super) message: String,
}

pub(super) struct RewriteScope<'a> {
    pub(super) catalog: &'a Catalog,
    pub(super) entry: EntryId,
    pub(super) moved: &'a HashMap<String, String>,
    pub(super) old_page: &'a str,
    pub(super) new_page: &'a str,
}

pub(super) struct Rewritten {
    pub(super) text: String,
    pub(super) links: usize,
}

struct Edit {
    range: Range<usize>,
    text: String,
}

struct Scan<'a> {
    whole: &'a str,
    ctx: &'a RewriteScope<'a>,
    edits: Vec<Edit>,
    problems: Vec<Problem>,
}

pub(super) fn rewrite_links(text: &str, ctx: &RewriteScope<'_>) -> Result<Rewritten, Vec<Problem>> {
    let parsed = parse_page(text, ctx.old_page).map_err(|d| {
        vec![Problem {
            line: 1,
            message: d.message,
        }]
    })?;
    let base = text.len() - parsed.body.len();
    let blocks = parse_blocks(&parsed.body);
    let mut scan = Scan {
        whole: text,
        ctx,
        edits: Vec::new(),
        problems: Vec::new(),
    };
    scan.nodes(&blocks.nodes, base);
    if !scan.problems.is_empty() {
        return Err(scan.problems);
    }
    let links = scan.edits.len();
    let mut out = text.to_owned();
    scan.edits.sort_by_key(|e| std::cmp::Reverse(e.range.start));
    for edit in scan.edits {
        out.replace_range(edit.range, &edit.text);
    }
    Ok(Rewritten { text: out, links })
}

impl Scan<'_> {
    fn line(&self, offset: usize) -> u32 {
        let count = self
            .whole
            .get(..offset)
            .map_or(0, |before| before.matches('\n').count());
        u32::try_from(count + 1).unwrap_or(u32::MAX)
    }

    fn stop(&mut self, offset: usize, message: String) {
        let line = self.line(offset);
        self.problems.push(Problem { line, message });
    }

    fn nodes(&mut self, nodes: &[Node<'_>], base: usize) {
        for node in nodes {
            match node {
                Node::Markdown { start, text } => self.markdown(text, base + start),
                Node::Block(block) => {
                    for key in ["href", "image"] {
                        if let Some(value) = block.attrs.get(key)
                            && new_href(self.ctx, value).is_some()
                        {
                            self.stop(
                                base + block.start,
                                format!(
                                    "the `{key}` of a `:::{}` block points at a page that moves or sits in a page that moves",
                                    block.name
                                ),
                            );
                        }
                    }
                    self.nodes(&block.children, base);
                }
            }
        }
    }

    fn markdown(&mut self, text: &str, offset: usize) {
        let parser = Parser::new_ext(text, markdown_options());
        let definitions: Vec<(String, Range<usize>)> = parser
            .reference_definitions()
            .iter()
            .map(|(_, def)| (def.dest.to_string(), def.span.clone()))
            .collect();
        for (dest, span) in definitions {
            let Some(new) = new_href(self.ctx, &dest) else {
                continue;
            };
            let found = text.get(span.clone()).and_then(|source| {
                let colon = source.find("]:")?;
                let (s, e) = read_destination(source, colon + 2)?;
                (source.get(s..e)? == dest).then_some((span.start + s, span.start + e))
            });
            match found {
                Some((s, e)) => self.edits.push(Edit {
                    range: offset + s..offset + e,
                    text: encode(&new),
                }),
                None => self.stop(
                    offset + span.start,
                    format!(
                        "the definition of `{dest}` is written in a way that cannot be rewritten"
                    ),
                ),
            }
        }

        let mut open: Vec<(usize, usize, String, bool, usize)> = Vec::new();
        for (event, range) in parser.into_offset_iter() {
            match &event {
                Event::Start(Tag::Link {
                    link_type,
                    dest_url,
                    ..
                }) => {
                    let inline = *link_type == LinkType::Inline;
                    open.push((
                        range.start,
                        1,
                        dest_url.to_string(),
                        inline,
                        range.start + 1,
                    ));
                    continue;
                }
                Event::Start(Tag::Image {
                    link_type,
                    dest_url,
                    ..
                }) => {
                    let inline = *link_type == LinkType::Inline;
                    open.push((
                        range.start,
                        2,
                        dest_url.to_string(),
                        inline,
                        range.start + 2,
                    ));
                    continue;
                }
                Event::End(TagEnd::Link | TagEnd::Image) => {
                    let Some((start, _, dest, inline, text_end)) = open.pop() else {
                        continue;
                    };
                    if let Some(parent) = open.last_mut() {
                        parent.4 = parent.4.max(range.end);
                    }
                    if inline {
                        self.inline_link(text, offset, start..range.end, text_end, &dest);
                    }
                    continue;
                }
                _ => {}
            }
            if let Some(parent) = open.last_mut() {
                parent.4 = parent.4.max(range.end);
            }
        }
    }

    fn inline_link(
        &mut self,
        text: &str,
        offset: usize,
        span: Range<usize>,
        text_end: usize,
        dest: &str,
    ) {
        let Some(new) = new_href(self.ctx, dest) else {
            return;
        };
        let found = text.get(span.clone()).and_then(|source| {
            let from = text_end.saturating_sub(span.start).min(source.len());
            let close = source.get(from..)?.find("](")? + from;
            let (s, e) = read_destination(source, close + 2)?;
            (source.get(s..e)? == dest).then_some((span.start + s, span.start + e))
        });
        match found {
            Some((s, e)) => self.edits.push(Edit {
                range: offset + s..offset + e,
                text: encode(&new),
            }),
            None => self.stop(
                offset + span.start,
                format!("the link to `{dest}` is written in a way that cannot be rewritten"),
            ),
        }
    }
}

fn new_href(ctx: &RewriteScope<'_>, href: &str) -> Option<String> {
    let target = ctx.catalog.local_target(ctx.entry, href)?;
    let new_target = ctx
        .moved
        .get(target.as_str())
        .map_or(target.as_str(), String::as_str);
    let new_dir = new_page_dir(ctx.new_page);
    if new_target == target.as_str() && new_dir == new_page_dir(ctx.old_page) {
        return None;
    }
    let mut out = relative(new_dir, new_target);
    if let Some((_, fragment)) = href.split_once('#') {
        out.push('#');
        out.push_str(fragment);
    }
    Some(out)
}

fn new_page_dir(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

pub(super) fn relative(from_dir: &str, target: &str) -> String {
    let from: Vec<&str> = from_dir.split('/').filter(|s| !s.is_empty()).collect();
    let to: Vec<&str> = target.split('/').collect();
    let common = from
        .iter()
        .zip(&to)
        .take(to.len().saturating_sub(1))
        .take_while(|(a, b)| a == b)
        .count();
    let mut parts: Vec<&str> = vec![".."; from.len() - common];
    parts.extend(&to[common..]);
    parts.join("/")
}

pub(super) fn encode(href: &str) -> String {
    let mut out = String::with_capacity(href.len());
    for c in href.chars() {
        if c.is_control()
            || matches!(
                c,
                ' ' | '%' | '(' | ')' | '<' | '>' | '[' | ']' | '\\' | '"'
            )
        {
            let mut buffer = [0u8; 4];
            for byte in c.encode_utf8(&mut buffer).bytes() {
                let _ = write!(out, "%{byte:02X}");
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub(super) fn read_destination(source: &str, from: usize) -> Option<(usize, usize)> {
    let bytes = source.as_bytes();
    let mut start = from;
    while bytes.get(start).is_some_and(u8::is_ascii_whitespace) {
        start += 1;
    }
    if bytes.get(start) == Some(&b'<') {
        let mut i = start + 1;
        while i < bytes.len() {
            match bytes[i] {
                b'\\' => i += 2,
                b'>' => return Some((start + 1, i)),
                b'\n' | b'<' => return None,
                _ => i += 1,
            }
        }
        return None;
    }
    let mut depth = 0usize;
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
                i += 1;
            }
            b if b.is_ascii_whitespace() || b.is_ascii_control() => break,
            _ => i += 1,
        }
    }
    let end = i.min(bytes.len());
    (end > start).then_some((start, end))
}

pub(super) fn set_aliases(text: &str, aliases: &[String]) -> Result<String, String> {
    let replacement: Vec<String> = if aliases.is_empty() {
        Vec::new()
    } else {
        std::iter::once("aliases:\n".to_owned())
            .chain(aliases.iter().map(|a| format!("  - {a}\n")))
            .collect()
    };
    let Some(rest) = text.strip_prefix("---\n") else {
        if aliases.is_empty() {
            return Ok(text.to_owned());
        }
        return Ok(format!("---\n{}---\n{text}", replacement.concat()));
    };
    let all: Vec<&str> = rest.split_inclusive('\n').collect();
    let close = all
        .iter()
        .position(|line| line.trim_end_matches('\n') == "---")
        .ok_or_else(|| "the front matter is never closed".to_owned())?;
    let mut head: Vec<String> = all[..close].iter().map(|l| (*l).to_owned()).collect();
    let tail: String = all[close..].concat();
    let key = all[..close].iter().position(|l| l.starts_with("aliases:"));
    match key {
        Some(k) => {
            let inline = all[k]["aliases:".len()..].trim();
            if !(inline.is_empty() || inline.starts_with('[')) {
                return Err(
                    "the `aliases` key is written in a way that cannot be edited".to_owned(),
                );
            }
            let mut end = k + 1;
            while end < close && is_continuation(all[end]) {
                end += 1;
            }
            while end > k + 1 && all[end - 1].trim().is_empty() {
                end -= 1;
            }
            if all[k + 1..end]
                .iter()
                .any(|l| l.trim_start().starts_with('#'))
            {
                return Err("the `aliases` list holds a comment, which would be lost".to_owned());
            }
            head.splice(k..end, replacement);
        }
        None => head.extend(replacement),
    }
    Ok(format!("---\n{}{tail}", head.concat()))
}

fn is_continuation(line: &str) -> bool {
    line.trim().is_empty() || line.starts_with([' ', '\t', '-'])
}
