//! The pages of a library, for the list the editor shows.
//!
//! The walk follows the rules the build follows: it looks in `volumes/` and in
//! `shelves/NAME/volumes/`, skips every name that starts with a dot, does not follow links, and
//! takes a file for a page when its name ends in `.md`, in any case. A page the editor cannot
//! open is listed with the reason instead of being left out.

use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

use anaquel_schema::parse_page;
use walkdir::WalkDir;

use crate::library_path::LibraryPath;
use crate::page_store::{MAX_PAGE_BYTES, library_text};

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
    /// One line for each folder or file that could not be examined.
    pub problems: Vec<String>,
}

/// Lists the pages of the library whose folder is `root`.
///
/// A library with no `volumes` and no `shelves` folder has no pages; that is an empty list, not
/// an error.
#[must_use]
pub fn list_pages(root: &Path) -> PageList {
    let mut list = PageList::default();
    for (shelf, volume, folder) in volume_folders(root, &mut list.problems) {
        collect_volume(root, shelf.as_deref(), &volume, &folder, &mut list);
    }
    list.pages
        .sort_by(|a, b| (&a.shelf, &a.volume, &a.path).cmp(&(&b.shelf, &b.volume, &b.path)));
    list
}

/// Every volume folder: its shelf, its name and its place on disk.
fn volume_folders(
    root: &Path,
    problems: &mut Vec<String>,
) -> Vec<(Option<String>, String, std::path::PathBuf)> {
    let mut found = Vec::new();
    for name in folder_names(&root.join("volumes"), problems) {
        found.push((None, name.clone(), root.join("volumes").join(name)));
    }
    let shelves = root.join("shelves");
    for shelf in folder_names(&shelves, problems) {
        let volumes = shelves.join(&shelf).join("volumes");
        for name in folder_names(&volumes, problems) {
            found.push((Some(shelf.clone()), name.clone(), volumes.join(name)));
        }
    }
    found
}

/// The names of the real folders directly inside `parent`, skipping dot names and links.
fn folder_names(parent: &Path, problems: &mut Vec<String>) -> Vec<String> {
    let entries = match fs::read_dir(parent) {
        Ok(entries) => entries,
        // A library does not have to have the folder.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            problems.push(format!("{}: {error}", parent.display()));
            return Vec::new();
        }
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                problems.push(format!("{}: {error}", parent.display()));
                continue;
            }
        };
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            problems.push(format!(
                "{}: a folder name is not UTF-8 and is skipped",
                parent.display()
            ));
            continue;
        };
        let is_real_folder = entry
            .file_type()
            .is_ok_and(|kind| kind.is_dir() && !kind.is_symlink());
        if is_real_folder && !name.starts_with('.') {
            names.push(name);
        }
    }
    names.sort();
    names
}

fn collect_volume(
    root: &Path,
    shelf: Option<&str>,
    volume: &str,
    folder: &Path,
    list: &mut PageList,
) {
    for item in WalkDir::new(folder)
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
        if relative.split('/').any(|part| part.starts_with('.')) {
            continue;
        }
        if !item.file_type().is_file() || !relative.to_ascii_lowercase().ends_with(".md") {
            continue;
        }
        list.pages
            .push(summarise(root, shelf, volume, &relative, item.path()));
    }
}

fn summarise(
    root: &Path,
    shelf: Option<&str>,
    volume: &str,
    relative: &str,
    file: &Path,
) -> PageSummary {
    let meta = fs::metadata(file).ok();
    let mut summary = PageSummary {
        path: relative.to_owned(),
        shelf: shelf.map(str::to_owned),
        volume: volume.to_owned(),
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
    let text = match fs::read(file) {
        Ok(bytes) => library_text(&bytes),
        Err(error) => {
            summary.status = PageStatus::Unavailable(error.to_string());
            return summary;
        }
    };
    let Some(text) = text else {
        summary.status = PageStatus::Unavailable("the file is not UTF-8 text".to_owned());
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
