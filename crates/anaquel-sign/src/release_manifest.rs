//! Writes `release-manifest.json` from the archives of a release.
//!
//! The manifest lists every release, so a new one is added to the previous manifest, whose
//! releases and advisories are carried over unchanged. Sizes and SHA-256 sums are computed
//! from the archive files themselves and must agree with the `.sha256` file `dist` wrote
//! beside each one, so a corrupted or swapped archive is found here and not by a user.
//! The result is checked with the parser the program ships before it is returned.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use anaquel_update::{
    MAX_ASSET_BYTES, MAX_MANIFEST_BYTES, Manifest, RELEASE_DOWNLOAD_PREFIX, Timestamp, UpdateError,
};
use semver::Version;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::workflow::{WorkflowError, read_limited, write_new};

/// Largest `.sha256` file read; a real one is under 200 bytes.
const MAX_SUM_FILE_BYTES: u64 = 4096;
/// Archive extensions the upgrade command can unpack, longest first.
const ARCHIVE_EXTENSIONS: [&str; 2] = [".tar.gz", ".zip"];
const READ_CHUNK: usize = 64 * 1024;

/// Why a manifest could not be written.
#[derive(Debug, Error)]
pub enum ManifestBuildError {
    /// The download address is not under the project's release address.
    #[error(
        "the download address `{0}` must start with {RELEASE_DOWNLOAD_PREFIX} and end with `/`"
    )]
    BadBaseUrl(String),
    /// The previous manifest is not one the program would accept.
    #[error("the previous manifest is not valid: {0}")]
    PreviousInvalid(UpdateError),
    /// The previous manifest is for another release line.
    #[error("the previous manifest is for the `{previous}` channel, not `{requested}`")]
    ChannelMismatch {
        /// Channel in the previous manifest.
        previous: String,
        /// Channel asked for.
        requested: String,
    },
    /// The new manifest would be older than the one before it, which clients refuse.
    #[error("the new manifest is dated {requested}, earlier than the previous one ({previous})")]
    IssuedGoesBack {
        /// When the previous manifest was issued.
        previous: Timestamp,
        /// The time asked for.
        requested: Timestamp,
    },
    /// The previous manifest already lists this version.
    #[error("version {0} is already in the previous manifest")]
    VersionExists(Version),
    /// No archive the upgrade command can use is in the folder.
    #[error("no `anaquel-TARGET.tar.gz` or `anaquel-TARGET.zip` archive found in {}", .0.display())]
    NoArchives(PathBuf),
    /// Two archives are for one platform.
    #[error("more than one archive for the platform `{0}`")]
    AmbiguousArchive(String),
    /// An archive has no `.sha256` file beside it.
    #[error("{} has no `.sha256` file beside it", .0.display())]
    MissingSum(PathBuf),
    /// The `.sha256` file is not one `dist` writes.
    #[error("{} is not a SHA-256 sum file: {reason}", path.display())]
    BadSumFile {
        /// The file concerned.
        path: PathBuf,
        /// What is wrong with it.
        reason: &'static str,
    },
    /// The sum in the `.sha256` file differs from the sum of the archive.
    #[error("{} has the sum {found}, but the sum in its `.sha256` file is {expected}", file.display())]
    SumMismatch {
        /// The archive concerned.
        file: PathBuf,
        /// The sum the `.sha256` file promises.
        expected: String,
        /// The sum of the archive.
        found: String,
    },
    /// An archive is empty or larger than the program will download.
    #[error("{} is {size} bytes; an archive must be between 1 and {MAX_ASSET_BYTES}", .file.display())]
    BadSize {
        /// The archive concerned.
        file: PathBuf,
        /// Its size.
        size: u64,
    },
    /// A file or folder could not be read.
    #[error("{path}: {source}", path = path.display())]
    Io {
        /// The file concerned.
        path: PathBuf,
        /// What the system said.
        source: io::Error,
    },
    /// A time or a validity was not usable.
    #[error("{0}")]
    BadTimes(String),
    /// The previous manifest could not be read, or the output could not be written.
    #[error(transparent)]
    File(#[from] WorkflowError),
    /// The finished manifest is not one the program would accept.
    #[error("the manifest written is not valid and was not saved: {0}")]
    Refused(UpdateError),
}

