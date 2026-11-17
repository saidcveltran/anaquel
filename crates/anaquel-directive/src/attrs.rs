//! Attributes of a directive opening line: `key="value"`, `key=value` or a bare flag.

/// Most attributes one directive may carry.
pub(crate) const MAX_ATTRS: usize = 8;
/// Longest attribute value, in bytes.
pub(crate) const MAX_VALUE_BYTES: usize = 500;

/// The attributes of one directive.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Attributes {
    /// `key=value` pairs, in the order written.
    pub pairs: Vec<(String, String)>,
    /// Bare words such as `warning` or `open`, in the order written.
    pub flags: Vec<String>,
}

impl Attributes {
    /// The value of `key`, when given.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.pairs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Whether the bare word `flag` was given.
    #[must_use]
    pub fn has_flag(&self, flag: &str) -> bool {
        self.flags.iter().any(|f| f == flag)
    }
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

/// Parses the text after the directive name.
///
/// Returns a message for the first problem: an unquoted space in a value, a missing closing
/// quote, a repeated key, a stray character, too many attributes or a value that is too long.
pub(crate) fn parse(text: &str) -> Result<Attributes, String> {
    let mut out = Attributes::default();
    let mut rest = text.trim();
    while !rest.is_empty() {
        let word_end = rest.find(|c: char| !is_word_char(c)).unwrap_or(rest.len());
        if word_end == 0 {
            return Err(format!("unexpected character `{}`", first_char(rest)));
        }
        let (word, after) = rest.split_at(word_end);
        if let Some(value_start) = after.strip_prefix('=') {
            if out.pairs.iter().any(|(k, _)| k == word) {
                return Err(format!("attribute `{word}` is given twice"));
            }
            let (value, remaining) = value(value_start)?;
            if value.len() > MAX_VALUE_BYTES {
                return Err(format!(
                    "the value of `{word}` is longer than {MAX_VALUE_BYTES} bytes"
                ));
            }
            out.pairs.push((word.to_owned(), value));
            rest = remaining.trim_start();
        } else {
            if out.flags.iter().any(|f| f == word) {
                return Err(format!("`{word}` is given twice"));
            }
            out.flags.push(word.to_owned());
            rest = after.trim_start();
        }
        if after.starts_with(|c: char| !c.is_whitespace() && c != '=') && !after.is_empty() {
            // A word must be followed by a space, `=` or the end of the line.
            return Err(format!("unexpected character `{}`", first_char(after)));
        }
        if out.pairs.len() + out.flags.len() > MAX_ATTRS {
            return Err(format!("more than {MAX_ATTRS} attributes"));
        }
    }
    Ok(out)
}

fn first_char(text: &str) -> char {
    text.chars().next().unwrap_or(' ')
}

/// Reads one value after `=`: quoted (no `"` inside) or a run without spaces.
fn value(text: &str) -> Result<(String, &str), String> {
    if let Some(quoted) = text.strip_prefix('"') {
        let Some(end) = quoted.find('"') else {
            return Err("a quoted value is not closed".to_owned());
        };
        let (value, rest) = quoted.split_at(end);
        let rest = &rest[1..];
        if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
            return Err("a quoted value must be followed by a space".to_owned());
        }
        return Ok((value.to_owned(), rest));
    }
    let end = text.find(char::is_whitespace).unwrap_or(text.len());
    let (value, rest) = text.split_at(end);
    if value.is_empty() {
        return Err("an attribute has no value".to_owned());
    }
    if value.contains('"') {
        return Err("a quote inside an unquoted value".to_owned());
    }
    Ok((value.to_owned(), rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_quoted_unquoted_and_flags() {
        let a = parse("warning title=\"A B\" cols=3 open").unwrap_or_default();
        assert_eq!(a.get("title"), Some("A B"));
        assert_eq!(a.get("cols"), Some("3"));
        assert!(a.has_flag("warning") && a.has_flag("open"));
    }

    #[test]
    fn empty_text_is_no_attributes() {
        assert_eq!(parse("  "), Ok(Attributes::default()));
    }

    #[test]
    fn refuses_what_is_malformed() {
        for bad in [
            "title=\"open",
            "title=",
            "a=1 a=2",
            "x y x",
            "t=\"a\"b",
            "t=a\"b",
            "=1",
            "w@rd",
            "a=1 b=2 c=3 d=4 e=5 f=6 g=7 h=8 i=9",
        ] {
            assert!(parse(bad).is_err(), "{bad} should be refused");
        }
    }

    #[test]
    fn a_long_value_is_refused() {
        let text = format!("t=\"{}\"", "x".repeat(MAX_VALUE_BYTES + 1));
        assert!(parse(&text).is_err());
    }
}
