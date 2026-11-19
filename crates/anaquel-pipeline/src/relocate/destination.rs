use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anaquel_diagnostics::{Code, Report};
use anaquel_schema::{parse_page, url_segment_problem};

use super::disk::TEMPORARY_EXTRA;
use super::{FileMove, SECTION_FILE, folder_of, refuse};
use crate::ingest::Ingested;

const MAX_WINDOWS_PATH_UNITS: usize = 259;
const MAX_SEGMENT_BYTES: usize = 255;

/// Whether Windows reserves the name for a device, with any extension: `CON`, `PRN`, `AUX`, `NUL`,
/// `CONIN$`, `CONOUT$`, and `COM` and `LPT` followed by one digit or by a superscript one, two or
/// three.
pub(super) fn windows_device(segment: &str) -> bool {
    let stem = segment.split('.').next().unwrap_or("").trim_end();
    let lower = stem.to_lowercase();
    matches!(
        lower.as_str(),
        "con" | "prn" | "aux" | "nul" | "conin$" | "conout$"
    ) || ["com", "lpt"].iter().any(|p| {
        lower.strip_prefix(p).is_some_and(|rest| {
            let mut chars = rest.chars();
            matches!(
                (chars.next(), chars.next()),
                (Some('0'..='9' | '¹' | '²' | '³'), None)
            )
        })
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
    let leaving_lib: HashSet<String> = items
        .iter()
        .map(|i| format!("{}/{}", folder_of(&volumes[i.from_volume]), i.from_rel))
        .collect();
    let mut listings: Listings = HashMap::new();
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
        // The disk is asked, not the library's list of files: that list leaves out hidden files,
        // empty folders and a folder named like a page.
        ok &= on_disk(root, &new_lib, &leaving_lib, &mut listings, report);
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
    for segment in item.to_rel.split('/') {
        if segment.len() > MAX_SEGMENT_BYTES {
            refuse(
                report,
                Code::RELOCATE_DESTINATION,
                format!("`{segment}` is longer than {MAX_SEGMENT_BYTES} bytes"),
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

/// The names found in each folder already looked at, so a move of many files lists a folder once.
type Listings = HashMap<PathBuf, Vec<String>>;

fn names_in(folder: &Path) -> Vec<String> {
    fs::read_dir(folder)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// Walks the destination down from the root of the library and compares each segment with what
/// is really in the folder, whatever the library lists: an entry that differs only by letter case
/// is refused, a folder in the way of a file or the other way round is refused, and so is
/// anything, of any kind, already at the destination. A name that another file of the same move
/// leaves is free. Nothing below a segment that does not exist can be in the way.
fn on_disk(
    root: &Path,
    new_lib: &str,
    leaving: &HashSet<String>,
    listings: &mut Listings,
    report: &mut Report,
) -> bool {
    let segments: Vec<&str> = new_lib.split('/').collect();
    let mut folder = root.to_path_buf();
    let mut parent_lib = String::new();
    for (index, segment) in segments.iter().enumerate() {
        let last = index + 1 == segments.len();
        let lower = segment.to_lowercase();
        let names = listings
            .entry(folder.clone())
            .or_insert_with_key(|key| names_in(key));
        let lib_of = |name: &str| {
            if parent_lib.is_empty() {
                name.to_owned()
            } else {
                format!("{parent_lib}/{name}")
            }
        };
        let same_letters: Vec<&String> = names
            .iter()
            .filter(|name| name.to_lowercase() == lower)
            .collect();
        if same_letters.is_empty() {
            return true;
        }
        // The only entry allowed to differ by case is a file this very move takes away.
        if let Some(other) = same_letters
            .iter()
            .find(|name| name.as_str() != *segment && !(last && leaving.contains(&lib_of(name))))
        {
            refuse(
                report,
                Code::RELOCATE_DESTINATION,
                format!(
                    "`{}` and `{}` differ only by letter case, which a case-insensitive disk cannot tell apart",
                    lib_of(segment),
                    lib_of(other)
                ),
                new_lib,
            );
            return false;
        }
        let here = lib_of(segment);
        let exact = same_letters.iter().any(|name| name.as_str() == *segment);
        if !exact {
            return true;
        }
        let is_folder = fs::symlink_metadata(folder.join(segment)).is_ok_and(|m| m.is_dir());
        if last && !leaving.contains(&here) {
            refuse(
                report,
                Code::RELOCATE_TAKEN,
                format!(
                    "`{here}` already exists{}",
                    if is_folder { " as a folder" } else { "" }
                ),
                new_lib,
            );
            return false;
        }
        if !last && !is_folder {
            if leaving.contains(&here) {
                return true;
            }
            refuse(
                report,
                Code::RELOCATE_TAKEN,
                format!("`{here}` is a file, so it cannot also be a folder"),
                new_lib,
            );
            return false;
        }
        folder.push(segment);
        parent_lib = here;
    }
    true
}
