use unicode_normalization::UnicodeNormalization;

/// A path inside a library: `/`-separated, NFC-normalised, never absolute and
/// never leaving the library. The same text on Windows, Linux and macOS, so a
/// build gives the same URLs everywhere.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LogicalPath(String);

impl LogicalPath {
    /// Validates and normalises a path read from the disk or from a link.
    ///
    /// # Errors
    /// Returns a message when the path is empty, absolute, contains `\`, a
    /// control character, an empty segment or a `.`/`..` segment.
    pub fn new(raw: &str) -> Result<Self, String> {
        let normalised: String = raw.nfc().collect();
        if normalised.is_empty() {
            return Err("the path is empty".to_owned());
        }
        if normalised.starts_with('/') {
            return Err("the path is absolute".to_owned());
        }
        if normalised.contains('\\') {
            return Err("the path contains a backslash".to_owned());
        }
        if normalised.chars().any(char::is_control) {
            return Err("the path contains a control character".to_owned());
        }
        for segment in normalised.split('/') {
            if segment.is_empty() || segment == "." || segment == ".." {
                return Err(format!("the path has an invalid segment `{segment}`"));
            }
        }
        Ok(Self(normalised))
    }

    /// The path as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The folder part, or an empty string for a path with one segment.
    pub fn dir(&self) -> &str {
        self.0.rsplit_once('/').map_or("", |(dir, _)| dir)
    }

    /// The last segment.
    pub fn file_name(&self) -> &str {
        self.0
            .rsplit_once('/')
            .map_or(self.0.as_str(), |(_, name)| name)
    }
}

/// Resolves `href` (already percent-decoded, without fragment) against the
/// folder `dir`, as a browser would. Returns `None` when it climbs above the
/// root or yields an empty path.
pub(crate) fn join(dir: &str, href: &str) -> Option<LogicalPath> {
    let mut parts: Vec<&str> = if dir.is_empty() {
        Vec::new()
    } else {
        dir.split('/').collect()
    };
    for segment in href.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    if parts.is_empty() {
        return None;
    }
    LogicalPath::new(&parts.join("/")).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises_to_nfc() {
        let decomposed = "cafe\u{301}.md";
        let p = LogicalPath::new(decomposed).unwrap();
        assert_eq!(p.as_str(), "caf\u{e9}.md");
    }

    #[test]
    fn rejects_bad_paths() {
        for bad in ["", "/a", "a\\b", "a//b", "a/../b", "./a", "a/\u{7}"] {
            assert!(LogicalPath::new(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn splits_dir_and_name() {
        let p = LogicalPath::new("a/b/c.md").unwrap();
        assert_eq!(p.dir(), "a/b");
        assert_eq!(p.file_name(), "c.md");
        assert_eq!(LogicalPath::new("c.md").unwrap().dir(), "");
    }

    #[test]
    fn join_resolves_dots() {
        assert_eq!(join("a/b", "../c.md").unwrap().as_str(), "a/c.md");
        assert_eq!(join("a", "./c.md").unwrap().as_str(), "a/c.md");
        assert_eq!(join("", "c.md").unwrap().as_str(), "c.md");
    }

    #[test]
    fn join_refuses_to_leave_the_root() {
        assert!(join("a", "../../x.md").is_none());
        assert!(join("", "../x.md").is_none());
    }
}
