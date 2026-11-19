//! The procedure against a scripted network, a scratch folder, real archives and a manifest signed
//! with a key made for the test. The real network and the real program check are tested apart.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;

use anaquel_sign::SigningKey;
use anaquel_update::PlanKind;
use sha2::{Digest, Sha256};

use super::*;
use crate::fixtures::{Entry, tar_gz, zip_archive};
use crate::scratch::Scratch;
use crate::state::LAST_SEEN_FILE;

const MANIFEST_ADDRESS: &str =
    "https://github.com/saidcveltran/anaquel/releases/latest/download/release-manifest.json";
const PREFIX: &str = "https://github.com/saidcveltran/anaquel/releases/download/";
const PLATFORM: &str = "x86_64-unknown-linux-gnu";
const NOW: &str = "2026-11-19T12:00:00Z";
const ISSUED: &str = "2026-11-19T10:00:00Z";
const EXPIRES: &str = "2027-01-19T10:00:00Z";
const OLD: &[u8] = b"the program installed now";
const NEW: &[u8] = b"the program of the new release";

fn v(text: &str) -> Version {
    Version::parse(text).unwrap()
}

fn ts(text: &str) -> Timestamp {
    Timestamp::parse(text).unwrap()
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::new();
    for byte in Sha256::digest(bytes) {
        write!(text, "{byte:02x}").unwrap();
    }
    text
}

fn signature_address() -> String {
    format!("{MANIFEST_ADDRESS}.minisig")
}

fn archive_address(version: &str, extension: &str) -> String {
    format!("{PREFIX}v{version}/anaquel-{PLATFORM}.{extension}")
}

// ---- the network ---------------------------------------------------------------------------

/// A network of a few files by address; anything else is "not found". It records what was asked.
#[derive(Default)]
struct Network {
    files: RefCell<BTreeMap<String, Vec<u8>>>,
    asked: RefCell<Vec<String>>,
}

impl Network {
    fn put(&self, address: &str, bytes: Vec<u8>) {
        self.files.borrow_mut().insert(address.to_owned(), bytes);
    }

    fn asked(&self) -> Vec<String> {
        self.asked.borrow().clone()
    }
}

impl Fetch for Network {
    fn fetch(
        &self,
        address: &str,
        max_bytes: u64,
        sink: &mut dyn Write,
    ) -> Result<u64, FetchError> {
        self.asked.borrow_mut().push(address.to_owned());
        let files = self.files.borrow();
        let bytes = files.get(address).ok_or(FetchError::Status(404))?;
        let length = bytes.len() as u64;
        if length > max_bytes {
            return Err(FetchError::TooLarge { limit: max_bytes });
        }
        sink.write_all(bytes).map_err(FetchError::Sink)?;
        Ok(length)
    }
}

// ---- what a release is ---------------------------------------------------------------------

/// One release as the test publishes it.
struct Rel {
    version: &'static str,
    program: &'static [u8],
    /// `tar.gz`, `zip`, or anything else, which is published as bytes no reader understands.
    extension: &'static str,
    /// The name of the file inside the archive.
    entry: &'static str,
    /// The address the manifest gives the archive, when it is not under the project's prefix.
    base: &'static str,
}

impl Rel {
    fn new(version: &'static str) -> Self {
        Self {
            version,
            program: NEW,
            extension: "tar.gz",
            entry: "anaquel",
            base: PREFIX,
        }
    }

    fn archive(&self) -> Vec<u8> {
        let path = format!("anaquel-{PLATFORM}/{}", self.entry);
        let entries = [Entry::File(&path, self.program)];
        match self.extension {
            "tar.gz" => tar_gz(&entries),
            "zip" => zip_archive(&entries),
            _ => b"not an archive of any kind".to_vec(),
        }
    }
}

/// The parts of a manifest the tests change.
struct Doc {
    channel: &'static str,
    issued: &'static str,
    expires: &'static str,
    advisories: &'static str,
}

impl Default for Doc {
    fn default() -> Self {
        Self {
            channel: "stable",
            issued: ISSUED,
            expires: EXPIRES,
            advisories: "[]",
        }
    }
}

fn no_probe() -> Probes {
    Probes::default()
}

