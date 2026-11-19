//! Reading a page and saving it back without losing what another program wrote meanwhile.
//!
//! The editor loads a page together with its [`Revision`], and a save names the revision it
//! started from. When the page on disk no longer has that revision, nothing is written and the
//! caller is told to reload. The write itself is `anaquel_pipeline::rewrite_page`, the one that
//! `relocate` proved: a staged file, the permission bits of the page, a rename, and every failed
//! step undone. The revision a save returns is the hash of the bytes that function wrote, not of
//! a second read of the file, so a program that writes the page right after the save cannot give
//! the editor a revision of a text it does not hold.

use std::fmt::{self, Write as _};
use std::fs::{self, File};
use std::io::{self, Read as _};
use std::path::Path;

pub use anaquel_pipeline::MAX_PAGE_BYTES;
use anaquel_pipeline::{RewriteError, rewrite_page};
use sha2::{Digest, Sha256};

use crate::library_path::LibraryPath;

/// The identity of the bytes of a page: the SHA-256 of the file, in lowercase hexadecimal.
///
/// It is computed over the raw file, so a change of line ends or of the byte order mark counts as
/// a change.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Revision(String);

impl Revision {
    /// The revision of `bytes`.
    #[must_use]
    pub fn of(bytes: &[u8]) -> Self {
        let digest = Sha256::digest(bytes);
        let mut hex = String::with_capacity(64);
        for byte in digest {
            // Writing to a `String` cannot fail.
            let _ = write!(hex, "{byte:02x}");
        }
        Self(hex)
    }

    /// Reads a revision a client sent back.
    ///
    /// # Errors
    /// Returns [`SaveError::BadRevision`] unless `text` is 64 lowercase hexadecimal digits.
    pub fn parse(text: &str) -> Result<Self, SaveError> {
        let valid =
            text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        if valid {
            Ok(Self(text.to_owned()))
        } else {
            Err(SaveError::BadRevision)
        }
    }

    /// The 64 hexadecimal digits.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Revision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A page as the editor loads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageText {
    /// The text as the library reads it: without a byte order mark and with `\n` line ends.
    pub text: String,
    /// The revision to name when this text is saved.
    pub revision: Revision,
}

/// Why a page could not be read or saved.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SaveError {
    /// The path is not a page of the library.
    #[error("`{0}` is not a page of the library")]
    NotAPage(String),
    /// There is no such page.
    #[error("`{0}` does not exist")]
    Missing(String),
    /// The page, or the page as it would be written, is longer than [`MAX_PAGE_BYTES`].
    #[error(
        "a page may hold at most {MAX_PAGE_BYTES} bytes, line ends and byte order mark included"
    )]
    TooLarge,
    /// The text cannot be a page the editor round-trips: it is not UTF-8, or it holds a NUL
    /// character, a carriage return that does not end a line, or a leading byte order mark.
    #[error(
        "a page is UTF-8 text with no NUL character, no lone carriage return and no leading byte order mark"
    )]
    NotText,
    /// The revision sent is not 64 lowercase hexadecimal digits.
    #[error("the revision is not 64 lowercase hexadecimal digits")]
    BadRevision,
    /// The page changed after it was loaded. Nothing was written.
    #[error("the page changed after it was loaded; reload it before saving")]
    Stale,
    /// The page mixes `\r\n` and `\n` line ends. Nothing was written.
    #[error("the page mixes `\\r\\n` and `\\n` line ends, so the editor leaves it as it is")]
    MixedLineEndings,
    /// The disk refused a step.
    #[error("the page could not be written: {message}")]
    Failed {
        /// What the disk reported.
        message: String,
        /// One line for each file that could not be put back; empty when the page is as it was.
        unrestored: Vec<String>,
    },
}

/// Reads a page.
///
/// # Errors
/// Returns the reason the page cannot be opened in the editor.
pub fn read_page(path: &LibraryPath) -> Result<PageText, SaveError> {
    let bytes = read_bytes(path)?;
    let revision = Revision::of(&bytes);
    let text = editable_text(&bytes).ok_or(SaveError::NotText)?;
    Ok(PageText { text, revision })
}

