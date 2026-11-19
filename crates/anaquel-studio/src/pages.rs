//! The pages of a library, for the list the editor shows.
//!
//! The walk follows the rules of the build (`ingest` in `anaquel-pipeline`): it looks in
//! `volumes/` and in `shelves/NAME/volumes/`, takes a folder for a volume or a shelf only when
//! its name is a URL segment and its `volume.toml` or `shelf.toml` is valid, skips every name
//! that starts with a dot, does not descend into a link inside a volume, and takes a file for a
//! page when its name ends in `.md`, in any case. What the build skips is skipped here, with a
//! line in [`PageList::problems`] when the build reports it. A page the editor cannot open, for
//! example because a link leads to it, is listed with the reason instead of being left out.
//!
//! Every page is read to give its title and status, so a listing costs a read of the library.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anaquel_pipeline::{MAX_PAGE_BYTES, is_temporary_file_name};
use anaquel_schema::{ShelfManifest, VolumeManifest, parse_page};
use walkdir::WalkDir;

use crate::library_path::LibraryPath;
use crate::page_store::editable_text;

/// What the editor knows about a page without opening it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageStatus {
    /// The page builds into the site.
    Published,
    /// The front matter says `draft: true`, so the build reads the page and does not emit it.
    Draft,
    /// The front matter does not parse. The editor opens the page so that it can be fixed.
    Invalid(String),
    /// The editor cannot open the page, for the reason given.
    Unavailable(String),
}

/// One page of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageSummary {
    /// The path relative to the library, names separated by `/`.
    pub path: String,
    /// The shelf the page's volume belongs to, if the library has shelves.
    pub shelf: Option<String>,
    /// The volume folder.
    pub volume: String,
    /// The title in the front matter, when it sets one.
    pub title: Option<String>,
    /// Whether the page builds, is a draft, or cannot be used.
    pub status: PageStatus,
    /// Size of the file, in bytes.
    pub bytes: u64,
    /// Last change of the file, in seconds since 1970-01-01 UTC, when the system reports it.
    pub modified: Option<u64>,
}

/// The pages of a library, and what could not be examined while listing them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageList {
    /// The pages, sorted by shelf, volume and path.
    pub pages: Vec<PageSummary>,
    /// One line for each folder or file that could not be examined, each volume or shelf the build
    /// skips, and each temporary file that a move or a save left behind.
    pub problems: Vec<String>,
}

/// A volume folder: its shelf, its name and its place on disk.
struct VolumeFolder {
    shelf: Option<String>,
    name: String,
    path: PathBuf,
}

/// Lists the pages of the library whose folder is `root`.
///
/// A library with no `volumes` and no `shelves` folder has no pages; that is an empty list, not
/// an error.
#[must_use]
pub fn list_pages(root: &Path) -> PageList {
    let mut list = PageList::default();
    for volume in volume_folders(root, &mut list.problems) {
        collect_volume(root, &volume, &mut list);
    }
    list.pages
        .sort_by(|a, b| (&a.shelf, &a.volume, &a.path).cmp(&(&b.shelf, &b.volume, &b.path)));
    list.problems.sort();
    list
}

/// The volumes the build reads: loose ones, and those of every shelf it reads.
fn volume_folders(root: &Path, problems: &mut Vec<String>) -> Vec<VolumeFolder> {
    let mut found = Vec::new();
    for volume in readable_volumes(root, "volumes", problems) {
        found.push(VolumeFolder {
            shelf: None,
            name: volume.0,
            path: volume.1,
        });
    }
    for (shelf, shelf_dir) in subfolders(root, "shelves", problems) {
        let label = format!("shelves/{shelf}");
        if !manifest_is_valid(
            &shelf_dir.join("shelf.toml"),
            &label,
            "shelf.toml",
            problems,
        ) {
            continue;
        }
        for volume in readable_volumes(root, &format!("{label}/volumes"), problems) {
            found.push(VolumeFolder {
                shelf: Some(shelf.clone()),
                name: volume.0,
                path: volume.1,
            });
        }
    }
    found
}

/// The volume folders directly inside `label` (a folder of the library) whose `volume.toml` the
/// build accepts.
fn readable_volumes(
    root: &Path,
    label: &str,
    problems: &mut Vec<String>,
) -> Vec<(String, PathBuf)> {
    subfolders(root, label, problems)
        .into_iter()
        .filter(|(name, dir)| {
            let place = format!("{label}/{name}");
            manifest_is_valid(&dir.join("volume.toml"), &place, "volume.toml", problems)
        })
        .collect()
}

