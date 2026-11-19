use std::collections::BTreeMap;

use super::*;
use crate::manifest::{Severity, Withdrawal};

const PLATFORM: &str = "x86_64-unknown-linux-gnu";
const PREFIX: &str = "https://github.com/saidcveltran/anaquel/releases/download/";

fn time(text: &str) -> Timestamp {
    Timestamp::parse(text).unwrap()
}

fn version(text: &str) -> Version {
    Version::parse(text).unwrap()
}

fn release(text: &str) -> Release {
    let asset = Asset {
        url: format!("{PREFIX}v{text}/anaquel-{PLATFORM}.tar.xz"),
        sha256: "0".repeat(64),
        size: 1000,
    };
    Release {
        version: version(text),
        published: time("2026-11-01T00:00:00Z"),
        withdrawn: None,
        assets: BTreeMap::from([(PLATFORM.to_owned(), asset)]),
    }
}

fn manifest(releases: Vec<Release>) -> Manifest {
    Manifest {
        schema: 1,
        channel: "stable".to_owned(),
        issued: time("2026-11-19T00:00:00Z"),
        expires: time("2026-12-19T00:00:00Z"),
        releases,
        advisories: Vec::new(),
    }
}

fn request(current: &str, target: Target) -> Request {
    Request {
        current: version(current),
        platform: PLATFORM.to_owned(),
        channel: "stable".to_owned(),
        target,
        now: time("2026-11-20T00:00:00Z"),
        last_seen_issued: None,
        allow_downgrade: false,
        url_prefix: PREFIX.to_owned(),
    }
}

fn advisory(id: &str, severity: Severity, affects: &str) -> Advisory {
    Advisory {
        id: id.to_owned(),
        severity,
        affects: semver::VersionReq::parse(affects).unwrap(),
        message: "m".to_owned(),
    }
}

fn withdrawn(mut release: Release) -> Release {
    release.withdrawn = Some(Withdrawal {
        reason: "broken".to_owned(),
    });
    release
}