/// Replaces the text of a page, if the page is still the one the editor loaded.
///
/// `loaded` is the revision that came with the text the editor started from. Line ends in `text`
/// may be `\n` or `\r\n`; the page keeps the line ends and the byte order mark it had.
///
/// Returns the revision of the page as written.
///
/// # Errors
/// Returns [`SaveError::Stale`] when the page changed since it was loaded, and another variant
/// when the text or the page is refused or the disk fails.
pub fn save_page(path: &LibraryPath, text: &str, loaded: &Revision) -> Result<Revision, SaveError> {
    save_with(path, text, loaded, rewrite_page)
}

/// `save_page` with the write given, so that a test can make it fail in each way.
fn save_with(
    path: &LibraryPath,
    text: &str,
    loaded: &Revision,
    rewrite: impl FnOnce(&Path, &str, &str) -> Result<Vec<u8>, RewriteError>,
) -> Result<Revision, SaveError> {
    // The write checks the size it will produce; this keeps a hostile body from being scanned.
    if u64::try_from(text.len()).map_or(true, |len| len > MAX_PAGE_BYTES) {
        return Err(SaveError::TooLarge);
    }
    let text = text_to_save(text)?;
    let current = read_bytes(path)?;
    if Revision::of(&current) != *loaded {
        return Err(SaveError::Stale);
    }
    let expected = editable_text(&current).ok_or(SaveError::NotText)?;
    let written = rewrite(path.absolute(), &text, &expected).map_err(|error| match error {
        RewriteError::Changed => SaveError::Stale,
        RewriteError::MixedLineEndings => SaveError::MixedLineEndings,
        RewriteError::TooLarge => SaveError::TooLarge,
        RewriteError::Failed {
            message,
            unrestored,
        } => SaveError::Failed {
            message,
            unrestored,
        },
    })?;
    Ok(Revision::of(&written))
}

/// The bytes of a page that passed the checks every read and save shares.
fn read_bytes(path: &LibraryPath) -> Result<Vec<u8>, SaveError> {
    if !path.is_page() {
        return Err(SaveError::NotAPage(path.as_str().to_owned()));
    }
    let meta = fs::symlink_metadata(path.absolute()).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => SaveError::Missing(path.as_str().to_owned()),
        _ => failed(&error),
    })?;
    // `LibraryPath` checked the names when it was built; the file could have become a link since.
    if !meta.is_file() {
        return Err(SaveError::NotAPage(path.as_str().to_owned()));
    }
    // The size above is a first look; the read itself is bounded too, for a file that grows.
    let mut bytes = Vec::new();
    File::open(path.absolute())
        .and_then(|file| file.take(MAX_PAGE_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|error| failed(&error))?;
    if u64::try_from(bytes.len()).map_or(true, |len| len > MAX_PAGE_BYTES) {
        return Err(SaveError::TooLarge);
    }
    Ok(bytes)
}

fn failed(error: &io::Error) -> SaveError {
    SaveError::Failed {
        message: error.to_string(),
        unrestored: Vec::new(),
    }
}

/// The text of a file as the library reads it: no byte order mark, `\n` line ends.
fn library_text(bytes: &[u8]) -> Option<String> {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let text = std::str::from_utf8(bytes).ok()?;
    Some(text.replace("\r\n", "\n"))
}

/// The text of a page as the editor can hold it and give it back unchanged, or `None`.
///
/// A page the editor cannot round-trip is refused when it is opened, so that nobody edits a text
/// that the save would then change or refuse.
pub(crate) fn editable_text(bytes: &[u8]) -> Option<String> {
    let text = library_text(bytes)?;
    is_round_trippable(&text).then_some(text)
}

/// The text a client sent, with `\r\n` made `\n`, or the reason it is refused.
fn text_to_save(text: &str) -> Result<String, SaveError> {
    let text = text.replace("\r\n", "\n");
    if is_round_trippable(&text) {
        Ok(text)
    } else {
        Err(SaveError::NotText)
    }
}

/// Whether a text with `\n` line ends reads back as itself: no NUL, no carriage return left
/// (one that is not part of `\r\n` is not a line end for the library), and no leading byte order
/// mark, which the library strips.
fn is_round_trippable(text: &str) -> bool {
    !text.contains(['\0', '\r']) && !text.starts_with('\u{feff}')
}

#[cfg(test)]
mod tests;
