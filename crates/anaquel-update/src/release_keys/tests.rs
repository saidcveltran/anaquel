use std::fmt::Write as _;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use super::*;

/// The key id as minisign prints it: the eight id bytes read backwards, in uppercase hex.
fn printed_key_id(line: &str) -> String {
    let bytes = STANDARD.decode(line).unwrap();
    assert_eq!(bytes.len(), 42, "a minisign public key is 42 bytes");
    assert_eq!(&bytes[..2], b"Ed", "the algorithm tag");
    let mut id = String::new();
    for byte in bytes[2..10].iter().rev() {
        write!(id, "{byte:02X}").unwrap();
    }
    id
}

#[test]
fn the_embedded_lines_are_the_two_keys_the_maintainer_made() {
    // The ids were printed by `anaquel-sign keygen` apart from the lines themselves,
    // so a line pasted wrongly or swapped cannot match both.
    assert_eq!(printed_key_id(ACTIVE_RELEASE_KEY), "FE3C7F0BC0CD1CD3");
    assert_eq!(printed_key_id(RESERVE_RELEASE_KEY), "37BA8498688F09C1");
}

#[test]
fn the_two_keys_are_different_and_both_load() {
    assert_ne!(ACTIVE_RELEASE_KEY, RESERVE_RELEASE_KEY);
    assert!(TrustedKeys::release().is_ok());
}

/// A manifest on the `fixture` channel, signed once by the maintainer with each real
/// private key through `anaquel-sign sign`. A real client on the `stable` channel
/// refuses it, and it expires on 2026-11-20.
const ACTIVE_SIGNED: &[u8] = include_bytes!("../../tests/fixtures/real-keys/active-signed.json");
const ACTIVE_SIGNATURE: &str =
    include_str!("../../tests/fixtures/real-keys/active-signed.json.minisig");
const RESERVE_SIGNED: &[u8] = include_bytes!("../../tests/fixtures/real-keys/reserve-signed.json");
const RESERVE_SIGNATURE: &str =
    include_str!("../../tests/fixtures/real-keys/reserve-signed.json.minisig");

fn only(line: &str) -> TrustedKeys {
    TrustedKeys::from_base64(&[line]).unwrap()
}

#[test]
fn each_embedded_line_is_the_public_half_of_a_private_key_the_maintainer_holds() {
    // This is what the key id check cannot show: the key bytes themselves are right,
    // because a signature made by the real private key verifies with the line.
    assert!(
        only(ACTIVE_RELEASE_KEY)
            .verify(ACTIVE_SIGNED, ACTIVE_SIGNATURE)
            .is_ok()
    );
    assert!(
        only(RESERVE_RELEASE_KEY)
            .verify(RESERVE_SIGNED, RESERVE_SIGNATURE)
            .is_ok()
    );
    let both = TrustedKeys::release().unwrap();
    assert!(both.verify(ACTIVE_SIGNED, ACTIVE_SIGNATURE).is_ok());
    assert!(both.verify(RESERVE_SIGNED, RESERVE_SIGNATURE).is_ok());
}

#[test]
fn a_signature_does_not_verify_under_the_other_key_or_after_a_change() {
    assert_eq!(
        only(RESERVE_RELEASE_KEY).verify(ACTIVE_SIGNED, ACTIVE_SIGNATURE),
        Err(UpdateError::UntrustedSignature)
    );
    assert_eq!(
        only(ACTIVE_RELEASE_KEY).verify(RESERVE_SIGNED, RESERVE_SIGNATURE),
        Err(UpdateError::UntrustedSignature)
    );
    let mut changed = ACTIVE_SIGNED.to_vec();
    changed[0] = b' ';
    assert_eq!(
        TrustedKeys::release()
            .unwrap()
            .verify(&changed, ACTIVE_SIGNATURE),
        Err(UpdateError::UntrustedSignature)
    );
}

#[test]
fn a_signature_by_a_key_that_is_not_a_release_key_is_refused() {
    // The fixture manifest is signed by the crate's test key, which is not one of the two.
    let manifest = include_bytes!("../../tests/fixtures/release-manifest.json");
    let signature = include_str!("../../tests/fixtures/release-manifest.json.minisig");
    let keys = TrustedKeys::release().unwrap();
    assert_eq!(
        keys.verify(manifest, signature),
        Err(UpdateError::UntrustedSignature)
    );
}
