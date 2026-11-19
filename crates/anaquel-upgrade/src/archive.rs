//! Takes the program out of a release archive.
//!
//! The release archives `dist` writes hold the program, the licence files and a README, in one
//! folder or at the top. Only the program is wanted, so nothing else is written to disk and no
//! path inside the archive is ever used as a path on disk: the program goes to the place the
//! caller names, whatever the archive calls it. Both the program and the whole decompressed
//! stream are limited in size, so a small archive that expands enormously cannot fill the disk.

use std::cell::Cell;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, Write};
use std::path::{Component, Path, PathBuf};

use flate2::read::GzDecoder;
use thiserror::Error;

const COPY_CHUNK: usize = 64 * 1024;

/// The format of a release archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    /// A tar archive compressed with gzip, `.tar.gz`: the Linux archives.
    TarGz,
    /// A zip archive, `.zip`: the Windows archives. Only the deflate method is read.
    Zip,
}

impl ArchiveKind {
    /// The kind a file name says, or `None` for a name this crate cannot read.
    #[must_use]
    // The names are those `dist` writes, in lower case, and the manifest lists them exactly,
    // so an exact comparison is the stricter and the intended one.
    #[allow(clippy::case_sensitive_file_extension_comparisons)]
    pub fn of_file_name(name: &str) -> Option<Self> {
        if name.ends_with(".tar.gz") {
            Some(Self::TarGz)
        } else if name.ends_with(".zip") {
            Some(Self::Zip)
        } else {
            None
        }
    }
}

/// How much may be written and read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Largest program accepted, in bytes.
    pub program_bytes: u64,
    /// Most the decompressed archive may hold in all, in bytes.
    pub archive_bytes: u64,
}

impl Limits {
    /// The limits for a program the manifest says is at most `MAX_ASSET_BYTES` as an archive:
    /// the program may be as large as that, and the archive twice as much with its other files.
    #[must_use]
    pub const fn for_release() -> Self {
        let program_bytes = anaquel_update::MAX_ASSET_BYTES;
        Self {
            program_bytes,
            archive_bytes: program_bytes * 2,
        }
    }
}

/// Why the program could not be taken out of an archive.
#[derive(Debug, Error)]
pub enum ArchiveError {
    /// The archive is damaged, truncated or not of the kind its name says.
    #[error("the archive could not be read: {0}")]
    Unreadable(io::Error),
    /// The archive has no file with the program's name.
    #[error("the archive holds no file named `{0}`")]
    ProgramMissing(String),
    /// The archive has two files with the program's name.
    #[error("the archive holds more than one file named `{0}`")]
    ProgramRepeated(String),
    /// The entry with the program's name is a link, a folder or another special entry.
    #[error("the entry `{0}` in the archive is not a plain file")]
    NotAPlainFile(String),
    /// The program is empty or larger than the limit.
    #[error("the program in the archive must be between 1 and {limit} bytes")]
    BadProgramSize {
        /// The most accepted.
        limit: u64,
    },
    /// The archive expands to more than the limit.
    #[error("the archive expands to more than {limit} bytes")]
    ArchiveTooLarge {
        /// The most accepted.
        limit: u64,
    },
    /// The program could not be written where it was to go.
    #[error("{path}: {source}", path = path.display())]
    Destination {
        /// Where the program was to be written.
        path: PathBuf,
        /// What the system said.
        source: io::Error,
    },
}

/// Writes the file called `program_name` of the archive to `destination`, which must not exist,
/// and returns its size. On any failure nothing is left at `destination`.
///
/// The program may be at the top of the archive or one folder down, as `dist` writes it. On Unix
/// the file is made executable by its owner.
///
/// # Errors
/// See [`ArchiveError`].
pub fn extract_program(
    archive: impl Read + Seek,
    kind: ArchiveKind,
    program_name: &str,
    destination: &Path,
    limits: Limits,
) -> Result<u64, ArchiveError> {
    let exceeded = Cell::new(false);
    let result = match kind {
        ArchiveKind::TarGz => {
            // The cap sits after the decompressor: it is the expanded size that is limited.
            let capped = CappedReader {
                inner: GzDecoder::new(archive),
                remaining: limits.archive_bytes,
                exceeded: &exceeded,
            };
            extract_from_tar(capped, program_name, destination, limits)
        }
        ArchiveKind::Zip => extract_from_zip(archive, program_name, destination, limits),
    };
    result.map_err(|error| match error {
        // A cap hit inside the decompressor surfaces as a plain read error; say what it was.
        ArchiveError::Unreadable(_) if exceeded.get() => ArchiveError::ArchiveTooLarge {
            limit: limits.archive_bytes,
        },
        other => other,
    })
}

/// Reads at most `remaining` bytes and then fails, noting that the cap was the cause.
struct CappedReader<'flag, R> {
    inner: R,
    remaining: u64,
    exceeded: &'flag Cell<bool>,
}

impl<R: Read> Read for CappedReader<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        // One byte more than allowed is read, to tell "exactly at the limit" from "over it".
        let room = usize::try_from(self.remaining.saturating_add(1)).unwrap_or(usize::MAX);
        let wanted = buffer.len().min(room);
        let count = self.inner.read(&mut buffer[..wanted])?;
        let count_bytes = u64::try_from(count).unwrap_or(u64::MAX);
        if count_bytes > self.remaining {
            self.exceeded.set(true);
            return Err(io::Error::other("the archive is larger than allowed"));
        }
        self.remaining -= count_bytes;
        Ok(count)
    }
}

