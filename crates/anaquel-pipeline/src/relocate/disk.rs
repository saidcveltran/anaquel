//! Applies a planned move to the disk so that either every change is made or none is.
//!
//! Order of the steps, with nothing visible to a reader of the library changed before step 2:
//!
//! 1. Every moving file is renamed to a *held* name in the folder it leaves, and the new text of
//!    a file whose links change is written to a *staged* name beside the file it replaces.
//! 2. Every held or staged file is renamed to its destination, creating the folders it needs.
//!    Because every source was vacated in step 1, files that trade places, chains of moves, a
//!    destination under a folder that another item empties, and a rename that changes only the
//!    case of a name work the same way as any other move.
//! 3. The pages outside the move whose links change are rewritten through a staged name.
//! 4. Only when all of that succeeded are the originals of the rewritten files removed, and the
//!    folders the move emptied.
//!
//! A held or staged name carries the position of its change, so no two files of one move can
//! share one. Staged files are created with `create_new`, which refuses a name that is already
//! taken and never writes through a link planted there. A step that fails is undone in reverse
//! order, and what cannot be undone is named, with the place each file belongs in.
//!
//! Two limits remain. The check that a destination is free and the rename that fills it are two
//! calls, so a file another program creates in between is replaced. And a text is written from
//! what the library read earlier, so each file is compared with it first and a file that changed
//! meanwhile stops the move.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::rewrite::RewriteError;

