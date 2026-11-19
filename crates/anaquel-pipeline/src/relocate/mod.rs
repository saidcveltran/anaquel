//! `anaquel relocate`: moves a page and leaves a redirect at its old address.
//!
//! The whole move is planned in memory and checked with a full build of the result; the files
//! are written only when that build has no error.

mod destination;
mod disk;
mod plan;
mod rewrite;
mod text;

use std::collections::HashMap;
use std::path::PathBuf;

use anaquel_catalog::{Catalog, Entry, LogicalPath, PageSource, VolumeSource};
use anaquel_diagnostics::{Code, Diagnostic, Report};

use crate::ingest::Ingested;
use disk::{Disk, RealDisk};
pub use rewrite::{RewriteError, rewrite_page};

#[cfg(test)]
mod tests;

const SECTION_FILE: &str = "_index.md";

/// What to move and how.
#[derive(Debug, Clone)]
pub struct RelocateOptions {
    /// The library folder, the one with `anaquel.toml`.
    pub root: PathBuf,
    /// Page to move, relative to the library folder.
    pub from: String,
    /// New place of the page, relative to the library folder.
    pub to: String,
    /// Plan and check the move without writing anything.
    pub dry_run: bool,
}

/// What a relocation did, or would do.
#[derive(Debug)]
pub struct RelocateOutcome {
    /// Everything that stopped the move, sorted.
    pub report: Report,
    /// The steps of the move, one line each.
    pub plan: Vec<String>,
    /// Whether the files were changed.
    pub applied: bool,
    /// Whether a failed write left files that could not be put back.
    pub incomplete: bool,
}

/// Moves a page, leaves a redirect and rewrites the links that point at it.
pub fn relocate(options: &RelocateOptions) -> RelocateOutcome {
    relocate_on(options, &mut RealDisk)
}