fn extract_from_tar(
    stream: impl Read,
    program_name: &str,
    destination: &Path,
    limits: Limits,
) -> Result<u64, ArchiveError> {
    let mut archive = tar::Archive::new(stream);
    let mut written = None;
    let outcome = (|| {
        for entry in archive.entries().map_err(ArchiveError::Unreadable)? {
            let mut entry = entry.map_err(ArchiveError::Unreadable)?;
            let path = entry.path().map_err(ArchiveError::Unreadable)?.into_owned();
            if !names_the_program(&path, program_name) {
                continue;
            }
            if !entry.header().entry_type().is_file() {
                return Err(ArchiveError::NotAPlainFile(path.display().to_string()));
            }
            if written.is_some() {
                return Err(ArchiveError::ProgramRepeated(program_name.to_owned()));
            }
            written = Some(write_program(
                &mut entry,
                destination,
                limits.program_bytes,
            )?);
        }
        written.ok_or_else(|| ArchiveError::ProgramMissing(program_name.to_owned()))
    })();
    if outcome.is_err() && written.is_some() {
        // Only a file this call made is removed: `write_program` refuses to touch an existing one.
        let _ = fs::remove_file(destination);
    }
    outcome
}

/// The same job for a zip archive. A zip lists its files at the end, so the list is read first
/// (nothing is decompressed for it) and only the one entry that is the program is read.
fn extract_from_zip(
    source: impl Read + Seek,
    program_name: &str,
    destination: &Path,
    limits: Limits,
) -> Result<u64, ArchiveError> {
    let unreadable = |error: zip::result::ZipError| ArchiveError::Unreadable(error.into());
    let mut archive = zip::ZipArchive::new(source).map_err(unreadable)?;

    // What the headers declare for every file is added up and limited, the same limit that
    // the expanded size of a `.tar.gz` has. Only the program is ever expanded.
    let mut declared = 0_u64;
    let mut found: Option<usize> = None;
    for index in 0..archive.len() {
        let entry = archive.by_index_raw(index).map_err(unreadable)?;
        declared = declared.saturating_add(entry.size());
        if declared > limits.archive_bytes {
            return Err(ArchiveError::ArchiveTooLarge {
                limit: limits.archive_bytes,
            });
        }
        // A name that is not valid text cannot be the program's, whose name is ASCII.
        let Ok(name) = entry.name().map(std::borrow::Cow::into_owned) else {
            continue;
        };
        if !names_the_program(Path::new(&name), program_name) {
            continue;
        }
        if !entry.is_file() || entry.is_symlink() {
            return Err(ArchiveError::NotAPlainFile(name));
        }
        if found.replace(index).is_some() {
            return Err(ArchiveError::ProgramRepeated(program_name.to_owned()));
        }
    }
    let index = found.ok_or_else(|| ArchiveError::ProgramMissing(program_name.to_owned()))?;
    let mut entry = archive.by_index(index).map_err(unreadable)?;
    write_program(&mut entry, destination, limits.program_bytes)
}

/// Whether an archive path is the program: its name, at the top or in one folder.
fn names_the_program(path: &Path, program_name: &str) -> bool {
    let parts: Vec<Component<'_>> = path.components().collect();
    // A leading `./` is how some tools write a top-level file; it is not a folder.
    let parts: Vec<Component<'_>> = parts
        .into_iter()
        .filter(|part| !matches!(part, Component::CurDir))
        .collect();
    let plain = parts
        .iter()
        .all(|part| matches!(part, Component::Normal(_)));
    plain
        && (1..=2).contains(&parts.len())
        && parts
            .last()
            .is_some_and(|last| last.as_os_str() == program_name)
}

fn write_program(
    entry: &mut impl Read,
    destination: &Path,
    limit: u64,
) -> Result<u64, ArchiveError> {
    let destination_error = |source| ArchiveError::Destination {
        path: destination.to_owned(),
        source,
    };
    let mut file = create_program_file(destination).map_err(destination_error)?;
    let copied = copy_within(entry, &mut file, limit, destination);
    let size = match copied.and_then(|size| {
        file.sync_all().map_err(destination_error)?;
        Ok(size)
    }) {
        Ok(size) if (1..=limit).contains(&size) => size,
        Ok(_) => {
            drop(file);
            let _ = fs::remove_file(destination);
            return Err(ArchiveError::BadProgramSize { limit });
        }
        Err(error) => {
            drop(file);
            let _ = fs::remove_file(destination);
            return Err(error);
        }
    };
    Ok(size)
}

/// Copies up to `limit` bytes and one more, so the caller can tell "at the limit" from "over it".
/// A failure to read the archive and a failure to write the file are told apart.
fn copy_within(
    from: &mut impl Read,
    to: &mut File,
    limit: u64,
    destination: &Path,
) -> Result<u64, ArchiveError> {
    let mut buffer = vec![0_u8; COPY_CHUNK];
    let mut total = 0_u64;
    while total <= limit {
        let count = from.read(&mut buffer).map_err(ArchiveError::Unreadable)?;
        if count == 0 {
            break;
        }
        to.write_all(&buffer[..count])
            .map_err(|source| ArchiveError::Destination {
                path: destination.to_owned(),
                source,
            })?;
        total += u64::try_from(count).unwrap_or(u64::MAX);
    }
    Ok(total)
}

fn create_program_file(destination: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o755);
    }
    options.open(destination)
}

#[cfg(test)]
mod tests;
