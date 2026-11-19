//! Applies a planned move to the disk so that either every change is made or none is.
//!
//! Order of the steps, with nothing visible to a reader of the library changed before the last one:
//!
//! 1. Every moving file is renamed to a *held* name in its own folder, and the new text of a file
//!    whose links change is written to a *staged* name beside its destination.
//! 2. Every held or staged file is renamed to its destination. Because every source was vacated in
//!    step 1, files that trade places, chains of moves and a rename that changes only the case of a
//!    name work the same way as any other move.
//! 3. The pages outside the move whose links change are rewritten through a staged name.
//! 4. Only when all of that succeeded are the originals of the rewritten files removed, and the
//!    folders the move emptied.
//!
//! A held or staged name carries the position of its change, so no two files of one move can share
//! one. Staged files are created with `create_new`, which refuses a name that is already taken and
//! never writes through a link planted there. A step that fails is undone in reverse order.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub(super) trait Disk {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;
    fn exists(&self, path: &Path) -> bool;
    /// Replaces the content of a file the move already owns.
    fn write(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()>;
    /// Creates a file that must not exist yet. It fails when anything is at the name, a link
    /// included, and it never writes through a link.
    fn create_new(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()>;
    fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()>;
    fn remove_file(&mut self, path: &Path) -> io::Result<()>;
    fn create_dir(&mut self, path: &Path) -> io::Result<()>;
    fn remove_dir(&mut self, path: &Path) -> io::Result<()>;
}

pub(super) struct RealDisk;

impl Disk for RealDisk {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        fs::read(path)
    }

    fn exists(&self, path: &Path) -> bool {
        fs::symlink_metadata(path).is_ok()
    }

    fn write(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        fs::write(path, bytes)
    }

    fn create_new(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        // The file is this call's own, so a failed write removes what it left.
        if let Err(error) = file.write_all(bytes).and_then(|()| file.flush()) {
            drop(file);
            let _ = fs::remove_file(path);
            return Err(error);
        }
        Ok(())
    }

    fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    fn remove_file(&mut self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }

    fn create_dir(&mut self, path: &Path) -> io::Result<()> {
        fs::create_dir(path)
    }

    fn remove_dir(&mut self, path: &Path) -> io::Result<()> {
        fs::remove_dir(path)
    }
}

pub(super) enum Change {
    Move {
        from: PathBuf,
        to: PathBuf,
        text: Option<String>,
    },
    Rewrite {
        path: PathBuf,
        text: String,
    },
}

enum Undo {
    RemoveFile(PathBuf),
    RemoveDir(PathBuf),
    Rename { from: PathBuf, to: PathBuf },
    Restore { path: PathBuf, bytes: Vec<u8> },
}

pub(super) struct Failure {
    pub(super) message: String,
    pub(super) restored: bool,
}

/// Name prefix of the file a moving page waits in, in the folder it leaves.
pub(super) const HELD_PREFIX: &str = ".anaquel-held-";
/// Name prefix of the new text of a file, in the folder it will live in.
pub(super) const STAGED_PREFIX: &str = ".anaquel-staged-";
/// Digits of the position a temporary name carries.
pub(super) const POSITION_DIGITS: usize = 6;
/// The most changes one move may hold, so that every position fits its digits.
pub(super) const MAX_CHANGES: usize = 999_999;

pub(super) fn within_limit(changes: usize) -> bool {
    changes <= MAX_CHANGES
}

/// The characters a temporary name takes from a folder's path budget.
pub(super) const TEMPORARY_EXTRA: usize = STAGED_PREFIX.len() + POSITION_DIGITS;

pub(super) fn apply(
    disk: &mut dyn Disk,
    changes: &[Change],
    emptied: &[PathBuf],
) -> Result<Vec<PathBuf>, Failure> {
    if !within_limit(changes.len()) {
        return Err(Failure {
            message: format!("a move may hold at most {MAX_CHANGES} files"),
            restored: true,
        });
    }
    let mut undo: Vec<Undo> = Vec::new();
    let mut originals: Vec<PathBuf> = Vec::new();
    if let Err(error) = steps(disk, changes, &mut undo, &mut originals) {
        let restored = roll_back(disk, undo);
        return Err(Failure {
            message: error.to_string(),
            restored,
        });
    }
    // Past this point the move is done; what is left is tidying. A file that cannot be removed is
    // returned so that it can be reported, and nothing is undone.
    let mut leftovers = Vec::new();
    for held in originals {
        if disk.remove_file(&held).is_err() {
            leftovers.push(held);
        }
    }
    let mut dirs: Vec<&PathBuf> = emptied.iter().collect();
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    for dir in dirs {
        let _ = disk.remove_dir(dir);
    }
    Ok(leftovers)
}