fn relocate_on(options: &RelocateOptions, disk: &mut dyn Disk) -> RelocateOutcome {
    let mut report = Report::new();
    let mut plan = Vec::new();
    let finish = plan::run(options, disk, &mut report, &mut plan);
    report.sort();
    RelocateOutcome {
        report,
        plan,
        applied: finish.applied,
        incomplete: finish.incomplete,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Place {
    shelf: Option<String>,
    volume: String,
    rel: String,
}

impl Place {
    fn parse(path: &str) -> Option<Self> {
        let parts: Vec<&str> = path.split('/').collect();
        match parts.as_slice() {
            ["volumes", volume, rest @ ..] if !rest.is_empty() => Some(Self {
                shelf: None,
                volume: (*volume).to_owned(),
                rel: rest.join("/"),
            }),
            ["shelves", shelf, "volumes", volume, rest @ ..] if !rest.is_empty() => Some(Self {
                shelf: Some((*shelf).to_owned()),
                volume: (*volume).to_owned(),
                rel: rest.join("/"),
            }),
            _ => None,
        }
    }
}

struct FileMove {
    from_volume: usize,
    to_volume: usize,
    from_rel: String,
    to_rel: String,
    page: bool,
}

fn clean_argument(raw: &str) -> String {
    let mut text = raw.replace('\\', "/");
    while let Some(rest) = text.strip_prefix("./") {
        text = rest.to_owned();
    }
    text
}

fn is_markdown(path: &str) -> bool {
    path.to_ascii_lowercase().ends_with(".md")
}

fn refuse(report: &mut Report, code: Code, message: String, path: &str) {
    report.push(Diagnostic::error(code, message).at(path));
}

fn parse_argument(raw: &str, code: Code, report: &mut Report) -> Option<Place> {
    let text = match LogicalPath::new(&clean_argument(raw)) {
        Ok(path) => path.as_str().to_owned(),
        Err(reason) => {
            refuse(
                report,
                code,
                format!("`{raw}` is not a path inside the library: {reason}"),
                raw,
            );
            return None;
        }
    };
    let Some(place) = Place::parse(&text) else {
        refuse(
            report,
            code,
            format!("`{raw}` is not under `volumes/NAME/` or `shelves/SHELF/volumes/NAME/`"),
            raw,
        );
        return None;
    };
    if !is_markdown(&place.rel) {
        refuse(report, code, format!("`{raw}` is not a Markdown file"), raw);
        return None;
    }
    Some(place)
}

fn find_volume(volumes: &[VolumeSource], place: &Place) -> Option<usize> {
    volumes
        .iter()
        .position(|v| v.shelf == place.shelf && v.slug == place.volume)
}

fn folder_of(volume: &VolumeSource) -> String {
    volume.folder()
}

fn locate(
    options: &RelocateOptions,
    ingested: &Ingested,
    report: &mut Report,
) -> Option<Vec<FileMove>> {
    let from = parse_argument(&options.from, Code::RELOCATE_SOURCE, report);
    let to = parse_argument(&options.to, Code::RELOCATE_DESTINATION, report);
    let (from, to) = (from?, to?);
    let volumes = &ingested.volumes;
    let Some(from_volume) = find_volume(volumes, &from) else {
        refuse(
            report,
            Code::RELOCATE_SOURCE,
            format!("the volume of `{}` does not exist", options.from),
            &options.from,
        );
        return None;
    };
    if !volumes[from_volume]
        .pages
        .iter()
        .any(|p| p.path.as_str() == from.rel)
    {
        refuse(
            report,
            Code::RELOCATE_SOURCE,
            format!("`{}` does not exist", options.from),
            &options.from,
        );
        return None;
    }
    let Some(to_volume) = find_volume(volumes, &to) else {
        let what = match &to.shelf {
            Some(shelf) if !ingested.shelves.iter().any(|s| &s.slug == shelf) => {
                format!("the shelf `{shelf}` does not exist")
            }
            _ => format!("the volume `{}` does not exist", to.volume),
        };
        refuse(
            report,
            Code::RELOCATE_DESTINATION,
            format!("{what}; relocate does not create volumes or shelves"),
            &options.to,
        );
        return None;
    };

    let from_dir = from.rel.rsplit_once('/').map_or("", |(d, _)| d);
    let from_name = from.rel.rsplit('/').next().unwrap_or("");
    let to_dir = to.rel.rsplit_once('/').map_or("", |(d, _)| d);
    let to_name = to.rel.rsplit('/').next().unwrap_or("");
    if from_name != SECTION_FILE || from_dir.is_empty() {
        return Some(vec![FileMove {
            from_volume,
            to_volume,
            from_rel: from.rel,
            to_rel: to.rel,
            page: true,
        }]);
    }
    if to_name != SECTION_FILE || to_dir.is_empty() {
        refuse(
            report,
            Code::RELOCATE_DESTINATION,
            format!(
                "`{SECTION_FILE}` moves its whole folder, so the destination must be `FOLDER/{SECTION_FILE}`"
            ),
            &options.to,
        );
        return None;
    }
    let prefix = format!("{from_dir}/");
    if from_volume == to_volume && to_dir != from_dir && to_dir.starts_with(&prefix) {
        refuse(
            report,
            Code::RELOCATE_DESTINATION,
            "a folder cannot move inside itself".to_owned(),
            &options.to,
        );
        return None;
    }
    Some(folder_items(
        &volumes[from_volume],
        (from_volume, to_volume),
        &prefix,
        to_dir,
    ))
}

fn folder_items(
    volume: &VolumeSource,
    (from_volume, to_volume): (usize, usize),
    prefix: &str,
    to_dir: &str,
) -> Vec<FileMove> {
    let rebase = |rel: &str| format!("{to_dir}/{}", &rel[prefix.len()..]);
    let pages = volume
        .pages
        .iter()
        .map(|p| p.path.as_str())
        .filter(|p| p.starts_with(prefix))
        .map(|p| (p, true));
    let files = volume
        .files
        .iter()
        .map(LogicalPath::as_str)
        .filter(|f| f.starts_with(prefix))
        .map(|f| (f, false));
    pages
        .chain(files)
        .map(|(path, page)| FileMove {
            from_volume,
            to_volume,
            from_rel: path.to_owned(),
            to_rel: rebase(path),
            page,
        })
        .collect()
}

fn moved_sources(
    volumes: &[VolumeSource],
    items: &[FileMove],
    texts: &HashMap<String, String>,
) -> Vec<VolumeSource> {
    let mut out: Vec<VolumeSource> = volumes.to_vec();
    let originals: Vec<(usize, &FileMove)> = items.iter().map(|i| (i.from_volume, i)).collect();
    for volume in &mut out {
        let folder = volume.folder();
        for page in &mut volume.pages {
            if let Some(text) = texts.get(&format!("{folder}/{}", page.path.as_str())) {
                page.text.clone_from(text);
            }
        }
    }
    for (from_volume, item) in &originals {
        let source = &mut out[*from_volume];
        if item.page {
            source.pages.retain(|p| p.path.as_str() != item.from_rel);
        } else {
            source.files.retain(|f| f.as_str() != item.from_rel);
        }
    }
    for (from_volume, item) in originals {
        let old_lib = format!("{}/{}", volumes[from_volume].folder(), item.from_rel);
        let Ok(path) = LogicalPath::new(&item.to_rel) else {
            continue;
        };
        let text = volumes[from_volume]
            .pages
            .iter()
            .find(|p| p.path.as_str() == item.from_rel)
            .map(|p| {
                texts
                    .get(&old_lib)
                    .cloned()
                    .unwrap_or_else(|| p.text.clone())
            });
        let target = &mut out[item.to_volume];
        match text {
            Some(text) if item.page => target.pages.push(PageSource { path, text }),
            _ => target.files.push(path),
        }
    }
    out
}

fn clone_ingested(ingested: &Ingested, volumes: Vec<VolumeSource>) -> Ingested {
    Ingested {
        library: ingested.library.clone(),
        shelves: ingested.shelves.clone(),
        volumes,
    }
}

fn entries_by_source(catalog: &Catalog) -> HashMap<&str, &Entry> {
    catalog
        .entries()
        .iter()
        .map(|e| (e.source.as_str(), e))
        .collect()
}