/// What a new release needs besides its archives.
#[derive(Debug, Clone)]
pub struct ReleaseInput {
    /// Release line, such as `stable`.
    pub channel: String,
    /// The version being released.
    pub version: Version,
    /// When the release was published.
    pub published: Timestamp,
    /// When this manifest is issued.
    pub issued: Timestamp,
    /// After this instant the manifest is no longer believed.
    pub expires: Timestamp,
    /// The folder address the archives are under, ending in `/`.
    pub base_url: String,
}

/// Longest time a manifest may be valid for, in days. A longer one gives a stolen key or a
/// captured manifest more time to hide a withdrawn release.
pub const MAX_VALID_DAYS: u64 = 365;

/// The times the maintainer chose, each optional, and how long the manifest should last.
#[derive(Debug, Clone, Copy)]
pub struct TimeChoice<'a> {
    /// When the release was published; the clock when absent.
    pub published: Option<&'a str>,
    /// When the manifest is issued; the clock when absent.
    pub issued: Option<&'a str>,
    /// When it expires; `valid_days` after the clock when absent.
    pub expires: Option<&'a str>,
    /// Days from now until expiry, used when `expires` is absent.
    pub valid_days: u64,
}

/// The publication, issue and expiry times, from what was given and the clock.
///
/// # Errors
/// Returns [`ManifestBuildError::BadTimes`] for a time that is not
/// `YYYY-MM-DDTHH:MM:SSZ`, a validity outside 1 to [`MAX_VALID_DAYS`] days, or an issue time
/// given without an expiry (the validity counts from now, not from a chosen time).
pub fn resolve_times(
    now_unix: u64,
    choice: &TimeChoice<'_>,
) -> Result<(Timestamp, Timestamp, Timestamp), ManifestBuildError> {
    let bad = ManifestBuildError::BadTimes;
    if !(1..=MAX_VALID_DAYS).contains(&choice.valid_days) {
        return Err(bad(format!(
            "the validity must be between 1 and {MAX_VALID_DAYS} days"
        )));
    }
    let parse = |text: &str| Timestamp::parse(text).map_err(ManifestBuildError::BadTimes);
    let now =
        crate::calendar::timestamp_from_unix(now_unix).map_err(ManifestBuildError::BadTimes)?;
    let published = choice.published.map_or_else(|| Ok(now.clone()), parse)?;
    let issued = choice.issued.map_or_else(|| Ok(now.clone()), parse)?;
    let expires = match (choice.expires, choice.issued) {
        (Some(text), _) => parse(text)?,
        (None, None) => {
            let seconds = choice
                .valid_days
                .checked_mul(86_400)
                .and_then(|span| now_unix.checked_add(span))
                .ok_or_else(|| bad("the expiry is too far away".to_owned()))?;
            crate::calendar::timestamp_from_unix(seconds).map_err(ManifestBuildError::BadTimes)?
        }
        (None, Some(_)) => {
            return Err(bad(
                "give the expiry as well when the issue time is chosen".to_owned()
            ));
        }
    };
    Ok((published, issued, expires))
}

/// The download address of a version's archives: `{prefix}vX.Y.Z/`.
#[must_use]
pub fn default_base_url(version: &Version) -> String {
    format!("{RELEASE_DOWNLOAD_PREFIX}v{version}/")
}

