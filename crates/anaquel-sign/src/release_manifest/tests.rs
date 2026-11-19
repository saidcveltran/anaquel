use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};

use anaquel_update::{Outcome, Request, Target, TrustedKeys, decide, verify_manifest};
use zeroize::Zeroizing;

use super::*;
use crate::KdfCost;
use crate::workflow::{PassphraseSource, keygen, sign_manifest};

/// The SHA-256 of the three bytes `abc`, from FIPS 180-4; it pins the digest code to a
/// value that does not come from the same library.
const SUM_OF_ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

const LINUX: &str = "anaquel-x86_64-unknown-linux-gnu.tar.gz";
const WINDOWS: &str = "anaquel-x86_64-pc-windows-msvc.zip";

/// A scratch folder removed when the test ends, even if it fails.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "anaquel-manifest-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// Writes an archive and, when `sum` is given, the `.sha256` file beside it.
    fn archive(&self, name: &str, bytes: &[u8], sum: Option<&str>) {
        fs::write(self.path(name), bytes).unwrap();
        if let Some(sum) = sum {
            fs::write(self.path(&format!("{name}.sha256")), sum).unwrap();
        }
    }

    /// An archive with the correct `.sha256` file, in the form `dist` writes.
    fn good_archive(&self, name: &str, bytes: &[u8]) {
        self.archive(name, bytes, Some(&format!("{}  {name}\n", sum_of(bytes))));
    }

    fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&self.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn sum_of(bytes: &[u8]) -> String {
    let mut hex = String::new();
    for byte in Sha256::digest(bytes) {
        write!(hex, "{byte:02x}").unwrap();
    }
    hex
}

fn ts(text: &str) -> Timestamp {
    Timestamp::parse(text).unwrap()
}

fn version(text: &str) -> Version {
    Version::parse(text).unwrap()
}

fn input(release: &str) -> ReleaseInput {
    ReleaseInput {
        channel: "stable".to_owned(),
        version: version(release),
        published: ts("2026-11-20T00:00:00Z"),
        issued: ts("2026-11-20T00:00:00Z"),
        expires: ts("2027-01-19T00:00:00Z"),
        base_url: default_base_url(&version(release)),
    }
}

fn built(scratch: &Scratch, previous: Option<&[u8]>, input: &ReleaseInput) -> Value {
    let text = build_release_manifest(previous, input, &scratch.0).unwrap();
    assert!(text.ends_with("}\n"), "one trailing newline");
    serde_json::from_str(&text).unwrap()
}

fn failure(scratch: &Scratch, previous: Option<&[u8]>, input: &ReleaseInput) -> ManifestBuildError {
    build_release_manifest(previous, input, &scratch.0).unwrap_err()
}

// ---- the digest and the shape of what is written ----------------------------------------

#[test]
fn an_archive_is_listed_with_its_known_sum_size_and_download_address() {
    let scratch = Scratch::new("known");
    scratch.good_archive(LINUX, b"abc");
    let manifest = built(&scratch, None, &input("0.2.0"));

    assert_eq!(manifest["schema"], 1);
    assert_eq!(manifest["channel"], "stable");
    assert_eq!(manifest["issued"], "2026-11-20T00:00:00Z");
    assert_eq!(manifest["expires"], "2027-01-19T00:00:00Z");
    let release = &manifest["releases"][0];
    assert_eq!(release["version"], "0.2.0");
    assert_eq!(release["published"], "2026-11-20T00:00:00Z");
    let asset = &release["assets"]["x86_64-unknown-linux-gnu"];
    assert_eq!(asset["sha256"], SUM_OF_ABC);
    assert_eq!(asset["size"], 3);
    assert_eq!(
        asset["url"],
        format!("{RELEASE_DOWNLOAD_PREFIX}v0.2.0/{LINUX}")
    );
}

