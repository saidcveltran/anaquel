//! A path inside a library, checked once when it is built.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Longest relative path accepted, in bytes.
const MAX_PATH_BYTES: usize = 1024;
/// Longest single name accepted, in bytes (the limit of the common file systems).
const MAX_SEGMENT_BYTES: usize = 255;

/// Why a path was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PathError {
    /// The path is empty.
    #[error("the path is empty")]
    Empty,
    /// The path is longer than the limit (`MAX_PATH_BYTES`, 1024 bytes).
    #[error("the path is longer than {MAX_PATH_BYTES} bytes")]
    TooLong,
    /// The path is absolute, names a drive or uses a backslash.
    #[error("`{0}` is not a relative path of plain names separated by `/`")]
    NotRelative(String),
    /// One name of the path is refused.
    #[error("the name `{segment}` is refused: {reason}")]
    Segment {
        /// The name.
        segment: String,
        /// Why.
        reason: &'static str,
    },
    /// A name of the path is a symbolic link.
    #[error("`{0}` is a symbolic link, and the editor does not follow links")]
    Link(String),
    /// The path resolves outside the library.
    #[error("`{0}` leaves the library")]
    Outside(String),
    /// The folders of the path could not be examined.
    #[error("the path could not be examined: {0}")]
    Unreadable(String),
}

/// A path inside a library: relative, made of plain names, and with no link on the way.
///
/// The check is made once, here. A file system can change after the check (another program can
/// replace a folder by a link); the write path checks again what it can, and this type does not
/// claim to close that gap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryPath {
    relative: String,
    absolute: PathBuf,
}

impl LibraryPath {
    /// Builds the path `relative` (names separated by `/`) under the library folder `root`.
    ///
    /// The file does not have to exist, but every folder that exists on the way is examined.
    ///
    /// # Errors
    /// Returns the first reason the path is refused.
    pub fn new(root: &Path, relative: &str) -> Result<Self, PathError> {
        check_text(relative)?;
        let root_real = fs::canonicalize(root).map_err(|e| {
            PathError::Unreadable(format!("the library folder {}: {e}", root.display()))
        })?;
        let mut current = root_real.clone();
        let mut deepest_existing = root_real.clone();
        let segments: Vec<&str> = relative.split('/').collect();
        for (index, segment) in segments.iter().enumerate() {
            current.push(segment);
            match fs::symlink_metadata(&current) {
                Ok(meta) => {
                    if meta.file_type().is_symlink() {
                        return Err(PathError::Link(relative.to_owned()));
                    }
                    if index + 1 < segments.len() && !meta.is_dir() {
                        return Err(PathError::Segment {
                            segment: (*segment).to_owned(),
                            reason: "it is a file, so it cannot hold the rest of the path",
                        });
                    }
                    deepest_existing.clone_from(&current);
                }
                // Nothing below a name that does not exist can exist either.
                Err(e) if e.kind() == io::ErrorKind::NotFound => break,
                Err(e) => return Err(PathError::Unreadable(e.to_string())),
            }
        }
        // A folder that is a junction or sits behind one resolves elsewhere without being a link.
        let resolved = fs::canonicalize(&deepest_existing)
            .map_err(|e| PathError::Unreadable(e.to_string()))?;
        if !resolved.starts_with(&root_real) {
            return Err(PathError::Outside(relative.to_owned()));
        }
        let mut absolute = root_real;
        absolute.extend(&segments);
        Ok(Self {
            relative: relative.to_owned(),
            absolute,
        })
    }

    /// The path relative to the library, names separated by `/`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.relative
    }

    /// The path on disk.
    #[must_use]
    pub fn absolute(&self) -> &Path {
        &self.absolute
    }

    /// Whether this is a page of a library: a file named `*.md`, in any case, inside a volume
    /// folder, which is `volumes/NAME` or `shelves/SHELF/volumes/NAME`.
    #[must_use]
    pub fn is_page(&self) -> bool {
        let segments: Vec<&str> = self.relative.split('/').collect();
        let inside_a_volume = matches!(
            segments.as_slice(),
            ["volumes", _, _, ..] | ["shelves", _, "volumes", _, _, ..]
        );
        inside_a_volume
            && segments.last().is_some_and(|name| {
                name.len() > ".md".len() && name.to_ascii_lowercase().ends_with(".md")
            })
    }
}

/// The checks that need no file system.
fn check_text(relative: &str) -> Result<(), PathError> {
    if relative.is_empty() {
        return Err(PathError::Empty);
    }
    if relative.len() > MAX_PATH_BYTES {
        return Err(PathError::TooLong);
    }
    if relative.starts_with('/') || relative.contains('\\') || relative.contains(':') {
        return Err(PathError::NotRelative(relative.to_owned()));
    }
    for segment in relative.split('/') {
        if let Some(reason) = segment_problem(segment) {
            return Err(PathError::Segment {
                segment: segment.to_owned(),
                reason,
            });
        }
    }
    Ok(())
}

fn segment_problem(segment: &str) -> Option<&'static str> {
    if segment.is_empty() {
        return Some("it is empty");
    }
    if segment == "." || segment == ".." {
        return Some("it is a reference to the current or the parent folder");
    }
    if segment.starts_with('.') {
        return Some("it starts with a dot, so the library skips it");
    }
    if segment.len() > MAX_SEGMENT_BYTES {
        return Some("it is too long");
    }
    if segment.ends_with([' ', '.']) {
        return Some("it ends with a space or a dot, which Windows drops");
    }
    if segment
        .chars()
        .any(|c| c.is_control() || "<>\"|?*".contains(c))
    {
        return Some("it holds a character that a file name cannot hold");
    }
    // The address of a page is made from its path, and these two would end or change it.
    if segment.contains(['#', '%']) {
        return Some("it holds a character that would change the address of the page");
    }
    if is_windows_device(segment) {
        return Some("it is a reserved name on Windows");
    }
    None
}

/// Whether Windows reserves the name for a device, with any extension.
fn is_windows_device(segment: &str) -> bool {
    let stem = segment.split('.').next().unwrap_or("").trim_end();
    let lower = stem.to_lowercase();
    matches!(
        lower.as_str(),
        "con" | "prn" | "aux" | "nul" | "conin$" | "conout$"
    ) || ["com", "lpt"].iter().any(|prefix| {
        lower.strip_prefix(prefix).is_some_and(|rest| {
            let mut chars = rest.chars();
            matches!(
                (chars.next(), chars.next()),
                (Some('0'..='9' | '¹' | '²' | '³'), None)
            )
        })
    })
}

#[cfg(test)]
mod tests;