/// A manifest listing every release of `previous` and the new one found in `archives_dir`.
///
/// `previous` is the manifest of the last release, or `None` for the first one. It is
/// checked for shape, not for a signature: it is the maintainer's own earlier file.
///
/// # Errors
/// See [`ManifestBuildError`].
pub fn build_release_manifest(
    previous: Option<&[u8]>,
    input: &ReleaseInput,
    archives_dir: &Path,
) -> Result<String, ManifestBuildError> {
    check_base_url(&input.base_url)?;
    let (mut releases, advisories) = carry_over(previous, input)?;

    let archives = scan_archives(archives_dir)?;
    let mut assets = Map::new();
    for (platform, file_name) in archives {
        let path = archives_dir.join(&file_name);
        let (sha256, size) = digest_of(&path, MAX_ASSET_BYTES)?;
        check_sum_file(&path, &sha256)?;
        assets.insert(
            platform,
            json!({
                "url": format!("{}{file_name}", input.base_url),
                "sha256": sha256,
                "size": size,
            }),
        );
    }
    releases.push(json!({
        "version": input.version.to_string(),
        "published": input.published.as_str(),
        "assets": assets,
    }));

    let mut manifest = Map::new();
    manifest.insert("schema".to_owned(), json!(1));
    manifest.insert("channel".to_owned(), json!(input.channel));
    manifest.insert("issued".to_owned(), json!(input.issued.as_str()));
    manifest.insert("expires".to_owned(), json!(input.expires.as_str()));
    manifest.insert("releases".to_owned(), Value::Array(releases));
    if let Some(advisories) = advisories {
        manifest.insert("advisories".to_owned(), advisories);
    }
    let mut text = serde_json::to_string_pretty(&Value::Object(manifest)).map_err(|error| {
        // Writing a value tree cannot fail; keep the error visible rather than hide it.
        ManifestBuildError::Refused(UpdateError::BadManifest(error.to_string()))
    })?;
    text.push('\n');
    Manifest::parse(text.as_bytes()).map_err(ManifestBuildError::Refused)?;
    Ok(text)
}

/// Builds the manifest and saves it to `out_path`, which must not exist yet.
///
/// `previous_path` is the manifest of the last release, read with the size limit the
/// program applies to manifests; `None` is for the first release.
///
/// # Errors
/// See [`ManifestBuildError`]. Nothing is written unless the whole manifest was built and
/// accepted.
pub fn write_release_manifest(
    previous_path: Option<&Path>,
    input: &ReleaseInput,
    archives_dir: &Path,
    out_path: &Path,
) -> Result<(), ManifestBuildError> {
    if out_path.exists() {
        return Err(WorkflowError::AlreadyExists(out_path.to_owned()).into());
    }
    let previous = previous_path
        .map(|path| read_limited(path, MAX_MANIFEST_BYTES as u64))
        .transpose()?;
    let text = build_release_manifest(previous.as_deref(), input, archives_dir)?;
    write_new(out_path, text.as_bytes(), false)?;
    Ok(())
}

/// The releases and advisories of the previous manifest, after the checks that tie the new
/// manifest to it: same channel, not older, and a version it does not list yet.
fn carry_over(
    previous: Option<&[u8]>,
    input: &ReleaseInput,
) -> Result<(Vec<Value>, Option<Value>), ManifestBuildError> {
    let Some(bytes) = previous else {
        return Ok((Vec::new(), None));
    };
    let parsed = Manifest::parse(bytes).map_err(ManifestBuildError::PreviousInvalid)?;
    if parsed.channel != input.channel {
        return Err(ManifestBuildError::ChannelMismatch {
            previous: parsed.channel,
            requested: input.channel.clone(),
        });
    }
    if input.issued < parsed.issued {
        return Err(ManifestBuildError::IssuedGoesBack {
            previous: parsed.issued,
            requested: input.issued.clone(),
        });
    }
    if parsed
        .releases
        .iter()
        .any(|release| release.version == input.version)
    {
        return Err(ManifestBuildError::VersionExists(input.version.clone()));
    }
    // The text was just parsed as a manifest, so it is JSON with a `releases` array.
    let value: Value = serde_json::from_slice(bytes).map_err(|error| {
        ManifestBuildError::PreviousInvalid(UpdateError::BadManifest(error.to_string()))
    })?;
    let releases = value
        .get("releases")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok((releases, value.get("advisories").cloned()))
}

fn check_base_url(url: &str) -> Result<(), ManifestBuildError> {
    let plain = url
        .bytes()
        .all(|byte| byte.is_ascii_graphic() && byte != b'?' && byte != b'#');
    // Past the project's prefix there must be at least one folder name, and every name must
    // be a real one: no empty segment (`//`, also right after the prefix) and no `.` or `..`.
    let acceptable = plain
        && url
            .strip_prefix(RELEASE_DOWNLOAD_PREFIX)
            .and_then(|rest| rest.strip_suffix('/'))
            .is_some_and(|folders| {
                folders
                    .split('/')
                    .all(|name| !name.is_empty() && name != "." && name != "..")
            });
    if acceptable {
        Ok(())
    } else {
        Err(ManifestBuildError::BadBaseUrl(url.to_owned()))
    }
}