/// Records every program check and answers it.
#[derive(Default)]
struct Probes {
    calls: RefCell<Vec<ProbeCall>>,
    refuse_with: Option<&'static str>,
}

struct ProbeCall {
    path: PathBuf,
    expected: Option<Version>,
    content: Vec<u8>,
}

impl Probes {
    fn refusing(reason: &'static str) -> Self {
        Self {
            refuse_with: Some(reason),
            ..Self::default()
        }
    }

    fn run(&self, path: &Path, expected: Option<&Version>) -> Result<(), String> {
        let content = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
        self.calls.borrow_mut().push(ProbeCall {
            path: path.to_owned(),
            expected: expected.cloned(),
            content,
        });
        self.refuse_with
            .map_or(Ok(()), |reason| Err(reason.to_owned()))
    }
}

// ---- the machine ---------------------------------------------------------------------------

/// A scratch folder with an installed program, a state folder, a key and a network.
struct Rig {
    scratch: Scratch,
    network: Network,
    key: SigningKey,
    keys: TrustedKeys,
    installed: PathBuf,
    state: PathBuf,
    program_name: &'static str,
    current: Version,
    now: Timestamp,
}

impl Rig {
    fn new(name: &str) -> Self {
        Self::at(name, "bin/anaquel")
    }

    /// A rig whose installed program is at `relative` inside the scratch folder.
    fn at(name: &str, relative: &str) -> Self {
        let scratch = Scratch::new(name);
        let installed = scratch.path(relative);
        fs::create_dir_all(installed.parent().unwrap()).unwrap();
        fs::write(&installed, OLD).unwrap();
        let key = SigningKey::generate().unwrap();
        let keys = TrustedKeys::from_base64(&[&key.public_key_line()]).unwrap();
        let state = scratch.path("state");
        Self {
            scratch,
            network: Network::default(),
            key,
            keys,
            installed,
            state,
            program_name: "anaquel",
            current: v("0.1.0"),
            now: ts(NOW),
        }
    }

    /// Publishes a signed manifest of `releases` and their archives. Returns the manifest bytes.
    fn serve(&self, doc: &Doc, releases: &[Rel]) -> Vec<u8> {
        let mut entries = Vec::new();
        for release in releases {
            let archive = release.archive();
            let url = format!(
                "{}v{}/anaquel-{PLATFORM}.{}",
                release.base, release.version, release.extension
            );
            entries.push(format!(
                r#"{{"version":"{}","published":"{ISSUED}","assets":{{"{PLATFORM}":{{"url":"{url}","sha256":"{}","size":{}}}}}}}"#,
                release.version,
                hex(&archive),
                archive.len()
            ));
            self.network.put(&url, archive);
        }
        let manifest = format!(
            r#"{{"schema":1,"channel":"{}","issued":"{}","expires":"{}","releases":[{}],"advisories":{}}}"#,
            doc.channel,
            doc.issued,
            doc.expires,
            entries.join(","),
            doc.advisories
        )
        .into_bytes();
        self.network.put(MANIFEST_ADDRESS, manifest.clone());
        let signature = self.key.sign(&manifest, "test manifest").unwrap();
        self.network
            .put(&signature_address(), signature.into_bytes());
        manifest
    }

    fn context<'a>(&'a self, probe: Probe<'a>) -> Context<'a> {
        Context {
            fetch: &self.network,
            keys: &self.keys,
            manifest_address: MANIFEST_ADDRESS,
            url_prefix: PREFIX,
            channel: "stable",
            platform: PLATFORM,
            current: &self.current,
            now: &self.now,
            installed: &self.installed,
            program_name: self.program_name,
            state_dir: Some(&self.state),
            probe,
        }
    }

