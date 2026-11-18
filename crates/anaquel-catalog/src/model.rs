use anaquel_schema::{ShelfManifest, VolumeManifest};

use crate::logical_path::LogicalPath;

/// A heading of a page, with the anchor its `id` gets in the HTML.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    /// 1 to 6.
    pub level: u8,
    /// Plain text of the heading.
    pub text: String,
    /// Value of the `id` attribute; unique within its page.
    pub anchor: String,
    /// 1-based line of the file.
    pub line: u32,
}

/// Index of an entry in [`crate::Catalog::entries`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntryId(pub usize);

/// What an entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// An `_index.md`: the page of a folder, which lists its children.
    Section,
    /// Any other Markdown file.
    Page,
}

/// A Markdown file of a volume, with everything resolved about it.
#[derive(Debug, Clone)]
pub struct Entry {
    /// Position in the catalog.
    pub id: EntryId,
    /// Index of its volume in [`crate::Catalog::volumes`].
    pub volume: usize,
    /// Path from the library root, e.g. `volumes/guide/setup.md`.
    pub source: LogicalPath,
    /// Path from the volume folder, e.g. `setup.md`.
    pub rel: LogicalPath,
    /// Public URL, always starting and ending with `/`.
    pub url: String,
    /// Section or page.
    pub kind: EntryKind,
    /// Display title.
    pub title: String,
    /// One-sentence description, when the page has one.
    pub description: Option<String>,
    /// Sort key among siblings.
    pub weight: i64,
    /// Drafts are checked but never emitted or linked.
    pub draft: bool,
    /// Former URLs that redirect here.
    pub aliases: Vec<String>,
    /// Headings in document order.
    pub headings: Vec<Heading>,
    /// Section that lists this entry.
    pub parent: Option<EntryId>,
    /// Entries listed under this one, in display order.
    pub children: Vec<EntryId>,
    /// Previous entry in reading order within the volume.
    pub prev: Option<EntryId>,
    /// Next entry in reading order within the volume.
    pub next: Option<EntryId>,
    /// Markdown after the front matter.
    pub body: String,
    /// 1-based line of the file where `body` starts.
    pub body_line: u32,
    /// Short content hash of the source file.
    pub hash: String,
}

/// A book of the library.
#[derive(Debug, Clone)]
pub struct Volume {
    /// Folder name of the volume.
    pub slug: String,
    /// Slug of the shelf that holds the volume, if any.
    pub shelf: Option<String>,
    /// Parsed `volume.toml`.
    pub manifest: VolumeManifest,
    /// The volume's `_index.md`.
    pub root: Option<EntryId>,
    /// Entries in reading order, drafts excluded.
    pub order: Vec<EntryId>,
}

impl Volume {
    /// Library path of the volume folder.
    pub fn folder(&self) -> String {
        volume_folder(self.shelf.as_deref(), &self.slug)
    }

    /// First URL segments of the volume, without slashes at the ends.
    pub fn address(&self) -> String {
        volume_address(self.shelf.as_deref(), &self.slug)
    }
}

/// A group of volumes with a common first URL segment.
#[derive(Debug, Clone)]
pub struct Shelf {
    /// Folder name under `shelves/`, also the first URL segment.
    pub slug: String,
    /// Parsed `shelf.toml`.
    pub manifest: ShelfManifest,
}

/// A shelf folder as read by the caller.
#[derive(Debug, Clone)]
pub struct ShelfSource {
    /// Folder name under `shelves/`.
    pub slug: String,
    /// Parsed `shelf.toml`.
    pub manifest: ShelfManifest,
}

fn volume_folder(shelf: Option<&str>, slug: &str) -> String {
    shelf.map_or_else(
        || format!("volumes/{slug}"),
        |shelf| format!("shelves/{shelf}/volumes/{slug}"),
    )
}

fn volume_address(shelf: Option<&str>, slug: &str) -> String {
    shelf.map_or_else(|| slug.to_owned(), |shelf| format!("{shelf}/{slug}"))
}

/// A page of Markdown or other file found in a volume folder, as read by the
/// caller. Reading the disk is not this crate's business.
#[derive(Debug, Clone)]
pub struct PageSource {
    /// Path relative to the volume folder.
    pub path: LogicalPath,
    /// File contents with `\n` line endings.
    pub text: String,
}

/// Everything found in one volume folder.
#[derive(Debug, Clone)]
pub struct VolumeSource {
    /// Folder name of the volume.
    pub slug: String,
    /// Slug of the shelf folder that holds the volume, if any.
    pub shelf: Option<String>,
    /// Parsed `volume.toml`.
    pub manifest: VolumeManifest,
    /// Markdown files.
    pub pages: Vec<PageSource>,
    /// Other files (images, downloads), relative to the volume folder.
    pub files: Vec<LogicalPath>,
}

impl VolumeSource {
    /// Library path of the volume folder.
    pub fn folder(&self) -> String {
        volume_folder(self.shelf.as_deref(), &self.slug)
    }

    /// First URL segments of the volume, without slashes at the ends.
    pub fn address(&self) -> String {
        volume_address(self.shelf.as_deref(), &self.slug)
    }
}

/// Where a `[[term]]` points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermTarget {
    /// URL of the page, with `#anchor`.
    pub url: String,
    /// Heading text, as written.
    pub text: String,
}
