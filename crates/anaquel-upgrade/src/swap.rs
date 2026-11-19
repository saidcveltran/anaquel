//! Puts a new program in place of the installed one, keeps the one before it, and goes back.
//!
//! The steps are renames inside one folder. Windows lets a running program be renamed but not
//! overwritten, so the installed file is moved aside first and the new one takes its name; the
//! same two renames are safe on Linux. Whenever the second rename fails the first is undone,
//! so a failure leaves the program that was there.

use std::fmt;
use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Component, Path, PathBuf};

use thiserror::Error;

/// Name suffix of the program kept from before the last upgrade.
const PREVIOUS_SUFFIX: &str = ".previous";
/// Name suffix of the new program while it waits to be swapped in.
const STAGED_SUFFIX: &str = ".new";
/// Name suffix of the program that is put aside for a moment during a rollback.
const ASIDE_SUFFIX: &str = ".swap";

/// The file next to `installed` that holds the program kept from before the last upgrade.
#[must_use]
pub fn previous_path(installed: &Path) -> PathBuf {
    with_suffix(installed, PREVIOUS_SUFFIX)
}

/// The file next to `installed` where a new program is written before it is swapped in.
#[must_use]
pub fn staged_path(installed: &Path) -> PathBuf {
    with_suffix(installed, STAGED_SUFFIX)
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Why a swap or a rollback did not happen, or did not finish.
#[derive(Debug, Error)]
pub enum SwapError {
    /// The new program is not where it should be.
    #[error("the new program {} is missing", .0.display())]
    NothingStaged(PathBuf),
    /// There is no earlier program to go back to.
    #[error("there is no earlier program to go back to ({} does not exist)", .0.display())]
    NothingToRestore(PathBuf),
    /// The folder of the program cannot be written to.
    #[error("the folder {} cannot be written to: {source}", folder.display())]
    NotWritable {
        /// The folder of the installed program.
        folder: PathBuf,
        /// What the system said.
        source: io::Error,
    },
    /// A file step failed and was undone; the installed program is as it was.
    #[error("could not {action} {}: {source}", path.display())]
    Step {
        /// What was being done.
        action: &'static str,
        /// The file concerned.
        path: PathBuf,
        /// What the system said.
        source: io::Error,
    },
    /// A step failed and the undo failed too; the files are where this says.
    #[error(
        "the swap failed ({cause}) and could not be undone ({undo}); the program that was installed is now {}",
        .moved_to.display()
    )]
    Stranded {
        /// What went wrong first.
        cause: io::Error,
        /// What went wrong while undoing it.
        undo: io::Error,
        /// Where the previously installed program now is.
        moved_to: PathBuf,
    },
}

/// A tool that owns the installed program and should be the one to update it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageManager {
    /// Scoop on Windows.
    Scoop,
    /// The Windows Package Manager.
    Winget,
    /// The package manager of a Linux distribution.
    System,
}

impl fmt::Display for PackageManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Scoop => "Scoop (`scoop update anaquel`)",
            Self::Winget => "winget (`winget upgrade anaquel`)",
            Self::System => "the system package manager",
        })
    }
}

/// The package manager that owns a program at `path`, judged from where it is.
///
/// This looks at the folder names only: a program under `scoop\apps`, under
/// `WinGet\Packages` or `WinGet\Links`, or in `/usr/bin` or `/bin`. A program anywhere else is
/// the user's own.
#[must_use]
pub fn managed_by(path: &Path) -> Option<PackageManager> {
    let names: Vec<String> = path
        .components()
        .filter_map(|part| match part {
            Component::Normal(name) => Some(name.to_string_lossy().to_ascii_lowercase()),
            _ => None,
        })
        .collect();
    let follows = |first: &str, second: &str| {
        names
            .windows(2)
            .any(|pair| pair[0] == first && pair[1] == second)
    };
    if follows("scoop", "apps") {
        Some(PackageManager::Scoop)
    } else if follows("winget", "packages") || follows("winget", "links") {
        Some(PackageManager::Winget)
    } else if path.starts_with("/usr/bin") || path.starts_with("/bin") {
        // A drive letter is a path prefix, so `C:\bin` does not start with `/bin`.
        Some(PackageManager::System)
    } else {
        None
    }
}

