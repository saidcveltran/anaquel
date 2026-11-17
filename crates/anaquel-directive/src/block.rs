//! Splits a page body into Markdown and `:::` directive blocks.
//!
//! The scan is line based. Nothing inside a fenced code block is read as a directive. A block
//! opens with at least three colons directly followed by its name, and closes with a line of
//! as many colons alone. A block inside another uses fewer colons than its container.

use crate::attrs::{self, Attributes};
use crate::registry::{self, Body, Spec};

/// Deepest nesting of directive blocks.
pub const MAX_DEPTH: usize = 6;
/// Most directive blocks on one page.
pub const MAX_BLOCKS: usize = 500;
/// Longest opening line, in bytes.
const MAX_OPENER_BYTES: usize = 1000;

/// A piece of a page body.
#[derive(Debug, Clone, PartialEq)]
pub enum Node<'a> {
    /// Markdown between directives.
    Markdown {
        /// Byte offset of `text` in the body.
        start: usize,
        /// The Markdown source.
        text: &'a str,
    },
    /// A valid directive block.
    Block(Block<'a>),
}

/// One valid directive block.
#[derive(Debug, Clone, PartialEq)]
pub struct Block<'a> {
    /// The directive name.
    pub name: &'a str,
    /// Its rules.
    pub spec: &'static Spec,
    /// Its attributes.
    pub attrs: Attributes,
    /// How many colons open it.
    pub colons: usize,
    /// Byte offset of the opening line.
    pub start: usize,
    /// What it holds.
    pub children: Vec<Node<'a>>,
}

/// A problem found while splitting a body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockError {
    /// Byte offset of the line it is about.
    pub offset: usize,
    /// What is wrong.
    pub message: String,
}

/// The result of [`parse_blocks`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Parsed<'a> {
    /// The valid content, in order. Invalid blocks are left out.
    pub nodes: Vec<Node<'a>>,
    /// Every problem found.
    pub errors: Vec<BlockError>,
}

struct Frame<'a> {
    name: &'a str,
    spec: Option<&'static Spec>,
    attrs: Attributes,
    colons: usize,
    start: usize,
    children: Vec<Node<'a>>,
    bad: bool,
}

struct Scan<'a> {
    body: &'a str,
    stack: Vec<Frame<'a>>,
    root: Vec<Node<'a>>,
    errors: Vec<BlockError>,
    blocks: usize,
}

/// Splits `body` into Markdown and directive blocks, checking every block against the registry.
#[must_use]
pub fn parse_blocks(body: &str) -> Parsed<'_> {
    let mut scan = Scan {
        body,
        stack: Vec::new(),
        root: Vec::new(),
        errors: Vec::new(),
        blocks: 0,
    };
    let aborted = scan.run();
    if !aborted {
        while let Some(frame) = scan.stack.pop() {
            scan.error(frame.start, format!("`{}` is never closed", frame.name));
        }
    }
    Parsed {
        nodes: scan.root,
        errors: scan.errors,
    }
}

impl<'a> Scan<'a> {
    fn error(&mut self, offset: usize, message: String) {
        self.errors.push(BlockError { offset, message });
    }

    /// Reads every line. Returns true when a limit stopped the scan.
    fn run(&mut self) -> bool {
        let body = self.body;
        let mut seg_start = 0;
        let mut pos = 0;
        let mut fence: Option<(char, usize)> = None;
        while pos < body.len() {
            let line_end = body[pos..].find('\n').map_or(body.len(), |i| pos + i);
            let line = body[pos..line_end].trim_end_matches('\r');
            let next = (line_end + 1).min(body.len());
            if let Some((ch, len)) = fence {
                if closes_fence(line, ch, len) {
                    fence = None;
                }
            } else if let Some(open) = fence_open(line) {
                fence = Some(open);
            } else if line.starts_with(":::") {
                self.flush(seg_start, pos);
                if self.directive_line(line, pos) {
                    return true;
                }
                seg_start = next;
            }
            pos = next;
        }
        self.flush(seg_start, body.len());
        false
    }

    /// Adds the Markdown in `body[from..to]` to the block being read.
    fn flush(&mut self, from: usize, to: usize) {
        let text = &self.body[from..to];
        let Some(first) = text.find(|c: char| !c.is_whitespace()) else {
            return;
        };
        let holds_text = match self.stack.last().and_then(|f| f.spec) {
            Some(spec) => match spec.body {
                Body::Markdown => true,
                Body::Only(child) => {
                    let line = text[..first].rfind('\n').map_or(0, |i| i + 1);
                    let message = format!("`{}` holds only `{child}` blocks, not text", spec.name);
                    self.error(from + line, message);
                    false
                }
                Body::Empty => {
                    let line = text[..first].rfind('\n').map_or(0, |i| i + 1);
                    let message = format!("`{}` has no body", spec.name);
                    self.error(from + line, message);
                    false
                }
            },
            None => true,
        };
        if holds_text {
            self.push(Node::Markdown { start: from, text });
        }
    }

