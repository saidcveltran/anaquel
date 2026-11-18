use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use anaquel_catalog::{Catalog, Entry, normalise_alias};
use anaquel_compose::headings_of;
use anaquel_diagnostics::{Code, Diagnostic, Report, Severity};

use super::destination;
use super::disk::{Change, Disk, apply, mixed_line_endings};
use super::text::{RewriteScope, rewrite_links, set_aliases};
use super::{
    FileMove, RelocateOptions, clone_ingested, entries_by_source, folder_of, locate, moved_sources,
    refuse,
};
use crate::ingest::{self, Ingested};
use crate::{Analysis, ENGINE_VERSION, analyse_sources};

#[derive(Default)]
pub(super) struct Finish {
    pub(super) applied: bool,
    pub(super) incomplete: bool,
}

pub(super) struct Address {
    pub(super) old: String,
    pub(super) new: String,
}

struct Pages {
    texts: HashMap<String, String>,
    lines: Vec<String>,
}

fn lib_path(ingested: &Ingested, volume: usize, rel: &str) -> String {
    format!("{}/{}", folder_of(&ingested.volumes[volume]), rel)
}

pub(super) fn run(
    options: &RelocateOptions,
    disk: &mut dyn Disk,
    report: &mut Report,
    plan: &mut Vec<String>,
) -> Finish {
    let mut read = Report::new();
    let Some(ingested) = ingest::ingest(&options.root, ENGINE_VERSION, &mut read) else {
        report.extend(read);
        return Finish::default();
    };
    if read.has_errors() {
        report.extend(read);
        return Finish::default();
    }
    let Some(items) = locate(options, &ingested, report) else {
        return Finish::default();
    };
    if !destination::check(&items, &ingested, &options.root, report) {
        return Finish::default();
    }

    let mut before = Report::new();
    let base = analyse_sources(
        clone_ingested(&ingested, ingested.volumes.clone()),
        &mut before,
    );
    if before.has_errors() {
        push_errors(report, &before, "before moving anything");
        return Finish::default();
    }
    let moved: HashMap<String, String> = items
        .iter()
        .map(|i| {
            (
                lib_path(&ingested, i.from_volume, &i.from_rel),
                lib_path(&ingested, i.to_volume, &i.to_rel),
            )
        })
        .collect();
    let Some(addresses) = find_addresses(&items, &ingested, &base, &moved, report) else {
        return Finish::default();
    };
    let old_entries = entries_by_source(&base.catalog);
    if !check_addresses(&addresses, &old_entries, report) {
        return Finish::default();
    }
    let Some(pages) = rewrite_pages(&ingested, &base, &moved, &addresses, report) else {
        return Finish::default();
    };
    if !check_endings(options, disk, &pages.texts, report) {
        return Finish::default();
    }
    if !check_result(&ingested, &items, &base, &pages.texts, report) {
        return Finish::default();
    }

    describe(plan, &items, &ingested, &moved, &addresses, &old_entries);
    plan.extend(pages.lines);
    if options.dry_run {
        return Finish::default();
    }
    write(
        options,
        disk,
        (&ingested, &items),
        &moved,
        &pages.texts,
        report,
    )
}

