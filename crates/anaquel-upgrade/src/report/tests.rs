use std::path::PathBuf;

use anaquel_update::{Advisory, Asset, Plan, Refusal, Severity};
use semver::VersionReq;

use super::*;
use crate::procedure::Blocker;
use crate::swap::PackageManager;

fn v(text: &str) -> Version {
    Version::parse(text).unwrap()
}

fn plan(version: &str, kind: PlanKind) -> Plan {
    Plan {
        version: v(version),
        kind,
        asset: Asset {
            url: "https://github.com/saidcveltran/anaquel/releases/download/v1/a.zip".to_owned(),
            sha256: "0".repeat(64),
            size: 1,
        },
    }
}

fn report(verdict: Verdict) -> Report {
    Report {
        advisories: Vec::new(),
        replay_protection: true,
        verdict,
    }
}

#[test]
fn each_result_is_told_in_one_clear_sentence() {
    let current = v("0.1.0");
    let cases = [
        (Verdict::UpToDate, "anaquel 0.1.0 is up to date."),
        (
            Verdict::Refused(Refusal::NoStableRelease),
            "Not upgrading: the manifest lists no installable stable release.",
        ),
        (
            Verdict::WouldInstall {
                plan: plan("0.2.0", PlanKind::Upgrade),
                blocker: None,
            },
            "anaquel 0.2.0 is available; you have 0.1.0. Run the same command without `--check` to install it.",
        ),
        (
            Verdict::WouldInstall {
                plan: plan("0.0.9", PlanKind::Downgrade),
                blocker: None,
            },
            "anaquel 0.0.9 is older than the 0.1.0 you have. Run the same command without `--check` to install it.",
        ),
        (
            Verdict::WouldInstall {
                plan: plan("0.1.0", PlanKind::Reinstall),
                blocker: None,
            },
            "anaquel 0.1.0 is the version you have. Run the same command without `--check` to install it again.",
        ),
    ];
    for (verdict, expected) in cases {
        let lines = describe(&report(verdict), &current);
        assert_eq!(lines, [expected]);
    }
}

#[test]
fn an_installation_says_what_was_done_and_how_to_go_back() {
    let current = v("0.1.0");
    let kept = PathBuf::from("bin").join("anaquel.previous");
    for (kind, version, first) in [
        (
            PlanKind::Upgrade,
            "0.2.0",
            "Upgraded anaquel from 0.1.0 to 0.2.0.",
        ),
        (
            PlanKind::Downgrade,
            "0.0.9",
            "Installed anaquel 0.0.9 in place of 0.1.0.",
        ),
        (
            PlanKind::Reinstall,
            "0.1.0",
            "Installed anaquel 0.1.0 again.",
        ),
    ] {
        let lines = describe(
            &report(Verdict::Installed {
                plan: plan(version, kind),
                kept: kept.clone(),
            }),
            &current,
        );
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert_eq!(lines[0], first);
        assert!(
            lines[1].contains(&kept.display().to_string()),
            "{}",
            lines[1]
        );
        assert!(
            lines[1].contains("anaquel upgrade --rollback"),
            "{}",
            lines[1]
        );
    }
}

#[test]
fn what_would_stop_an_installation_is_told_after_the_result() {
    let lines = describe(
        &report(Verdict::WouldInstall {
            plan: plan("0.2.0", PlanKind::Upgrade),
            blocker: Some(Blocker::PackageManager(PackageManager::Winget)),
        }),
        &v("0.1.0"),
    );
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(
        lines[1],
        "It cannot be installed here: this program is managed by winget (`winget upgrade anaquel`); update it with that tool."
    );
}

#[test]
fn advisories_come_first_and_the_missing_replay_protection_is_a_warning_at_the_end() {
    let mut full = report(Verdict::UpToDate);
    full.replay_protection = false;
    full.advisories = vec![Advisory {
        id: "AQ-7".to_owned(),
        severity: Severity::Critical,
        affects: VersionReq::parse("<0.2.0").unwrap(),
        message: "stop using it".to_owned(),
    }];

    let lines = describe(&full, &v("0.1.0"));

    assert_eq!(lines.len(), 3, "{lines:?}");
    assert_eq!(lines[0], "advisory AQ-7 (critical): stop using it");
    assert_eq!(lines[1], "anaquel 0.1.0 is up to date.");
    assert!(lines[2].starts_with("warning: "), "{}", lines[2]);
}

#[test]
fn there_is_no_warning_when_the_time_of_the_manifest_is_remembered() {
    let lines = describe(&report(Verdict::UpToDate), &v("0.1.0"));
    assert_eq!(lines.len(), 1);
}