    /// The names of the files in the folder of the installed program.
    fn beside(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.installed.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn remembered(&self) -> Option<Timestamp> {
        read_last_seen(&self.state).unwrap()
    }

    /// Runs an upgrade with a probe that records into `probes`.
    fn run(&self, probes: &Probes, want: &Want, mode: Mode) -> Result<Report, UpgradeError> {
        upgrade(
            &self.context(&|path, expected| probes.run(path, expected)),
            want,
            mode,
        )
    }
}

fn latest() -> Want {
    Want {
        target: Target::Latest,
        allow_downgrade: false,
    }
}

fn exact(version: &str, allow_downgrade: bool) -> Want {
    Want {
        target: Target::Exact(v(version)),
        allow_downgrade,
    }
}

fn manifest_and_signature_were_asked(rig: &Rig) -> bool {
    rig.network.asked() == [MANIFEST_ADDRESS.to_owned(), signature_address()]
}

fn unchanged(rig: &Rig) {
    assert_eq!(fs::read(&rig.installed).unwrap(), OLD);
    assert_eq!(rig.beside(), ["anaquel"]);
}

// ---- installing ----------------------------------------------------------------------------

#[test]
fn a_newer_release_replaces_the_program_and_the_old_one_is_kept() {
    let rig = Rig::new("install-tar");
    rig.serve(&Doc::default(), &[Rel::new("0.2.0")]);
    let probes = no_probe();

    let report = rig.run(&probes, &latest(), Mode::Install).unwrap();

    let Verdict::Installed { plan, kept } = report.verdict else {
        panic!("{:?}", report.verdict);
    };
    assert_eq!(plan.version, v("0.2.0"));
    assert_eq!(plan.kind, PlanKind::Upgrade);
    assert_eq!(kept, previous_path(&rig.installed));
    assert_eq!(fs::read(&rig.installed).unwrap(), NEW);
    assert_eq!(fs::read(&kept).unwrap(), OLD);
    assert_eq!(rig.beside(), ["anaquel", "anaquel.previous"]);
    assert!(report.replay_protection);
    assert_eq!(
        rig.network.asked(),
        [
            MANIFEST_ADDRESS.to_owned(),
            signature_address(),
            archive_address("0.2.0", "tar.gz")
        ],
        "manifest, then its signature, then the one archive"
    );
    // The new program was checked where it waited, with the content that was in the archive and
    // the version the manifest promised, before it took the place of the old one.
    let calls = probes.calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].path, staged_path(&rig.installed));
    assert_eq!(calls[0].expected, Some(v("0.2.0")));
    assert_eq!(calls[0].content, NEW);
    assert_eq!(rig.remembered(), Some(ts(ISSUED)));
}

#[test]
fn a_windows_zip_is_installed_the_same_way() {
    let mut rig = Rig::new("install-zip");
    rig.program_name = "anaquel.exe";
    let release = Rel {
        extension: "zip",
        entry: "anaquel.exe",
        ..Rel::new("0.2.0")
    };
    rig.serve(&Doc::default(), &[release]);

    let report = rig.run(&no_probe(), &latest(), Mode::Install).unwrap();

    assert!(
        matches!(report.verdict, Verdict::Installed { .. }),
        "{:?}",
        report.verdict
    );
    assert_eq!(fs::read(&rig.installed).unwrap(), NEW);
    assert_eq!(rig.beside(), ["anaquel", "anaquel.previous"]);
}

#[test]
fn files_left_by_a_crashed_upgrade_are_cleared_and_do_not_stop_the_next_one() {
    let rig = Rig::new("install-after-crash");
    rig.serve(&Doc::default(), &[Rel::new("0.2.0")]);
    fs::write(download_path(&rig.installed), b"half a download").unwrap();
    fs::write(staged_path(&rig.installed), b"half a program").unwrap();

    rig.run(&no_probe(), &latest(), Mode::Install).unwrap();

    assert_eq!(fs::read(&rig.installed).unwrap(), NEW);
    assert_eq!(rig.beside(), ["anaquel", "anaquel.previous"]);
}

#[test]
fn a_leftover_that_cannot_be_removed_stops_the_upgrade_before_the_archive_is_downloaded() {
    let rig = Rig::new("stuck-leftover");
    rig.serve(&Doc::default(), &[Rel::new("0.2.0")]);
    // A folder where the program waits for its swap cannot be removed as a file.
    let staged = staged_path(&rig.installed);
    fs::create_dir(&staged).unwrap();

    let error = rig.run(&no_probe(), &latest(), Mode::Install).unwrap_err();

    assert!(
        matches!(&error, UpgradeError::Io { path, .. } if *path == staged),
        "{error:?}"
    );
    assert!(
        manifest_and_signature_were_asked(&rig),
        "the archive must not be downloaded"
    );
    assert_eq!(fs::read(&rig.installed).unwrap(), OLD);
}