/// The archives in `dir`, as platform name to file name.
fn scan_archives(dir: &Path) -> Result<BTreeMap<String, String>, ManifestBuildError> {
    let io_error = |source| ManifestBuildError::Io {
        path: dir.to_owned(),
        source,
    };
    let mut found = BTreeMap::new();
    for entry in fs::read_dir(dir).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        if !entry.file_type().map_err(io_error)?.is_file() {
            continue;
        }
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let Some(platform) = platform_of(&name).map(str::to_owned) else {
            continue;
        };
        if found.contains_key(&platform) {
            return Err(ManifestBuildError::AmbiguousArchive(platform));
        }
        found.insert(platform, name);
    }
    if found.is_empty() {
        return Err(ManifestBuildError::NoArchives(dir.to_owned()));
    }
    Ok(found)
}

/// `anaquel-x86_64-pc-windows-msvc.zip` is for `x86_64-pc-windows-msvc`.
fn platform_of(file_name: &str) -> Option<&str> {
    let stem = file_name.strip_prefix("anaquel-")?;
    let platform = ARCHIVE_EXTENSIONS
        .iter()
        .find_map(|extension| stem.strip_suffix(extension))?;
    let valid = !platform.is_empty()
        && platform
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
    valid.then_some(platform)
}

/// SHA-256 and size of a file, read in chunks. The size must be between 1 and `max_bytes`.
fn digest_of(path: &Path, max_bytes: u64) -> Result<(String, u64), ManifestBuildError> {
    let io_error = |source| ManifestBuildError::Io {
        path: path.to_owned(),
        source,
    };
    let mut file = File::open(path).map_err(io_error)?;
    let size = file.metadata().map_err(io_error)?.len();
    if size == 0 || size > max_bytes {
        return Err(ManifestBuildError::BadSize {
            file: path.to_owned(),
            size,
        });
    }
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; READ_CHUNK];
    let mut read_total = 0_u64;
    loop {
        let count = file.read(&mut buffer).map_err(io_error)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
        read_total += u64::try_from(count).unwrap_or(u64::MAX);
    }
    if read_total != size {
        return Err(io_error(io::Error::other(
            "the file changed while it was read",
        )));
    }
    let mut hex = String::with_capacity(64);
    for byte in hasher.finalize() {
        // Writing to a String cannot fail.
        let _ = write!(hex, "{byte:02x}");
    }
    Ok((hex, size))
}

/// Checks the `.sha256` file `dist` wrote beside an archive against the computed sum.
fn check_sum_file(archive: &Path, computed: &str) -> Result<(), ManifestBuildError> {
    let mut name = archive.as_os_str().to_owned();
    name.push(".sha256");
    let path = PathBuf::from(name);
    let bad = |reason| ManifestBuildError::BadSumFile {
        path: path.clone(),
        reason,
    };
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(ManifestBuildError::MissingSum(path));
        }
        Err(source) => return Err(ManifestBuildError::Io { path, source }),
    };
    let mut bytes = Vec::new();
    file.take(MAX_SUM_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| ManifestBuildError::Io {
            path: path.clone(),
            source,
        })?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_SUM_FILE_BYTES {
        return Err(bad("the file is too large"));
    }
    let text = String::from_utf8(bytes).map_err(|_| bad("the file is not text"))?;
    let mut words = text.split_whitespace();
    let expected = words.next().ok_or_else(|| bad("the file is empty"))?;
    let well_formed = expected.len() == 64
        && expected
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if !well_formed {
        return Err(bad("the first word is not 64 lowercase hexadecimal digits"));
    }
    // `dist` writes the file name after the sum; when it is there it must name this archive.
    if let Some(named) = words.next() {
        let archive_name = archive.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if named.trim_start_matches('*') != archive_name || words.next().is_some() {
            return Err(bad("the file names another archive"));
        }
    }
    if expected == computed {
        Ok(())
    } else {
        Err(ManifestBuildError::SumMismatch {
            file: archive.to_owned(),
            expected: expected.to_owned(),
            found: computed.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests;