/// Checks that files can be made and removed in the folder of `installed`, which a swap needs.
///
/// # Errors
/// Returns [`SwapError::NotWritable`] when they cannot.
pub fn check_writable(installed: &Path) -> Result<(), SwapError> {
    let folder = installed
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let probe = folder.join(format!(".anaquel-write-probe-{}", std::process::id()));
    let not_writable = |source| SwapError::NotWritable {
        folder: folder.to_owned(),
        source,
    };
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .map_err(not_writable)?;
    fs::remove_file(&probe).map_err(not_writable)
}

/// Swaps `staged` in as the installed program and keeps what was installed as the previous one.
///
/// Any earlier previous program is removed first, so one version is kept and no more. If the
/// new program cannot take its place, the old one is put back.
///
/// # Errors
/// See [`SwapError`].
pub fn install(installed: &Path, staged: &Path) -> Result<(), SwapError> {
    install_with(installed, staged, &mut |from, to| fs::rename(from, to))
}

/// A rename of one file; tests replace it to make a step fail.
type Rename<'a> = &'a mut dyn FnMut(&Path, &Path) -> io::Result<()>;

fn install_with(installed: &Path, staged: &Path, rename: Rename<'_>) -> Result<(), SwapError> {
    if !staged.is_file() {
        return Err(SwapError::NothingStaged(staged.to_owned()));
    }
    let previous = previous_path(installed);
    remove_if_present(&previous)?;
    step(rename, installed, &previous, "move aside")?;
    if let Err(source) = rename(staged, installed) {
        return Err(undo_move(
            rename,
            &previous,
            installed,
            source,
            "put in place",
            staged,
        ));
    }
    Ok(())
}

/// Goes back to the previous program, and keeps the one just replaced as the new previous, so a
/// second rollback goes forward again.
///
/// # Errors
/// See [`SwapError`].
pub fn rollback(installed: &Path) -> Result<(), SwapError> {
    rollback_with(installed, &mut |from, to| fs::rename(from, to))
}

fn rollback_with(installed: &Path, rename: Rename<'_>) -> Result<(), SwapError> {
    let previous = previous_path(installed);
    if !previous.is_file() {
        return Err(SwapError::NothingToRestore(previous));
    }
    let aside = with_suffix(installed, ASIDE_SUFFIX);
    remove_if_present(&aside)?;
    step(rename, installed, &aside, "move aside")?;
    if let Err(source) = rename(&previous, installed) {
        return Err(undo_move(
            rename, &aside, installed, source, "restore", &previous,
        ));
    }
    // The program that was replaced becomes the previous one. If that cannot be done the
    // rollback itself has happened, so the leftover is removed and the loss is only that
    // the newer program is not kept.
    if rename(&aside, &previous).is_err() {
        let _ = fs::remove_file(&aside);
    }
    Ok(())
}

/// Undoes a first rename after the second failed: `moved` goes back to `original`.
fn undo_move(
    rename: Rename<'_>,
    moved: &Path,
    original: &Path,
    cause: io::Error,
    action: &'static str,
    failed_path: &Path,
) -> SwapError {
    match rename(moved, original) {
        Ok(()) => SwapError::Step {
            action,
            path: failed_path.to_owned(),
            source: cause,
        },
        Err(undo) => SwapError::Stranded {
            cause,
            undo,
            moved_to: moved.to_owned(),
        },
    }
}

fn step(rename: Rename<'_>, from: &Path, to: &Path, action: &'static str) -> Result<(), SwapError> {
    rename(from, to).map_err(|source| SwapError::Step {
        action,
        path: from.to_owned(),
        source,
    })
}

fn remove_if_present(path: &Path) -> Result<(), SwapError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(SwapError::Step {
            action: "remove",
            path: path.to_owned(),
            source,
        }),
    }
}

#[cfg(test)]
mod tests;
