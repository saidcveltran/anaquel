use std::fs;
use std::path::{Path, PathBuf};

use anaquel_catalog::{LogicalPath, PageSource, ShelfSource, VolumeSource};
use anaquel_diagnostics::{Code, Diagnostic, Report};
use anaquel_schema::{LibraryManifest, ShelfManifest, VolumeManifest};
use rayon::prelude::*;
use walkdir::WalkDir;

/// Largest Markdown file read, in bytes. Bounds memory on hostile input.
pub const MAX_PAGE_BYTES: u64 = 5 * 1024 * 1024;

/// Name of the folder that holds the volumes.
pub(crate) const VOLUMES_DIR: &str = "volumes";

/// Name of the folder that holds the shelves.
pub(crate) const SHELVES_DIR: &str = "shelves";

pub(crate) struct Ingested {
    pub(crate) library: LibraryManifest,
    pub(crate) shelves: Vec<ShelfSource>,
    pub(crate) volumes: Vec<VolumeSource>,
}

fn io_error(path: &Path, error: &std::io::Error) -> Diagnostic {
    Diagnostic::error(Code::IO, format!("{}: {error}", path.display()))
}

/// Reads a library folder into memory: the manifest, every volume, every
/// Markdown page (line endings made `\n`) and the list of other files.
///
/// Returns `None` for the library when `anaquel.toml` is missing or invalid;
/// the reason is in the report.
pub(crate) fn ingest(root: &Path, engine: &str, report: &mut Report) -> Option<Ingested> {
    let manifest_path = root.join("anaquel.toml");
    let text = match fs::read_to_string(&manifest_path) {
        Ok(text) => text,
        Err(error) => {
            report.push(
                Diagnostic::error(Code::MANIFEST, format!("cannot read anaquel.toml: {error}"))
                    .with_help(
                        "run the command inside a library folder, or create one with `anaquel new`",
                    ),
            );
            return None;
        }
    };
    let library = match LibraryManifest::parse(&text, engine) {
        Ok(library) => library,
        Err(diagnostic) => {
            report.push(diagnostic);
            return None;
        }
    };

    let mut volumes = Vec::new();
    let mut shelves = Vec::new();
    let volumes_dir = root.join(VOLUMES_DIR);
    let has_loose = volumes_dir.is_dir();
    if has_loose {
        volumes.extend(read_volumes(&volumes_dir, None, report));
    }
    let shelves_dir = root.join(SHELVES_DIR);
    if shelves_dir.is_dir() {
        for (slug, dir) in folders_of(&shelves_dir, SHELVES_DIR, report) {
            if let Some(shelf) = read_shelf(&dir, &slug, report) {
                volumes.extend(read_volumes(&dir.join(VOLUMES_DIR), Some(&slug), report));
                shelves.push(shelf);
            }
        }
    }
    if !has_loose && shelves.is_empty() {
        report.push(
            Diagnostic::error(Code::MANIFEST, "the library has no `volumes` folder")
                .with_help("put each book in its own folder under `volumes/`"),
        );
        return None;
    }
    if volumes.is_empty() && !report.has_errors() {
        report.push(
            Diagnostic::error(Code::MANIFEST, "the library has no volumes")
                .with_help("add a folder with a `volume.toml` under `volumes/`"),
        );
    }
    Some(Ingested {
        library,
        shelves,
        volumes,
    })
}

fn folders_of(dir: &Path, label: &str, report: &mut Report) -> Vec<(String, std::path::PathBuf)> {
    let Ok(listing) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<_> = listing
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .collect();
    found.sort_by_key(std::fs::DirEntry::file_name);
    let mut folders = Vec::new();
    for entry in found {
        let Some(slug) = entry.file_name().to_str().map(str::to_owned) else {
            report.push(Diagnostic::error(
                Code::IO,
                format!(
                    "folder name is not UTF-8: {}",
                    entry.file_name().to_string_lossy()
                ),
            ));
            continue;
        };
        if slug.starts_with('.') {
            continue;
        }
        if !is_url_segment(&slug) {
            report.push(
                Diagnostic::error(
                    Code::MANIFEST,
                    format!("folder `{slug}` cannot be a URL segment"),
                )
                .at(format!("{label}/{slug}"))
                .with_help("use letters, digits, `-` and `_`"),
            );
            continue;
        }
        folders.push((slug, entry.path()));
    }
    folders
}

fn read_volumes(dir: &Path, shelf: Option<&str>, report: &mut Report) -> Vec<VolumeSource> {
    let label = shelf.map_or_else(
        || VOLUMES_DIR.to_owned(),
        |shelf| format!("{SHELVES_DIR}/{shelf}/{VOLUMES_DIR}"),
    );
    folders_of(dir, &label, report)
        .into_iter()
        .filter_map(|(slug, path)| read_volume(&path, &slug, shelf, report))
        .collect()
}

fn read_shelf(dir: &Path, slug: &str, report: &mut Report) -> Option<ShelfSource> {
    let manifest_name = format!("{SHELVES_DIR}/{slug}/shelf.toml");
    let text = match fs::read_to_string(dir.join("shelf.toml")) {
        Ok(text) => text,
        Err(error) => {
            report.push(
                Diagnostic::error(Code::MANIFEST, format!("cannot read shelf.toml: {error}"))
                    .at(manifest_name),
            );
            return None;
        }
    };
    match ShelfManifest::parse(&text, &manifest_name) {
        Ok(manifest) => Some(ShelfSource {
            slug: slug.to_owned(),
            manifest,
        }),
        Err(diagnostic) => {
            report.push(diagnostic);
            None
        }
    }
}

fn is_url_segment(slug: &str) -> bool {
    !slug.is_empty()
        && slug
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}

