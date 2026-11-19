use super::*;

const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// A valid manifest as JSON with `releases` and `extra` filled in by the caller.
fn manifest_json(releases: &str, extra: &str) -> String {
    format!(
        r#"{{"schema":1,"channel":"stable","issued":"2026-11-19T00:00:00Z","expires":"2026-12-19T00:00:00Z","releases":[{releases}]{extra}}}"#
    )
}

fn release(version: &str, assets: &str) -> String {
    format!(r#"{{"version":"{version}","published":"2026-11-18T00:00:00Z","assets":{{{assets}}}}}"#)
}

fn asset(platform: &str, url: &str, sha: &str, size: u64) -> String {
    format!(r#""{platform}":{{"url":"{url}","sha256":"{sha}","size":{size}}}"#)
}

fn good_asset() -> String {
    asset(
        "x86_64-unknown-linux-gnu",
        "https://example.test/anaquel.tar.xz",
        SHA,
        2_000_000,
    )
}

fn parse(text: &str) -> Result<Manifest, UpdateError> {
    Manifest::parse(text.as_bytes())
}

fn bad_reason(text: &str) -> String {
    match parse(text) {
        Err(UpdateError::BadManifest(reason)) => reason,
        other => panic!("expected BadManifest, got {other:?}"),
    }
}

#[test]
fn reads_a_complete_manifest() {
    let text = manifest_json(
        &format!(
            "{},{}",
            release("0.1.1", &good_asset()),
            r#"{"version":"0.1.2","published":"2026-11-19T00:00:00Z","withdrawn":{"reason":"broken installer"},"assets":{}}"#
        ),
        r#","advisories":[{"id":"ANAQUEL-2026-001","severity":"high","affects":"<0.1.2","message":"fixed in 0.1.2"}]"#,
    );
    let manifest = parse(&text).unwrap();
    assert_eq!(manifest.channel, "stable");
    assert_eq!(manifest.releases.len(), 2);
    assert_eq!(
        manifest.releases[1].withdrawn.as_ref().unwrap().reason,
        "broken installer"
    );
    assert_eq!(manifest.advisories[0].severity, Severity::High);
    assert!(
        manifest.advisories[0]
            .affects
            .matches(&Version::new(0, 1, 1))
    );
    assert!(
        !manifest.advisories[0]
            .affects
            .matches(&Version::new(0, 1, 2))
    );
}

#[test]
fn advisories_are_optional() {
    assert!(parse(&manifest_json(&release("0.1.1", &good_asset()), "")).is_ok());
}

#[test]
fn refuses_text_that_is_not_json_or_not_this_format() {
    for text in ["", "not json", "[]", "{}", r#"{"schema":1}"#] {
        assert!(
            matches!(parse(text), Err(UpdateError::BadManifest(_))),
            "{text:?}"
        );
    }
}

#[test]
fn refuses_another_schema() {
    let text = manifest_json("", "").replace(r#""schema":1"#, r#""schema":2"#);
    assert_eq!(
        parse(&text),
        Err(UpdateError::UnsupportedSchema {
            found: 2,
            expected: 1
        })
    );
}

#[test]
fn refuses_a_manifest_over_the_size_limit() {
    let padded = format!(
        "{}{}",
        manifest_json("", ""),
        " ".repeat(MAX_MANIFEST_BYTES)
    );
    assert!(matches!(
        parse(&padded),
        Err(UpdateError::ManifestTooLarge { .. })
    ));
}

#[test]
fn refuses_a_repeated_version() {
    let both = format!(
        "{},{}",
        release("0.1.1", &good_asset()),
        release("0.1.1", &good_asset())
    );
    assert!(bad_reason(&manifest_json(&both, "")).contains("listed twice"));
}

#[test]
fn refuses_an_expiry_that_is_not_after_the_issue_time() {
    let same = manifest_json("", "").replace("2026-12-19T00:00:00Z", "2026-11-19T00:00:00Z");
    assert!(bad_reason(&same).contains("later than"));
    let before = manifest_json("", "").replace("2026-12-19T00:00:00Z", "2026-11-18T00:00:00Z");
    assert!(bad_reason(&before).contains("later than"));
}

#[test]
fn refuses_a_badly_written_time() {
    let text = manifest_json("", "").replace("2026-11-19T00:00:00Z", "yesterday");
    assert!(matches!(parse(&text), Err(UpdateError::BadManifest(_))));
}

#[test]
fn refuses_a_bad_channel() {
    for channel in ["", "Stable", "stable2", "../x"] {
        let text = manifest_json("", "").replace("stable", channel);
        assert!(bad_reason(&text).contains("channel"), "{channel:?}");
    }
}

#[test]
fn refuses_downloads_that_could_not_be_trusted() {
    let cases = [
        // Not https.
        ("http://example.test/a", SHA, 10),
        ("ftp://example.test/a", SHA, 10),
        ("file:///c/a", SHA, 10),
        ("example.test/a", SHA, 10),
        ("https://", SHA, 10),
        // Characters a URL cannot carry safely.
        ("https://example.test/a b", SHA, 10),
        ("https://example.test/a\\nb", SHA, 10),
        ("https://exämple.test/a", SHA, 10),
        // The digest.
        ("https://example.test/a", "ABCDEF", 10),
        ("https://example.test/a", &SHA.to_uppercase(), 10),
        ("https://example.test/a", &SHA[..63], 10),
        ("https://example.test/a", &format!("{SHA}0"), 10),
        ("https://example.test/a", &SHA.replace('0', "g"), 10),
        // The size.
        ("https://example.test/a", SHA, 0),
        ("https://example.test/a", SHA, MAX_ASSET_BYTES + 1),
    ];
    for (url, sha, size) in cases {
        let text = manifest_json(
            &release("0.1.1", &asset("x86_64-unknown-linux-gnu", url, sha, size)),
            "",
        );
        assert!(
            matches!(parse(&text), Err(UpdateError::BadManifest(_))),
            "{url} {sha} {size}"
        );
    }
}

#[test]
fn refuses_a_platform_name_that_is_not_a_target_triple() {
    for platform in ["", "Linux", "x86 64", "../x"] {
        let text = manifest_json(
            &release("0.1.1", &asset(platform, "https://example.test/a", SHA, 10)),
            "",
        );
        assert!(bad_reason(&text).contains("target triple"), "{platform:?}");
    }
}

#[test]
fn refuses_a_withdrawal_without_a_reason() {
    let text = manifest_json(
        r#"{"version":"0.1.1","published":"2026-11-18T00:00:00Z","withdrawn":{"reason":"  "},"assets":{}}"#,
        "",
    );
    assert!(bad_reason(&text).contains("without a reason"));
}

#[test]
fn refuses_a_version_that_is_not_semver_and_a_requirement_that_is_not_one() {
    let version = manifest_json(&release("0.1", &good_asset()), "");
    assert!(matches!(parse(&version), Err(UpdateError::BadManifest(_))));
    let requirement = manifest_json(
        "",
        r#","advisories":[{"id":"x","severity":"low","affects":"newer than","message":"m"}]"#,
    );
    assert!(matches!(
        parse(&requirement),
        Err(UpdateError::BadManifest(_))
    ));
}

#[test]
fn refuses_an_unknown_severity() {
    let text = manifest_json(
        "",
        r#","advisories":[{"id":"x","severity":"catastrophic","affects":"*","message":"m"}]"#,
    );
    assert!(matches!(parse(&text), Err(UpdateError::BadManifest(_))));
}

#[test]
fn severities_are_ordered_from_low_to_critical() {
    assert!(Severity::Low < Severity::Moderate);
    assert!(Severity::Moderate < Severity::High);
    assert!(Severity::High < Severity::Critical);
    assert_eq!(Severity::Critical.to_string(), "critical");
}