#[test]
fn a_release_that_is_not_newer_installs_nothing() {
    let mut rig = Rig::new("up-to-date");
    rig.current = v("0.2.0");
    rig.serve(&Doc::default(), &[Rel::new("0.2.0")]);

    let report = rig.run(&no_probe(), &latest(), Mode::Install).unwrap();

    assert!(
        matches!(report.verdict, Verdict::UpToDate),
        "{:?}",
        report.verdict
    );
    assert!(
        manifest_and_signature_were_asked(&rig),
        "no archive may be fetched"
    );
    unchanged(&rig);
}

#[test]
fn a_downgrade_needs_to_be_allowed_and_a_reinstall_does_not() {
    let mut rig = Rig::new("downgrade");
    rig.current = v("0.3.0");
    rig.serve(&Doc::default(), &[Rel::new("0.2.0"), Rel::new("0.3.0")]);

    let refused = rig
        .run(&no_probe(), &exact("0.2.0", false), Mode::Install)
        .unwrap();
    assert!(
        matches!(
            refused.verdict,
            Verdict::Refused(Refusal::DowngradeNotAllowed { .. })
        ),
        "{:?}",
        refused.verdict
    );
    unchanged(&rig);

    let allowed = rig
        .run(&no_probe(), &exact("0.2.0", true), Mode::Install)
        .unwrap();
    let Verdict::Installed { plan, .. } = allowed.verdict else {
        panic!("{:?}", allowed.verdict);
    };
    assert_eq!(plan.kind, PlanKind::Downgrade);

    let again = rig
        .run(&no_probe(), &exact("0.3.0", false), Mode::Install)
        .unwrap();
    let Verdict::Installed { plan, .. } = again.verdict else {
        panic!("{:?}", again.verdict);
    };
    assert_eq!(plan.kind, PlanKind::Reinstall);
}

// ---- checking ------------------------------------------------------------------------------

#[test]
fn a_check_says_what_would_be_installed_and_writes_nothing_beside_the_program() {
    let rig = Rig::new("check");
    rig.serve(&Doc::default(), &[Rel::new("0.2.0")]);
    let probes = no_probe();

    let report = rig.run(&probes, &latest(), Mode::Check).unwrap();

    let Verdict::WouldInstall { plan, blocker } = report.verdict else {
        panic!("{:?}", report.verdict);
    };
    assert_eq!(plan.version, v("0.2.0"));
    assert_eq!(blocker, None);
    assert!(
        manifest_and_signature_were_asked(&rig),
        "a check downloads no archive"
    );
    assert!(probes.calls.borrow().is_empty());
    unchanged(&rig);
    // The time of a believed manifest is remembered even by a check.
    assert_eq!(rig.remembered(), Some(ts(ISSUED)));
}

#[test]
fn a_check_names_the_package_manager_that_owns_the_program() {
    let rig = Rig::at("check-managed", "scoop/apps/anaquel/current/anaquel");
    rig.serve(&Doc::default(), &[Rel::new("0.2.0")]);

    let report = rig.run(&no_probe(), &latest(), Mode::Check).unwrap();

    let Verdict::WouldInstall { blocker, .. } = report.verdict else {
        panic!("{:?}", report.verdict);
    };
    assert_eq!(
        blocker,
        Some(Blocker::PackageManager(PackageManager::Scoop))
    );
}

// ---- what stops an upgrade before anything is downloaded -----------------------------------

#[test]
fn a_program_owned_by_a_package_manager_is_not_touched_and_nothing_is_downloaded() {
    let rig = Rig::at("managed", "scoop/apps/anaquel/current/anaquel");
    rig.serve(&Doc::default(), &[Rel::new("0.2.0")]);

    let error = rig.run(&no_probe(), &latest(), Mode::Install).unwrap_err();

    assert!(
        matches!(
            error,
            UpgradeError::Blocked(Blocker::PackageManager(PackageManager::Scoop))
        ),
        "{error:?}"
    );
    assert_eq!(
        rig.network.asked(),
        Vec::<String>::new(),
        "nothing may be downloaded"
    );
    assert_eq!(fs::read(&rig.installed).unwrap(), OLD);
}