fn read_volume(
    dir: &Path,
    slug: &str,
    shelf: Option<&str>,
    report: &mut Report,
) -> Option<VolumeSource> {
    let folder = shelf.map_or_else(
        || format!("{VOLUMES_DIR}/{slug}"),
        |shelf| format!("{SHELVES_DIR}/{shelf}/{VOLUMES_DIR}/{slug}"),
    );
    let manifest_path = dir.join("volume.toml");
    let manifest_name = format!("{folder}/volume.toml");
    let text = match fs::read_to_string(&manifest_path) {
        Ok(text) => text,
        Err(error) => {
            report.push(
                Diagnostic::error(Code::MANIFEST, format!("cannot read volume.toml: {error}"))
                    .at(manifest_name),
            );
            return None;
        }
    };
    let manifest = match VolumeManifest::parse(&text, &manifest_name) {
        Ok(manifest) => manifest,
        Err(diagnostic) => {
            report.push(diagnostic);
            return None;
        }
    };

    let mut pages = Vec::new();
    let mut files = Vec::new();
    // The pages are read on all threads; the walk order decides where everything lands.
    let read: Vec<Read> = walk_volume(dir, &folder)
        .into_par_iter()
        .map(Found::read)
        .collect();
    for item in read {
        match item {
            Read::Note(note) => report.push(note),
            Read::File(path) => files.push(path),
            Read::Page {
                path,
                text,
                report: page_report,
            } => {
                report.extend(page_report);
                if let Some(text) = text {
                    pages.push(PageSource { path, text });
                }
            }
        }
    }
    Some(VolumeSource {
        slug: slug.to_owned(),
        shelf: shelf.map(str::to_owned),
        manifest,
        pages,
        files,
    })
}

/// One thing the walk of a volume found, in the order the walk met it.
enum Found {
    /// A problem with a name, a link or a walk step; nothing is left to read.
    Note(Diagnostic),
    /// A file that is not a page.
    File(LogicalPath),
    /// A Markdown file still to be read from `disk`; `name` is how reports call it.
    Page {
        path: LogicalPath,
        disk: PathBuf,
        name: String,
    },
}

/// What a [`Found`] came to once read.
enum Read {
    Note(Diagnostic),
    File(LogicalPath),
    /// A page, its text when it could be read, and the problems met reading it.
    Page {
        path: LogicalPath,
        text: Option<String>,
        report: Report,
    },
}

impl Found {
    fn read(self) -> Read {
        match self {
            Self::Note(note) => Read::Note(note),
            Self::File(path) => Read::File(path),
            Self::Page { path, disk, name } => {
                let mut report = Report::new();
                let text = read_page(&disk, &name, &mut report);
                Read::Page { path, text, report }
            }
        }
    }
}

/// Lists what a volume folder holds, in file-name order, without reading any page.
///
/// `folder` is the volume folder as the library names it, for the reports.
fn walk_volume(dir: &Path, folder: &str) -> Vec<Found> {
    let mut found = Vec::new();
    for item in WalkDir::new(dir)
        .follow_links(false)
        .sort_by_file_name()
        .min_depth(1)
    {
        let item = match item {
            Ok(item) => item,
            Err(error) => {
                found.push(Found::Note(Diagnostic::error(Code::IO, error.to_string())));
                continue;
            }
        };
        let relative = item.path().strip_prefix(dir).unwrap_or(item.path());
        let Some(relative_text) = relative.to_str() else {
            found.push(Found::Note(Diagnostic::error(
                Code::IO,
                format!("file name is not UTF-8: {}", item.path().display()),
            )));
            continue;
        };
        let relative_text = relative_text.replace('\\', "/");
        if relative_text.split('/').any(|part| part.starts_with('.'))
            || relative_text == "volume.toml"
        {
            continue;
        }
        if item.path_is_symlink() {
            found.push(Found::Note(
                Diagnostic::info(Code::IO, "symbolic links are not followed")
                    .at(format!("{folder}/{relative_text}")),
            ));
            continue;
        }
        if !item.file_type().is_file() {
            continue;
        }
        let logical = match LogicalPath::new(&relative_text) {
            Ok(path) => path,
            Err(reason) => {
                found.push(Found::Note(
                    Diagnostic::error(Code::IO, reason).at(format!("{folder}/{relative_text}")),
                ));
                continue;
            }
        };
        if let Some(bad) = relative_text.chars().find(|c| matches!(c, '#' | '%' | '?')) {
            found.push(Found::Note(
                Diagnostic::error(
                    Code::IO,
                    format!("`{bad}` in a file or folder name would end or change its address"),
                )
                .with_help("rename it; letters, digits, spaces and `- _ . ( ) , ;` are fine")
                .at(format!("{folder}/{relative_text}")),
            ));
            continue;
        }
        if relative_text.to_ascii_lowercase().ends_with(".md") {
            found.push(Found::Page {
                path: logical,
                disk: item.into_path(),
                name: format!("{folder}/{relative_text}"),
            });
        } else {
            found.push(Found::File(logical));
        }
    }
    found
}

fn read_page(path: &Path, name: &str, report: &mut Report) -> Option<String> {
    match fs::metadata(path) {
        Ok(meta) if meta.len() > MAX_PAGE_BYTES => {
            report.push(Diagnostic::error(Code::IO, "page is larger than 5 MiB").at(name));
            return None;
        }
        Err(error) => {
            report.push(io_error(path, &error));
            return None;
        }
        Ok(_) => {}
    }
    match fs::read_to_string(path) {
        Ok(text) => {
            let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
            Some(text.replace("\r\n", "\n"))
        }
        Err(error) => {
            report.push(
                Diagnostic::error(
                    Code::IO,
                    format!("cannot read the page as UTF-8 text: {error}"),
                )
                .at(name),
            );
            None
        }
    }
}