fn find_addresses(
    items: &[FileMove],
    ingested: &Ingested,
    base: &Analysis,
    moved: &HashMap<String, String>,
    report: &mut Report,
) -> Option<HashMap<String, Address>> {
    let (catalog, _) = Catalog::build(
        ingested.library.clone(),
        ingested.shelves.clone(),
        moved_sources(&ingested.volumes, items, &HashMap::new()),
        &headings_of,
    );
    let new_entries = entries_by_source(&catalog);
    let old_entries = entries_by_source(&base.catalog);
    let mut addresses = HashMap::new();
    for item in items.iter().filter(|i| i.page) {
        let old_lib = lib_path(ingested, item.from_volume, &item.from_rel);
        let new_lib = &moved[&old_lib];
        let (Some(old), Some(new)) = (
            old_entries.get(old_lib.as_str()),
            new_entries.get(new_lib.as_str()),
        ) else {
            report.push(Diagnostic::error(
                Code::IO,
                format!("`{old_lib}` could not be matched with `{new_lib}` after the move"),
            ));
            return None;
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
    Some(addresses)
}

fn rewrite_pages(
    ingested: &Ingested,
    base: &Analysis,
    moved: &HashMap<String, String>,
    addresses: &HashMap<String, Address>,
    report: &mut Report,
) -> Option<Pages> {
    let originals: HashMap<String, &str> = ingested
        .volumes
        .iter()
        .flat_map(|v| {
            v.pages.iter().map(move |p| {
                (
                    format!("{}/{}", v.folder(), p.path.as_str()),
                    p.text.as_str(),
                )
            })
        })
        .collect();
    let mut pages = Pages {
        texts: HashMap::new(),
        lines: Vec::new(),
    };
    let mut stopped = false;
    for entry in base.catalog.entries() {
        let old_lib = entry.source.as_str().to_owned();
        let new_lib = moved
            .get(&old_lib)
            .cloned()
            .unwrap_or_else(|| old_lib.clone());
        let Some(original) = originals.get(&old_lib) else {
            continue;
        };
        let scope = RewriteScope {
            catalog: &base.catalog,
            entry: entry.id,
            moved,
            old_page: &old_lib,
            new_page: &new_lib,
        };
        let mut current = match rewrite_links(original, &scope) {
            Ok(done) => {
                if done.links > 0 {
                    pages
                        .lines
                        .push(format!("rewrite {} link(s) in {new_lib}", done.links));
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
                stopped = true;
                continue;
            }
        };
        if let Some(address) = addresses.get(&old_lib) {
            match set_aliases(&current, &aliases_after(entry, address)) {
                Ok(text) => current = text,
                Err(message) => {
                    report.push(
                        Diagnostic::error(Code::RELOCATE_REWRITE, message)
                            .at(old_lib.clone())
                            .with_help(
                                "write the aliases as a plain list without comments, or add the alias by hand and move the page after",
                            ),
                    );
                    stopped = true;
                    continue;
                }
            }
        }
        if current != *original {
            pages.texts.insert(old_lib, current);
        }
    }
    (!stopped).then_some(pages)
}

fn check_endings(
    options: &RelocateOptions,
    disk: &dyn Disk,
    texts: &HashMap<String, String>,
    report: &mut Report,
) -> bool {
    let mut ok = true;
    let mut names: Vec<&String> = texts.keys().collect();
    names.sort();
    for name in names {
        let Ok(bytes) = disk.read(&options.root.join(name)) else {
            continue;
        };
        if mixed_line_endings(&bytes) {
            report.push(
                Diagnostic::error(
                    Code::RELOCATE_REWRITE,
                    "the file mixes `\\r\\n` and `\\n` line endings, and writing it back would change them",
                )
                .at(name.clone())
                .with_help("make the line endings the same in the file, then move it"),
            );
            ok = false;
        }
    }
    ok
}

pub(super) fn lost_addresses(before: &Catalog, after: &Catalog) -> Vec<String> {
    let addresses = |catalog: &Catalog| -> BTreeSet<String> {
        catalog
            .entries()
            .iter()
            .filter(|e| !e.draft)
            .map(|e| e.url.clone())
            .chain(catalog.redirects().iter().map(|(from, _)| from.clone()))
            .collect()
    };
    let kept = addresses(after);
    addresses(before)
        .into_iter()
        .filter(|a| !kept.contains(a))
        .collect()
}

fn check_result(
    ingested: &Ingested,
    items: &[FileMove],
    base: &Analysis,
    texts: &HashMap<String, String>,
    report: &mut Report,
) -> bool {
    let mut after = Report::new();
    let result = analyse_sources(
        clone_ingested(ingested, moved_sources(&ingested.volumes, items, texts)),
        &mut after,
    );
    if after.has_errors() {
        push_errors(report, &after, "after the move");
        return false;
    }
    let lost = lost_addresses(&base.catalog, &result.catalog);
    if lost.is_empty() {
        return true;
    }
    for address in lost {
        report.push(
            Diagnostic::error(
                Code::LOST_URL,
                format!("the address {address} would lead nowhere after the move"),
            )
            .with_help("nothing was written"),
        );
    }
    false
}

fn describe(
    plan: &mut Vec<String>,
    items: &[FileMove],
    ingested: &Ingested,
    moved: &HashMap<String, String>,
    addresses: &HashMap<String, Address>,
    old_entries: &HashMap<&str, &Entry>,
) {
    for item in items {
        let old_lib = lib_path(ingested, item.from_volume, &item.from_rel);
        plan.push(format!(
            "move {}{old_lib} -> {}",
            if item.page { "" } else { "file " },
            moved[&old_lib]
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
    let statics = items.iter().filter(|i| !i.page).count();
    if statics > 0 {
        plan.push(format!(
            "{statics} file(s) change address and leave no redirect"
        ));
    }
}

fn write(
    options: &RelocateOptions,
    disk: &mut dyn Disk,
    (ingested, items): (&Ingested, &[FileMove]),
    moved: &HashMap<String, String>,
    texts: &HashMap<String, String>,
    report: &mut Report,
) -> Finish {
    let mut changes: Vec<Change> = Vec::new();
    let mut emptied: Vec<PathBuf> = Vec::new();
    for item in items {
        let old_lib = lib_path(ingested, item.from_volume, &item.from_rel);
        changes.push(Change::Move {
            from: options.root.join(&old_lib),
            to: options.root.join(&moved[&old_lib]),
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
    let mut rewrites: Vec<&String> = texts.keys().filter(|k| !moved.contains_key(*k)).collect();
    rewrites.sort();
    for old_lib in rewrites {
        changes.push(Change::Rewrite {
            path: options.root.join(old_lib),
            text: texts[old_lib].clone(),
        });
    }
    match apply(disk, &changes, &emptied) {
        Ok(()) => Finish {
            applied: true,
            incomplete: false,
        },
        Err(failure) => {
            report.push(
                Diagnostic::error(
                    Code::IO,
                    format!("the move stopped: {}", failure.message),
                )
                .with_help(if failure.restored {
                    "every file is back as it was"
                } else {
                    "some files could not be put back; look for files named `.NAME.anaquel-tmp` and check the folder against your version control or a backup"
                }),
            );
            Finish {
                applied: false,
                incomplete: !failure.restored,
            }
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

pub(super) fn check_addresses(
    addresses: &HashMap<String, Address>,
    entries: &HashMap<&str, &Entry>,
    report: &mut Report,
) -> bool {
    let mut ok = true;
    let old_urls: HashMap<&str, &str> = addresses
        .iter()
        .map(|(source, a)| (a.old.as_str(), source.as_str()))
        .collect();
    let moving: HashSet<&str> = addresses.keys().map(String::as_str).collect();
    for (source, address) in addresses {
        if address.old.to_lowercase() == address.new.to_lowercase() {
            refuse(
                report,
                Code::RELOCATE_DESTINATION,
                format!(
                    "the address would change only by letter case ({} to {}), and a case-insensitive disk cannot hold both",
                    address.old, address.new
                ),
                source,
            );
            ok = false;
            continue;
        }
        if let Some(other) = old_urls.get(address.new.as_str())
            && other != &source.as_str()
            && moving.contains(other)
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