    fn push(&mut self, node: Node<'a>) {
        match self.stack.last_mut() {
            Some(frame) => frame.children.push(node),
            None => self.root.push(node),
        }
    }

    /// Handles a line that starts with `:::`. Returns true when a limit stopped the scan.
    fn directive_line(&mut self, line: &'a str, offset: usize) -> bool {
        let colons = line.bytes().take_while(|&b| b == b':').count();
        if colons < 3 {
            return false;
        }
        let rest = &line[colons..];
        if rest.trim().is_empty() {
            self.close(colons, offset);
            return false;
        }
        if !rest.starts_with(|c: char| c.is_ascii_alphabetic()) {
            self.error(
                offset,
                "a line that starts with `:::` must be a directive name or a closing `:::`"
                    .to_owned(),
            );
            return false;
        }
        self.open(line, colons, offset)
    }

    fn open(&mut self, line: &'a str, colons: usize, offset: usize) -> bool {
        if self.stack.len() >= MAX_DEPTH {
            self.error(
                offset,
                format!("directives are nested more than {MAX_DEPTH} deep"),
            );
            return true;
        }
        self.blocks += 1;
        if self.blocks > MAX_BLOCKS {
            self.error(
                offset,
                format!("more than {MAX_BLOCKS} directive blocks on one page"),
            );
            return true;
        }
        let rest = &line[colons..];
        let name_end = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
            .unwrap_or(rest.len());
        let (name, text) = rest.split_at(name_end);
        let mut bad = false;
        let mut found = None;
        let mut parsed = Attributes::default();
        if line.len() > MAX_OPENER_BYTES {
            self.error(
                offset,
                format!("the line is longer than {MAX_OPENER_BYTES} bytes"),
            );
            bad = true;
        } else if !text.is_empty() && !text.starts_with(char::is_whitespace) {
            let c = text.chars().next().unwrap_or(' ');
            self.error(offset, format!("unexpected character `{c}` after `{name}`"));
            bad = true;
        } else if let Some(spec) = registry::spec(name) {
            found = Some(spec);
            match attrs::parse(text) {
                Ok(a) => match registry::check_attributes(spec, &a) {
                    Ok(()) => parsed = a,
                    Err(message) => {
                        self.error(offset, message);
                        bad = true;
                    }
                },
                Err(message) => {
                    self.error(offset, message);
                    bad = true;
                }
            }
        } else {
            self.error(offset, format!("unknown directive `{name}`"));
            bad = true;
        }
        if let Some(parent) = self.stack.last()
            && colons >= parent.colons
        {
            let message = format!(
                "`{name}` is inside `{}`, so it needs fewer than {} colons",
                parent.name, parent.colons
            );
            self.error(offset, message);
            bad = true;
        }
        self.stack.push(Frame {
            name,
            spec: found,
            attrs: parsed,
            colons,
            start: offset,
            children: Vec::new(),
            bad,
        });
        false
    }

    /// Closes the innermost block that was opened with `colons` colons.
    fn close(&mut self, colons: usize, offset: usize) {
        let Some(depth) = self.stack.iter().rposition(|f| f.colons == colons) else {
            self.error(
                offset,
                format!("a closing `{}` has no matching opening", ":".repeat(colons)),
            );
            return;
        };
        while self.stack.len() > depth + 1 {
            if let Some(inner) = self.stack.pop() {
                self.error(inner.start, format!("`{}` is never closed", inner.name));
            }
        }
        if let Some(frame) = self.stack.pop() {
            self.finish(frame);
        }
    }

    /// Puts a closed block in its container, after checking where it sits.
    fn finish(&mut self, frame: Frame<'a>) {
        let Some(spec) = frame.spec else { return };
        if frame.bad {
            return;
        }
        let parent = self.stack.last().and_then(|f| f.spec);
        let placed = match (spec.parent, parent) {
            (Some(need), Some(have)) if have.name == need => true,
            (Some(need), _) => {
                self.error(
                    frame.start,
                    format!("`{}` must be inside `{need}`", spec.name),
                );
                false
            }
            (None, Some(have)) => match have.body {
                Body::Markdown => true,
                Body::Only(only) => {
                    self.error(
                        frame.start,
                        format!(
                            "`{}` holds only `{only}` blocks, not `{}`",
                            have.name, spec.name
                        ),
                    );
                    false
                }
                Body::Empty => {
                    self.error(frame.start, format!("`{}` has no body", have.name));
                    false
                }
            },
            (None, None) => true,
        };
        if placed {
            self.push(Node::Block(Block {
                name: frame.name,
                spec,
                attrs: frame.attrs,
                colons: frame.colons,
                start: frame.start,
                children: frame.children,
            }));
        }
    }
}

/// The character and length of a code fence that opens on `line`.
fn fence_open(line: &str) -> Option<(char, usize)> {
    let text = line.trim_start_matches(' ');
    if line.len() - text.len() > 3 {
        return None;
    }
    let ch = text.chars().next()?;
    if ch != '`' && ch != '~' {
        return None;
    }
    let len = text.chars().take_while(|&c| c == ch).count();
    if len < 3 || (ch == '`' && text[len..].contains('`')) {
        return None;
    }
    Some((ch, len))
}

