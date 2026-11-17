//! Syntax highlighting of fenced code blocks.
//!
//! The markup uses class names only (`s-keyword`, `s-string`, ...), never inline styles, so the
//! strict Content-Security-Policy holds; the theme colours the classes for both colour schemes.
//! A language that is not known, a block that is too big, or a grammar that fails all fall back
//! to plain escaped code.

use std::sync::OnceLock;

use syntect::html::{ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

/// Largest block that is highlighted, in bytes. Bounds the work a page can ask for.
const MAX_CODE_BYTES: usize = 100_000;
/// Longest accepted language name.
const MAX_LANG_BYTES: usize = 32;

fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

/// The language name of a fence info string such as `rust`, `rust,ignore` or `rust title="x"`.
///
/// Only letters, digits and `+ # . _ -` are accepted, so the name can be used in a class.
pub(crate) fn language_of(info: &str) -> Option<&str> {
    let name = info.split([' ', '\t', ',', '{']).next()?;
    let valid = !name.is_empty()
        && name.len() <= MAX_LANG_BYTES
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '#' | '.' | '_' | '-'));
    valid.then_some(name)
}

/// Names that people write for a language the bundled grammars know under another name.
fn alias(language: &str) -> &str {
    match language {
        "shell" | "console" | "shell-session" | "shellsession" => "bash",
        "csharp" => "cs",
        other => other,
    }
}

/// HTML for the inside of `<code>`, or `None` when the block should stay plain.
pub(crate) fn highlight(language: &str, code: &str) -> Option<String> {
    if code.len() > MAX_CODE_BYTES {
        return None;
    }
    let set = syntaxes();
    let syntax = set.find_syntax_by_token(alias(language))?;
    let mut generator = ClassedHTMLGenerator::new_with_class_style(
        syntax,
        set,
        ClassStyle::SpacedPrefixed { prefix: "s-" },
    );
    for line in LinesWithEndings::from(code) {
        generator
            .parse_html_for_line_which_includes_newline(line)
            .ok()?;
    }
    Some(generator.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_names_are_checked() {
        assert_eq!(language_of("rust"), Some("rust"));
        assert_eq!(language_of("rust,ignore"), Some("rust"));
        assert_eq!(language_of("c++ title=\"x\""), Some("c++"));
        assert_eq!(language_of(""), None);
        assert_eq!(language_of("a\"b"), None);
        assert_eq!(language_of("<script>"), None);
        assert_eq!(language_of(&"x".repeat(MAX_LANG_BYTES + 1)), None);
    }

    #[test]
    fn known_languages_get_class_markup() {
        let html = highlight("rust", "fn main() { let x = 1; }\n").unwrap_or_default();
        assert!(html.contains("<span class=\"s-"), "{html}");
        assert!(!html.contains("style="), "{html}");
    }

    #[test]
    fn code_is_escaped() {
        let html = highlight("html", "<script>alert(1)</script>\n").unwrap_or_default();
        assert!(!html.contains("<script>"), "{html}");
        assert!(html.contains("&lt;"), "{html}");
    }

    #[test]
    fn the_common_languages_are_bundled() {
        let missing: Vec<_> = [
            "rust",
            "python",
            "javascript",
            "js",
            "json",
            "bash",
            "sh",
            "shell",
            "html",
            "css",
            "yaml",
            "markdown",
            "sql",
            "c",
            "cpp",
            "go",
            "java",
            "xml",
            "diff",
            "php",
            "ruby",
            "lua",
            "makefile",
            "cs",
        ]
        .into_iter()
        .filter(|l| highlight(l, "x\n").is_none())
        .collect();
        assert!(missing.is_empty(), "not highlighted: {missing:?}");
    }

    #[test]
    fn unknown_languages_and_huge_blocks_stay_plain() {
        assert_eq!(highlight("nonsense-lang", "x"), None);
        assert_eq!(highlight("rust", &"a".repeat(MAX_CODE_BYTES + 1)), None);
    }
}
