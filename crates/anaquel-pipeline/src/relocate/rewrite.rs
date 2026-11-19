//! Replaces the text of one existing page, atomically, through the disk layer of `relocate`.
//!
//! The editor of `anaquel studio` saves a page with this function, so a save has the guarantees
//! of a move: the new text is written to a staged name beside the page with `create_new`, takes
//! the permission bits of the page, and replaces the page by a rename. A failed step is undone,
//! and a crash leaves the old file or the new one.
//!
//! Two limits, the same as a move has. The comparison of the page with the text the caller read
//! and the rename that replaces it are two calls, so a program that writes the page in between
//! loses its write. And the page keeps its byte order mark and its `\r\n` line ends, so a page
//! that mixes `\r\n` and `\n` is refused instead of being normalised without notice.

use std::collections::HashMap;
use std::fmt;
use std::path::Path;

use super::disk::{Change, Disk, RealDisk, apply, mixed_line_endings};

/// Why a page was not rewritten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RewriteError {
    /// The page no longer reads as the text the caller started from. Nothing was written.
    Changed,
    /// The page mixes `\r\n` and `\n` line ends, and a rewrite would change them. Nothing was
    /// written.
    MixedLineEndings,
    /// A step failed.
    Failed {
        /// What the disk reported.
        message: String,
        /// One line for each file that could not be put back; empty when the page is as it was.
        unrestored: Vec<String>,
    },
}

impl fmt::Display for RewriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Changed => f.write_str("the page changed after it was read"),
            Self::MixedLineEndings => {
                f.write_str("the page mixes `\\r\\n` and `\\n` line ends, so it is left as it is")
            }
            Self::Failed { message, .. } => f.write_str(message),
        }
    }
}

impl std::error::Error for RewriteError {}

/// Replaces the text of the page at `path` with `text`.
///
/// `expected` is the text the caller read from the page, as the library reads it: without a byte
/// order mark and with `\n` line ends. A page that no longer reads as `expected` is not written.
///
/// # Errors
/// Returns [`RewriteError`] when the page changed, mixes line ends or could not be written.
pub fn rewrite_page(path: &Path, text: &str, expected: &str) -> Result<(), RewriteError> {
    rewrite_page_on(&mut RealDisk, path, text, expected)
}

pub(super) fn rewrite_page_on(
    disk: &mut dyn Disk,
    path: &Path,
    text: &str,
    expected: &str,
) -> Result<(), RewriteError> {
    let bytes = disk.read(path).map_err(|error| RewriteError::Failed {
        message: error.to_string(),
        unrestored: Vec::new(),
    })?;
    if mixed_line_endings(&bytes) {
        return Err(RewriteError::MixedLineEndings);
    }
    let change = Change::Rewrite {
        path: path.to_path_buf(),
        text: text.to_owned(),
    };
    let expected = HashMap::from([(path.to_path_buf(), expected.to_owned())]);
    match apply(disk, &[change], &[], &expected) {
        Ok(_) => Ok(()),
        Err(failure) if failure.changed => Err(RewriteError::Changed),
        Err(failure) => Err(RewriteError::Failed {
            message: failure.message,
            unrestored: failure.unrestored,
        }),
    }
}

#[cfg(test)]
mod tests;