#[test]
fn the_digest_reads_a_file_larger_than_one_chunk_whole() {
    let scratch = Scratch::new("large");
    let bytes: Vec<u8> = (0..READ_CHUNK * 3 + 17)
        .map(|index| u8::try_from(index % 251).unwrap())
        .collect();
    scratch.good_archive(LINUX, &bytes);
    let manifest = built(&scratch, None, &input("0.2.0"));
    let asset = &manifest["releases"][0]["assets"]["x86_64-unknown-linux-gnu"];
    assert_eq!(asset["sha256"], sum_of(&bytes));
    assert_eq!(asset["size"], bytes.len());
}

#[test]
fn both_archive_kinds_are_found_and_other_files_are_ignored() {
    let scratch = Scratch::new("kinds");
    scratch.good_archive(LINUX, b"linux");
    scratch.good_archive(WINDOWS, b"windows");
    // None of these is an archive of the program.
    fs::write(scratch.path("README.txt"), b"x").unwrap();
    fs::write(scratch.path("anaquel-installer.sh"), b"x").unwrap();
    fs::write(scratch.path("anaquel-.zip"), b"x").unwrap();
    fs::write(scratch.path("Anaquel-x86_64-apple-darwin.tar.gz"), b"x").unwrap();
    fs::write(scratch.path("anaquel-x y.zip"), b"x").unwrap();
    fs::create_dir(scratch.path("anaquel-aarch64-unknown-linux-gnu.tar.gz")).unwrap();

    let manifest = built(&scratch, None, &input("0.2.0"));
    let assets = manifest["releases"][0]["assets"].as_object().unwrap();
    let platforms: Vec<&str> = assets.keys().map(String::as_str).collect();
    assert_eq!(
        platforms,
        ["x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu"]
    );
    assert!(
        assets["x86_64-pc-windows-msvc"]["url"]
            .as_str()
            .unwrap()
            .ends_with(WINDOWS)
    );
}

#[test]
fn the_first_release_has_no_advisories_key_and_the_output_is_stable() {
    let scratch = Scratch::new("first");
    scratch.good_archive(LINUX, b"abc");
    let first = build_release_manifest(None, &input("0.2.0"), &scratch.0).unwrap();
    let second = build_release_manifest(None, &input("0.2.0"), &scratch.0).unwrap();
    assert_eq!(first, second, "the same inputs give the same bytes");
    assert!(!first.contains("advisories"));
    assert!(Manifest::parse(first.as_bytes()).is_ok());
}

// ---- carrying the previous manifest over -----------------------------------------------

