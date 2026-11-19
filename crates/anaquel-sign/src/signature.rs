use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use blake2::{Blake2b512, Digest};

use crate::error::SignError;
use crate::key::SigningKey;

impl SigningKey {
    /// Signs `data` and returns the text of a minisign signature file.
    ///
    /// The data is hashed with BLAKE2b-512 first (the `ED` algorithm), and the trusted
    /// comment is covered by a second signature, as minisign does. The older form that
    /// signs the data directly is not written: readers refuse it.
    ///
    /// # Errors
    /// Returns [`SignError::BadTrustedComment`] when the comment is not one plain line.
    pub fn sign(&self, data: &[u8], trusted_comment: &str) -> Result<String, SignError> {
        if trusted_comment.chars().any(is_unsafe_in_comment) {
            return Err(SignError::BadTrustedComment);
        }
        let pair = self.key_pair()?;
        let signature = pair.sk.sign(Blake2b512::digest(data), None);

        let mut covered = signature.to_vec();
        covered.extend_from_slice(trusted_comment.as_bytes());
        let global = pair.sk.sign(&covered, None);

        let mut body = Vec::with_capacity(2 + 8 + signature.len());
        body.extend_from_slice(b"ED");
        body.extend_from_slice(&self.key_id());
        body.extend_from_slice(&signature[..]);
        Ok(format!(
            "untrusted comment: signature from anaquel-sign key {}\n{}\ntrusted comment: {trusted_comment}\n{}\n",
            self.key_id_hex(),
            STANDARD.encode(body),
            STANDARD.encode(global),
        ))
    }
}

/// Control characters, line and paragraph separators, and the bidirectional
/// overrides that can make a comment read differently from what it holds.
fn is_unsafe_in_comment(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{2028}' | '\u{2029}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// `text` without the characters a trusted comment may not hold, so a value taken from
/// outside (such as a file name) can be put in a comment that [`SigningKey::sign`] accepts.
#[must_use]
pub fn plain_comment_text(text: &str) -> String {
    text.chars().filter(|c| !is_unsafe_in_comment(*c)).collect()
}

#[cfg(test)]
mod tests;
