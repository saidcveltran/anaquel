use std::collections::{BTreeMap, HashMap, HashSet};

use anaquel_diagnostics::{Code, Diagnostic, Report};
use anaquel_schema::{LibraryManifest, parse_page, url_segment_problem};
use unicode_normalization::UnicodeNormalization;

use crate::model::{
    Entry, EntryId, EntryKind, Heading, PageSource, Shelf, ShelfSource, Volume, VolumeSource,
};
use crate::{Catalog, LogicalPath};

/// Lists the headings of a Markdown body, lines counted from 1 within the body.
pub type HeadingsOf<'a> = dyn Fn(&str) -> Vec<Heading> + Sync + 'a;

const SECTION_FILE: &str = "_index.md";

pub(crate) fn build(
    library: LibraryManifest,
    shelf_sources: Vec<ShelfSource>,
    mut sources: Vec<VolumeSource>,
    headings_of: &HeadingsOf<'_>,
) -> (Catalog, Report) {
    let mut report = Report::new();
    let mut shelves: Vec<Shelf> = shelf_sources
        .into_iter()
        .map(|s| Shelf {
            slug: s.slug,
            manifest: s.manifest,
        })
        .collect();
    shelves.sort_by(|a, b| {
        (a.manifest.weight, a.manifest.title.to_lowercase(), &a.slug).cmp(&(
            b.manifest.weight,
            b.manifest.title.to_lowercase(),
            &b.slug,
        ))
    });
    check_shelves(&shelves, &sources, &mut report);
    let rank = |shelf: Option<&str>| {
        shelf.map_or(0, |slug| {
            shelves
                .iter()
                .position(|s| s.slug == slug)
                .map_or(usize::MAX, |i| i + 1)
        })
    };
    sources.sort_by(|a, b| {
        (
            rank(a.shelf.as_deref()),
            a.manifest.weight,
            a.manifest.title.to_lowercase(),
            &a.slug,
        )
            .cmp(&(
                rank(b.shelf.as_deref()),
                b.manifest.weight,
                b.manifest.title.to_lowercase(),
                &b.slug,
            ))
    });

    let mut entries: Vec<Entry> = Vec::new();
    let mut volumes: Vec<Volume> = Vec::new();
    let mut files: HashMap<String, String> = HashMap::new();

    for (volume_index, source) in sources.into_iter().enumerate() {
        check_case_collisions(&source, &mut report);
        let (folder, address) = (source.folder(), source.address());
        for file in &source.files {
            let library_path = format!("{folder}/{}", file.as_str());
            files.insert(library_path, format!("/{address}/{}", file.as_str()));
        }
        for page in &source.pages {
            if let Some(entry) = read_entry(
                volume_index,
                (&folder, &address, &source.slug),
                page,
                headings_of,
                &mut report,
            ) {
                entries.push(entry);
            }
        }
        volumes.push(Volume {
            slug: source.slug,
            shelf: source.shelf,
            manifest: source.manifest,
            root: None,
            order: Vec::new(),
        });
    }

    entries.sort_by(|a, b| (a.volume, a.rel.as_str()).cmp(&(b.volume, b.rel.as_str())));
    for (index, entry) in entries.iter_mut().enumerate() {
        entry.id = EntryId(index);
    }

    let by_source: HashMap<String, EntryId> = entries
        .iter()
        .map(|e| (e.source.as_str().to_owned(), e.id))
        .collect();

    check_unique_urls(&entries, &mut report);
    link_tree(&mut entries, &mut volumes, &mut report);
    let lexicon = index_terms(&entries, &volumes, &mut report);
    let redirects = collect_redirects(&entries, &files, &mut report);

    report.sort();
    (
        Catalog {
            library,
            shelves,
            volumes,
            entries,
            by_source,
            files,
            lexicon,
            redirects,
        },
        report,
    )
}

fn check_shelves(shelves: &[Shelf], sources: &[VolumeSource], report: &mut Report) {
    for source in sources {
        match &source.shelf {
            Some(shelf) if !shelves.iter().any(|s| &s.slug == shelf) => report.push(
                Diagnostic::error(
                    Code::MANIFEST,
                    format!(
                        "volume `{}` names the shelf `{shelf}`, which does not exist",
                        source.slug
                    ),
                )
                .at(source.folder()),
            ),
            None => {
                if let Some(shelf) = shelves.iter().find(|s| s.slug == source.slug) {
                    report.push(
                        Diagnostic::error(
                            Code::DUPLICATE_URL,
                            format!(
                                "the volume `{}` and the shelf `{}` both become /{}/",
                                source.slug, shelf.slug, shelf.slug
                            ),
                        )
                        .at(source.folder())
                        .with_help("rename the volume folder or the shelf folder"),
                    );
                }
            }
            Some(_) => {}
        }
    }
}