const PREVIOUS: &str = r#"{
  "schema": 1,
  "channel": "stable",
  "issued": "2026-11-20T00:00:00Z",
  "expires": "2027-01-19T00:00:00Z",
  "releases": [
    {"version": "0.1.0", "published": "2026-10-01T00:00:00Z",
     "withdrawn": {"reason": "broken installer"},
     "assets": {"x86_64-unknown-linux-gnu": {"url": "https://github.com/saidcveltran/anaquel/releases/download/v0.1.0/anaquel-x86_64-unknown-linux-gnu.tar.gz",
       "sha256": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad", "size": 3}}},
    {"version": "0.1.1", "published": "2026-11-01T00:00:00Z", "assets": {}}
  ],
  "advisories": [
    {"id": "ANAQUEL-2026-001", "severity": "high", "affects": "<0.1.1", "message": "Upgrade."}
  ]
}"#;

#[test]
fn earlier_releases_withdrawals_and_advisories_are_kept_unchanged() {
    let scratch = Scratch::new("carry");
    scratch.good_archive(LINUX, b"abc");
    let manifest = built(&scratch, Some(PREVIOUS.as_bytes()), &input("0.2.0"));
    let before: Value = serde_json::from_str(PREVIOUS).unwrap();

    let releases = manifest["releases"].as_array().unwrap();
    assert_eq!(releases.len(), 3);
    assert_eq!(releases[0], before["releases"][0], "withdrawal kept");
    assert_eq!(releases[1], before["releases"][1]);
    assert_eq!(releases[2]["version"], "0.2.0");
    assert_eq!(manifest["advisories"], before["advisories"]);
}

#[test]
fn a_previous_manifest_with_the_same_issue_time_is_accepted() {
    let scratch = Scratch::new("same-time");
    scratch.good_archive(LINUX, b"abc");
    // `input` is issued exactly when PREVIOUS was: not earlier, so it is allowed.
    assert!(build_release_manifest(Some(PREVIOUS.as_bytes()), &input("0.2.0"), &scratch.0).is_ok());
}

#[test]
fn the_previous_manifest_must_be_one_the_program_accepts() {
    let scratch = Scratch::new("bad-previous");
    scratch.good_archive(LINUX, b"abc");
    for broken in [&b"not json"[..], br#"{"schema":2}"#, b""] {
        assert!(
            matches!(
                failure(&scratch, Some(broken), &input("0.2.0")),
                ManifestBuildError::PreviousInvalid(_)
            ),
            "{:?}",
            String::from_utf8_lossy(broken)
        );
    }
}

#[test]
fn a_new_manifest_may_not_change_the_channel_go_back_in_time_or_repeat_a_version() {
    let scratch = Scratch::new("ties");
    scratch.good_archive(LINUX, b"abc");

    let mut other = input("0.2.0");
    other.channel = "beta".to_owned();
    assert!(matches!(
        failure(&scratch, Some(PREVIOUS.as_bytes()), &other),
        ManifestBuildError::ChannelMismatch { .. }
    ));

    let mut earlier = input("0.2.0");
    earlier.issued = ts("2026-11-19T23:59:59Z");
    assert!(matches!(
        failure(&scratch, Some(PREVIOUS.as_bytes()), &earlier),
        ManifestBuildError::IssuedGoesBack { .. }
    ));

    assert!(matches!(
        failure(&scratch, Some(PREVIOUS.as_bytes()), &input("0.1.1")),
        ManifestBuildError::VersionExists(_)
    ));
}

// ---- what is refused -------------------------------------------------------------------

#[test]
fn an_archive_without_a_sum_file_is_refused() {
    let scratch = Scratch::new("no-sum");
    scratch.archive(LINUX, b"abc", None);
    assert!(matches!(
        failure(&scratch, None, &input("0.2.0")),
        ManifestBuildError::MissingSum(_)
    ));
}

#[test]
fn a_sum_that_differs_from_the_archive_is_refused() {
    let scratch = Scratch::new("mismatch");
    scratch.archive(LINUX, b"abd", Some(&format!("{SUM_OF_ABC}  {LINUX}\n")));
    match failure(&scratch, None, &input("0.2.0")) {
        ManifestBuildError::SumMismatch {
            expected, found, ..
        } => {
            assert_eq!(expected, SUM_OF_ABC);
            assert_eq!(found, sum_of(b"abd"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_sum_file_that_is_not_what_dist_writes_is_refused() {
    let scratch = Scratch::new("bad-sum");
    let upper = SUM_OF_ABC.to_uppercase();
    for text in [
        String::new(),
        "   \n".to_owned(),
        "not-a-sum\n".to_owned(),
        SUM_OF_ABC[..63].to_owned(),
        format!("{SUM_OF_ABC}0"),
        upper,
        format!("{SUM_OF_ABC}  another.tar.gz\n"),
        format!("{SUM_OF_ABC}  {LINUX}  extra\n"),
    ] {
        scratch.archive(LINUX, b"abc", Some(&text));
        assert!(
            matches!(
                failure(&scratch, None, &input("0.2.0")),
                ManifestBuildError::BadSumFile { .. }
            ),
            "{text:?}"
        );
    }
}

#[test]
fn the_sum_file_forms_of_dist_and_of_sha256sum_are_accepted() {
    let scratch = Scratch::new("sum-forms");
    for text in [
        format!("{SUM_OF_ABC}  {LINUX}\n"),
        format!("{SUM_OF_ABC} *{LINUX}\n"),
        format!("{SUM_OF_ABC}\n"),
        SUM_OF_ABC.to_owned(),
    ] {
        scratch.archive(LINUX, b"abc", Some(&text));
        assert!(
            build_release_manifest(None, &input("0.2.0"), &scratch.0).is_ok(),
            "{text:?}"
        );
    }
}

#[test]
fn an_oversized_sum_file_is_refused_without_being_read_whole() {
    let scratch = Scratch::new("big-sum");
    let padding = " ".repeat(usize::try_from(MAX_SUM_FILE_BYTES).unwrap() + 10);
    scratch.archive(LINUX, b"abc", Some(&format!("{SUM_OF_ABC}{padding}")));
    assert!(matches!(
        failure(&scratch, None, &input("0.2.0")),
        ManifestBuildError::BadSumFile { .. }
    ));
}

#[test]
fn the_size_limit_of_an_archive_is_inclusive() {
    let scratch = Scratch::new("limit");
    let file = scratch.path("archive");
    fs::write(&file, b"abc").unwrap();
    assert_eq!(digest_of(&file, 3).unwrap(), (SUM_OF_ABC.to_owned(), 3));
    assert!(matches!(
        digest_of(&file, 2),
        Err(ManifestBuildError::BadSize { size: 3, .. })
    ));
}

#[test]
fn a_sum_file_of_exactly_the_largest_size_is_read_and_one_byte_more_is_not() {
    let scratch = Scratch::new("sum-limit");
    let limit = usize::try_from(MAX_SUM_FILE_BYTES).unwrap();
    let line = format!("{SUM_OF_ABC} ");
    for (padding, accepted) in [(limit - line.len(), true), (limit - line.len() + 1, false)] {
        scratch.archive(
            LINUX,
            b"abc",
            Some(&format!("{line}{}", " ".repeat(padding))),
        );
        assert_eq!(
            build_release_manifest(None, &input("0.2.0"), &scratch.0).is_ok(),
            accepted,
            "padding {padding}"
        );
    }
}

#[test]
fn a_sum_path_that_cannot_be_read_is_an_input_error_not_a_missing_file() {
    let scratch = Scratch::new("sum-dir");
    fs::write(scratch.path(LINUX), b"abc").unwrap();
    fs::create_dir(scratch.path(&format!("{LINUX}.sha256"))).unwrap();
    assert!(matches!(
        failure(&scratch, None, &input("0.2.0")),
        ManifestBuildError::Io { .. }
    ));
}

#[test]
fn an_empty_archive_two_archives_for_one_platform_or_none_are_refused() {
    let scratch = Scratch::new("shapes");
    assert!(matches!(
        failure(&scratch, None, &input("0.2.0")),
        ManifestBuildError::NoArchives(_)
    ));

    scratch.good_archive(LINUX, b"");
    assert!(matches!(
        failure(&scratch, None, &input("0.2.0")),
        ManifestBuildError::BadSize { size: 0, .. }
    ));

    scratch.good_archive(LINUX, b"abc");
    scratch.good_archive("anaquel-x86_64-unknown-linux-gnu.zip", b"abc");
    assert!(matches!(
        failure(&scratch, None, &input("0.2.0")),
        ManifestBuildError::AmbiguousArchive(platform) if platform == "x86_64-unknown-linux-gnu"
    ));
}

#[test]
fn a_missing_folder_is_an_input_error_not_a_panic() {
    let scratch = Scratch::new("missing");
    let error = build_release_manifest(None, &input("0.2.0"), &scratch.path("absent")).unwrap_err();
    assert!(matches!(error, ManifestBuildError::Io { .. }));
}

#[test]
fn a_download_address_outside_the_projects_releases_is_refused() {
    let scratch = Scratch::new("urls");
    scratch.good_archive(LINUX, b"abc");
    let good = default_base_url(&version("0.2.0"));
    for bad in [
        "http://github.com/saidcveltran/anaquel/releases/download/v0.2.0/".to_owned(),
        "https://github.com/someone-else/anaquel/releases/download/v0.2.0/".to_owned(),
        "https://example.com/".to_owned(),
        String::new(),
        good.trim_end_matches('/').to_owned(),
        format!("{good}../v0.1.0/"),
        format!("{good}./"),
        format!("{RELEASE_DOWNLOAD_PREFIX}/v0.2.0/"),
        // The prefix alone names no version folder.
        RELEASE_DOWNLOAD_PREFIX.to_owned(),
        format!("{good}x//"),
        format!("{good}x?y=1/"),
        format!("{good}x#y/"),
        format!("{good}a b/"),
        format!("{good}caf\u{e9}/"),
        format!("{good}a\nb/"),
    ] {
        let mut request = input("0.2.0");
        request.base_url = bad.clone();
        assert!(
            matches!(
                failure(&scratch, None, &request),
                ManifestBuildError::BadBaseUrl(_)
            ),
            "{bad:?}"
        );
    }
    // A longer folder path under the prefix is still the project's.
    let mut nested = input("0.2.0");
    nested.base_url = format!("{good}extra/");
    assert!(build_release_manifest(None, &nested, &scratch.0).is_ok());
}

#[test]
fn a_manifest_the_program_would_refuse_is_never_returned() {
    let scratch = Scratch::new("refused");
    scratch.good_archive(LINUX, b"abc");

    let mut not_later = input("0.2.0");
    not_later.expires = not_later.issued.clone();
    assert!(matches!(
        failure(&scratch, None, &not_later),
        ManifestBuildError::Refused(_)
    ));

    let mut uppercase = input("0.2.0");
    uppercase.channel = "Stable".to_owned();
    assert!(matches!(
        failure(&scratch, None, &uppercase),
        ManifestBuildError::Refused(_)
    ));
}

// ---- writing the file ------------------------------------------------------------------

#[test]
fn the_file_is_written_once_and_an_existing_one_is_never_replaced() {
    let scratch = Scratch::new("write");
    let archives = scratch.path("archives");
    fs::create_dir(&archives).unwrap();
    fs::write(archives.join(LINUX), b"abc").unwrap();
    fs::write(
        archives.join(format!("{LINUX}.sha256")),
        format!("{SUM_OF_ABC}  {LINUX}\n"),
    )
    .unwrap();
    let out = scratch.path("release-manifest.json");

    write_release_manifest(None, &input("0.2.0"), &archives, &out).unwrap();
    let written = fs::read(&out).unwrap();
    assert!(Manifest::parse(&written).is_ok());

    let error = write_release_manifest(None, &input("0.2.0"), &archives, &out).unwrap_err();
    assert!(matches!(
        error,
        ManifestBuildError::File(WorkflowError::AlreadyExists(_))
    ));
    assert_eq!(fs::read(&out).unwrap(), written, "the first file is intact");
}

#[test]
fn a_failed_build_leaves_no_output_file() {
    let scratch = Scratch::new("no-output");
    let archives = scratch.path("archives");
    fs::create_dir(&archives).unwrap();
    fs::write(archives.join(LINUX), b"abc").unwrap();
    let out = scratch.path("release-manifest.json");

    let error = write_release_manifest(None, &input("0.2.0"), &archives, &out).unwrap_err();
    assert!(matches!(error, ManifestBuildError::MissingSum(_)));
    assert!(!out.exists());
    assert_eq!(scratch.names(), ["archives"]);
}

#[test]
fn the_previous_manifest_is_read_from_its_file_with_the_programs_size_limit() {
    let scratch = Scratch::new("previous-file");
    let archives = scratch.path("archives");
    fs::create_dir(&archives).unwrap();
    fs::write(archives.join(LINUX), b"abc").unwrap();
    fs::write(
        archives.join(format!("{LINUX}.sha256")),
        format!("{SUM_OF_ABC}  {LINUX}\n"),
    )
    .unwrap();

    let previous = scratch.path("previous.json");
    fs::write(&previous, PREVIOUS).unwrap();
    let out = scratch.path("next.json");
    write_release_manifest(Some(&previous), &input("0.2.0"), &archives, &out).unwrap();
    let next = Manifest::parse(&fs::read(&out).unwrap()).unwrap();
    assert_eq!(next.releases.len(), 3);

    let huge = scratch.path("huge.json");
    fs::write(&huge, vec![b' '; MAX_MANIFEST_BYTES + 1]).unwrap();
    let error = write_release_manifest(
        Some(&huge),
        &input("0.2.0"),
        &archives,
        &scratch.path("other.json"),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        ManifestBuildError::File(WorkflowError::TooLarge { .. })
    ));

    let error = write_release_manifest(
        Some(&scratch.path("absent.json")),
        &input("0.2.0"),
        &archives,
        &scratch.path("other.json"),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        ManifestBuildError::File(WorkflowError::Io { .. })
    ));
}

// ---- times ---------------------------------------------------------------------------

/// 2026-01-01T00:00:00Z.
const NEW_YEAR: u64 = 1_767_225_600;

fn choice() -> TimeChoice<'static> {
    TimeChoice {
        published: None,
        issued: None,
        expires: None,
        valid_days: 60,
    }
}

#[test]
fn the_clock_gives_the_default_times_and_the_validity_counts_in_whole_days() {
    let (published, issued, expires) = resolve_times(NEW_YEAR, &choice()).unwrap();
    assert_eq!(published.as_str(), "2026-01-01T00:00:00Z");
    assert_eq!(issued.as_str(), "2026-01-01T00:00:00Z");
    // 31 days of January and 28 of February make 59; one more reaches 1 March, two the 2nd.
    assert_eq!(expires.as_str(), "2026-03-02T00:00:00Z");

    let one_day = TimeChoice {
        valid_days: 1,
        ..choice()
    };
    let (_, _, expires) = resolve_times(NEW_YEAR + 3661, &one_day).unwrap();
    assert_eq!(expires.as_str(), "2026-01-02T01:01:01Z");
}

#[test]
fn chosen_times_replace_the_clock_one_by_one() {
    let chosen = TimeChoice {
        published: Some("2026-05-05T05:05:05Z"),
        issued: Some("2026-06-06T06:06:06Z"),
        expires: Some("2026-07-07T07:07:07Z"),
        valid_days: 60,
    };
    let (published, issued, expires) = resolve_times(NEW_YEAR, &chosen).unwrap();
    assert_eq!(published.as_str(), "2026-05-05T05:05:05Z");
    assert_eq!(issued.as_str(), "2026-06-06T06:06:06Z");
    assert_eq!(expires.as_str(), "2026-07-07T07:07:07Z");

    // An expiry alone is allowed: the issue time is the clock.
    let only_expiry = TimeChoice {
        expires: Some("2026-02-02T00:00:00Z"),
        ..choice()
    };
    let (_, issued, expires) = resolve_times(NEW_YEAR, &only_expiry).unwrap();
    assert_eq!(issued.as_str(), "2026-01-01T00:00:00Z");
    assert_eq!(expires.as_str(), "2026-02-02T00:00:00Z");

    // Only the publication time is chosen: the others still come from the clock.
    let only_published = TimeChoice {
        published: Some("2025-12-31T00:00:00Z"),
        ..choice()
    };
    let (published, issued, _) = resolve_times(NEW_YEAR, &only_published).unwrap();
    assert_eq!(published.as_str(), "2025-12-31T00:00:00Z");
    assert_eq!(issued.as_str(), "2026-01-01T00:00:00Z");
}

#[test]
fn unusable_times_and_validities_are_refused() {
    for bad in [
        TimeChoice {
            published: Some("yesterday"),
            ..choice()
        },
        TimeChoice {
            issued: Some("2026-02-30T00:00:00Z"),
            expires: Some("2026-03-01T00:00:00Z"),
            ..choice()
        },
        TimeChoice {
            expires: Some("2026-03-01"),
            ..choice()
        },
        // The issue time chosen, no expiry: the validity would count from the wrong instant.
        TimeChoice {
            issued: Some("2026-02-01T00:00:00Z"),
            ..choice()
        },
        TimeChoice {
            valid_days: 0,
            ..choice()
        },
        TimeChoice {
            valid_days: MAX_VALID_DAYS + 1,
            ..choice()
        },
        TimeChoice {
            valid_days: u64::MAX,
            ..choice()
        },
    ] {
        assert!(
            matches!(
                resolve_times(NEW_YEAR, &bad),
                Err(ManifestBuildError::BadTimes(_))
            ),
            "{bad:?}"
        );
    }
    let longest = TimeChoice {
        valid_days: MAX_VALID_DAYS,
        ..choice()
    };
    assert!(resolve_times(NEW_YEAR, &longest).is_ok());
    // A clock beyond the year 9999 cannot be written in the manifest.
    assert!(resolve_times(u64::MAX, &choice()).is_err());
}

// ---- the whole route -----------------------------------------------------------------

/// Types the same passphrase every time it is asked.
struct Fixed(VecDeque<&'static str>);

impl PassphraseSource for Fixed {
    fn ask(&mut self, _prompt: &str) -> io::Result<Zeroizing<String>> {
        self.0
            .pop_front()
            .map(|text| Zeroizing::new(text.to_owned()))
            .ok_or_else(|| io::Error::other("no more answers"))
    }
}

#[test]
fn a_generated_signed_manifest_is_believed_and_chosen_by_the_program() {
    const PASSPHRASE: &str = "a long enough passphrase";
    let scratch = Scratch::new("route");
    let archives = scratch.path("archives");
    fs::create_dir(&archives).unwrap();
    let windows = b"windows archive bytes";
    for (name, bytes) in [
        (LINUX, &b"linux archive bytes"[..]),
        (WINDOWS, &windows[..]),
    ] {
        fs::write(archives.join(name), bytes).unwrap();
        fs::write(
            archives.join(format!("{name}.sha256")),
            format!("{}  {name}\n", sum_of(bytes)),
        )
        .unwrap();
    }

    // The maintainer's steps: keys, manifest from the archives, signature.
    let keys = scratch.path("keys");
    let report = keygen(
        &keys,
        "test",
        KdfCost::CHEAPEST,
        &mut Fixed(VecDeque::from([PASSPHRASE, PASSPHRASE])),
    )
    .unwrap();
    let (published, issued, expires) = resolve_times(NEW_YEAR, &choice()).unwrap();
    let release = ReleaseInput {
        channel: "stable".to_owned(),
        version: version("0.2.0"),
        published,
        issued,
        expires,
        base_url: default_base_url(&version("0.2.0")),
    };
    let manifest_path = scratch.path("release-manifest.json");
    write_release_manifest(None, &release, &archives, &manifest_path).unwrap();
    let signature_path = sign_manifest(
        &report.key_path,
        &report.public_path,
        &manifest_path,
        "timestamp:1 file:release-manifest.json",
        &mut Fixed(VecDeque::from([PASSPHRASE])),
    )
    .unwrap();

    // The reader's steps with the program's own code.
    let manifest_bytes = fs::read(&manifest_path).unwrap();
    let signature = fs::read_to_string(&signature_path).unwrap();
    let trusted = TrustedKeys::from_base64(&[report.public_key_line.as_str()]).unwrap();
    let believed = verify_manifest(&manifest_bytes, &signature, &trusted).unwrap();
    let request = Request {
        current: version("0.1.0"),
        platform: "x86_64-pc-windows-msvc".to_owned(),
        channel: "stable".to_owned(),
        target: Target::Latest,
        now: ts("2026-01-02T00:00:00Z"),
        last_seen_issued: None,
        allow_downgrade: false,
        url_prefix: RELEASE_DOWNLOAD_PREFIX.to_owned(),
    };
    let Outcome::Install(plan) = decide(&believed, &request).outcome else {
        panic!("the program should install the new release");
    };
    assert_eq!(plan.version, version("0.2.0"));
    assert_eq!(
        plan.asset.url,
        format!("{RELEASE_DOWNLOAD_PREFIX}v0.2.0/{WINDOWS}")
    );
    assert_eq!(plan.asset.sha256, sum_of(windows));
    assert_eq!(plan.asset.size, windows.len() as u64);

    // The negative case: the same signature does not cover a manifest changed afterwards.
    let mut changed = manifest_bytes.clone();
    let position = changed
        .windows(5)
        .position(|part| part == b"0.2.0")
        .unwrap();
    changed[position + 4] = b'1';
    assert!(verify_manifest(&changed, &signature, &trusted).is_err());

    // And a platform the release has no archive for gets no installation.
    let other = Request {
        platform: "aarch64-unknown-linux-gnu".to_owned(),
        ..request
    };
    assert!(matches!(
        decide(&believed, &other).outcome,
        Outcome::Refused(_)
    ));
}
