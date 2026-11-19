use std::fmt::Write as _;

use sha2::{Digest, Sha256};

use crate::error::UpdateError;
use crate::manifest::Asset;

/// Streaming check of a downloaded file against what the signed manifest promised.
///
/// Feed the bytes as they arrive with [`DigestCheck::update`] so the file never has
/// to sit in memory, and finish with [`DigestCheck::finish`]. A caller may also stop
/// early: [`DigestCheck::over_limit`] reports a body larger than promised, which is
/// the sign of a hostile or broken server, before the rest is downloaded.
#[derive(Debug)]
pub struct DigestCheck {
    hasher: Sha256,
    expected_sha256: String,
    expected_size: u64,
    received: u64,
}

impl DigestCheck {
    /// Starts a check against `asset`.
    #[must_use]
    pub fn new(asset: &Asset) -> Self {
        Self {
            hasher: Sha256::new(),
            expected_sha256: asset.sha256.clone(),
            expected_size: asset.size,
            received: 0,
        }
    }

    /// Adds the next chunk of the download.
    pub fn update(&mut self, chunk: &[u8]) {
        self.hasher.update(chunk);
        self.received = self.received.saturating_add(chunk.len() as u64);
    }

    /// Whether more bytes arrived than the manifest promised.
    #[must_use]
    pub fn over_limit(&self) -> bool {
        self.received > self.expected_size
    }

    /// Ends the check.
    ///
    /// # Errors
    /// Returns [`UpdateError::DigestMismatch`] when the size or the SHA-256 differs.
    pub fn finish(self) -> Result<(), UpdateError> {
        if self.received != self.expected_size {
            return Err(UpdateError::DigestMismatch(format!(
                "expected {} bytes, received {}",
                self.expected_size, self.received
            )));
        }
        let mut actual = String::with_capacity(64);
        for byte in self.hasher.finalize() {
            // Writing to a String cannot fail.
            let _ = write!(actual, "{byte:02x}");
        }
        if actual == self.expected_sha256 {
            Ok(())
        } else {
            Err(UpdateError::DigestMismatch(format!(
                "expected sha256 {}, computed {actual}",
                self.expected_sha256
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SHA-256 of the three bytes `abc`, a published test vector.
    const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn asset(sha256: &str, size: u64) -> Asset {
        Asset {
            url: "https://example.test/a".to_owned(),
            sha256: sha256.to_owned(),
            size,
        }
    }

    #[test]
    fn accepts_the_file_the_manifest_described() {
        let mut check = DigestCheck::new(&asset(ABC_SHA256, 3));
        check.update(b"a");
        check.update(b"bc");
        assert!(!check.over_limit());
        assert_eq!(check.finish(), Ok(()));
    }

    #[test]
    fn refuses_a_changed_byte() {
        let mut check = DigestCheck::new(&asset(ABC_SHA256, 3));
        check.update(b"abd");
        assert!(
            matches!(check.finish(), Err(UpdateError::DigestMismatch(m)) if m.contains("sha256"))
        );
    }

    #[test]
    fn refuses_a_short_and_a_long_file_even_when_the_prefix_matches() {
        let mut short = DigestCheck::new(&asset(ABC_SHA256, 3));
        short.update(b"ab");
        assert!(
            matches!(short.finish(), Err(UpdateError::DigestMismatch(m)) if m.contains("bytes"))
        );

        let mut long = DigestCheck::new(&asset(ABC_SHA256, 3));
        long.update(b"abcd");
        assert!(long.over_limit());
        assert!(matches!(long.finish(), Err(UpdateError::DigestMismatch(_))));
    }

    #[test]
    fn refuses_an_empty_download() {
        let check = DigestCheck::new(&asset(ABC_SHA256, 3));
        assert!(matches!(
            check.finish(),
            Err(UpdateError::DigestMismatch(_))
        ));
    }
}