fn steps(
    disk: &mut dyn Disk,
    changes: &[Change],
    undo: &mut Vec<Undo>,
    originals: &mut Vec<PathBuf>,
) -> io::Result<()> {
    // Step 1: vacate every source and stage every new text.
    let mut placements: Vec<(PathBuf, &Path)> = Vec::new();
    for (position, change) in changes.iter().enumerate() {
        let Change::Move { from, to, text } = change else {
            continue;
        };
        make_parents(disk, to, undo)?;
        let staged = match text {
            Some(text) => {
                let original = disk.read(from)?;
                let staged = temporary_name(to, STAGED_PREFIX, position);
                disk.create_new(&staged, &encode_like(&original, text))?;
                undo.push(Undo::RemoveFile(staged.clone()));
                Some(staged)
            }
            None => None,
        };
        let held = temporary_name(from, HELD_PREFIX, position);
        refuse_taken(disk, &held)?;
        disk.rename(from, &held)?;
        undo.push(Undo::Rename {
            from: held.clone(),
            to: from.clone(),
        });
        match staged {
            Some(staged) => {
                originals.push(held);
                placements.push((staged, to));
            }
            None => placements.push((held, to)),
        }
    }
    // Step 2: put every file where it goes.
    for (source, to) in placements {
        refuse_taken(disk, to)?;
        disk.rename(&source, to)?;
        undo.push(Undo::Rename {
            from: to.to_path_buf(),
            to: source,
        });
    }
    // Step 3: rewrite the pages that only change their links.
    for (position, change) in changes.iter().enumerate() {
        let Change::Rewrite { path, text } = change else {
            continue;
        };
        let original = disk.read(path)?;
        let staged = temporary_name(path, STAGED_PREFIX, position);
        disk.create_new(&staged, &encode_like(&original, text))?;
        if let Err(error) = disk.rename(&staged, path) {
            let _ = disk.remove_file(&staged);
            return Err(error);
        }
        undo.push(Undo::Restore {
            path: path.clone(),
            bytes: original,
        });
    }
    Ok(())
}

fn refuse_taken(disk: &dyn Disk, path: &Path) -> io::Result<()> {
    if disk.exists(path) {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", path.display()),
        ));
    }
    Ok(())
}

fn make_parents(disk: &mut dyn Disk, file: &Path, undo: &mut Vec<Undo>) -> io::Result<()> {
    let mut missing: Vec<PathBuf> = Vec::new();
    let mut probe = file.parent();
    while let Some(dir) = probe {
        if dir.as_os_str().is_empty() || disk.exists(dir) {
            break;
        }
        missing.push(dir.to_path_buf());
        probe = dir.parent();
    }
    for dir in missing.into_iter().rev() {
        disk.create_dir(&dir)?;
        undo.push(Undo::RemoveDir(dir));
    }
    Ok(())
}

/// A name for a temporary file in the folder of `path`. It does not depend on the name of `path`,
/// so it cannot be too long, and it carries `position` so that no two files of one move share it.
fn temporary_name(path: &Path, prefix: &str, position: usize) -> PathBuf {
    path.with_file_name(format!("{prefix}{position:0POSITION_DIGITS$}"))
}

pub(super) fn mixed_line_endings(bytes: &[u8]) -> bool {
    let crlf = bytes.windows(2).filter(|pair| pair == b"\r\n").count();
    let lf = bytes.split(|&b| b == b'\n').count() - 1;
    crlf > 0 && lf > crlf
}

fn encode_like(original: &[u8], text: &str) -> Vec<u8> {
    let crlf = original.windows(2).any(|pair| pair == b"\r\n");
    let bom = original.starts_with(&[0xEF, 0xBB, 0xBF]);
    let mut bytes = Vec::with_capacity(text.len() + 3);
    if bom {
        bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    }
    if crlf {
        bytes.extend_from_slice(text.replace('\n', "\r\n").as_bytes());
    } else {
        bytes.extend_from_slice(text.as_bytes());
    }
    bytes
}

fn roll_back(disk: &mut dyn Disk, undo: Vec<Undo>) -> bool {
    let mut clean = true;
    for step in undo.into_iter().rev() {
        let result = match step {
            Undo::RemoveFile(path) => disk.remove_file(&path),
            Undo::RemoveDir(path) => disk.remove_dir(&path),
            Undo::Rename { from, to } => disk.rename(&from, &to),
            Undo::Restore { path, bytes } => disk.write(&path, &bytes),
        };
        if result.is_err() {
            clean = false;
        }
    }
    clean
}
