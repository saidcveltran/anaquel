use thiserror::Error;

/// Why a manifest or a download cannot be trusted.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum UpdateError {
    /// No public key was given to verify with.
    #[error("no trusted key is configured")]
    NoKeys,
    /// A public key is not valid minisign text.
    #[error("trusted key {index} is not a minisign public key: {reason}")]
    BadKey {
        /// Position of the key in the list given.
        index: usize,
        /// What is wrong with it.
        reason: String,
    },
    /// The signature file is not valid minisign text.
    #[error("the signature file is not readable: {0}")]
    BadSignatureFile(String),
    /// No trusted key produced this signature.
    #[error("the signature does not match any trusted key")]
    UntrustedSignature,
    /// The manifest is larger than any real one can be.
    #[error("the manifest is {size} bytes, more than the {limit} allowed")]
    ManifestTooLarge {
        /// Size received.
        size: usize,
        /// Largest size accepted.
        limit: usize,
    },
    /// The signed text is not a valid manifest.
    #[error("the manifest is not valid: {0}")]
    BadManifest(String),
    /// The manifest was written for another format.
    #[error("the manifest uses schema {found}, this program reads schema {expected}")]
    UnsupportedSchema {
        /// Schema found.
        found: u32,
        /// Schema this program reads.
        expected: u32,
    },
    /// A downloaded file is not the one the manifest promised.
    #[error("the download does not match the manifest: {0}")]
    DigestMismatch(String),
}