/// Whether the build reads the manifest at `file`; when it does not, says why in `problems`.
fn manifest_is_valid(file: &Path, place: &str, kind: &str, problems: &mut Vec<String>) -> bool {
    let name = format!("{place}/{kind}");
    let text = match fs::read_to_string(file) {
        Ok(text) => text,
        Err(error) => {
            problems.push(format!(
                "{name}: cannot be read ({error}), so the build skips {place}"
            ));
            return false;
        }
    };
    let parsed = if kind == "shelf.toml" {
        ShelfManifest::parse(&text, &name).err()
    } else {
        VolumeManifest::parse(&text, &name).err()
    };
    match parsed {
        None => true,
        Some(diagnostic) => {
            problems.push(format!(
                "{name}: {}, so the build skips {place}",
                diagnostic.message
            ));
            false
        }
    }
}

/// The folders directly inside the folder `label` of the library, as the build takes them: a link
/// to a folder counts (the build follows it), a name that starts with a dot does not, and a name
/// that is not a URL segment is reported and left out. A folder that is missing, or is a file, is
/// an empty list, as it is for the build.
fn subfolders(root: &Path, label: &str, problems: &mut Vec<String>) -> Vec<(String, PathBuf)> {
    let parent = root.join(label);
    if !parent.is_dir() {
        return Vec::new();
    }
    let entries = match fs::read_dir(&parent) {
        Ok(entries) => entries,
        Err(error) => {
            problems.push(format!("{label}: {error}"));
            return Vec::new();
        }
    };
    let mut found = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                problems.push(format!("{label}: {error}"));
                continue;
            }
        };
        if !entry.path().is_dir() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            problems.push(format!(
                "{label}: a folder name is not UTF-8, so the build skips it"
            ));
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        if !is_url_segment(&name) {
            problems.push(format!(
                "{label}/{name}: the name cannot be a URL segment, so the build skips it"
            ));
            continue;
        }
        found.push((name, entry.path()));
    }
    found.sort();
    found
}

/// The rule of the build for the name of a volume or a shelf folder.
fn is_url_segment(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}

fn collect_volume(root: &Path, volume: &VolumeFolder, list: &mut PageList) {
    for item in WalkDir::new(&volume.path)
        .follow_links(false)
        .sort_by_file_name()
        .min_depth(1)
    {
        let item = match item {
            Ok(item) => item,
            Err(error) => {
                list.problems.push(error.to_string());
                continue;
            }
        };
        let Ok(relative) = item.path().strip_prefix(root) else {
            continue;
        };
        let Some(relative) = relative.to_str().map(|text| text.replace('\\', "/")) else {
            list.problems.push(format!(
                "{}: a file name is not UTF-8 and is skipped",
                item.path().display()
            ));
            continue;
        };
        let leftover = item.file_type().is_file()
            && item
                .file_name()
                .to_str()
                .is_some_and(is_temporary_file_name);
        if leftover {
            list.problems.push(format!(
                "{relative}: a temporary file that an interrupted save or move left; it can be deleted when no anaquel program is running"
            ));
            continue;
        }
        if relative.split('/').any(|part| part.starts_with('.')) {
            continue;
        }
        if !item.file_type().is_file() || !relative.to_ascii_lowercase().ends_with(".md") {
            continue;
        }
        list.pages
            .push(summarise(root, volume, &relative, item.path()));
    }
}

fn summarise(root: &Path, volume: &VolumeFolder, relative: &str, file: &Path) -> PageSummary {
    let meta = fs::metadata(file).ok();
    let mut summary = PageSummary {
        path: relative.to_owned(),
        shelf: volume.shelf.clone(),
        volume: volume.name.clone(),
        title: None,
        status: PageStatus::Published,
        bytes: meta.as_ref().map_or(0, fs::Metadata::len),
        modified: meta
            .and_then(|meta| meta.modified().ok())
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|elapsed| elapsed.as_secs()),
    };
    if let Err(error) = LibraryPath::new(root, relative) {
        summary.status = PageStatus::Unavailable(error.to_string());
        return summary;
    }
    if summary.bytes > MAX_PAGE_BYTES {
        summary.status =
            PageStatus::Unavailable(format!("the page is larger than {MAX_PAGE_BYTES} bytes"));
        return summary;
    }
    let bytes = match fs::read(file) {
        Ok(bytes) => bytes,
        Err(error) => {
            summary.status = PageStatus::Unavailable(error.to_string());
            return summary;
        }
    };
    let Some(text) = editable_text(&bytes) else {
        summary.status = PageStatus::Unavailable(
            "the editor cannot open this file: it is not UTF-8 text, or it holds a NUL character, a lone carriage return or a leading byte order mark"
                .to_owned(),
        );
        return summary;
    };
    match parse_page(&text, relative) {
        Ok(parsed) => {
            summary.title = parsed.front.title;
            if parsed.front.draft {
                summary.status = PageStatus::Draft;
            }
        }
        Err(diagnostic) => summary.status = PageStatus::Invalid(diagnostic.message),
    }
    summary
}

#[cfg(test)]
mod tests;
