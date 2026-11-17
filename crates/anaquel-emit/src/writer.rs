use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anaquel_diagnostics::{Code, Diagnostic};

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

/// Resolves an output path under `dist`, refusing anything that could leave it.
fn target(dist: &Path, relative: &str) -> Result<PathBuf, Diagnostic> {
    let candidate = Path::new(relative);
    let safe = !relative.is_empty()
        && candidate
            .components()
            .all(|c| matches!(c, Component::Normal(_)));
    if safe {
        Ok(dist.join(candidate))
    } else {
        Err(Diagnostic::error(
            Code::IO,
            format!("refusing to write outside the output folder: `{relative}`"),
        ))
    }
}

/// Writes `files` under `dist`, removes what `previous` produced and `files`
/// no longer does, and returns the ledger fields for what was written.
///
/// The caller fills `urls` and `redirects` of the returned ledger and stores
/// it with [`write_ledger`].
///
/// # Errors
/// Returns a diagnostic for the first path that is unsafe or cannot be written.
pub fn write_outputs(
    dist: &Path,
    files: &[OutputFile],
    previous: Option<&Ledger>,
) -> Result<(Ledger, WriteStats), Diagnostic> {
    let mut stats = WriteStats::default();
    let mut hashes = BTreeMap::new();
    for file in files {
        let path = target(dist, &file.path)?;
        let hash = blake3::hash(&file.bytes).to_hex().as_str()[..16].to_owned();
        let already_right = previous.is_some_and(|p| p.files.get(&file.path) == Some(&hash))
            && fs::metadata(&path).is_ok_and(|m| m.len() == file.bytes.len() as u64);
        if already_right {
            stats.unchanged += 1;
        } else {
            write_atomically(&path, &file.bytes)?;
            stats.written += 1;
        }
        hashes.insert(file.path.clone(), hash);
    }
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

fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), Diagnostic> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| io_error(parent, &e))?;
    }
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".anaquel-tmp");
    let temporary = PathBuf::from(temporary);
    fs::write(&temporary, bytes).map_err(|e| io_error(&temporary, &e))?;
    fs::rename(&temporary, path).map_err(|e| io_error(path, &e))
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
    fn a_deleted_file_is_rewritten_even_if_the_ledger_says_it_exists() {
        let dist = scratch("deleted");
        let (ledger, _) = write_outputs(&dist, &[file("x.txt", "hello")], None).unwrap();
        fs::remove_file(dist.join("x.txt")).unwrap();
        let (_, stats) = write_outputs(&dist, &[file("x.txt", "hello")], Some(&ledger)).unwrap();
        assert_eq!(stats.written, 1);
        fs::remove_dir_all(&dist).unwrap();
    }
}
