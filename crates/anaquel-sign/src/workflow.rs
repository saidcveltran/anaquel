//! The two maintainer commands, written against files and a passphrase source.
//!
//! The program's `main` supplies a terminal prompt; the tests supply a scripted one.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use anaquel_update::{MAX_MANIFEST_BYTES, Manifest, TrustedKeys, UpdateError, verify_manifest};
use thiserror::Error;
use zeroize::Zeroizing;

use crate::error::SignError;
use crate::key::{KdfCost, SigningKey, check_new_passphrase};

/// Largest key file read; a real one is under 300 bytes.
const MAX_KEY_FILE_BYTES: u64 = 4096;
/// Largest public key file read; a real one is about 100 bytes.
const MAX_PUBLIC_FILE_BYTES: u64 = 4096;
/// Longest name for a key.
const MAX_NAME_CHARS: usize = 32;

/// Where passphrases come from.
pub trait PassphraseSource {
    /// Asks for a passphrase without showing it.
    ///
    /// # Errors
    /// Returns the error of the terminal or of whatever supplies the text.
    fn ask(&mut self, prompt: &str) -> io::Result<Zeroizing<String>>;
}

/// Why a command did not finish.
#[derive(Debug, Error)]
pub enum WorkflowError {
    /// A key could not be made, opened or used.
    #[error(transparent)]
    Sign(#[from] SignError),
    /// A file could not be read or written.
    #[error("{path}: {source}", path = path.display())]
    Io {
        /// The file concerned.
        path: PathBuf,
        /// What the system said.
        source: io::Error,
    },
    /// The passphrase could not be asked for.
    #[error("the passphrase could not be read: {0}")]
    Prompt(io::Error),
    /// The two typed passphrases differ.
    #[error("the two passphrases are not the same; nothing was written")]
    PassphraseMismatch,
    /// A command refuses to replace a file.
    #[error("{} already exists; nothing was written", .0.display())]
    AlreadyExists(PathBuf),
    /// The name cannot be part of a file name.
    #[error("the key name `{0}` must be 1 to {MAX_NAME_CHARS} lowercase letters, digits or `-`")]
    BadName(String),
    /// The file is larger than any real one of its kind.
    #[error("{path} is {size} bytes, more than the {limit} allowed", path = path.display())]
    TooLarge {
        /// The file concerned.
        path: PathBuf,
        /// Its size.
        size: u64,
        /// The most accepted.
        limit: u64,
    },
    /// The manifest is not one the program would accept, so signing it would ship a broken release.
    #[error("the manifest is not valid and was not signed: {0}")]
    ManifestRefused(UpdateError),
    /// The signature just made is not accepted by the verifier the program ships.
    #[error("the new signature does not verify, so nothing was written: {0}")]
    SignatureRejected(UpdateError),
    /// The public key file is not a minisign public key file the program can use.
    #[error("{path} is not a usable public key file: {reason}", path = path.display())]
    BadPublicKeyFile {
        /// The file concerned.
        path: PathBuf,
        /// What is wrong with it.
        reason: String,
    },
    /// The private key opened is not the one the public key file names.
    #[error(
        "this private key is not the one the public key file names (it is {opened}, the file has {expected}); nothing was signed"
    )]
    KeyMismatch {
        /// Public key line of the key that was opened.
        opened: String,
        /// Public key line in the public key file.
        expected: String,
    },
    /// The key file just sealed does not open with the passphrase that was typed.
    #[error("the new key file did not open again with the typed passphrase; nothing was written")]
    SealCheckFailed,
}

/// What `keygen` made.
#[derive(Debug, PartialEq, Eq)]
pub struct KeygenReport {
    /// Key id as minisign shows it.
    pub key_id: String,
    /// The base64 line to give to `TrustedKeys::from_base64`.
    pub public_key_line: String,
    /// The passphrase-protected private key file.
    pub key_path: PathBuf,
    /// The public key file.
    pub public_path: PathBuf,
}

