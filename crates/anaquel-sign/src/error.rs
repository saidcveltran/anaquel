use thiserror::Error;

/// Why a key could not be made, opened or used to sign.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SignError {
    /// The operating system could not supply random bytes.
    #[error("no random bytes are available: {0}")]
    Random(String),
    /// The passphrase is too short to protect a release key.
    #[error("the passphrase must be at least {minimum} characters")]
    WeakPassphrase {
        /// Shortest passphrase accepted for a new key.
        minimum: usize,
    },
    /// The text is not an anaquel signing key file.
    #[error("not a signing key file: {0}")]
    KeyFileFormat(String),
    /// The key file was written by a newer program.
    #[error("the key file is version {found}, this program reads version {supported}")]
    UnsupportedKeyFile {
        /// Version found in the file.
        found: u8,
        /// Version this program reads.
        supported: u8,
    },
    /// The file asks for more work or memory than a real key ever does.
    #[error("the key file asks for key stretching beyond the accepted bounds")]
    KdfCostRefused,
    /// Key stretching failed for the given parameters.
    #[error("key stretching failed: {0}")]
    Kdf(String),
    /// The passphrase is wrong or the file was changed; the two cannot be told apart.
    #[error("wrong passphrase, or the key file was changed")]
    WrongPassphraseOrDamaged,
    /// The trusted comment cannot be written on one line.
    #[error("the trusted comment must be one line without control characters")]
    BadTrustedComment,
}
