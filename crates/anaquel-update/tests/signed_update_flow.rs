//! End-to-end checks of the trust chain on real minisign fixtures.
//!
//! The fixtures are written by `tools/update_fixtures.py` from public test seeds.
//! Each negative test changes exactly one thing from the accepted case, so a pass
//! cannot come from some other failure.

use anaquel_update::{
    DigestCheck, Outcome, PlanKind, Refusal, Request, Target, Timestamp, TrustedKeys, UpdateError,
    decide, verify_manifest,
};
use semver::Version;

const MANIFEST: &str = include_str!("fixtures/release-manifest.json");
const SIGNED_BY_ACTIVE: &str = include_str!("fixtures/release-manifest.json.minisig");
const SIGNED_BY_RESERVE: &str = include_str!("fixtures/release-manifest.reserve.minisig");
const SIGNED_BY_FOREIGN: &str = include_str!("fixtures/release-manifest.foreign.minisig");
const SIGNED_LEGACY: &str = include_str!("fixtures/release-manifest.legacy.minisig");
const ACTIVE_KEY: &str = include_str!("fixtures/active.pub");
const RESERVE_KEY: &str = include_str!("fixtures/reserve.pub");
const FOREIGN_KEY: &str = include_str!("fixtures/foreign.pub");
const PAYLOAD: &[u8] = include_bytes!("fixtures/payload.bin");

const PLATFORM: &str = "x86_64-unknown-linux-gnu";

/// Builders that may panic on a broken fixture; marked as test code so the
/// workspace's `expect` lint, which exempts tests, applies the same way here.
#[cfg(test)]
mod support {
    use super::*;

    pub fn trusted() -> TrustedKeys {
        TrustedKeys::from_base64(&[ACTIVE_KEY, RESERVE_KEY]).expect("fixture keys are valid")
    }

    pub fn version(text: &str) -> Version {
        Version::parse(text).expect("test version")
    }

    pub fn time(text: &str) -> Timestamp {
        Timestamp::parse(text).expect("test time")
    }

    pub fn request(current: &str, target: Target) -> Request {
        Request {
            current: version(current),
            platform: PLATFORM.to_owned(),
            channel: "stable".to_owned(),
            target,
            now: time("2026-11-20T00:00:00Z"),
            last_seen_issued: None,
            allow_downgrade: false,
            url_prefix: "https://github.com/saidcveltran/anaquel/releases/download/".to_owned(),
        }
    }
}

use support::{request, time, trusted, version};

#[test]
fn the_active_key_and_the_reserve_key_each_verify_their_own_signature() {
    assert!(verify_manifest(MANIFEST.as_bytes(), SIGNED_BY_ACTIVE, &trusted()).is_ok());
    assert!(verify_manifest(MANIFEST.as_bytes(), SIGNED_BY_RESERVE, &trusted()).is_ok());
}

#[test]
fn a_key_that_is_not_trusted_is_refused_even_with_a_valid_signature() {
    assert_eq!(
        verify_manifest(MANIFEST.as_bytes(), SIGNED_BY_FOREIGN, &trusted()).unwrap_err(),
        UpdateError::UntrustedSignature
    );
    let only_reserve = TrustedKeys::from_base64(&[RESERVE_KEY]).expect("key");
    assert_eq!(
        verify_manifest(MANIFEST.as_bytes(), SIGNED_BY_ACTIVE, &only_reserve).unwrap_err(),
        UpdateError::UntrustedSignature
    );
    // The same foreign signature is accepted once its key is trusted: the failure
    // above is about trust, not about the signature being malformed.
    let with_foreign = TrustedKeys::from_base64(&[FOREIGN_KEY]).expect("key");
    assert!(verify_manifest(MANIFEST.as_bytes(), SIGNED_BY_FOREIGN, &with_foreign).is_ok());
}

#[test]
fn a_signature_without_pre_hashing_is_refused() {
    assert_eq!(
        verify_manifest(MANIFEST.as_bytes(), SIGNED_LEGACY, &trusted()).unwrap_err(),
        UpdateError::UntrustedSignature
    );
}

#[test]
fn any_change_to_the_signed_text_breaks_the_signature() {
    let original = MANIFEST.as_bytes();
    for position in [0, original.len() / 2, original.len() - 1] {
        let mut tampered = original.to_vec();
        tampered[position] ^= 0x01;
        assert_eq!(
            verify_manifest(&tampered, SIGNED_BY_ACTIVE, &trusted()).unwrap_err(),
            UpdateError::UntrustedSignature,
            "byte {position}"
        );
    }
    let mut extended = original.to_vec();
    extended.push(b'\n');
    assert_eq!(
        verify_manifest(&extended, SIGNED_BY_ACTIVE, &trusted()).unwrap_err(),
        UpdateError::UntrustedSignature
    );
}

