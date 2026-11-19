//! What the program remembers between runs: the newest manifest time it ever accepted.
//!
//! `anaquel-update` refuses a manifest issued before the one already seen, which defeats a
//! replay of an old manifest that still looks valid. That needs the time to be kept. It is one
//! line in a file in the user's state folder, written only after a manifest's signature was
//! verified and only when the time is newer than the one stored.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use anaquel_update::Timestamp;
use thiserror::Error;

/// Name of the file that holds the time.
const LAST_SEEN_FILE: &str = "last-manifest-time";
/// Name of the folder inside the system's state folder.
const FOLDER_NAME: &str = "anaquel";
/// A time is 20 characters; anything much larger is not this file.
const MAX_FILE_BYTES: u64 = 64;

/// Which system's rules locate the state folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// Windows: `%LOCALAPPDATA%`.
    Windows,
    /// Linux and other Unix systems: `$XDG_STATE_HOME`, else `~/.local/state`.
    Unix,
}

impl Platform {
    /// The system this program is running on.
    #[must_use]
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Unix
        }
    }
}

/// Why the remembered time could not be read or written.
#[derive(Debug, Error)]
pub enum StateError {
    /// The file holds something that is not a time.
    #[error(
        "{} does not hold a time ({reason}); remove the file to start again",
        path.display()
    )]
    Corrupt {
        /// The file concerned.
        path: PathBuf,
        /// What is wrong with it.
        reason: String,
    },
    /// The file or its folder could not be read or written.
    #[error("{}: {source}", path.display())]
    Io {
        /// The file or folder concerned.
        path: PathBuf,
        /// What the system said.
        source: io::Error,
    },
}

/// The folder where the program keeps what it remembers, or `None` when the system does not say
/// where the user's folders are.
///
/// `lookup` returns an environment variable. A value that is empty or not an absolute path is
/// ignored, as the XDG rules say, so a stray variable never makes the program write to a folder
/// relative to wherever it was started.
#[must_use]
pub fn state_dir(platform: Platform, lookup: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let absolute = |name: &str| {
        lookup(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    let base = match platform {
        Platform::Windows => absolute("LOCALAPPDATA")
            .or_else(|| absolute("USERPROFILE").map(|home| home.join("AppData").join("Local"))),
        Platform::Unix => absolute("XDG_STATE_HOME")
            .or_else(|| absolute("HOME").map(|home| home.join(".local").join("state"))),
    }?;
    Some(base.join(FOLDER_NAME))
}

/// The newest manifest time ever recorded in `dir`, or `None` when nothing was recorded yet.
///
/// # Errors
/// See [`StateError`].
pub fn read_last_seen(dir: &Path) -> Result<Option<Timestamp>, StateError> {
    let path = dir.join(LAST_SEEN_FILE);
    let io_error = |source| StateError::Io {
        path: path.clone(),
        source,
    };
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(io_error(source)),
    };
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    let corrupt = |reason: &str| StateError::Corrupt {
        path: path.clone(),
        reason: reason.to_owned(),
    };
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_FILE_BYTES {
        return Err(corrupt("it is too large"));
    }
    let text = String::from_utf8(bytes).map_err(|_| corrupt("it is not text"))?;
    Timestamp::parse(text.trim())
        .map(Some)
        .map_err(|reason| corrupt(&reason))
}

/// Records `issued` as the newest manifest time, unless a newer or equal one is already there.
/// Returns whether the file was changed.
///
/// The file is written whole beside the old one and then renamed over it, so a crash leaves
/// either the old time or the new one.
///
/// # Errors
/// See [`StateError`].
pub fn record_last_seen(dir: &Path, issued: &Timestamp) -> Result<bool, StateError> {
    if read_last_seen(dir)?.is_some_and(|stored| stored >= *issued) {
        return Ok(false);
    }
    let path = dir.join(LAST_SEEN_FILE);
    let io_error = |target: &Path| {
        let target = target.to_owned();
        move |source| StateError::Io {
            path: target,
            source,
        }
    };
    fs::create_dir_all(dir).map_err(io_error(dir))?;
    let temporary = dir.join(format!("{LAST_SEEN_FILE}.{}.tmp", std::process::id()));
    let mut file = File::create(&temporary).map_err(io_error(&temporary))?;
    let written = file
        .write_all(format!("{issued}\n").as_bytes())
        .and_then(|()| file.sync_all());
    drop(file);
    if let Err(source) = written {
        let _ = fs::remove_file(&temporary);
        return Err(StateError::Io {
            path: temporary,
            source,
        });
    }
    fs::rename(&temporary, &path).map_err(|source| {
        let _ = fs::remove_file(&temporary);
        StateError::Io { path, source }
    })?;
    Ok(true)
}

#[cfg(test)]
mod tests;