#[test]
fn a_folder_that_cannot_be_written_to_stops_the_upgrade_before_any_download() {
    let rig = Rig::new("not-writable");
    // The installed program's folder is gone, so no file can be made in it.
    let missing = rig.scratch.path("gone/anaquel");
    rig.serve(&Doc::default(), &[Rel::new("0.2.0")]);
    let context = Context {
        installed: &missing,
        ..rig.context(&|_, _| Ok(()))
    };

    let error = upgrade(&context, &latest(), Mode::Install).unwrap_err();

    assert!(
        matches!(error, UpgradeError::Blocked(Blocker::NotWritable(_))),
        "{error:?}"
    );
    assert_eq!(
        rig.network.asked(),
        Vec::<String>::new(),
        "nothing may be downloaded"
    );
}

// ---- what is not believed ------------------------------------------------------------------

#[test]
fn a_manifest_signed_by_another_key_is_refused_and_nothing_is_remembered_or_installed() {
    let rig = Rig::new("other-key");
    let manifest = rig.serve(&Doc::default(), &[Rel::new("0.2.0")]);
    let stranger = SigningKey::generate().unwrap();
    rig.network.put(
        &signature_address(),
        stranger.sign(&manifest, "forged").unwrap().into_bytes(),
    );

    let error = rig.run(&no_probe(), &latest(), Mode::Install).unwrap_err();

    assert!(
        matches!(
            error,
            UpgradeError::Untrusted(UpdateError::UntrustedSignature)
        ),
        "{error:?}"
    );
    assert!(manifest_and_signature_were_asked(&rig));
    assert_eq!(rig.remembered(), None);
    unchanged(&rig);
}

#[test]
fn a_manifest_changed_after_it_was_signed_is_refused() {
    let rig = Rig::new("tampered");
    let manifest = rig.serve(&Doc::default(), &[Rel::new("0.2.0")]);
    let tampered = String::from_utf8(manifest)
        .unwrap()
        .replace("0.2.0", "0.9.0");
    rig.network.put(MANIFEST_ADDRESS, tampered.into_bytes());

    let error = rig.run(&no_probe(), &latest(), Mode::Install).unwrap_err();

    assert!(
        matches!(
            error,
            UpgradeError::Untrusted(UpdateError::UntrustedSignature)
        ),
        "{error:?}"
    );
    assert_eq!(rig.remembered(), None);
    unchanged(&rig);
}

#[test]
fn a_signature_file_that_is_not_text_is_refused() {
    let rig = Rig::new("binary-signature");
    rig.serve(&Doc::default(), &[Rel::new("0.2.0")]);
    rig.network
        .put(&signature_address(), vec![0xff, 0xfe, 0x00]);

    let error = rig.run(&no_probe(), &latest(), Mode::Install).unwrap_err();

    assert!(matches!(error, UpgradeError::SignatureNotText), "{error:?}");
    unchanged(&rig);
}

#[test]
fn a_missing_manifest_or_signature_is_reported_as_the_file_that_is_missing() {
    let rig = Rig::new("missing-files");
    let error = rig.run(&no_probe(), &latest(), Mode::Install).unwrap_err();
    assert!(
        matches!(
            error,
            UpgradeError::Download {
                what: "the release manifest",
                source: FetchError::Status(404)
            }
        ),
        "{error:?}"
    );
    assert_eq!(rig.network.asked(), [MANIFEST_ADDRESS.to_owned()]);

    rig.serve(&Doc::default(), &[Rel::new("0.2.0")]);
    rig.network.files.borrow_mut().remove(&signature_address());
    let error = rig.run(&no_probe(), &latest(), Mode::Install).unwrap_err();
    assert!(
        matches!(
            error,
            UpgradeError::Download {
                what: "its signature",
                ..
            }
        ),
        "{error:?}"
    );
    unchanged(&rig);
}

#[test]
fn an_expired_manifest_installs_nothing_and_is_not_remembered() {
    let rig = Rig::new("expired");
    let doc = Doc {
        expires: "2026-11-19T11:00:00Z",
        ..Doc::default()
    };
    rig.serve(&doc, &[Rel::new("0.2.0")]);

    let report = rig.run(&no_probe(), &latest(), Mode::Install).unwrap();

    assert!(
        matches!(report.verdict, Verdict::Refused(Refusal::Expired { .. })),
        "{:?}",
        report.verdict
    );
    assert_eq!(rig.remembered(), None);
    unchanged(&rig);
}