#[test]
fn unsigned_text_is_never_read_as_a_manifest() {
    // Valid JSON of the right schema is still refused for its signature, not parsed.
    let forged = MANIFEST.replace("0.1.2", "9.9.9");
    assert_eq!(
        verify_manifest(forged.as_bytes(), SIGNED_BY_ACTIVE, &trusted()).unwrap_err(),
        UpdateError::UntrustedSignature
    );
    // Garbage that is not JSON gets the same answer: the signature is checked first.
    assert_eq!(
        verify_manifest(b"not json at all", SIGNED_BY_ACTIVE, &trusted()).unwrap_err(),
        UpdateError::UntrustedSignature
    );
}

#[test]
fn a_damaged_signature_file_is_reported_as_unreadable() {
    for text in ["", "garbage", "untrusted comment: x\n!!!\n"] {
        assert!(
            matches!(
                verify_manifest(MANIFEST.as_bytes(), text, &trusted()),
                Err(UpdateError::BadSignatureFile(_))
            ),
            "{text:?}"
        );
    }
}

#[test]
fn the_key_list_is_checked_when_it_is_built() {
    assert_eq!(
        TrustedKeys::from_base64(&[]).unwrap_err(),
        UpdateError::NoKeys
    );
    let error = TrustedKeys::from_base64(&[ACTIVE_KEY, "not a key"]).unwrap_err();
    assert!(
        matches!(error, UpdateError::BadKey { index: 1, .. }),
        "{error:?}"
    );
}

#[test]
fn an_oversized_manifest_is_refused_before_it_is_hashed() {
    let huge = vec![b' '; anaquel_update::MAX_MANIFEST_BYTES + 1];
    assert!(matches!(
        verify_manifest(&huge, SIGNED_BY_ACTIVE, &trusted()),
        Err(UpdateError::ManifestTooLarge { .. })
    ));
}

#[test]
fn the_whole_chain_installs_the_newest_stable_release_and_checks_the_download() {
    let manifest = verify_manifest(MANIFEST.as_bytes(), SIGNED_BY_ACTIVE, &trusted())
        .expect("signed manifest");
    let decision = decide(&manifest, &request("0.1.1", Target::Latest));

    let Outcome::Install(plan) = decision.outcome else {
        panic!("expected an install, got {:?}", decision.outcome);
    };
    // 0.1.3 is newer but withdrawn, so the newest installable release is 0.1.2.
    assert_eq!(plan.version, version("0.1.2"));
    assert_eq!(plan.kind, PlanKind::Upgrade);
    assert_eq!(decision.advisories.len(), 1);
    assert_eq!(decision.advisories[0].id, "ANAQUEL-2026-001");

    let mut download = DigestCheck::new(&plan.asset);
    download.update(PAYLOAD);
    assert_eq!(download.finish(), Ok(()));

    let mut corrupted = PAYLOAD.to_vec();
    corrupted[0] ^= 0xff;
    let mut download = DigestCheck::new(&plan.asset);
    download.update(&corrupted);
    assert!(matches!(
        download.finish(),
        Err(UpdateError::DigestMismatch(_))
    ));
}

#[test]
fn the_chain_refuses_the_withdrawn_release_and_a_replayed_manifest() {
    let manifest = verify_manifest(MANIFEST.as_bytes(), SIGNED_BY_ACTIVE, &trusted())
        .expect("signed manifest");

    let withdrawn = decide(
        &manifest,
        &request("0.1.1", Target::Exact(version("0.1.3"))),
    );
    assert!(matches!(
        withdrawn.outcome,
        Outcome::Refused(Refusal::Withdrawn { .. })
    ));

    let mut replayed = request("0.1.1", Target::Latest);
    replayed.last_seen_issued = Some(time("2026-11-19T00:00:01Z"));
    assert!(matches!(
        decide(&manifest, &replayed).outcome,
        Outcome::Refused(Refusal::Stale { .. })
    ));

    let mut late = request("0.1.1", Target::Latest);
    late.now = time("2027-06-01T00:00:00Z");
    assert!(matches!(
        decide(&manifest, &late).outcome,
        Outcome::Refused(Refusal::Expired { .. })
    ));
}

#[test]
fn the_running_newest_release_is_up_to_date() {
    let manifest = verify_manifest(MANIFEST.as_bytes(), SIGNED_BY_ACTIVE, &trusted())
        .expect("signed manifest");
    let decision = decide(&manifest, &request("0.1.2", Target::Latest));
    assert_eq!(decision.outcome, Outcome::UpToDate);
    assert_eq!(decision.advisories, Vec::new());
}