/// Whether `line` closes a fence opened with `len` of `ch`.
fn closes_fence(line: &str, ch: char, len: usize) -> bool {
    let text = line.trim_start_matches(' ');
    if line.len() - text.len() > 3 {
        return false;
    }
    let run = text.chars().take_while(|&c| c == ch).count();
    run >= len && text[run..].trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(nodes: &[Node<'_>]) -> Vec<String> {
        nodes
            .iter()
            .map(|n| match n {
                Node::Markdown { .. } => "md".to_owned(),
                Node::Block(b) => format!("{}({})", b.name, names(&b.children).join(",")),
            })
            .collect()
    }

    #[test]
    fn plain_markdown_is_one_node() {
        let p = parse_blocks("# Title\n\ntext\n");
        assert_eq!(p.errors, Vec::new());
        assert_eq!(names(&p.nodes), ["md"]);
    }

    #[test]
    fn reads_a_hint_between_paragraphs() {
        let p = parse_blocks("before\n\n:::hint warning\ninside\n:::\n\nafter\n");
        assert!(p.errors.is_empty(), "{:?}", p.errors);
        assert_eq!(names(&p.nodes), ["md", "hint(md)", "md"]);
    }

    #[test]
    fn nests_with_more_colons_outside() {
        let src = "::::tabs\n:::tab title=\"A\"\none\n:::\n:::tab title=\"B\"\ntwo\n:::\n::::\n";
        let p = parse_blocks(src);
        assert!(p.errors.is_empty(), "{:?}", p.errors);
        assert_eq!(names(&p.nodes), ["tabs(tab(md),tab(md))"]);
    }

    #[test]
    fn nothing_inside_code_is_a_directive() {
        let src = "```\n:::bogus\n```\n\n~~~~\n:::\n~~~~\n";
        let p = parse_blocks(src);
        assert!(p.errors.is_empty(), "{:?}", p.errors);
        assert_eq!(names(&p.nodes), ["md"]);
    }

    #[test]
    fn a_fence_inside_a_block_hides_the_closer() {
        let p = parse_blocks(":::hint\n```\n:::\n```\n:::\n");
        assert!(p.errors.is_empty(), "{:?}", p.errors);
        assert_eq!(names(&p.nodes), ["hint(md)"]);
    }

    #[test]
    fn reports_unknown_unclosed_and_stray() {
        assert_eq!(parse_blocks(":::nope\nx\n:::\n").errors.len(), 1);
        assert_eq!(parse_blocks(":::hint\nx\n").errors.len(), 1);
        assert_eq!(parse_blocks("x\n:::\n").errors.len(), 1);
        // A space before the name is refused, and the closer then has no opening.
        assert_eq!(parse_blocks(":::  hint\nx\n:::\n").errors.len(), 2);
    }

    #[test]
    fn invalid_blocks_are_left_out() {
        let p = parse_blocks(":::hint danger success\nx\n:::\n\nok\n");
        assert_eq!(p.errors.len(), 1);
        assert_eq!(names(&p.nodes), ["md"]);
    }

    #[test]
    fn children_must_sit_in_their_parent() {
        assert_eq!(parse_blocks(":::tab title=\"A\"\nx\n:::\n").errors.len(), 1);
        let p = parse_blocks("::::tabs\ntext\n::::\n");
        assert_eq!(p.errors.len(), 1, "{:?}", p.errors);
        let p = parse_blocks("::::tabs\n:::hint\nx\n:::\n::::\n");
        assert_eq!(p.errors.len(), 1, "{:?}", p.errors);
    }

    #[test]
    fn a_block_needs_fewer_colons_than_its_container() {
        let p = parse_blocks(":::details title=\"T\"\n:::hint\nx\n:::\n:::\n");
        assert_ne!(p.errors.len(), 0);
    }

    #[test]
    fn embed_has_no_body() {
        let p = parse_blocks(":::embed url=\"https://example.com/v\"\ntext\n:::\n");
        assert_eq!(p.errors.len(), 1, "{:?}", p.errors);
    }

    #[test]
    fn depth_and_count_are_bounded() {
        let mut deep = String::new();
        for i in 0..=MAX_DEPTH {
            deep.push_str(&":".repeat(30 - i));
            deep.push_str("hint\n");
        }
        assert_ne!(parse_blocks(&deep).errors.len(), 0);
        let many = ":::hint\nx\n:::\n".repeat(MAX_BLOCKS + 1);
        assert_ne!(parse_blocks(&many).errors.len(), 0);
    }

    #[test]
    fn offsets_point_at_the_line() {
        let src = "ok\n\n:::nope\nx\n:::\n";
        let p = parse_blocks(src);
        assert_eq!(p.errors[0].offset, 4);
    }

    #[test]
    fn crlf_lines_work() {
        let p = parse_blocks(":::hint info\r\nx\r\n:::\r\n");
        assert!(p.errors.is_empty(), "{:?}", p.errors);
        assert_eq!(names(&p.nodes), ["hint(md)"]);
    }
}
