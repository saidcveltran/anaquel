//! User-facing diagnostics with stable codes.
//!
//! Every problem the engine reports to a person is a [`Diagnostic`]: a stable
//! code (`AQ0101`), a severity, a message, an optional location and a hint.
//! Libraries return diagnostics; only the binary decides how to print them.

use std::fmt;

use serde::Serialize;

/// How serious a diagnostic is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Worth knowing; never fails a build.
    Info,
    /// Something is wrong but the build can continue; fails under `--strict`.
    Warning,
    /// The build cannot produce a correct result.
    Error,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        })
    }
}

/// Stable identifier of a kind of problem. The number never changes meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Code(pub u16);

impl Code {
    /// A file or folder could not be read.
    pub const IO: Self = Self(1);
    /// `anaquel.toml` or `volume.toml` is missing or invalid.
    pub const MANIFEST: Self = Self(101);
    /// The library asks for a schema or engine this build does not support.
    pub const SCHEMA_VERSION: Self = Self(102);
    /// Front matter could not be parsed.
    pub const FRONT_MATTER: Self = Self(201);
    /// A front matter key is unknown or has the wrong type.
    pub const FRONT_MATTER_KEY: Self = Self(202);
    /// Two entries resolve to the same URL.
    pub const DUPLICATE_URL: Self = Self(301);
    /// A relative link points at a file that does not exist.
    pub const BROKEN_LINK: Self = Self(302);
    /// A link points at an anchor that does not exist in its target.
    pub const BROKEN_ANCHOR: Self = Self(303);
    /// A `[[term]]` reference has no matching term.
    pub const UNKNOWN_TERM: Self = Self(304);
    /// Two terms of one volume have the same name.
    pub const DUPLICATE_TERM: Self = Self(305);
    /// A link uses a URL scheme that is not allowed.
    pub const FORBIDDEN_URL: Self = Self(401);
    /// Raw HTML in content while the library does not allow it.
    pub const RAW_HTML: Self = Self(402);
    /// A theme file is missing or its manifest is invalid.
    pub const THEME: Self = Self(501);
    /// A template failed to render.
    pub const TEMPLATE: Self = Self(502);
    /// A URL that existed in the previous build disappeared without an alias.
    pub const LOST_URL: Self = Self(601);
    /// The search index could not be built; the site still works without it.
    pub const SEARCH: Self = Self(701);
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AQ{:04}", self.0)
    }
}

/// One reported problem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    /// Stable code.
    pub code: Code,
    /// Severity.
    pub severity: Severity,
    /// What is wrong, in one sentence.
    pub message: String,
    /// Path of the file concerned, relative to the library root, with `/`.
    pub path: Option<String>,
    /// 1-based line, when known.
    pub line: Option<u32>,
    /// How to fix it, when there is a clear way.
    pub help: Option<String>,
}

impl Diagnostic {
    /// Creates an error.
    pub fn error(code: Code, message: impl Into<String>) -> Self {
        Self::new(code, Severity::Error, message)
    }

    /// Creates a warning.
    pub fn warning(code: Code, message: impl Into<String>) -> Self {
        Self::new(code, Severity::Warning, message)
    }

    /// Creates an informational diagnostic.
    pub fn info(code: Code, message: impl Into<String>) -> Self {
        Self::new(code, Severity::Info, message)
    }

    fn new(code: Code, severity: Severity, message: impl Into<String>) -> Self {
        Self {
            code,
            severity,
            message: message.into(),
            path: None,
            line: None,
            help: None,
        }
    }

    /// Attaches the file the problem is in.
    #[must_use]
    pub fn at(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Attaches a 1-based line.
    #[must_use]
    pub fn on_line(mut self, line: u32) -> Self {
        self.line = Some(line);
        self
    }

    /// Attaches a hint.
    #[must_use]
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}[{}]: {}", self.severity, self.code, self.message)?;
        if let Some(path) = &self.path {
            write!(f, "\n  --> {path}")?;
            if let Some(line) = self.line {
                write!(f, ":{line}")?;
            }
        }
        if let Some(help) = &self.help {
            write!(f, "\n  help: {help}")?;
        }
        Ok(())
    }
}

/// Collects diagnostics during a build.
#[derive(Debug, Default, Clone)]
pub struct Report {
    items: Vec<Diagnostic>,
}

impl Report {
    /// Creates an empty report.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one diagnostic.
    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.items.push(diagnostic);
    }

    /// Adds every diagnostic of another report.
    pub fn extend(&mut self, other: Report) {
        self.items.extend(other.items);
    }

    /// All diagnostics in the order they were added.
    pub fn items(&self) -> &[Diagnostic] {
        &self.items
    }

    /// Number of diagnostics of the given severity.
    pub fn count(&self, severity: Severity) -> usize {
        self.items.iter().filter(|d| d.severity == severity).count()
    }

    /// True when at least one error was reported.
    pub fn has_errors(&self) -> bool {
        self.count(Severity::Error) > 0
    }

    /// Sorts by path, line and code so that output is stable across runs.
    pub fn sort(&mut self) {
        self.items.sort_by(|a, b| {
            (&a.path, a.line, a.code, &a.message).cmp(&(&b.path, b.line, b.code, &b.message))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_is_padded_to_four_digits() {
        assert_eq!(Code::MANIFEST.to_string(), "AQ0101");
        assert_eq!(Code::IO.to_string(), "AQ0001");
    }

    #[test]
    fn display_includes_location_and_help() {
        let d = Diagnostic::error(Code::BROKEN_LINK, "no such file")
            .at("a/b.md")
            .on_line(7)
            .with_help("check the path");
        let text = d.to_string();
        assert!(text.starts_with("error[AQ0302]: no such file"));
        assert!(text.contains("--> a/b.md:7"));
        assert!(text.contains("help: check the path"));
    }

    #[test]
    fn report_counts_and_sorts() {
        let mut r = Report::new();
        r.push(Diagnostic::warning(Code::UNKNOWN_TERM, "z").at("b.md"));
        r.push(Diagnostic::error(Code::BROKEN_LINK, "a").at("a.md"));
        r.sort();
        assert_eq!(r.items()[0].path.as_deref(), Some("a.md"));
        assert!(r.has_errors());
        assert_eq!(r.count(Severity::Warning), 1);
    }
}