pub(super) trait Disk {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;
    fn exists(&self, path: &Path) -> bool;
    /// Replaces the content of a file the move already owns.
    fn write(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()>;
    /// Creates a file that must not exist yet. It fails when anything is at the name, a link
    /// included, and it never writes through a link.
    fn create_new(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()>;
    /// Gives `to` the permissions of `from`, so a rewritten file keeps its mode.
    fn copy_permissions(&mut self, from: &Path, to: &Path) -> io::Result<()>;
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

    fn copy_permissions(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        fs::set_permissions(to, fs::metadata(from)?.permissions())
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

/// The reason inside the error of a file that no longer reads as the text the library read, so a
/// caller can tell it from a failure of the disk.
#[derive(Debug)]
struct FileChanged(PathBuf);

impl std::fmt::Display for FileChanged {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} changed while the move was running", self.0.display())
    }
}

impl std::error::Error for FileChanged {}

fn is_file_changed(error: &io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(<dyn std::error::Error + Send + Sync>::is::<FileChanged>)
}

pub(super) struct Failure {
    pub(super) message: String,
    pub(super) restored: bool,
    /// One line for each step that could not be undone: where a file is and where it belongs.
    pub(super) unrestored: Vec<String>,
}

/// Name prefix of the file a moving page waits in, in the folder it leaves.
pub(super) const HELD_PREFIX: &str = ".anaquel-held-";
/// Name prefix of the new text of a file, in the folder of the file it replaces.
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

/// Makes every change, or none. `expected` holds the text the library read for a file, by its
/// path on disk; a file that no longer reads as that text stops the move. A path with no entry
/// is not compared.
///
/// Returns the files of a finished move that could not be removed afterwards.
pub(super) fn apply(
    disk: &mut dyn Disk,
    changes: &[Change],
    emptied: &[PathBuf],
    expected: &HashMap<PathBuf, String>,
) -> Result<Vec<PathBuf>, Failure> {
    if !within_limit(changes.len()) {
        return Err(Failure {
            message: format!("a move may hold at most {MAX_CHANGES} files"),
            restored: true,
            unrestored: Vec::new(),
        });
    }
    let mut undo: Vec<Undo> = Vec::new();
    let mut originals: Vec<PathBuf> = Vec::new();
    if let Err(error) = steps(disk, changes, expected, &mut undo, &mut originals) {
        let unrestored = roll_back(disk, undo);
        return Err(Failure {
            message: error.to_string(),
            restored: unrestored.is_empty(),
            unrestored,
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

/// The most staged names tried for one rewrite before it gives up.
const MAX_STAGED_SLOTS: usize = 16;

/// Replaces the text of one existing file: the single step of a move that rewrites a page, with
/// the checks a caller that is not a move needs. `expected` is the text the caller read, as the
/// library reads it; `max_bytes` bounds what is written. Returns the bytes written.
///
/// A staged name that is already taken (by a file a crash left, or by a save in progress) is
/// never written through: the next of [`MAX_STAGED_SLOTS`] names is tried instead.
pub(super) fn replace_text(
    disk: &mut dyn Disk,
    path: &Path,
    text: &str,
    expected: &str,
    max_bytes: usize,
) -> Result<Vec<u8>, RewriteError> {
    let failed = |error: &io::Error| RewriteError::Failed {
        message: error.to_string(),
        unrestored: Vec::new(),
    };
    let expected = HashMap::from([(path.to_path_buf(), expected.to_owned())]);
    let original = read_expected(disk, path, &expected).map_err(|error| {
        if is_file_changed(&error) {
            RewriteError::Changed
        } else {
            failed(&error)
        }
    })?;
    if mixed_line_endings(&original) {
        return Err(RewriteError::MixedLineEndings);
    }
    let written = encode_like(&original, text);
    if written.len() > max_bytes {
        return Err(RewriteError::TooLarge);
    }
    let mut undo = Vec::new();
    match replace_through_staged_file(disk, path, text, &original, &mut undo) {
        Ok(()) => Ok(written),
        Err(error) => {
            let unrestored = roll_back(disk, undo);
            Err(RewriteError::Failed {
                message: error.to_string(),
                unrestored,
            })
        }
    }
}

fn replace_through_staged_file(
    disk: &mut dyn Disk,
    path: &Path,
    text: &str,
    original: &[u8],
    undo: &mut Vec<Undo>,
) -> io::Result<()> {
    for slot in 0..MAX_STAGED_SLOTS {
        match stage_with(disk, path, text, original, slot, undo) {
            // A failed rename leaves the staged file in place, and its removal entry stays.
            Ok(staged) => return disk.rename(&staged, path),
            // Nothing was created, so nothing is undone and the next name is tried.
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists && undo.is_empty() => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("every staged name beside {} is taken", path.display()),
    ))
}

fn steps(
    disk: &mut dyn Disk,
    changes: &[Change],
    expected: &HashMap<PathBuf, String>,
    undo: &mut Vec<Undo>,
    originals: &mut Vec<PathBuf>,
) -> io::Result<()> {
    // Step 1: vacate every source and stage every new text.
    let mut placements: Vec<(PathBuf, &Path)> = Vec::new();
    for (position, change) in changes.iter().enumerate() {
        let Change::Move { from, to, text } = change else {
            continue;
        };
        let staged = match text {
            Some(text) => Some(stage(disk, from, text, position, expected, undo)?),
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
    // Step 2: put every file where it goes, now that every source is out of the way.
    for (source, to) in placements {
        make_parents(disk, to, undo)?;
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
        let original = read_expected(disk, path, expected)?;
        let staged = stage_with(disk, path, text, &original, position, undo)?;
        // A failed rename leaves the staged file in place, and the entry that removes it stays.
        disk.rename(&staged, path)?;
        // The staged file is the page now: its removal entry, the last one pushed, no longer applies.
        let removal = undo.pop();
        debug_assert!(matches!(removal, Some(Undo::RemoveFile(ref p)) if *p == staged));
        undo.push(Undo::Restore {
            path: path.clone(),
            bytes: original,
        });
    }
    Ok(())
}

/// Writes the new `text` of `from` to a staged name beside it.
fn stage(
    disk: &mut dyn Disk,
    from: &Path,
    text: &str,
    position: usize,
    expected: &HashMap<PathBuf, String>,
    undo: &mut Vec<Undo>,
) -> io::Result<PathBuf> {
    let original = read_expected(disk, from, expected)?;
    stage_with(disk, from, text, &original, position, undo)
}

fn stage_with(
    disk: &mut dyn Disk,
    from: &Path,
    text: &str,
    original: &[u8],
    position: usize,
    undo: &mut Vec<Undo>,
) -> io::Result<PathBuf> {
    let staged = temporary_name(from, STAGED_PREFIX, position);
    disk.create_new(&staged, &encode_like(original, text))?;
    // From here the file is this move's own, so a failure below removes it.
    undo.push(Undo::RemoveFile(staged.clone()));
    disk.copy_permissions(from, &staged)?;
    Ok(staged)
}

/// Reads a file and checks that it is still the text the library read.
fn read_expected(
    disk: &dyn Disk,
    path: &Path,
    expected: &HashMap<PathBuf, String>,
) -> io::Result<Vec<u8>> {
    let bytes = disk.read(path)?;
    if let Some(text) = expected.get(path)
        && as_read_by_library(&bytes).as_deref() != Some(text.as_str())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            FileChanged(path.to_path_buf()),
        ));
    }
    Ok(bytes)
}

/// The text of a file as the library reads it: without a byte order mark and with `\n` line ends.
fn as_read_by_library(bytes: &[u8]) -> Option<String> {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let text = std::str::from_utf8(bytes).ok()?;
    Some(text.replace("\r\n", "\n"))
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

/// Undoes every step in reverse order and returns a line for each one that could not be undone.
fn roll_back(disk: &mut dyn Disk, undo: Vec<Undo>) -> Vec<String> {
    let mut unrestored = Vec::new();
    for step in undo.into_iter().rev() {
        let (result, line) = match step {
            Undo::RemoveFile(path) => {
                let result = disk.remove_file(&path);
                (
                    result,
                    format!("`{}` is a temporary file to delete", path.display()),
                )
            }
            Undo::RemoveDir(path) => {
                let result = disk.remove_dir(&path);
                (
                    result,
                    format!(
                        "`{}` is a folder made by the move to delete",
                        path.display()
                    ),
                )
            }
            Undo::Rename { from, to } => {
                let result = disk.rename(&from, &to);
                (
                    result,
                    format!("`{}` belongs at `{}`", from.display(), to.display()),
                )
            }
            Undo::Restore { path, bytes } => {
                let result = disk.write(&path, &bytes);
                (
                    result,
                    format!(
                        "`{}` has its new text and must get its old one back",
                        path.display()
                    ),
                )
            }
        };
        if result.is_err() {
            unrestored.push(line);
        }
    }
    unrestored
}
