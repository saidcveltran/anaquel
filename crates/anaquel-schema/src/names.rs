//! Names that become part of a URL.

/// Why `segment` cannot be one segment of a page address, or `None` when it can.
///
/// A segment may hold letters of any script, digits and `- _ . ~ + @`. Everything else is
/// refused: the characters that end or change a URL (`# ? % & / \`), quotes and brackets that
/// break the attribute or the line a name is written into, white space, control and invisible
/// characters (including the ones that reorder text), and a final dot, which Windows drops.
#[must_use]
pub fn url_segment_problem(segment: &str) -> Option<String> {
    if segment.is_empty() || segment == "." || segment == ".." {
        return Some("is empty or only dots".to_owned());
    }
    if segment.ends_with('.') {
        return Some("ends with a dot".to_owned());
    }
    segment
        .chars()
        .find(|&c| !allowed(c))
        .map(|c| format!("contains `{}`", c.escape_default()))
}

fn allowed(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~' | '+' | '@');
    }
    !(c.is_control()
        || c.is_whitespace()
        || matches!(
            c,
            '\u{200b}'..='\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2060}'..='\u{2064}'
                | '\u{2066}'..='\u{2069}'
                | '\u{feff}'
        ))
}

#[cfg(test)]
mod tests {
    use super::url_segment_problem as problem;

    #[test]
    fn plain_names_pass() {
        for name in [
            "setup",
            "Setup-2",
            "a_b",
            "v1.2",
            "a~b",
            "c++",
            "a@b",
            "ünï",
            "日本語",
            "ñ",
        ] {
            assert_eq!(problem(name), None, "{name}");
        }
    }

    #[test]
    fn characters_that_change_an_address_are_refused() {
        for name in [
            "",
            ".",
            "..",
            "a.",
            "a b",
            "a\tb",
            "a#b",
            "a?b",
            "a%b",
            "a&b",
            "a/b",
            "a\\b",
            "a\"b",
            "a'b",
            "a<b",
            "a>b",
            "a`b",
            "a|b",
            "a*b",
            "a:b",
            "a;b",
            "a,b",
            "a(b",
            "a)b",
            "a[b",
            "a]b",
            "a{b",
            "a}b",
            "a^b",
            "a=b",
            "a$b",
            "a!b",
            "a\nb",
            "a\u{0}b",
            "a\u{202e}b",
            "a\u{200b}b",
            "a\u{feff}b",
            "a\u{a0}b",
            "a\u{2028}b",
        ] {
            assert_ne!(problem(name), None, "{name:?}");
        }
    }
}
