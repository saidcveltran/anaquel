use anaquel_diagnostics::{Code, Diagnostic};

use crate::Catalog;
use crate::LogicalPath;
use crate::build::term_key;
use crate::logical_path::join;
use crate::model::{EntryId, TermTarget};

/// Where a link leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedLink {
    /// An allowed absolute URL, copied as written.
    External(String),
    /// A page or file of the library, as a site URL with optional `#anchor`.
    Internal(String),
}

pub(crate) fn resolve(
    catalog: &Catalog,
    from: EntryId,
    href: &str,
) -> Result<ResolvedLink, Diagnostic> {
    let origin = catalog
        .entry(from)
        .ok_or_else(|| Diagnostic::error(Code::BROKEN_LINK, "link written outside any page"))?;
    let fail =
        |code: Code, message: String| Diagnostic::error(code, message).at(origin.source.as_str());

    if href.is_empty() {
        return Err(fail(Code::BROKEN_LINK, "empty link".to_owned()));
    }
    if let Some(anchor) = href.strip_prefix('#') {
        return check_anchor(catalog, from, anchor)
            .map(|()| ResolvedLink::Internal(format!("#{anchor}")));
    }
    if let Some(scheme) = scheme_of(href) {
        return if matches!(
            scheme.to_ascii_lowercase().as_str(),
            "http" | "https" | "mailto"
        ) {
            Ok(ResolvedLink::External(href.to_owned()))
        } else {
            Err(fail(
                Code::FORBIDDEN_URL,
                format!("links with `{scheme}:` are not allowed"),
            )
            .with_help("use http, https or mailto"))
        };
    }
    if href.starts_with("//") {
        return Err(fail(
            Code::FORBIDDEN_URL,
            "protocol-relative links are not allowed".to_owned(),
        )
        .with_help("write the full https:// address"));
    }
    if href.starts_with('/') {
        return Err(
            fail(Code::BROKEN_LINK, format!("`{href}` is absolute")).with_help(
                "link to the source file with a relative path, such as `../guide/setup.md`",
            ),
        );
    }
    if href.contains('?') {
        return Err(fail(
            Code::BROKEN_LINK,
            format!("`{href}` has a query string"),
        ));
    }

    let (path_part, fragment) = href
        .split_once('#')
        .map_or((href, None), |(p, f)| (p, Some(f)));
    let decoded = percent_decode(path_part).ok_or_else(|| {
        fail(
            Code::BROKEN_LINK,
            format!("`{href}` is not valid percent-encoding"),
        )
    })?;
    let origin_dir = catalog.volumes[origin.volume].folder();
    let base = if origin.rel.dir().is_empty() {
        origin_dir
    } else {
        format!("{origin_dir}/{}", origin.rel.dir())
    };
    let target = join(&base, &decoded)
        .ok_or_else(|| fail(Code::BROKEN_LINK, format!("`{href}` leaves the library")))?;

    if let Some(file_url) = catalog.files.get(target.as_str()) {
        return Ok(ResolvedLink::Internal(file_url.clone()));
    }
    let Some(&target_id) = catalog.by_source.get(target.as_str()) else {
        let help = if decoded.ends_with('/') || !decoded.contains('.') {
            "link to the `.md` file, not to a folder or a URL"
        } else {
            "check the path and the letter case"
        };
        return Err(fail(Code::BROKEN_LINK, format!("`{href}` does not exist")).with_help(help));
    };
    let target_entry = &catalog.entries[target_id.0];
    if target_entry.draft {
        return Err(fail(
            Code::BROKEN_LINK,
            format!("`{href}` is a draft and is not published"),
        ));
    }
    match fragment {
        None | Some("") => Ok(ResolvedLink::Internal(target_entry.url.clone())),
        Some(anchor) => {
            check_anchor(catalog, target_id, anchor).map_err(|d| d.at(origin.source.as_str()))?;
            Ok(ResolvedLink::Internal(format!(
                "{}#{anchor}",
                target_entry.url
            )))
        }
    }
}

pub(crate) fn local_target(catalog: &Catalog, from: EntryId, href: &str) -> Option<LogicalPath> {
    if href.is_empty()
        || href.starts_with(['#', '/'])
        || href.contains('?')
        || scheme_of(href).is_some()
    {
        return None;
    }
    let origin = catalog.entry(from)?;
    let path_part = href.split_once('#').map_or(href, |(p, _)| p);
    let decoded = percent_decode(path_part)?;
    let origin_dir = catalog.volumes[origin.volume].folder();
    let base = if origin.rel.dir().is_empty() {
        origin_dir
    } else {
        format!("{origin_dir}/{}", origin.rel.dir())
    };
    join(&base, &decoded)
}

fn check_anchor(catalog: &Catalog, entry: EntryId, anchor: &str) -> Result<(), Diagnostic> {
    let target = &catalog.entries[entry.0];
    if anchor.is_empty() || target.headings.iter().any(|h| h.anchor == anchor) {
        return Ok(());
    }
    Err(Diagnostic::error(
        Code::BROKEN_ANCHOR,
        format!(
            "`{}` has no heading with anchor `#{anchor}`",
            target.source.as_str()
        ),
    )
    .at(target.source.as_str()))
}

pub(crate) fn resolve_term(
    catalog: &Catalog,
    from: EntryId,
    name: &str,
) -> Result<TermTarget, Diagnostic> {
    let origin = catalog
        .entry(from)
        .ok_or_else(|| Diagnostic::error(Code::UNKNOWN_TERM, "term written outside any page"))?;
    let Some((entry_id, position)) = catalog.lexicon.get(&(origin.volume, term_key(name))) else {
        return Err(Diagnostic::error(
            Code::UNKNOWN_TERM,
            format!("no term `{name}` in this volume"),
        )
        .at(origin.source.as_str())
        .with_help(
            "terms are the `###` headings of the volume's `terms.md` and `glossary.md` pages",
        ));
    };
    let target = &catalog.entries[entry_id.0];
    let heading = &target.headings[*position];
    Ok(TermTarget {
        url: format!("{}#{}", target.url, heading.anchor),
        text: heading.text.clone(),
    })
}

/// The URL scheme of `href`, if it has one (`https` in `https://x`).
fn scheme_of(href: &str) -> Option<&str> {
    let (scheme, _) = href.split_once(':')?;
    let mut chars = scheme.chars();
    let first = chars.next()?;
    (first.is_ascii_alphabetic()
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')))
    .then_some(scheme)
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn scheme_detection() {
        assert_eq!(scheme_of("https://a.b"), Some("https"));
        assert_eq!(scheme_of("javascript:alert(1)"), Some("javascript"));
        assert_eq!(scheme_of("../a.md"), None);
        assert_eq!(scheme_of("a/b:c.md"), None);
    }

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("a%20b.md").as_deref(), Some("a b.md"));
        assert_eq!(percent_decode("%zz"), None);
        assert_eq!(percent_decode("%"), None);
        assert_eq!(percent_decode("%ff"), None);
    }
}