#[test]
fn a_manifest_of_another_release_line_is_refused_and_not_remembered() {
    let rig = Rig::new("wrong-channel");
    let doc = Doc {
        channel: "beta",
        ..Doc::default()
    };
    rig.serve(&doc, &[Rel::new("0.2.0")]);

    let report = rig.run(&no_probe(), &latest(), Mode::Install).unwrap();

    assert!(
        matches!(
            report.verdict,
            Verdict::Refused(Refusal::WrongChannel { .. })
        ),
        "{:?}",
        report.verdict
    );
    assert_eq!(
        rig.remembered(),
        None,
        "a manifest of another line must not set the time"
    );
    unchanged(&rig);
}

#[test]
fn an_older_manifest_replayed_later_is_refused_and_a_newer_one_moves_the_time_forward() {
    let rig = Rig::new("replay");
    rig.serve(&Doc::default(), &[Rel::new("0.2.0")]);
    rig.run(&no_probe(), &latest(), Mode::Check).unwrap();
    assert_eq!(rig.remembered(), Some(ts(ISSUED)));

    let older = Doc {
        issued: "2026-11-19T09:00:00Z",
        ..Doc::default()
    };
    rig.serve(&older, &[Rel::new("0.2.0")]);
    let report = rig.run(&no_probe(), &latest(), Mode::Install).unwrap();
    assert!(
        matches!(report.verdict, Verdict::Refused(Refusal::Stale { .. })),
        "{:?}",
        report.verdict
    );
    assert_eq!(
        rig.remembered(),
        Some(ts(ISSUED)),
        "the time never goes back"
    );
    unchanged(&rig);

    let newer = Doc {
        issued: "2026-11-19T11:00:00Z",
        ..Doc::default()
    };
    rig.serve(&newer, &[Rel::new("0.2.0")]);
    rig.run(&no_probe(), &latest(), Mode::Check).unwrap();
    assert_eq!(rig.remembered(), Some(ts("2026-11-19T11:00:00Z")));
}

#[test]
fn a_manifest_that_is_believed_is_remembered_even_when_it_has_nothing_to_install() {
    let rig = Rig::new("remembered-refusal");
    rig.serve(&Doc::default(), &[Rel::new("0.2.0")]);

    let report = rig
        .run(&no_probe(), &exact("9.9.9", false), Mode::Install)
        .unwrap();

    assert!(
        matches!(report.verdict, Verdict::Refused(Refusal::UnknownVersion(_))),
        "{:?}",
        report.verdict
    );
    assert_eq!(rig.remembered(), Some(ts(ISSUED)));
}

#[test]
fn a_download_address_outside_the_project_is_refused_and_never_fetched() {
    let rig = Rig::new("foreign-url");
    let release = Rel {
        base: "https://example.net/releases/",
        ..Rel::new("0.2.0")
    };
    rig.serve(&Doc::default(), &[release]);

    let report = rig.run(&no_probe(), &latest(), Mode::Install).unwrap();

    assert!(
        matches!(report.verdict, Verdict::Refused(Refusal::ForeignUrl { .. })),
        "{:?}",
        report.verdict
    );
    assert!(manifest_and_signature_were_asked(&rig));
    unchanged(&rig);
}

#[test]
fn a_release_with_no_file_for_this_platform_is_refused() {
    let rig = Rig::new("no-platform");
    rig.serve(&Doc::default(), &[Rel::new("0.2.0")]);
    let context = Context {
        platform: "aarch64-unknown-linux-gnu",
        ..rig.context(&|_, _| Ok(()))
    };

    let report = upgrade(&context, &latest(), Mode::Install).unwrap();

    assert!(
        matches!(
            report.verdict,
            Verdict::Refused(Refusal::NoAssetForPlatform { .. })
        ),
        "{:?}",
        report.verdict
    );
    unchanged(&rig);
}

