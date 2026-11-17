//! Inline references of Anaquel content.
//!
//! `[[term]]` and `[[term|text]]` point at a term of the same volume. They are
//! recognised here and not by the Markdown parser, whose own wiki-link option
//! has had denial-of-service advisories. Block directives (`:::`) join this
//! crate in a later milestone.

/// Longest accepted reference target, in bytes. Bounds the work per reference.
const MAX_TARGET_BYTES: usize = 200;

/// One `[[term]]` or `[[term|text]]` found in a piece of text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermRef<'a> {
    /// Byte offset of the opening `[[`.
    pub start: usize,
    /// Byte offset just after the closing `]]`.
    pub end: usize,
    /// The term being referred to.
    pub target: &'a str,
    /// Text to show instead of the term, when given.
    pub label: Option<&'a str>,
}

/// Finds every well-formed reference in `text`, left to right.
///
/// A reference must sit on one line, have a non-empty target and contain no
/// `[`, `]` or `|` inside the target. Anything else is left as plain text.
pub fn find_term_refs(text: &str) -> Vec<TermRef<'_>> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(open) = text[from..].find("[[").map(|i| i + from) {
        let inner_start = open + 2;
        let Some(close) = text[inner_start..].find("]]").map(|i| i + inner_start) else {
            break;
        };
        let inner = &text[inner_start..close];
        match parse_inner(inner) {
            Some((target, label)) => {
                found.push(TermRef {
                    start: open,
                    end: close + 2,
                    target,
                    label,
                });
                from = close + 2;
            }
            None => from = open + 1,
        }
    }
    found
}

fn parse_inner(inner: &str) -> Option<(&str, Option<&str>)> {
    if inner.contains('\n') || inner.contains('[') || inner.contains(']') {
        return None;
    }
    let (target, label) = match inner.split_once('|') {
        Some((target, label)) => (target, Some(label.trim())),
        None => (inner, None),
    };
    let target = target.trim();
    if target.is_empty() || target.len() > MAX_TARGET_BYTES || target.contains('|') {
        return None;
    }
    if label.is_some_and(|l| l.is_empty() || l.contains('|')) {
        return None;
    }
    Some((target, label))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_plain_and_labelled_references() {
        let refs = find_term_refs("see [[Cache]] and [[cache|the cache]] now");
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].target, "Cache");
        assert_eq!(refs[0].label, None);
        assert_eq!(refs[1].target, "cache");
        assert_eq!(refs[1].label, Some("the cache"));
    }

    #[test]
    fn offsets_cover_the_whole_reference() {
        let text = "a [[b]] c";
        let r = &find_term_refs(text)[0];
        assert_eq!(&text[r.start..r.end], "[[b]]");
    }

    #[test]
    fn ignores_malformed_references() {
        for text in [
            "[[]]",
            "[[a\nb]]",
            "[[a|]]",
            "[[a|b|c]]",
            "[[unclosed",
            "[[a [b]] c]]",
        ] {
            assert_eq!(find_term_refs(text).len(), 0, "{text:?}");
        }
    }

    #[test]
    fn a_stray_bracket_does_not_hide_the_reference_after_it() {
        let refs = find_term_refs("[[[x]] and [[y]]");
        let targets: Vec<_> = refs.iter().map(|r| r.target).collect();
        assert_eq!(targets, vec!["x", "y"]);
    }

    #[test]
    fn handles_multibyte_text() {
        let text = "área [[término]] fin";
        let r = &find_term_refs(text)[0];
        assert_eq!(r.target, "término");
        assert_eq!(&text[r.start..r.end], "[[término]]");
    }
}