fn check_case_collisions(source: &VolumeSource, report: &mut Report) {
    let mut seen: HashMap<String, &str> = HashMap::new();
    let all = source
        .pages
        .iter()
        .map(|p| p.path.as_str())
        .chain(source.files.iter().map(LogicalPath::as_str));
    for path in all {
        if let Some(first) = seen.insert(path.to_lowercase(), path) {
            report.push(
                Diagnostic::error(
                    Code::DUPLICATE_URL,
                    format!("`{path}` and `{first}` differ only by letter case"),
                )
                .at(format!("{}/{path}", source.folder()))
                .with_help("rename one of them; Windows and macOS cannot hold both"),
            );
        }
    }
}

fn read_entry(
    volume: usize,
    (folder, address, slug): (&str, &str, &str),
    page: &PageSource,
    headings_of: &HeadingsOf<'_>,
    report: &mut Report,
) -> Option<Entry> {
    let source_text = format!("{folder}/{}", page.path.as_str());
    let Ok(source) = LogicalPath::new(&source_text) else {
        report.push(
            Diagnostic::error(Code::IO, "path is not valid inside a library").at(source_text),
        );
        return None;
    };
    let parsed = match parse_page(&page.text, source.as_str()) {
        Ok(parsed) => parsed,
        Err(diagnostic) => {
            report.push(diagnostic);
            return None;
        }
    };
    for note in parsed.notes {
        report.push(note);
    }
    let mut headings = headings_of(&parsed.body);
    for heading in &mut headings {
        heading.line += parsed.body_line - 1;
    }
    let kind = if page.path.file_name() == SECTION_FILE {
        EntryKind::Section
    } else {
        EntryKind::Page
    };
    let stem = page.path.file_name().trim_end_matches(".md");
    if kind == EntryKind::Section && parsed.front.slug.is_some() {
        report.push(
            Diagnostic::warning(
                Code::FRONT_MATTER_KEY,
                "`slug` has no effect on `_index.md`; rename the folder",
            )
            .at(source.as_str()),
        );
    }
    let stem_in_address = (kind == EntryKind::Page && parsed.front.slug.is_none()).then_some(stem);
    let folders = page.path.dir().split('/').filter(|s| !s.is_empty());
    for segment in folders.chain(stem_in_address) {
        if let Some(why) = url_segment_problem(segment) {
            report.push(
                Diagnostic::error(
                    Code::IO,
                    format!("`{segment}` cannot be part of a page address: it {why}"),
                )
                .with_help(
                    "rename the file or folder to letters, digits and `- _ . ~ + @`; a page can also set `slug`",
                )
                .at(source.as_str()),
            );
            return None;
        }
    }
    let url = entry_url(
        address,
        page.path.dir(),
        stem,
        kind,
        parsed.front.slug.as_deref(),
    );
    let title = parsed
        .front
        .title
        .clone()
        .or_else(|| {
            headings
                .iter()
                .find(|h| h.level == 1)
                .map(|h| h.text.clone())
        })
        .unwrap_or_else(|| {
            humanise(if kind == EntryKind::Section {
                dir_name(page.path.dir(), slug)
            } else {
                stem
            })
        });
    Some(Entry {
        id: EntryId(0),
        volume,
        source,
        rel: page.path.clone(),
        url,
        kind,
        title,
        description: parsed.front.description,
        weight: parsed.front.weight.unwrap_or(0),
        draft: parsed.front.draft,
        aliases: parsed.front.aliases,
        headings,
        parent: None,
        children: Vec::new(),
        prev: None,
        next: None,
        body: parsed.body,
        body_line: parsed.body_line,
        hash: blake3::hash(page.text.as_bytes()).to_hex().as_str()[..16].to_owned(),
    })
}

fn dir_name<'a>(dir: &'a str, volume_slug: &'a str) -> &'a str {
    if dir.is_empty() {
        volume_slug
    } else {
        dir.rsplit('/').next().unwrap_or(dir)
    }
}

