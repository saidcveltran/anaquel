//! `anaquel relocate`: moves a page and leaves a redirect at its old address.
//!
//! The whole move is planned in memory and checked with a full build of the result; the files
//! are written only when that build has no error.

mod disk;
mod text;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use anaquel_catalog::{Catalog, Entry, LogicalPath, PageSource, VolumeSource, normalise_alias};
use anaquel_compose::headings_of;
use anaquel_diagnostics::{Code, Diagnostic, Report, Severity};
use anaquel_schema::{parse_page, url_segment_problem};

use crate::ingest::{self, Ingested};
use crate::{ENGINE_VERSION, analyse_sources};
use disk::{Change, Disk, RealDisk, apply};
use text::{RewriteScope, rewrite_links, set_aliases};

#[cfg(test)]
mod tests;

const MAX_PATH_CHARS: usize = 240;
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
}

/// Moves a page, leaves a redirect and rewrites the links that point at it.
pub fn relocate(options: &RelocateOptions) -> RelocateOutcome {
    relocate_on(options, &mut RealDisk)
}

fn relocate_on(options: &RelocateOptions, disk: &mut dyn Disk) -> RelocateOutcome {
    let mut report = Report::new();
    let mut plan = Vec::new();
    let applied = run(options, disk, &mut report, &mut plan);
    report.sort();
    RelocateOutcome {
        report,
        plan,
        applied,
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
    let text = clean_argument(raw);
    if let Err(reason) = LogicalPath::new(&text) {
        refuse(
            report,
            code,
            format!("`{raw}` is not a path inside the library: {reason}"),
            raw,
        );
        return None;
    }
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

fn windows_device(segment: &str) -> bool {
    let stem = segment.split('.').next().unwrap_or("").trim_end();
    let lower = stem.to_lowercase();
    matches!(lower.as_str(), "con" | "prn" | "aux" | "nul")
        || ["com", "lpt"].iter().any(|p| {
            lower
                .strip_prefix(p)
                .is_some_and(|n| n.len() == 1 && n.as_bytes()[0].is_ascii_digit())
        })
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

fn check_destinations(items: &[FileMove], ingested: &Ingested, report: &mut Report) -> bool {
    let volumes = &ingested.volumes;
    let leaving: HashSet<(usize, &str)> = items
        .iter()
        .map(|i| (i.from_volume, i.from_rel.as_str()))
        .collect();
    let mut ok = true;
    for item in items {
        let target = &volumes[item.to_volume];
        let new_lib = format!("{}/{}", folder_of(target), item.to_rel);
        let old_lib = format!(
            "{}/{}",
            folder_of(&volumes[item.from_volume]),
            item.from_rel
        );
        if new_lib == old_lib {
            refuse(
                report,
                Code::RELOCATE_TAKEN,
                format!("`{old_lib}` is already there"),
                &old_lib,
            );
            return false;
        }
        let text = volumes[item.from_volume]
            .pages
            .iter()
            .find(|p| p.path.as_str() == item.from_rel)
            .map(|p| p.text.as_str());
        let has_slug = item.page
            && text
                .and_then(|t| parse_page(t, &old_lib).ok())
                .is_some_and(|p| p.front.slug.is_some());
        ok &= check_names(item, has_slug, &new_lib, report);
        ok &= check_collisions(item, target, &leaving, &new_lib, report);
    }
    ok
}

fn check_names(item: &FileMove, has_slug: bool, new_lib: &str, report: &mut Report) -> bool {
    let mut ok = true;
    let segments: Vec<&str> = item.to_rel.split('/').collect();
    let last = segments.len() - 1;
    for (index, segment) in segments.iter().enumerate() {
        let in_address = index < last || (item.page && *segment != SECTION_FILE && !has_slug);
        let stem = if index == last && item.page {
            &segment[..segment.len() - 3]
        } else {
            segment
        };
        let problem = if in_address {
            url_segment_problem(stem).map(|why| format!("it {why}"))
        } else {
            None
        }
        .or_else(|| windows_device(segment).then(|| "it is a reserved name on Windows".to_owned()));
        if let Some(why) = problem {
            refuse(
                report,
                Code::RELOCATE_DESTINATION,
                format!("`{segment}` cannot be part of a page address: {why}"),
                new_lib,
            );
            ok = false;
        }
    }
    if new_lib.chars().count() > MAX_PATH_CHARS {
        refuse(
            report,
            Code::RELOCATE_DESTINATION,
            format!("the path is longer than {MAX_PATH_CHARS} characters"),
            new_lib,
        );
        ok = false;
    }
    ok
}

fn check_collisions(
    item: &FileMove,
    target: &VolumeSource,
    leaving: &HashSet<(usize, &str)>,
    new_lib: &str,
    report: &mut Report,
) -> bool {
    let mut ok = true;
    let mut taken: HashMap<String, &str> = HashMap::new();
    for path in target
        .pages
        .iter()
        .map(|p| p.path.as_str())
        .chain(target.files.iter().map(LogicalPath::as_str))
    {
        if !leaving.contains(&(item.to_volume, path)) {
            taken.insert(path.to_lowercase(), path);
        }
    }
    let parts: Vec<&str> = item.to_rel.split('/').collect();
    for end in 1..=parts.len() {
        let probe = parts[..end].join("/");
        let Some(existing) = taken.get(&probe.to_lowercase()) else {
            continue;
        };
        if end < parts.len() {
            refuse(
                report,
                Code::RELOCATE_TAKEN,
                format!("`{existing}` is a file, so it cannot also be a folder"),
                new_lib,
            );
        } else if *existing == probe {
            refuse(
                report,
                Code::RELOCATE_TAKEN,
                format!("`{new_lib}` already exists"),
                new_lib,
            );
        } else {
            refuse(
                report,
                Code::RELOCATE_DESTINATION,
                format!("`{new_lib}` and `{existing}` differ only by letter case"),
                new_lib,
            );
        }
        ok = false;
    }
    ok
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
    for (from_volume, item) in originals {
        let old_lib = format!("{}/{}", volumes[from_volume].folder(), item.from_rel);
        let source = &mut out[from_volume];
        if item.page {
            source.pages.retain(|p| p.path.as_str() != item.from_rel);
        } else {
            source.files.retain(|f| f.as_str() != item.from_rel);
        }
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

struct Address {
    old: String,
    new: String,
}

#[allow(
    clippy::too_many_lines,
    reason = "one pass over the plan; splitting it would scatter its state"
)]
fn run(
    options: &RelocateOptions,
    disk: &mut dyn Disk,
    report: &mut Report,
    plan: &mut Vec<String>,
) -> bool {
    let mut read = Report::new();
    let Some(ingested) = ingest::ingest(&options.root, ENGINE_VERSION, &mut read) else {
        report.extend(read);
        return false;
    };
    if read.has_errors() {
        report.extend(read);
        return false;
    }
    let Some(items) = locate(options, &ingested, report) else {
        return false;
    };
    if !check_destinations(&items, &ingested, report) {
        return false;
    }

    let mut before = Report::new();
    let base = analyse_sources(
        clone_ingested(&ingested, ingested.volumes.clone()),
        &mut before,
    );
    if before.has_errors() {
        push_errors(report, &before, "before moving anything");
        return false;
    }
    let (catalog, _) = Catalog::build(
        ingested.library.clone(),
        ingested.shelves.clone(),
        moved_sources(&ingested.volumes, &items, &HashMap::new()),
        &headings_of,
    );
    let moved_entries = entries_by_source(&catalog);
    let old_entries = entries_by_source(&base.catalog);

    let moved: HashMap<String, String> = items
        .iter()
        .map(|i| {
            (
                format!(
                    "{}/{}",
                    folder_of(&ingested.volumes[i.from_volume]),
                    i.from_rel
                ),
                format!("{}/{}", folder_of(&ingested.volumes[i.to_volume]), i.to_rel),
            )
        })
        .collect();

    let mut addresses: HashMap<String, Address> = HashMap::new();
    for item in items.iter().filter(|i| i.page) {
        let old_lib = format!(
            "{}/{}",
            folder_of(&ingested.volumes[item.from_volume]),
            item.from_rel
        );
        let new_lib = &moved[&old_lib];
        let (Some(old), Some(new)) = (
            old_entries.get(old_lib.as_str()),
            moved_entries.get(new_lib.as_str()),
        ) else {
            continue;
        };
        if !old.draft && old.url != new.url {
            addresses.insert(
                old_lib,
                Address {
                    old: old.url.clone(),
                    new: new.url.clone(),
                },
            );
        }
    }

    if !check_addresses(&addresses, &old_entries, report) {
        return false;
    }

    let mut texts: HashMap<String, String> = HashMap::new();
    let mut problems = false;
    let mut lines: Vec<String> = Vec::new();
    for entry in base.catalog.entries() {
        let old_lib = entry.source.as_str().to_owned();
        let new_lib = moved
            .get(&old_lib)
            .cloned()
            .unwrap_or_else(|| old_lib.clone());
        let Some(original) = ingested
            .volumes
            .iter()
            .flat_map(|v| v.pages.iter().map(move |p| (v, p)))
            .find(|(v, p)| format!("{}/{}", v.folder(), p.path.as_str()) == old_lib)
            .map(|(_, p)| p.text.clone())
        else {
            continue;
        };
        let context = RewriteScope {
            catalog: &base.catalog,
            entry: entry.id,
            moved: &moved,
            old_page: &old_lib,
            new_page: &new_lib,
        };
        let mut current = match rewrite_links(&original, &context) {
            Ok(done) => {
                if done.links > 0 {
                    lines.push(format!("rewrite {} link(s) in {new_lib}", done.links));
                }
                done.text
            }
            Err(found) => {
                for problem in found {
                    report.push(
                        Diagnostic::error(Code::RELOCATE_REWRITE, problem.message)
                            .at(old_lib.clone())
                            .on_line(problem.line)
                            .with_help("rewrite it by hand, or move the page after changing it"),
                    );
                }
                problems = true;
                continue;
            }
        };
        if let Some(address) = addresses.get(&old_lib) {
            let after = aliases_after(entry, address);
            match set_aliases(&current, &after) {
                Ok(text) => current = text,
                Err(message) => {
                    refuse(report, Code::RELOCATE_REWRITE, message, &old_lib);
                    problems = true;
                    continue;
                }
            }
        }
        if current != original {
            texts.insert(old_lib, current);
        }
    }
    if problems {
        return false;
    }

    let mut after = Report::new();
    let _ = analyse_sources(
        clone_ingested(&ingested, moved_sources(&ingested.volumes, &items, &texts)),
        &mut after,
    );
    if after.has_errors() {
        push_errors(report, &after, "after the move");
        return false;
    }

    for item in &items {
        let old_lib = format!(
            "{}/{}",
            folder_of(&ingested.volumes[item.from_volume]),
            item.from_rel
        );
        let new_lib = &moved[&old_lib];
        plan.push(format!(
            "move {}{old_lib} -> {new_lib}",
            if item.page { "" } else { "file " }
        ));
        if let Some(address) = addresses.get(&old_lib) {
            plan.push(format!("  {} redirects to {}", address.old, address.new));
            if let Some(entry) = old_entries.get(old_lib.as_str()) {
                for own in removed_aliases(entry, address) {
                    plan.push(format!("  alias {own} is dropped: it is the new address"));
                }
            }
        } else if item.page && old_entries.get(old_lib.as_str()).is_some_and(|e| e.draft) {
            plan.push("  a draft has no address, so no redirect".to_owned());
        } else if item.page {
            plan.push("  the address does not change, so no redirect".to_owned());
        }
    }
    plan.extend(lines);

    if options.dry_run {
        return false;
    }

    let mut changes: Vec<Change> = Vec::new();
    let mut emptied: Vec<PathBuf> = Vec::new();
    for item in &items {
        let old_lib = format!(
            "{}/{}",
            folder_of(&ingested.volumes[item.from_volume]),
            item.from_rel
        );
        let new_lib = &moved[&old_lib];
        changes.push(Change::Move {
            from: options.root.join(&old_lib),
            to: options.root.join(new_lib),
            text: texts.get(&old_lib).cloned().filter(|_| item.page),
        });
        let folder = folder_of(&ingested.volumes[item.from_volume]);
        let mut dir = item.from_rel.as_str();
        while let Some((parent, _)) = dir.rsplit_once('/') {
            emptied.push(options.root.join(format!("{folder}/{parent}")));
            dir = parent;
        }
    }
    emptied.sort();
    emptied.dedup();
    for (old_lib, text) in &texts {
        if !moved.contains_key(old_lib) {
            changes.push(Change::Rewrite {
                path: options.root.join(old_lib),
                text: text.clone(),
            });
        }
    }
    match apply(disk, &changes, &emptied) {
        Ok(()) => true,
        Err(failure) => {
            report.push(
                Diagnostic::error(
                    Code::IO,
                    format!("the move stopped: {}", failure.message),
                )
                .with_help(if failure.restored {
                    "every file is back as it was"
                } else {
                    "some files could not be put back; check the folder with `git status` or a backup"
                }),
            );
            false
        }
    }
}

fn push_errors(report: &mut Report, found: &Report, when: &str) {
    for item in found
        .items()
        .iter()
        .filter(|d| d.severity == Severity::Error)
    {
        let mut copy = item.clone();
        if copy.help.is_none() {
            copy.help = Some(format!("found {when}; nothing was written"));
        }
        report.push(copy);
    }
}

fn owner_map<'a>(entries: &'a HashMap<&'a str, &'a Entry>, skip: &str) -> HashMap<String, &'a str> {
    let mut owners = HashMap::new();
    for (source, entry) in entries {
        if *source == skip || entry.draft {
            continue;
        }
        for alias in &entry.aliases {
            if let Ok(normal) = normalise_alias(alias) {
                owners.insert(normal, *source);
            }
        }
    }
    owners
}

fn check_addresses(
    addresses: &HashMap<String, Address>,
    entries: &HashMap<&str, &Entry>,
    report: &mut Report,
) -> bool {
    let mut ok = true;
    let old_urls: HashMap<&str, &str> = addresses
        .iter()
        .map(|(source, a)| (a.old.as_str(), source.as_str()))
        .collect();
    for (source, address) in addresses {
        if let Some(other) = old_urls.get(address.new.as_str())
            && other != &source.as_str()
        {
            refuse(
                report,
                Code::RELOCATE_CHAIN,
                format!(
                    "{source} would take the address {} that `{other}` leaves in the same command",
                    address.new
                ),
                source,
            );
            ok = false;
            continue;
        }
        let owners = owner_map(entries, source);
        for url in [&address.new, &address.old] {
            if let Some(owner) = owners.get(url.as_str()) {
                refuse(
                    report,
                    Code::RELOCATE_TAKEN,
                    format!("{url} is an alias of `{owner}`"),
                    source,
                );
                ok = false;
            }
        }
    }
    ok
}

fn removed_aliases(entry: &Entry, address: &Address) -> Vec<String> {
    entry
        .aliases
        .iter()
        .filter(|a| normalise_alias(a).is_ok_and(|n| n == address.new))
        .cloned()
        .collect()
}

fn aliases_after(entry: &Entry, address: &Address) -> Vec<String> {
    let mut list: Vec<String> = entry
        .aliases
        .iter()
        .filter(|a| normalise_alias(a).is_ok_and(|n| n != address.new))
        .cloned()
        .collect();
    let present = list
        .iter()
        .any(|a| normalise_alias(a).is_ok_and(|n| n == address.old));
    if !present {
        list.push(address.old.clone());
    }
    list
}
