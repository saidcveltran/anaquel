use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use anaquel_diagnostics::{Code, Diagnostic};

use crate::concurrent_write::{
    Job, plan_lanes, refuse_file_folder_conflicts, write_lanes, writer_count,
};
use crate::ledger::{LEDGER_PATH, Ledger};

/// One file of the output, addressed by a `/`-separated path under `dist`.
#[derive(Debug, Clone)]
pub struct OutputFile {
    /// Path relative to the output folder.
    pub path: String,
    /// Contents.
    pub bytes: Vec<u8>,
}

/// What a write did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WriteStats {
    /// Files created or changed.
    pub written: usize,
    /// Files left alone because they were already right.
    pub unchanged: usize,
    /// Files of the previous build that no longer exist.
    pub removed: usize,
}

fn io_error(path: &Path, error: &std::io::Error) -> Diagnostic {
    Diagnostic::error(Code::IO, format!("{}: {error}", path.display()))
}

/// The ending of the temporary name a file is written under before it is renamed.
const TEMPORARY_SUFFIX: &str = ".anaquel-tmp";

/// Resolves an output path under `dist`, refusing anything that could leave it and any file name
/// that ends like a temporary one: the name of `x`'s temporary file must be free for `x` alone.
fn target(dist: &Path, relative: &str) -> Result<PathBuf, Diagnostic> {
    let candidate = Path::new(relative);
    let safe = !relative.is_empty()
        && candidate
            .components()
            .all(|c| matches!(c, Component::Normal(_)));
    if !safe {
        return Err(Diagnostic::error(
            Code::IO,
            format!("refusing to write outside the output folder: `{relative}`"),
        ));
    }
    let reserved = candidate.file_name().is_some_and(|name| {
        name.to_string_lossy()
            .to_lowercase()
            .ends_with(TEMPORARY_SUFFIX)
    });
    if reserved {
        return Err(Diagnostic::error(
            Code::IO,
            format!(
                "refusing to write `{relative}`: names ending in `{TEMPORARY_SUFFIX}` are reserved"
            ),
        ));
    }
    Ok(dist.join(candidate))
}

/// Writes `files` under `dist`, removes what `previous` produced and `files`
/// no longer does, and returns the ledger fields for what was written.
///
/// The caller fills `urls` and `redirects` of the returned ledger and stores
/// it with [`write_ledger`].
///
/// Every path of `files` is checked before the first file is written, so a set with an unsafe
/// or reserved path, or with a path that is both a file and a folder, writes nothing. The files
/// that changed are then written by up to eight threads; paths that are the same, or that
/// Windows would store as one file, are written in their original order. When a build fails,
/// every file that could be written has been, and the ledger of the previous build is left as
/// it was. A path that appears more than once is written again at each later appearance: the
/// last content wins, as it did when files were written one by one.
///
/// # Errors
/// Returns a diagnostic for the first unsafe or reserved path, in order; otherwise for the
/// first file that is also a folder; otherwise for the first file, in order, that cannot be
/// written. A path of `previous` that is no longer safe is reported after the writes.
pub fn write_outputs(
    dist: &Path,
    files: &[OutputFile],
    previous: Option<&Ledger>,
) -> Result<(Ledger, WriteStats), Diagnostic> {
    let mut stats = WriteStats::default();
    let mut hashes = BTreeMap::new();
    let mut jobs = Vec::new();
    let mut seen = HashSet::new();
    for file in files {
        let path = target(dist, &file.path)?;
        let hash = blake3::hash(&file.bytes).to_hex().as_str()[..16].to_owned();
        // The disk is not read again after an earlier file of the set with this path is
        // written, so a repeated path is always written: its last content must win.
        let repeated = !seen.insert(file.path.as_str());
        let already_right = !repeated
            && previous.is_some_and(|p| p.files.get(&file.path) == Some(&hash))
            && fs::metadata(&path).is_ok_and(|m| m.len() == file.bytes.len() as u64);
        if already_right {
            stats.unchanged += 1;
        } else {
            jobs.push(Job { path, file });
        }
        hashes.insert(file.path.clone(), hash);
    }
    refuse_file_folder_conflicts(files)?;
    let lanes = plan_lanes(&jobs);
    write_lanes(&jobs, &lanes, writer_count(lanes.len()))?;
    stats.written = jobs.len();
    if let Some(previous) = previous {
        for old in previous.files.keys().filter(|p| !hashes.contains_key(*p)) {
            let path = target(dist, old)?;
            if fs::remove_file(&path).is_ok() {
                stats.removed += 1;
                prune_empty_dirs(dist, &path);
            }
        }
    }
    Ok((
        Ledger {
            schema: Ledger::SCHEMA,
            files: hashes,
            ..Ledger::default()
        },
        stats,
    ))
}

