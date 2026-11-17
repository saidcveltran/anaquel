//! Resolved tree of entries, links and terms of a library.
//!
//! The catalog is built from text already read from the disk. It decides each
//! page's URL, order and neighbours, indexes the `###` terms of every `terms.md` and `glossary.md` page
//! and answers whether a link or a `[[term]]` points at something that exists.
//! It never touches the file system and knows no Markdown parser: callers pass
//! a function that lists the headings of a body.

mod build;
mod links;
mod logical_path;
mod model;

pub use build::HeadingsOf;
pub use links::ResolvedLink;
pub use logical_path::LogicalPath;
pub use model::{Entry, EntryId, EntryKind, Heading, PageSource, TermTarget, Volume, VolumeSource};

use std::collections::HashMap;

use anaquel_diagnostics::{Diagnostic, Report};
use anaquel_schema::LibraryManifest;

/// The whole library after resolution.
#[derive(Debug)]
pub struct Catalog {
    library: LibraryManifest,
    volumes: Vec<Volume>,
    entries: Vec<Entry>,
    by_source: HashMap<String, EntryId>,
    /// Static files by library path, with their public URL.
    files: HashMap<String, String>,
    /// Terms by (volume index, lowercase term): entry and heading position.
    lexicon: HashMap<(usize, String), (EntryId, usize)>,
    redirects: Vec<(String, String)>,
}

impl Catalog {
    /// Resolves a library from the text of its files.
    ///
    /// Problems that make a single page unusable are reported and the page is
    /// left out; the rest of the library is still resolved so one run shows
    /// every problem.
    pub fn build(
        library: LibraryManifest,
        volumes: Vec<VolumeSource>,
        headings_of: &HeadingsOf<'_>,
    ) -> (Self, Report) {
        build::build(library, volumes, headings_of)
    }

    /// The parsed `anaquel.toml`.
    pub fn library(&self) -> &LibraryManifest {
        &self.library
    }

    /// Volumes in display order.
    pub fn volumes(&self) -> &[Volume] {
        &self.volumes
    }

    /// Every entry, drafts included, in a stable order.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// One entry by id.
    pub fn entry(&self, id: EntryId) -> Option<&Entry> {
        self.entries.get(id.0)
    }

    /// Static files (images, downloads) as `(library path, URL)`, sorted.
    pub fn static_files(&self) -> Vec<(&str, &str)> {
        let mut all: Vec<_> = self
            .files
            .iter()
            .map(|(p, u)| (p.as_str(), u.as_str()))
            .collect();
        all.sort_unstable();
        all
    }

    /// Redirects from former URLs to current ones, sorted by former URL.
    pub fn redirects(&self) -> &[(String, String)] {
        &self.redirects
    }

    /// Checks a link written in `from` and says where it leads.
    ///
    /// # Errors
    /// Returns a diagnostic when the link is empty, uses a forbidden scheme,
    /// points at a missing file or anchor, or at a draft.
    pub fn resolve_link(&self, from: EntryId, href: &str) -> Result<ResolvedLink, Diagnostic> {
        links::resolve(self, from, href)
    }

    /// Finds the heading a `[[term]]` written in `from` points at.
    ///
    /// # Errors
    /// Returns a diagnostic when the volume of `from` has no such term.
    pub fn resolve_term(&self, from: EntryId, name: &str) -> Result<TermTarget, Diagnostic> {
        links::resolve_term(self, from, name)
    }
}

#[cfg(test)]
mod tests;