#[test]
fn advisories_for_the_running_version_are_reported_with_the_result() {
    let rig = Rig::new("advisories");
    let doc = Doc {
        advisories: r#"[{"id":"AQ-1","severity":"high","affects":"<0.2.0","message":"upgrade soon"}]"#,
        ..Doc::default()
    };
    rig.serve(&doc, &[Rel::new("0.2.0")]);

    let report = rig.run(&no_probe(), &latest(), Mode::Check).unwrap();

    assert_eq!(report.advisories.len(), 1);
    assert_eq!(report.advisories[0].id, "AQ-1");
}

// ---- what is wrong with a download ---------------------------------------------------------

#[test]
fn an_archive_that_differs_from_the_manifest_is_refused_and_nothing_is_left() {
    let rig = Rig::new("digest");
    rig.serve(&Doc::default(), &[Rel::new("0.2.0")]);
    let address = archive_address("0.2.0", "tar.gz");
    let mut swapped = rig.network.files.borrow()[&address].clone();
    let middle = swapped.len() / 2;
    swapped[middle] ^= 0xff;
    rig.network.put(&address, swapped);
    let probes = no_probe();

    let error = rig.run(&probes, &latest(), Mode::Install).unwrap_err();

    assert!(
        matches!(
            error,
            UpgradeError::Mismatch(UpdateError::DigestMismatch(_))
        ),
        "{error:?}"
    );
    assert!(probes.calls.borrow().is_empty());
    unchanged(&rig);
}

#[test]
fn an_archive_larger_or_smaller_than_promised_is_refused_and_nothing_is_left() {
    for (name, change) in [("longer", 1_i64), ("shorter", -1)] {
        let rig = Rig::new(&format!("size-{name}"));
        rig.serve(&Doc::default(), &[Rel::new("0.2.0")]);
        let address = archive_address("0.2.0", "tar.gz");
        let mut bytes = rig.network.files.borrow()[&address].clone();
        if change > 0 {
            bytes.push(0);
        } else {
            bytes.pop();
        }
        rig.network.put(&address, bytes);

        let error = rig.run(&no_probe(), &latest(), Mode::Install).unwrap_err();

        match change {
            1 => assert!(
                matches!(
                    error,
                    UpgradeError::Download {
                        what: "the release archive",
                        source: FetchError::TooLarge { .. }
                    }
                ),
                "{name}: {error:?}"
            ),
            _ => assert!(
                matches!(error, UpgradeError::Mismatch(_)),
                "{name}: {error:?}"
            ),
        }
        unchanged(&rig);
    }
}

#[test]
fn a_release_file_of_a_kind_this_program_cannot_read_is_never_downloaded() {
    let rig = Rig::new("unknown-archive");
    let release = Rel {
        extension: "tar.xz",
        ..Rel::new("0.2.0")
    };
    rig.serve(&Doc::default(), &[release]);

    let error = rig.run(&no_probe(), &latest(), Mode::Install).unwrap_err();

    assert!(
        matches!(&error, UpgradeError::UnknownArchive(name) if name == "anaquel-x86_64-unknown-linux-gnu.tar.xz"),
        "{error:?}"
    );
    assert!(manifest_and_signature_were_asked(&rig));
    unchanged(&rig);
}

#[test]
fn an_archive_without_the_program_is_refused_and_nothing_is_left() {
    let rig = Rig::new("no-program");
    let release = Rel {
        entry: "other-tool",
        ..Rel::new("0.2.0")
    };
    rig.serve(&Doc::default(), &[release]);

    let error = rig.run(&no_probe(), &latest(), Mode::Install).unwrap_err();

    assert!(
        matches!(
            error,
            UpgradeError::Archive(ArchiveError::ProgramMissing(_))
        ),
        "{error:?}"
    );
    unchanged(&rig);
}

#[test]
fn a_new_program_that_fails_its_check_never_takes_the_place_of_the_old_one() {
    let rig = Rig::new("probe-fails");
    rig.serve(&Doc::default(), &[Rel::new("0.2.0")]);
    let probes = Probes::refusing("it exited with status 1");

    let error = rig.run(&probes, &latest(), Mode::Install).unwrap_err();

    assert!(
        matches!(&error, UpgradeError::NewProgramRejected(reason) if reason == "it exited with status 1"),
        "{error:?}"
    );
    assert_eq!(probes.calls.borrow().len(), 1);
    unchanged(&rig);
}