/// Stores `ledger` for the next build.
///
/// # Errors
/// Returns a diagnostic when the file cannot be written.
pub fn write_ledger(dist: &Path, ledger: &Ledger) -> Result<(), Diagnostic> {
    write_atomically(&dist.join(LEDGER_PATH), ledger.to_json()?.as_bytes())
}

/// Writes `bytes` to `path` through `path.anaquel-tmp` and a rename, creating the folders above
/// it. When the rename fails the temporary file is removed, so a failed write leaves no stray.
pub(crate) fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), Diagnostic> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| io_error(parent, &e))?;
    }
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(TEMPORARY_SUFFIX);
    let temporary = PathBuf::from(temporary);
    fs::write(&temporary, bytes).map_err(|e| io_error(&temporary, &e))?;
    fs::rename(&temporary, path).map_err(|e| {
        // Best effort: the rename error is the one worth reporting.
        let _ = fs::remove_file(&temporary);
        io_error(path, &e)
    })
}

fn prune_empty_dirs(dist: &Path, removed_file: &Path) {
    let mut dir = removed_file.parent();
    while let Some(current) = dir {
        if current == dist || fs::remove_dir(current).is_err() {
            break;
        }
        dir = current.parent();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("anaquel-emit-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn file(path: &str, text: &str) -> OutputFile {
        OutputFile {
            path: path.to_owned(),
            bytes: text.as_bytes().to_vec(),
        }
    }

    #[test]
    fn writes_then_skips_unchanged_then_removes_stale() {
        let dist = scratch("cycle");
        let (ledger, stats) = write_outputs(
            &dist,
            &[file("a/index.html", "one"), file("b.txt", "two")],
            None,
        )
        .unwrap();
        assert_eq!(
            stats,
            WriteStats {
                written: 2,
                unchanged: 0,
                removed: 0
            }
        );
        write_ledger(&dist, &ledger).unwrap();
        let previous = Ledger::load(&dist).unwrap();

        let (ledger, stats) = write_outputs(
            &dist,
            &[file("a/index.html", "one changed"), file("b.txt", "two")],
            Some(&previous),
        )
        .unwrap();
        assert_eq!(
            stats,
            WriteStats {
                written: 1,
                unchanged: 1,
                removed: 0
            }
        );
        assert_eq!(
            fs::read_to_string(dist.join("a/index.html")).unwrap(),
            "one changed"
        );

        let (_, stats) = write_outputs(&dist, &[file("b.txt", "two")], Some(&ledger)).unwrap();
        assert_eq!(stats.removed, 1);
        assert!(!dist.join("a").exists(), "empty folder is pruned");
        assert!(fs::read_dir(&dist).unwrap().all(|e| {
            !e.unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".anaquel-tmp")
        }));
        fs::remove_dir_all(&dist).unwrap();
    }

    #[test]
    fn refuses_paths_that_leave_the_output_folder() {
        let dist = scratch("escape");
        for bad in ["../x", "/x", "a/../../x", ""] {
            assert!(
                write_outputs(&dist, &[file(bad, "x")], None).is_err(),
                "{bad:?}"
            );
        }
        fs::remove_dir_all(&dist).unwrap();
    }

    #[test]
    fn an_unsafe_path_late_in_the_set_writes_nothing() {
        let dist = scratch("late-unsafe");
        let files: Vec<OutputFile> = (0..100)
            .map(|n| file(&format!("page-{n}/index.html"), "x"))
            .chain([file("../escape.txt", "x")])
            .collect();
        assert!(write_outputs(&dist, &files, None).is_err());
        assert_eq!(
            fs::read_dir(&dist).unwrap().count(),
            0,
            "nothing was written"
        );
        fs::remove_dir_all(&dist).unwrap();
    }

    #[test]
    fn a_file_that_is_also_a_folder_writes_nothing() {
        let dist = scratch("file-and-folder");
        let files = [file("ok.txt", "x"), file("a", "1"), file("a/b", "2")];
        let error = write_outputs(&dist, &files, None).unwrap_err();
        assert!(
            error.message.contains("cannot be a folder"),
            "{}",
            error.message
        );
        assert_eq!(
            fs::read_dir(&dist).unwrap().count(),
            0,
            "nothing was written"
        );
        fs::remove_dir_all(&dist).unwrap();
    }

    #[test]
    fn a_large_set_is_written_once_and_then_found_unchanged() {
        let dist = scratch("large");
        let files: Vec<OutputFile> = (0..500)
            .map(|n| file(&format!("page-{n}/index.html"), &format!("page {n}")))
            .collect();
        let (ledger, stats) = write_outputs(&dist, &files, None).unwrap();
        assert_eq!(
            stats,
            WriteStats {
                written: 500,
                unchanged: 0,
                removed: 0
            }
        );
        let (_, again) = write_outputs(&dist, &files, Some(&ledger)).unwrap();
        assert_eq!(
            again,
            WriteStats {
                written: 0,
                unchanged: 500,
                removed: 0
            }
        );
        for output in &files {
            assert_eq!(fs::read(dist.join(&output.path)).unwrap(), output.bytes);
        }
        fs::remove_dir_all(&dist).unwrap();
    }

    #[test]
    fn a_failed_build_leaves_the_files_that_could_be_written_and_reports_the_first_failure() {
        let dist = scratch("failed-build");
        fs::create_dir_all(dist.join("page-7/index.html")).unwrap();
        fs::create_dir_all(dist.join("page-90/index.html")).unwrap();
        let files: Vec<OutputFile> = (0..200)
            .map(|n| file(&format!("page-{n}/index.html"), "x"))
            .collect();
        let error = write_outputs(&dist, &files, None).unwrap_err();
        assert!(error.message.contains("page-7"), "{}", error.message);
        assert!(dist.join("page-199/index.html").is_file());
        fs::remove_dir_all(&dist).unwrap();
    }

    #[test]
    fn a_repeated_path_ends_with_its_last_content_even_when_an_earlier_one_matches_the_disk() {
        let dist = scratch("repeated-path");
        let (ledger, _) = write_outputs(&dist, &[file("p.txt", "X")], None).unwrap();
        for files in [
            [file("p.txt", "YY"), file("p.txt", "X")],
            [file("p.txt", "X"), file("p.txt", "YY")],
        ] {
            let last = files[1].bytes.clone();
            let (new_ledger, _) = write_outputs(&dist, &files, Some(&ledger)).unwrap();
            assert_eq!(fs::read(dist.join("p.txt")).unwrap(), last);
            assert_eq!(
                new_ledger.files["p.txt"],
                blake3::hash(&last).to_hex().as_str()[..16]
            );
        }
        fs::remove_dir_all(&dist).unwrap();
    }

    #[test]
    fn names_ending_like_a_temporary_file_are_refused_before_anything_is_written() {
        let dist = scratch("reserved-suffix");
        for bad in ["x.anaquel-tmp", "dir/Y.ANAQUEL-TMP"] {
            let files = [file("ok.txt", "1"), file(bad, "2")];
            let error = write_outputs(&dist, &files, None).unwrap_err();
            assert!(error.message.contains("reserved"), "{}", error.message);
        }
        assert_eq!(
            fs::read_dir(&dist).unwrap().count(),
            0,
            "nothing was written"
        );
        write_outputs(&dist, &[file("x.anaquel-tmp.html", "ok")], None).unwrap();
        fs::remove_dir_all(&dist).unwrap();
    }

    #[test]
    fn a_deleted_file_is_rewritten_even_if_the_ledger_says_it_exists() {
        let dist = scratch("deleted");
        let (ledger, _) = write_outputs(&dist, &[file("x.txt", "hello")], None).unwrap();
        fs::remove_file(dist.join("x.txt")).unwrap();
        let (_, stats) = write_outputs(&dist, &[file("x.txt", "hello")], Some(&ledger)).unwrap();
        assert_eq!(stats.written, 1);
        fs::remove_dir_all(&dist).unwrap();
    }
}
