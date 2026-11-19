use minisign_verify::{PublicKey, Signature};

use crate::error::UpdateError;
use crate::manifest::{MAX_MANIFEST_BYTES, Manifest};

/// The public keys a release manifest may be signed with.
///
/// The program carries an active key and a reserve key so a leaked signing key can
/// be replaced without shipping a new trust anchor. The crate embeds no keys of its
/// own for the checks themselves: [`TrustedKeys::from_base64`] takes whatever keys the
/// caller passes, which keeps tests independent of production keys, and
/// [`TrustedKeys::release`] supplies the real ones.
#[derive(Debug)]
pub struct TrustedKeys {
    keys: Vec<PublicKey>,
}

impl TrustedKeys {
    /// Reads minisign public keys, each given as the base64 line of a `.pub` file.
    ///
    /// # Errors
    /// Returns [`UpdateError::NoKeys`] for an empty list and [`UpdateError::BadKey`]
    /// for the first entry that is not a minisign public key.
    pub fn from_base64(keys: &[&str]) -> Result<Self, UpdateError> {
        if keys.is_empty() {
            return Err(UpdateError::NoKeys);
        }
        let keys = keys
            .iter()
            .enumerate()
            .map(|(index, text)| {
                PublicKey::from_base64(text.trim()).map_err(|e| UpdateError::BadKey {
                    index,
                    reason: e.to_string(),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { keys })
    }

    /// Checks that `signature_text` is a valid signature of `data` by one trusted key.
    ///
    /// Signatures made without pre-hashing are refused: they allow forgeries that a
    /// pre-hashed signature does not, and every current minisign writes the safe form.
    ///
    /// # Errors
    /// Returns [`UpdateError::BadSignatureFile`] when the text is not a signature and
    /// [`UpdateError::UntrustedSignature`] when no trusted key verifies it.
    pub fn verify(&self, data: &[u8], signature_text: &str) -> Result<(), UpdateError> {
        let signature = Signature::decode(signature_text)
            .map_err(|e| UpdateError::BadSignatureFile(e.to_string()))?;
        if self
            .keys
            .iter()
            .any(|key| key.verify(data, &signature, false).is_ok())
        {
            Ok(())
        } else {
            Err(UpdateError::UntrustedSignature)
        }
    }
}

/// Verifies the signature of a manifest and only then reads it.
///
/// The size is checked first so an attacker cannot make the program hash an
/// arbitrarily large body before it has any reason to believe it.
///
/// # Errors
/// Returns the first failure: oversize, unreadable or untrusted signature, then an
/// invalid manifest.
pub fn verify_manifest(
    manifest: &[u8],
    signature_text: &str,
    keys: &TrustedKeys,
) -> Result<Manifest, UpdateError> {
    if manifest.len() > MAX_MANIFEST_BYTES {
        return Err(UpdateError::ManifestTooLarge {
            size: manifest.len(),
            limit: MAX_MANIFEST_BYTES,
        });
    }
    keys.verify(manifest, signature_text)?;
    Manifest::parse(manifest)
}