/// Makes a key pair named `name` in `dir`: `NAME.key` (private) and `NAME.pub`.
///
/// The passphrase is asked twice and nothing is written unless both agree. Existing
/// files are never replaced.
///
/// # Errors
/// See [`WorkflowError`].
pub fn keygen(
    dir: &Path,
    name: &str,
    cost: KdfCost,
    prompts: &mut dyn PassphraseSource,
) -> Result<KeygenReport, WorkflowError> {
    check_name(name)?;
    let key_path = dir.join(format!("{name}.key"));
    let public_path = dir.join(format!("{name}.pub"));
    for path in [&key_path, &public_path] {
        if path.exists() {
            return Err(WorkflowError::AlreadyExists(path.clone()));
        }
    }

    let first = ask(prompts, "New passphrase: ")?;
    check_new_passphrase(&first)?;
    let second = ask(prompts, "Repeat the passphrase: ")?;
    if first != second {
        return Err(WorkflowError::PassphraseMismatch);
    }

    let key = SigningKey::generate()?;
    let sealed = key.seal(&first, cost)?;
    confirm_sealed(&key, &sealed, &first)?;

    fs::create_dir_all(dir).map_err(io_error(dir))?;
    write_new(&public_path, key.public_key_file().as_bytes(), false)?;
    if let Err(error) = write_new(&key_path, sealed.as_bytes(), true) {
        // Remove only the public file this call just made, so no half pair is left.
        let _ = fs::remove_file(&public_path);
        return Err(error);
    }
    Ok(KeygenReport {
        key_id: key.key_id_hex(),
        public_key_line: key.public_key_line(),
        key_path,
        public_path,
    })
}

/// Signs `manifest_path` with the key in `key_path` and writes `MANIFEST.minisig` beside it.
///
/// `public_path` is the public key file the signature must verify with. The private
/// key is refused unless it is that key, so a wrong file chosen by mistake signs
/// nothing. Before signing, the manifest must be one the program would accept; after
/// signing, the signature must verify with the shipped verifier against that public
/// key. A file that already exists is never replaced.
///
/// # Errors
/// See [`WorkflowError`].
pub fn sign_manifest(
    key_path: &Path,
    public_path: &Path,
    manifest_path: &Path,
    trusted_comment: &str,
    prompts: &mut dyn PassphraseSource,
) -> Result<PathBuf, WorkflowError> {
    let signature_path = signature_path_for(manifest_path);
    if signature_path.exists() {
        return Err(WorkflowError::AlreadyExists(signature_path));
    }
    let manifest = read_limited(manifest_path, MAX_MANIFEST_BYTES as u64)?;
    Manifest::parse(&manifest).map_err(WorkflowError::ManifestRefused)?;
    let expected = read_public_key_line(public_path)?;
    let trusted = TrustedKeys::from_base64(&[expected.as_str()]).map_err(|error| {
        WorkflowError::BadPublicKeyFile {
            path: public_path.to_owned(),
            reason: error.to_string(),
        }
    })?;

    let key_text = read_limited(key_path, MAX_KEY_FILE_BYTES)?;
    let key_text = String::from_utf8(key_text)
        .map_err(|_| SignError::KeyFileFormat("the file is not text".to_owned()))?;
    SigningKey::check_file_format(&key_text)?;
    let passphrase = ask(prompts, "Passphrase: ")?;
    let key = SigningKey::open(&key_text, &passphrase)?;
    let opened = key.public_key_line();
    if opened != expected {
        return Err(WorkflowError::KeyMismatch { opened, expected });
    }

    let signature = key.sign(&manifest, trusted_comment)?;
    verify_manifest(&manifest, &signature, &trusted).map_err(WorkflowError::SignatureRejected)?;

    write_new(&signature_path, signature.as_bytes(), false)?;
    Ok(signature_path)
}

