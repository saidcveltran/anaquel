use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use anaquel_catalog::{LogicalPath, VolumeSource};
use anaquel_diagnostics::{Code, Report};
use anaquel_schema::{parse_page, url_segment_problem};

use super::disk::TEMPORARY_EXTRA;
use super::{FileMove, SECTION_FILE, folder_of, refuse};
use crate::ingest::Ingested;

const MAX_WINDOWS_PATH_UNITS: usize = 259;
const MAX_SEGMENT_BYTES: usize = 255;

pub(super) fn windows_device(segment: &str) -> bool {
    let stem = segment.split('.').next().unwrap_or("").trim_end();
    let lower = stem.to_lowercase();
    matches!(lower.as_str(), "con" | "prn" | "aux" | "nul")
        || ["com", "lpt"].iter().any(|p| {
            lower
                .strip_prefix(p)
                .is_some_and(|n| n.len() == 1 && n.as_bytes()[0].is_ascii_digit())
        })
}

pub(super) fn file_name_problem(segment: &str) -> Option<String> {
    if segment.starts_with('.') {
        return Some("it starts with a dot, so the library would skip it".to_owned());
    }
    if segment.ends_with([' ', '.']) {
        return Some("it ends with a space or a dot, which Windows drops".to_owned());
    }
    segment
        .chars()
        .find(|c| c.is_control() || "<>:\"|?*\\/#%".contains(*c))
        .map(|c| format!("it contains `{}`", c.escape_default()))
}

pub(super) fn check(
    items: &[FileMove],
    ingested: &Ingested,
    root: &Path,
    report: &mut Report,
) -> bool {
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
        let has_slug = item.page
            && volumes[item.from_volume]
                .pages
                .iter()
                .find(|p| p.path.as_str() == item.from_rel)
                .and_then(|p| parse_page(&p.text, &old_lib).ok())
                .is_some_and(|p| p.front.slug.is_some());
        ok &= names(item, has_slug, &new_lib, report);
        ok &= length(item, root, &new_lib, report);
        ok &= symbolic_links(root, &new_lib, report);
        ok &= collisions(item, target, &leaving, &new_lib, report);
    }
    ok
}

fn names(item: &FileMove, has_slug: bool, new_lib: &str, report: &mut Report) -> bool {
    let mut ok = true;
    let segments: Vec<&str> = item.to_rel.split('/').collect();
    let last = segments.len() - 1;
    for (index, segment) in segments.iter().enumerate() {
        let is_name = index == last;
        let own_name = is_name && !item.page;
        let in_address = !is_name || (*segment != SECTION_FILE && !has_slug);
        let stem = if is_name && item.page {
            &segment[..segment.len() - 3]
        } else {
            segment
        };
        let problem = if own_name {
            None
        } else {
            file_name_problem(segment)
        }
        .or_else(|| {
            if in_address && !own_name {
                url_segment_problem(stem).map(|why| format!("it {why}"))
            } else {
                None
            }
        })
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
    ok
}

fn length(item: &FileMove, root: &Path, new_lib: &str, report: &mut Report) -> bool {
    let mut ok = true;
    let last = item.to_rel.split('/').count() - 1;
    for (index, segment) in item.to_rel.split('/').enumerate() {
        let extra = if index == last { TEMPORARY_EXTRA } else { 0 };
        if segment.len() + extra > MAX_SEGMENT_BYTES {
            refuse(
                report,
                Code::RELOCATE_DESTINATION,
                format!(
                    "`{segment}` is longer than {MAX_SEGMENT_BYTES} bytes once the temporary name is counted"
                ),
                new_lib,
            );
            ok = false;
        }
    }
    let absolute = std::path::absolute(root).unwrap_or_else(|_| root.to_path_buf());
    let units = absolute
        .join(new_lib)
        .to_string_lossy()
        .encode_utf16()
        .count()
        + TEMPORARY_EXTRA;
    if units > MAX_WINDOWS_PATH_UNITS {
        refuse(
            report,
            Code::RELOCATE_DESTINATION,
            format!(
                "the full path of the destination is {units} characters with the temporary name; Windows stops at {MAX_WINDOWS_PATH_UNITS}"
            ),
            new_lib,
        );
        ok = false;
    }
    ok
}

fn symbolic_links(root: &Path, new_lib: &str, report: &mut Report) -> bool {
    let mut current = root.to_path_buf();
    for segment in new_lib.split('/') {
        current.push(segment);
        let linked = fs::symlink_metadata(&current).is_ok_and(|m| m.file_type().is_symlink());
        if linked {
            refuse(
                report,
                Code::RELOCATE_DESTINATION,
                format!("`{segment}` is a symbolic link, and a move would write through it"),
                new_lib,
            );
            return false;
        }
    }
    true
}

fn collisions(
    item: &FileMove,
    target: &VolumeSource,
    leaving: &HashSet<(usize, &str)>,
    new_lib: &str,
    report: &mut Report,
) -> bool {
    let mut ok = true;
    let all: Vec<&str> = target
        .pages
        .iter()
        .map(|p| p.path.as_str())
        .chain(target.files.iter().map(LogicalPath::as_str))
        .collect();
    let mut taken: HashMap<String, &str> = HashMap::new();
    let mut folders: HashMap<String, String> = HashMap::new();
    for path in &all {
        if !leaving.contains(&(item.to_volume, *path)) {
            taken.insert(path.to_lowercase(), path);
        }
        let parts: Vec<&str> = path.split('/').collect();
        for end in 1..parts.len() {
            let folder = parts[..end].join("/");
            folders.insert(folder.to_lowercase(), folder);
        }
    }
    let parts: Vec<&str> = item.to_rel.split('/').collect();
    for end in 1..=parts.len() {
        let probe = parts[..end].join("/");
        if end < parts.len()
            && let Some(existing) = folders.get(&probe.to_lowercase())
            && *existing != probe
        {
            refuse(
                report,
                Code::RELOCATE_DESTINATION,
                format!(
                    "the folder `{probe}` and `{existing}` differ only by letter case, which a case-insensitive disk cannot tell apart"
                ),
                new_lib,
            );
            ok = false;
            continue;
        }
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
