use anaquel_diagnostics::{Code, Diagnostic};
use yaml_rust2::{Yaml, YamlLoader};

/// Largest front matter block accepted, in bytes. Bounds parser work.
const MAX_FRONT_MATTER_BYTES: usize = 16 * 1024;

/// Keys a page may set in its front matter. The names follow Hugo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrontMatter {
    /// Title of the page; defaults to the first heading or the file name.
    pub title: Option<String>,
    /// One sentence used in metadata and in section listings.
    pub description: Option<String>,
    /// Position among siblings; lower comes first.
    pub weight: Option<i64>,
    /// Replaces the last URL segment of the page.
    pub slug: Option<String>,
    /// Former URLs of the page; each one becomes a redirect.
    pub aliases: Vec<String>,
    /// Drafts are read and checked but never emitted.
    pub draft: bool,
}

/// A page split into front matter and body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    /// Front matter, defaulted when the page has none.
    pub front: FrontMatter,
    /// The Markdown after the front matter.
    pub body: String,
    /// 1-based line of the file where `body` starts, for diagnostics.
    pub body_line: u32,
    /// Notes the parser wants to report (deprecated keys).
    pub notes: Vec<Diagnostic>,
}

/// Splits `source` and parses its front matter.
///
/// `path` only locates diagnostics. Line endings must already be `\n`.
///
/// # Errors
/// Returns a diagnostic when the block is unterminated, too large, invalid
/// YAML, uses anchors or nesting, or sets an unknown key or a wrong type.
pub fn parse_page(source: &str, path: &str) -> Result<Parsed, Diagnostic> {
    let Some(rest) = source.strip_prefix("---\n") else {
        return Ok(Parsed {
            front: FrontMatter::default(),
            body: source.to_owned(),
            body_line: 1,
            notes: Vec::new(),
        });
    };
    let (block, body, closing_lines) = split_block(rest).ok_or_else(|| {
        Diagnostic::error(
            Code::FRONT_MATTER,
            "front matter starts with `---` but is never closed",
        )
        .at(path)
        .on_line(1)
        .with_help("close it with a line that contains only `---`")
    })?;
    if block.len() > MAX_FRONT_MATTER_BYTES {
        return Err(
            Diagnostic::error(Code::FRONT_MATTER, "front matter is larger than 16 KiB").at(path),
        );
    }
    let mut notes = Vec::new();
    let front = parse_block(block, path, &mut notes)?;
    let body_line = u32::try_from(closing_lines + 2).unwrap_or(u32::MAX);
    Ok(Parsed {
        front,
        body: body.to_owned(),
        body_line,
        notes,
    })
}

/// Returns (yaml block, body, number of lines in the block).
fn split_block(rest: &str) -> Option<(&str, &str, usize)> {
    let mut offset = 0;
    for (lines, line) in rest.split_inclusive('\n').enumerate() {
        if line.trim_end_matches('\n') == "---" {
            return Some((&rest[..offset], &rest[offset + line.len()..], lines));
        }
        offset += line.len();
    }
    None
}