/// Opens the sealed text again with the typed passphrase and checks it is the same key,
/// so a passphrase that cannot be typed back is found before anything is written.
fn confirm_sealed(key: &SigningKey, sealed: &str, passphrase: &str) -> Result<(), WorkflowError> {
    match SigningKey::open(sealed, passphrase) {
        Ok(reopened) if reopened.public_key_line() == key.public_key_line() => Ok(()),
        _ => Err(WorkflowError::SealCheckFailed),
    }
}

/// The key line of a minisign public key file: a comment line, then the base64 key.
fn read_public_key_line(path: &Path) -> Result<String, WorkflowError> {
    let bad = |reason: &str| WorkflowError::BadPublicKeyFile {
        path: path.to_owned(),
        reason: reason.to_owned(),
    };
    let text = String::from_utf8(read_limited(path, MAX_PUBLIC_FILE_BYTES)?)
        .map_err(|_| bad("the file is not text"))?;
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let comment = lines.next().ok_or_else(|| bad("the file is empty"))?;
    if !comment.starts_with("untrusted comment:") {
        return Err(bad("the first line is not a comment"));
    }
    let key = lines.next().ok_or_else(|| bad("the key line is missing"))?;
    if lines.next().is_some() {
        return Err(bad("there is text after the key line"));
    }
    Ok(key.trim().to_owned())
}

/// `release-manifest.json` becomes `release-manifest.json.minisig`.
#[must_use]
pub fn signature_path_for(manifest_path: &Path) -> PathBuf {
    let mut name = manifest_path.as_os_str().to_owned();
    name.push(".minisig");
    PathBuf::from(name)
}

fn check_name(name: &str) -> Result<(), WorkflowError> {
    let valid = !name.is_empty()
        && name.chars().count() <= MAX_NAME_CHARS
        && !name.starts_with('-')
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if valid {
        Ok(())
    } else {
        Err(WorkflowError::BadName(name.to_owned()))
    }
}

fn ask(
    prompts: &mut dyn PassphraseSource,
    prompt: &str,
) -> Result<Zeroizing<String>, WorkflowError> {
    prompts.ask(prompt).map_err(WorkflowError::Prompt)
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> WorkflowError + '_ {
    move |source| WorkflowError::Io {
        path: path.to_owned(),
        source,
    }
}

/// Reads a file after checking its size, so a wrong path cannot fill memory.
fn read_limited(path: &Path, limit: u64) -> Result<Vec<u8>, WorkflowError> {
    let file = File::open(path).map_err(io_error(path))?;
    let size = file.metadata().map_err(io_error(path))?.len();
    if size > limit {
        return Err(WorkflowError::TooLarge {
            path: path.to_owned(),
            size,
            limit,
        });
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error(path))?;
    Ok(bytes)
}

/// Creates `path`, failing if it exists. A private file is readable by its owner only
/// where the system supports that; on Windows it keeps the permissions of its folder.
fn write_new(path: &Path, bytes: &[u8], private: bool) -> Result<(), WorkflowError> {
    create_new_with(path, private, |file| {
        file.write_all(bytes).and_then(|()| file.sync_all())
    })
}

/// Creates `path` and fills it with `write_contents`. If filling fails, the file this call made
/// is removed, so no truncated key or signature is left behind to be mistaken for a
/// real one.
fn create_new_with(
    path: &Path,
    private: bool,
    write_contents: impl FnOnce(&mut File) -> io::Result<()>,
) -> Result<(), WorkflowError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = private;
    let mut file = options.open(path).map_err(|source| {
        if source.kind() == io::ErrorKind::AlreadyExists {
            WorkflowError::AlreadyExists(path.to_owned())
        } else {
            WorkflowError::Io {
                path: path.to_owned(),
                source,
            }
        }
    })?;
    if let Err(source) = write_contents(&mut file) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(WorkflowError::Io {
            path: path.to_owned(),
            source,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests;