#[test]
fn without_a_state_folder_the_upgrade_works_and_says_it_cannot_tell_a_replay() {
    let rig = Rig::new("no-state");
    rig.serve(&Doc::default(), &[Rel::new("0.2.0")]);
    let context = Context {
        state_dir: None,
        ..rig.context(&|_, _| Ok(()))
    };

    let report = upgrade(&context, &latest(), Mode::Install).unwrap();

    assert!(!report.replay_protection);
    assert!(matches!(report.verdict, Verdict::Installed { .. }));
    assert_eq!(fs::read(&rig.installed).unwrap(), NEW);
}

#[test]
fn a_damaged_state_file_stops_the_upgrade_before_the_archive_is_downloaded() {
    let rig = Rig::new("corrupt-state");
    rig.serve(&Doc::default(), &[Rel::new("0.2.0")]);
    fs::create_dir_all(&rig.state).unwrap();
    fs::write(rig.state.join(LAST_SEEN_FILE), b"this is not a time").unwrap();

    let error = rig.run(&no_probe(), &latest(), Mode::Install).unwrap_err();

    assert!(
        matches!(error, UpgradeError::State(StateError::Corrupt { .. })),
        "{error:?}"
    );
    assert!(manifest_and_signature_were_asked(&rig));
    unchanged(&rig);
}

// ---- going back ----------------------------------------------------------------------------

#[test]
fn a_rollback_puts_the_old_program_back_and_a_second_one_goes_forward_again() {
    let rig = Rig::new("rollback");
    rig.serve(&Doc::default(), &[Rel::new("0.2.0")]);
    rig.run(&no_probe(), &latest(), Mode::Install).unwrap();
    let probes = no_probe();
    let check = |path: &Path, expected: Option<&Version>| probes.run(path, expected);

    roll_back(&rig.installed, &check).unwrap();

    assert_eq!(fs::read(&rig.installed).unwrap(), OLD);
    assert_eq!(fs::read(previous_path(&rig.installed)).unwrap(), NEW);
    {
        let calls = probes.calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].path, previous_path(&rig.installed));
        assert_eq!(
            calls[0].expected, None,
            "any version of the previous program will do"
        );
        assert_eq!(
            calls[0].content, OLD,
            "the program checked is the one put back"
        );
    }

    roll_back(&rig.installed, &check).unwrap();
    assert_eq!(fs::read(&rig.installed).unwrap(), NEW);
}

#[test]
fn a_rollback_with_nothing_to_go_back_to_says_so() {
    let rig = Rig::new("rollback-nothing");

    let error = roll_back(&rig.installed, &|_, _| Ok(())).unwrap_err();

    assert!(
        matches!(error, UpgradeError::Swap(SwapError::NothingToRestore(_))),
        "{error:?}"
    );
    unchanged(&rig);
}

#[test]
fn a_previous_program_that_fails_its_check_is_not_put_back() {
    let rig = Rig::new("rollback-broken");
    rig.serve(&Doc::default(), &[Rel::new("0.2.0")]);
    rig.run(&no_probe(), &latest(), Mode::Install).unwrap();

    let error = roll_back(&rig.installed, &|_, _| {
        Err("it exited with status 3".to_owned())
    })
    .unwrap_err();

    assert!(
        matches!(&error, UpgradeError::PreviousRejected(reason) if reason == "it exited with status 3"),
        "{error:?}"
    );
    assert_eq!(fs::read(&rig.installed).unwrap(), NEW);
    assert_eq!(fs::read(previous_path(&rig.installed)).unwrap(), OLD);
}

#[test]
fn a_program_owned_by_a_package_manager_is_not_rolled_back() {
    let rig = Rig::at("rollback-managed", "scoop/apps/anaquel/current/anaquel");
    fs::write(previous_path(&rig.installed), b"older").unwrap();

    let error = roll_back(&rig.installed, &|_, _| Ok(())).unwrap_err();

    assert!(
        matches!(
            error,
            UpgradeError::Blocked(Blocker::PackageManager(PackageManager::Scoop))
        ),
        "{error:?}"
    );
    assert_eq!(fs::read(&rig.installed).unwrap(), OLD);
}