fn parse_block(
    block: &str,
    path: &str,
    notes: &mut Vec<Diagnostic>,
) -> Result<FrontMatter, Diagnostic> {
    let fail = |message: String| Diagnostic::error(Code::FRONT_MATTER, message).at(path);
    let docs = YamlLoader::load_from_str(block)
        .map_err(|e| fail(format!("invalid YAML: {}", e.info())))?;
    let mut front = FrontMatter::default();
    let Some(root) = docs.first() else {
        return Ok(front);
    };
    let Yaml::Hash(map) = root else {
        if matches!(root, Yaml::Null) {
            return Ok(front);
        }
        return Err(fail(
            "front matter must be a list of `key: value` lines".to_owned(),
        ));
    };
    for (key, value) in map {
        let Yaml::String(name) = key else {
            return Err(fail("front matter keys must be plain words".to_owned()));
        };
        let wrong = |expected: &str| {
            Diagnostic::error(
                Code::FRONT_MATTER_KEY,
                format!("`{name}` must be {expected}"),
            )
            .at(path)
        };
        match name.as_str() {
            "title" => front.title = Some(text(value).ok_or_else(|| wrong("text"))?),
            "description" => front.description = Some(text(value).ok_or_else(|| wrong("text"))?),
            "weight" => front.weight = Some(value.as_i64().ok_or_else(|| wrong("a whole number"))?),
            "order" => {
                front.weight = Some(value.as_i64().ok_or_else(|| wrong("a whole number"))?);
                notes.push(
                    Diagnostic::info(
                        Code::FRONT_MATTER_KEY,
                        "`order` is a deprecated name for `weight`",
                    )
                    .at(path)
                    .with_help("rename the key to `weight`"),
                );
            }
            "slug" => {
                let slug = text(value).ok_or_else(|| wrong("text"))?;
                if slug.is_empty() || slug.contains('/') || slug.contains('\\') {
                    return Err(wrong("one non-empty URL segment without `/`"));
                }
                front.slug = Some(slug);
            }
            "aliases" => {
                front.aliases = list_of_text(value).ok_or_else(|| wrong("a list of text"))?;
            }
            "draft" => front.draft = value.as_bool().ok_or_else(|| wrong("true or false"))?,
            other => {
                return Err(Diagnostic::error(
                    Code::FRONT_MATTER_KEY,
                    format!("unknown front matter key `{other}`"),
                )
                .at(path)
                .with_help("known keys: title, description, weight, slug, aliases, draft"));
            }
        }
    }
    Ok(front)
}

/// Scalars only: a number or a boolean written where text is expected still
/// reads as text, so `title: 2024` works.
fn text(value: &Yaml) -> Option<String> {
    match value {
        Yaml::String(s) | Yaml::Real(s) => Some(s.clone()),
        Yaml::Integer(n) => Some(n.to_string()),
        Yaml::Boolean(b) => Some(b.to_string()),
        _ => None,
    }
}

fn list_of_text(value: &Yaml) -> Option<Vec<String>> {
    match value {
        Yaml::Array(items) => items.iter().map(text).collect(),
        Yaml::Null => Some(Vec::new()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_without_front_matter_keeps_everything() {
        let p = parse_page("# Hi\n", "a.md").expect("ok");
        assert_eq!(p.body, "# Hi\n");
        assert_eq!(p.body_line, 1);
        assert_eq!(p.front, FrontMatter::default());
    }

    #[test]
    fn parses_known_keys_and_body_line() {
        let src = "---\ntitle: Setup\nweight: 3\naliases:\n  - /old/\n  - /older/\ndraft: false\n---\nText\n";
        let p = parse_page(src, "a.md").expect("ok");
        assert_eq!(p.front.title.as_deref(), Some("Setup"));
        assert_eq!(p.front.weight, Some(3));
        assert_eq!(p.front.aliases, vec!["/old/", "/older/"]);
        assert_eq!(p.body, "Text\n");
        assert_eq!(p.body_line, 8);
    }

    #[test]
    fn unknown_key_is_an_error() {
        let err = parse_page("---\ncolour: red\n---\n", "a.md").expect_err("unknown");
        assert_eq!(err.code, Code::FRONT_MATTER_KEY);
    }

    #[test]
    fn unterminated_block_is_an_error() {
        let err = parse_page("---\ntitle: x\n", "a.md").expect_err("open");
        assert_eq!(err.code, Code::FRONT_MATTER);
    }

    #[test]
    fn order_is_a_deprecated_weight() {
        let p = parse_page("---\norder: 2\n---\n", "a.md").expect("ok");
        assert_eq!(p.front.weight, Some(2));
        assert_eq!(p.notes.len(), 1);
    }

    #[test]
    fn nested_values_are_rejected() {
        let err = parse_page("---\ntitle:\n  a: b\n---\n", "a.md").expect_err("nested");
        assert_eq!(err.code, Code::FRONT_MATTER_KEY);
    }

    #[test]
    fn slug_cannot_contain_a_slash() {
        assert!(parse_page("---\nslug: a/b\n---\n", "a.md").is_err());
    }
}