fn refusal(decision: &Decision) -> &Refusal {
    match &decision.outcome {
        Outcome::Refused(refusal) => refusal,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

fn plan(decision: &Decision) -> &Plan {
    match &decision.outcome {
        Outcome::Install(plan) => plan,
        other => panic!("expected an install, got {other:?}"),
    }
}

#[test]
fn upgrades_to_the_newest_stable_release() {
    let m = manifest(vec![release("0.1.1"), release("0.1.3"), release("0.1.2")]);
    let decision = decide(&m, &request("0.1.1", Target::Latest));
    assert_eq!(plan(&decision).version, version("0.1.3"));
    assert_eq!(plan(&decision).kind, PlanKind::Upgrade);
}

#[test]
fn latest_skips_prereleases_and_withdrawn_releases() {
    let m = manifest(vec![
        release("0.1.1"),
        release("0.1.2"),
        release("0.2.0-rc.1"),
        withdrawn(release("0.1.3")),
    ]);
    let decision = decide(&m, &request("0.1.1", Target::Latest));
    assert_eq!(plan(&decision).version, version("0.1.2"));
}

#[test]
fn latest_with_nothing_newer_is_up_to_date_never_a_downgrade() {
    let m = manifest(vec![release("0.1.1"), release("0.1.2")]);
    assert_eq!(
        decide(&m, &request("0.1.2", Target::Latest)).outcome,
        Outcome::UpToDate
    );
    assert_eq!(
        decide(&m, &request("0.3.0", Target::Latest)).outcome,
        Outcome::UpToDate
    );
}

#[test]
fn a_prerelease_of_the_newest_version_upgrades_to_it() {
    let m = manifest(vec![release("0.2.0")]);
    let decision = decide(&m, &request("0.2.0-rc.1", Target::Latest));
    assert_eq!(plan(&decision).version, version("0.2.0"));
}

#[test]
fn no_stable_release_is_refused() {
    let m = manifest(vec![release("0.2.0-rc.1"), withdrawn(release("0.1.0"))]);
    let decision = decide(&m, &request("0.0.9", Target::Latest));
    assert_eq!(refusal(&decision), &Refusal::NoStableRelease);
    let empty = manifest(Vec::new());
    assert_eq!(
        refusal(&decide(&empty, &request("0.0.9", Target::Latest))),
        &Refusal::NoStableRelease
    );
}

#[test]
fn an_exact_target_may_be_a_prerelease() {
    let m = manifest(vec![release("0.1.1"), release("0.2.0-rc.1")]);
    let decision = decide(&m, &request("0.1.1", Target::Exact(version("0.2.0-rc.1"))));
    assert_eq!(plan(&decision).kind, PlanKind::Upgrade);
}

#[test]
fn a_withdrawn_release_is_refused_with_its_reason_even_when_downgrades_are_allowed() {
    let m = manifest(vec![release("0.1.1"), withdrawn(release("0.1.2"))]);
    let mut asked = request("0.1.1", Target::Exact(version("0.1.2")));
    asked.allow_downgrade = true;
    assert_eq!(
        refusal(&decide(&m, &asked)),
        &Refusal::Withdrawn {
            version: version("0.1.2"),
            reason: "broken".to_owned()
        }
    );
}

#[test]
fn an_unknown_version_is_refused() {
    let m = manifest(vec![release("0.1.1")]);
    let decision = decide(&m, &request("0.1.1", Target::Exact(version("9.9.9"))));
    assert_eq!(
        refusal(&decision),
        &Refusal::UnknownVersion(version("9.9.9"))
    );
}

#[test]
fn a_downgrade_needs_permission() {
    let m = manifest(vec![release("0.1.1"), release("0.1.2")]);
    let mut asked = request("0.1.2", Target::Exact(version("0.1.1")));
    assert_eq!(
        refusal(&decide(&m, &asked)),
        &Refusal::DowngradeNotAllowed {
            current: version("0.1.2"),
            target: version("0.1.1")
        }
    );
    asked.allow_downgrade = true;
    let decision = decide(&m, &asked);
    assert_eq!(plan(&decision).kind, PlanKind::Downgrade);
    assert_eq!(plan(&decision).version, version("0.1.1"));
}

#[test]
fn asking_for_the_running_version_reinstalls_it() {
    let m = manifest(vec![release("0.1.2")]);
    let decision = decide(&m, &request("0.1.2", Target::Exact(version("0.1.2"))));
    assert_eq!(plan(&decision).kind, PlanKind::Reinstall);
}

#[test]
fn a_release_without_a_file_for_this_platform_is_refused() {
    let m = manifest(vec![release("0.1.2")]);
    let mut asked = request("0.1.1", Target::Latest);
    asked.platform = "aarch64-apple-darwin".to_owned();
    assert_eq!(
        refusal(&decide(&m, &asked)),
        &Refusal::NoAssetForPlatform {
            version: version("0.1.2"),
            platform: "aarch64-apple-darwin".to_owned()
        }
    );
}

#[test]
fn an_expired_manifest_is_refused_at_and_after_the_expiry_instant() {
    let m = manifest(vec![release("0.1.2")]);
    for now in ["2026-12-19T00:00:00Z", "2027-01-01T00:00:00Z"] {
        let mut asked = request("0.1.1", Target::Latest);
        asked.now = time(now);
        assert_eq!(
            refusal(&decide(&m, &asked)),
            &Refusal::Expired {
                expires: time("2026-12-19T00:00:00Z")
            }
        );
    }
    let mut last_second = request("0.1.1", Target::Latest);
    last_second.now = time("2026-12-18T23:59:59Z");
    assert!(matches!(
        decide(&m, &last_second).outcome,
        Outcome::Install(_)
    ));
}

#[test]
fn an_older_manifest_than_one_already_seen_is_refused_but_the_same_one_is_fine() {
    let m = manifest(vec![release("0.1.2")]);
    let mut asked = request("0.1.1", Target::Latest);
    asked.last_seen_issued = Some(time("2026-11-19T00:00:01Z"));
    assert_eq!(
        refusal(&decide(&m, &asked)),
        &Refusal::Stale {
            issued: time("2026-11-19T00:00:00Z"),
            last_seen: time("2026-11-19T00:00:01Z")
        }
    );
    asked.last_seen_issued = Some(time("2026-11-19T00:00:00Z"));
    assert!(matches!(decide(&m, &asked).outcome, Outcome::Install(_)));
}

#[test]
fn a_manifest_for_another_channel_is_refused() {
    let mut m = manifest(vec![release("0.1.2")]);
    m.channel = "nightly".to_owned();
    assert_eq!(
        refusal(&decide(&m, &request("0.1.1", Target::Latest))),
        &Refusal::WrongChannel {
            wanted: "stable".to_owned(),
            found: "nightly".to_owned()
        }
    );
}

#[test]
fn download_addresses_outside_the_project_are_refused() {
    let hostile = [
        "https://evil.example/anaquel.tar.xz",
        "https://github.com/saidcveltran/anaquel-fork/releases/download/v0.1.2/a",
        "https://github.com/saidcveltran/anaquel/releases/download/",
        "https://github.com/saidcveltran/anaquel/releases/download/../../../../evil/a",
        "https://github.com/saidcveltran/anaquel/releases/download/v1/./a",
        "https://github.com/saidcveltran/anaquel/releases/download/%2e%2e/a",
        "https://github.com/saidcveltran/anaquel/releases/download/%2E%2E%2Fa",
        "https://github.com/saidcveltran/anaquel/releases/download/v1\\a",
        "https://github.com/saidcveltran/anaquel/releases/download/v1/a?x=1",
        "https://github.com/saidcveltran/anaquel/releases/download/v1/a#frag",
        "http://github.com/saidcveltran/anaquel/releases/download/v1/a",
    ];
    for url in hostile {
        let mut r = release("0.1.2");
        r.assets.get_mut(PLATFORM).unwrap().url = url.to_owned();
        let m = manifest(vec![r]);
        assert_eq!(
            refusal(&decide(&m, &request("0.1.1", Target::Latest))),
            &Refusal::ForeignUrl {
                url: url.to_owned()
            },
            "{url}"
        );
    }
}

#[test]
fn a_prefix_that_is_not_a_closed_https_directory_matches_nothing() {
    let m = manifest(vec![release("0.1.2")]);
    for prefix in [
        "https://github.com/saidcveltran/anaquel/releases/download",
        "http://github.com/saidcveltran/anaquel/releases/download/",
        "",
    ] {
        let mut asked = request("0.1.1", Target::Latest);
        asked.url_prefix = prefix.to_owned();
        assert!(
            matches!(refusal(&decide(&m, &asked)), Refusal::ForeignUrl { .. }),
            "{prefix:?}"
        );
    }
}

#[test]
fn advisories_for_the_running_version_come_most_serious_first() {
    let mut m = manifest(vec![release("0.1.2")]);
    m.advisories = vec![
        advisory("B-low", Severity::Low, "<0.1.2"),
        advisory("C-critical", Severity::Critical, "<=0.1.1"),
        advisory("A-low", Severity::Low, "*"),
        advisory("D-not-mine", Severity::Critical, ">=0.1.2"),
    ];
    let decision = decide(&m, &request("0.1.1", Target::Latest));
    let ids: Vec<&str> = decision.advisories.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, ["C-critical", "A-low", "B-low"]);
}

#[test]
fn advisories_are_reported_with_a_refusal_but_not_from_an_unbelieved_manifest() {
    let mut m = manifest(vec![release("0.1.2")]);
    m.advisories = vec![advisory("A", Severity::High, "*")];
    let mut no_asset = request("0.1.1", Target::Latest);
    no_asset.platform = "other".to_owned();
    let decision = decide(&m, &no_asset);
    assert!(matches!(decision.outcome, Outcome::Refused(_)));
    assert_eq!(decision.advisories.len(), 1);

    let mut expired = request("0.1.1", Target::Latest);
    expired.now = time("2030-01-01T00:00:00Z");
    assert_eq!(decide(&m, &expired).advisories, Vec::<Advisory>::new());
}

#[test]
fn refusals_explain_themselves_to_a_person() {
    let text = Refusal::Withdrawn {
        version: version("0.1.2"),
        reason: "broken installer".to_owned(),
    }
    .to_string();
    assert_eq!(text, "version 0.1.2 was withdrawn: broken installer");
    assert!(
        Refusal::Expired {
            expires: time("2026-12-19T00:00:00Z")
        }
        .to_string()
        .contains("2026-12-19T00:00:00Z")
    );
}