fn entry_url(volume: &str, dir: &str, stem: &str, kind: EntryKind, slug: Option<&str>) -> String {
    let mut url = format!("/{volume}/");
    if !dir.is_empty() {
        url.push_str(dir);
        url.push('/');
    }
    if kind == EntryKind::Page {
        url.push_str(slug.unwrap_or(stem));
        url.push('/');
    }
    url
}

fn humanise(stem: &str) -> String {
    let spaced = stem.replace(['-', '_'], " ");
    let mut chars = spaced.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

fn check_unique_urls(entries: &[Entry], report: &mut Report) {
    let mut seen: HashMap<&str, &Entry> = HashMap::new();
    for entry in entries {
        if let Some(first) = seen.insert(entry.url.as_str(), entry) {
            report.push(
                Diagnostic::error(
                    Code::DUPLICATE_URL,
                    format!(
                        "`{}` and `{}` both become {}",
                        entry.rel.as_str(),
                        first.rel.as_str(),
                        entry.url
                    ),
                )
                .at(entry.source.as_str()),
            );
        }
    }
}

/// Parents, children, display order and reading order.
fn link_tree(entries: &mut [Entry], volumes: &mut [Volume], report: &mut Report) {
    for (volume_index, volume) in volumes.iter_mut().enumerate() {
        let sections: HashMap<String, EntryId> = entries
            .iter()
            .filter(|e| e.volume == volume_index && e.kind == EntryKind::Section)
            .map(|e| (e.rel.dir().to_owned(), e.id))
            .collect();
        volume.root = sections.get("").copied();
        if volume.root.is_none() {
            report.push(
                Diagnostic::error(
                    Code::MANIFEST,
                    format!("volume `{}` has no `_index.md`", volume.slug),
                )
                .at(volume.folder())
                .with_help(
                    "create `_index.md` at the top of the volume: it is the volume's front page",
                ),
            );
            continue;
        }

        let mut children: BTreeMap<EntryId, Vec<EntryId>> = BTreeMap::new();
        let mut warned: HashSet<String> = HashSet::new();
        for entry in entries.iter_mut() {
            if entry.volume != volume_index {
                continue;
            }
            let (id, dir, kind) = (entry.id, entry.rel.dir().to_owned(), entry.kind);
            let start = match kind {
                EntryKind::Page => dir.clone(),
                EntryKind::Section => parent_dir(&dir),
            };
            if kind == EntryKind::Section && dir.is_empty() {
                continue;
            }
            let mut probe = start.clone();
            let parent = loop {
                if let Some(section) = sections.get(&probe) {
                    break *section;
                }
                if probe.is_empty() {
                    break volume.root.unwrap_or(id);
                }
                probe = parent_dir(&probe);
            };
            if kind == EntryKind::Page && probe != start && warned.insert(start.clone()) {
                report.push(
                    Diagnostic::warning(
                        Code::MANIFEST,
                        format!("folder `{start}` has no `_index.md`"),
                    )
                    .at(format!("{}/{start}", volume.folder()))
                    .with_help("add an `_index.md` so the folder has a page of its own"),
                );
            }
            entry.parent = Some(parent);
            children.entry(parent).or_default().push(id);
        }

        for list in children.values_mut() {
            list.sort_by(|a, b| {
                let (x, y) = (&entries[a.0], &entries[b.0]);
                (x.weight, x.title.to_lowercase(), x.rel.as_str()).cmp(&(
                    y.weight,
                    y.title.to_lowercase(),
                    y.rel.as_str(),
                ))
            });
        }
        for (parent, list) in &children {
            entries[parent.0].children.clone_from(list);
        }

        let mut order = Vec::new();
        if let Some(root) = volume.root {
            flatten(root, entries, &mut order);
        }
        order.retain(|id| !entries[id.0].draft);
        for (position, id) in order.iter().enumerate() {
            entries[id.0].prev = position.checked_sub(1).and_then(|p| order.get(p)).copied();
            entries[id.0].next = order.get(position + 1).copied();
        }
        volume.order = order;
    }
}

fn parent_dir(dir: &str) -> String {
    dir.rsplit_once('/')
        .map_or(String::new(), |(parent, _)| parent.to_owned())
}

fn flatten(id: EntryId, entries: &[Entry], out: &mut Vec<EntryId>) {
    out.push(id);
    for child in &entries[id.0].children {
        flatten(*child, entries, out);
    }
}

fn index_terms(
    entries: &[Entry],
    volumes: &[Volume],
    report: &mut Report,
) -> HashMap<(usize, String), (EntryId, usize)> {
    let mut lexicon: HashMap<(usize, String), (EntryId, usize)> = HashMap::new();
    for volume in volumes {
        for id in &volume.order {
            let entry = &entries[id.0];
            if !is_glossary(entry) {
                continue;
            }
            for (position, heading) in entry
                .headings
                .iter()
                .enumerate()
                .filter(|(_, h)| h.level == 3)
            {
                let key = (entry.volume, term_key(&heading.text));
                if let Some((first, _)) = lexicon.get(&key) {
                    report.push(
                        Diagnostic::error(
                            Code::DUPLICATE_TERM,
                            format!(
                                "term `{}` is also defined in `{}`",
                                heading.text,
                                entries[first.0].source.as_str()
                            ),
                        )
                        .at(entry.source.as_str())
                        .on_line(heading.line),
                    );
                } else {
                    lexicon.insert(key, (entry.id, position));
                }
            }
        }
    }
    lexicon
}

/// Only pages named `terms.md` or `glossary.md` define terms: a `###` heading
/// in ordinary prose is just a heading.
fn is_glossary(entry: &Entry) -> bool {
    let name = entry.source.as_str().rsplit('/').next().unwrap_or_default();
    matches!(name, "terms.md" | "glossary.md")
}

pub(crate) fn term_key(text: &str) -> String {
    text.trim().nfc().collect::<String>().to_lowercase()
}

fn collect_redirects(
    entries: &[Entry],
    files: &HashMap<String, String>,
    report: &mut Report,
) -> Vec<(String, String)> {
    let live: HashSet<&str> = entries
        .iter()
        .filter(|e| !e.draft)
        .map(|e| e.url.as_str())
        .collect();
    let static_urls: HashSet<&str> = files.values().map(String::as_str).collect();
    let mut seen: HashMap<String, &Entry> = HashMap::new();
    let mut redirects = Vec::new();
    for entry in entries.iter().filter(|e| !e.draft) {
        for alias in &entry.aliases {
            let normalised = match normalise_alias(alias) {
                Ok(a) => a,
                Err(message) => {
                    report.push(
                        Diagnostic::error(Code::FRONT_MATTER_KEY, message)
                            .at(entry.source.as_str()),
                    );
                    continue;
                }
            };
            if live.contains(normalised.as_str()) || static_urls.contains(normalised.as_str()) {
                report.push(
                    Diagnostic::error(
                        Code::DUPLICATE_URL,
                        format!("alias {normalised} is already a page"),
                    )
                    .at(entry.source.as_str()),
                );
            } else if let Some(other) = seen.insert(normalised.clone(), entry) {
                report.push(
                    Diagnostic::error(
                        Code::DUPLICATE_URL,
                        format!(
                            "alias {normalised} is also an alias of `{}`",
                            other.source.as_str()
                        ),
                    )
                    .at(entry.source.as_str()),
                );
            } else {
                redirects.push((normalised, entry.url.clone()));
            }
        }
    }
    redirects.sort();
    redirects
}

/// Turns an alias as written in a page into the address it redirects from.
///
/// # Errors
/// Returns a message when the alias does not start with `/` or is not a plain path.
pub fn normalise_alias(alias: &str) -> Result<String, String> {
    let bad = |why: &str| format!("alias `{alias}` {why}");
    if !alias.starts_with('/') {
        return Err(bad("must start with `/`"));
    }
    if alias.contains(['#', '?', '\\'])
        || alias.contains(char::is_whitespace)
        || alias.split('/').any(|s| s == "..")
        || alias.contains("//")
    {
        return Err(bad("must be a plain path"));
    }
    let nfc: String = alias.nfc().collect();
    for segment in nfc.split('/').skip(1).filter(|s| !s.is_empty()) {
        if let Some(why) = url_segment_problem(segment) {
            return Err(bad(&format!(
                "has the segment `{segment}` which {why}; use letters, digits and `- _ . ~ + @`"
            )));
        }
    }
    let last = nfc.rsplit('/').next().unwrap_or("");
    Ok(if last.contains('.') || nfc.ends_with('/') {
        nfc
    } else {
        format!("{nfc}/")
    })
}
