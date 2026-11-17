use std::fs;
use std::path::Path;

use anaquel_catalog::{LogicalPath, PageSource, VolumeSource};
use anaquel_diagnostics::{Code, Diagnostic, Report};
use anaquel_schema::{LibraryManifest, VolumeManifest};
use walkdir::WalkDir;

/// Largest Markdown file read, in bytes. Bounds memory on hostile input.
const MAX_PAGE_BYTES: u64 = 5 * 1024 * 1024;

/// Name of the folder that holds the volumes.
pub(crate) const VOLUMES_DIR: &str = "volumes";

pub(crate) struct Ingested {
    pub(crate) library: LibraryManifest,
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
    let volumes_dir = root.join(VOLUMES_DIR);
    let Ok(listing) = fs::read_dir(&volumes_dir) else {
        report.push(
            Diagnostic::error(Code::MANIFEST, "the library has no `volumes` folder")
                .with_help("put each book in its own folder under `volumes/`"),
        );
        return None;
    };
    let mut slugs: Vec<_> = listing
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .collect();
    slugs.sort_by_key(std::fs::DirEntry::file_name);
    for entry in slugs {
        let Some(slug) = entry.file_name().to_str().map(str::to_owned) else {
            report.push(Diagnostic::error(
                Code::IO,
                format!(
                    "volume folder name is not UTF-8: {}",
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
                    format!("volume folder `{slug}` cannot be a URL segment"),
                )
                .at(format!("{VOLUMES_DIR}/{slug}"))
                .with_help("use letters, digits, `-` and `_`"),
            );
            continue;
        }
        if let Some(volume) = read_volume(&entry.path(), &slug, report) {
            volumes.push(volume);
        }
    }
    if volumes.is_empty() && !report.has_errors() {
        report.push(
            Diagnostic::error(Code::MANIFEST, "the library has no volumes")
                .with_help("add a folder with a `volume.toml` under `volumes/`"),
        );
    }
    Some(Ingested { library, volumes })
}

fn is_url_segment(slug: &str) -> bool {
    !slug.is_empty()
        && slug
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}

fn read_volume(dir: &Path, slug: &str, report: &mut Report) -> Option<VolumeSource> {
    let manifest_path = dir.join("volume.toml");
    let manifest_name = format!("{VOLUMES_DIR}/{slug}/volume.toml");
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
    for item in WalkDir::new(dir)
        .follow_links(false)
        .sort_by_file_name()
        .min_depth(1)
    {
        let item = match item {
            Ok(item) => item,
            Err(error) => {
                report.push(Diagnostic::error(Code::IO, error.to_string()));
                continue;
            }
        };
        let relative = item.path().strip_prefix(dir).unwrap_or(item.path());
        let Some(relative_text) = relative.to_str() else {
            report.push(Diagnostic::error(
                Code::IO,
                format!("file name is not UTF-8: {}", item.path().display()),
            ));
            continue;
        };
        let relative_text = relative_text.replace('\\', "/");
        if relative_text.split('/').any(|part| part.starts_with('.'))
            || relative_text == "volume.toml"
        {
            continue;
        }
        if item.path_is_symlink() {
            report.push(
                Diagnostic::info(Code::IO, "symbolic links are not followed")
                    .at(format!("{VOLUMES_DIR}/{slug}/{relative_text}")),
            );
            continue;
        }
        if !item.file_type().is_file() {
            continue;
        }
        let logical = match LogicalPath::new(&relative_text) {
            Ok(path) => path,
            Err(reason) => {
                report.push(
                    Diagnostic::error(Code::IO, reason)
                        .at(format!("{VOLUMES_DIR}/{slug}/{relative_text}")),
                );
                continue;
            }
        };
        if let Some(bad) = relative_text.chars().find(|c| matches!(c, '#' | '%' | '?')) {
            report.push(
                Diagnostic::error(
                    Code::IO,
                    format!("`{bad}` in a file or folder name would end or change its address"),
                )
                .with_help("rename it; letters, digits, spaces and `- _ . ( ) , ;` are fine")
                .at(format!("{VOLUMES_DIR}/{slug}/{relative_text}")),
            );
            continue;
        }
        if relative_text.to_ascii_lowercase().ends_with(".md") {
            if let Some(text) = read_page(
                item.path(),
                &format!("{VOLUMES_DIR}/{slug}/{relative_text}"),
                report,
            ) {
                pages.push(PageSource {
                    path: logical,
                    text,
                });
            }
        } else {
            files.push(logical);
        }
    }
    Some(VolumeSource {
        slug: slug.to_owned(),
        manifest,
        pages,
        files,
    })
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
