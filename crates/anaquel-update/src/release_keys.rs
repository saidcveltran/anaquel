//! The public keys every Anaquel release is checked against.
//!
//! Both keys were made on the maintainer's machine with `anaquel-sign keygen`; the
//! private halves never leave it. The active key signs every release manifest. The
//! reserve key signs nothing until the active key is lost or leaked: a version that
//! already trusts it can then be shipped with a new active key. Changing either line
//! here changes who the program believes, so it is a decision, not a refactor.

use crate::error::UpdateError;
use crate::trust::TrustedKeys;

/// Base64 line of the active key `FE3C7F0BC0CD1CD3`, which signs release manifests.
pub const ACTIVE_RELEASE_KEY: &str = "RWTTHM3AC388/uTQuW2oB0+fiNwNCXeykFXfJhgTwp55lguyuFcd1KUZ";

/// Base64 line of the reserve key `37BA8498688F09C1`, kept offline for key replacement.
pub const RESERVE_RELEASE_KEY: &str = "RWTBCY9omIS6N4YCLMH7Bh/bduRHJmpjhEpgjMAYzVClfbREHnib69RQ";

impl TrustedKeys {
    /// The keys a published release is checked against: the active key, then the reserve.
    ///
    /// # Errors
    /// Returns [`UpdateError::BadKey`] if an embedded line is not a minisign public key,
    /// which the tests of this crate rule out.
    pub fn release() -> Result<Self, UpdateError> {
        Self::from_base64(&[ACTIVE_RELEASE_KEY, RESERVE_RELEASE_KEY])
    }
}

#[cfg(test)]
mod tests;
