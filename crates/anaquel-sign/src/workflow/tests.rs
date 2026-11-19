use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use super::*;

const PASSPHRASE: &str = "a long enough passphrase";

/// The smallest manifest the program accepts.
const MANIFEST: &str = r#"{"schema":1,"channel":"stable","issued":"2026-11-19T00:00:00Z","expires":"2026-12-19T00:00:00Z","releases":[]}"#;

/// Answers prompts from a list and remembers how many were asked.
struct Script {
    answers: VecDeque<String>,
    asked: usize,
}

impl Script {
    fn new(answers: &[&str]) -> Self {
        Self {
            answers: answers.iter().map(|a| (*a).to_owned()).collect(),
            asked: 0,
        }
    }
}

impl PassphraseSource for Script {
    fn ask(&mut self, _prompt: &str) -> io::Result<Zeroizing<String>> {
        self.asked += 1;
        self.answers
            .pop_front()
            .map(Zeroizing::new)
            .ok_or_else(|| io::Error::other("no more answers"))
    }
}

/// A scratch folder removed when the test ends, even if it fails.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "anaquel-sign-{name}-{}-{}",
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

fn keygen_in(scratch: &Scratch, name: &str) -> KeygenReport {
    let mut prompts = Script::new(&[PASSPHRASE, PASSPHRASE]);
    keygen(&scratch.0, name, KdfCost::CHEAPEST, &mut prompts).unwrap()
}

fn error_of<T: std::fmt::Debug>(result: Result<T, WorkflowError>) -> WorkflowError {
    result.unwrap_err()
}

#[test]
fn keygen_writes_a_private_and_a_public_file_that_belong_together() {
    let scratch = Scratch::new("keygen");
    let mut prompts = Script::new(&[PASSPHRASE, PASSPHRASE]);
    let report = keygen(&scratch.0, "active", KdfCost::CHEAPEST, &mut prompts).unwrap();

    assert_eq!(prompts.asked, 2);
    assert_eq!(scratch.names(), ["active.key", "active.pub"]);
    let public = fs::read_to_string(&report.public_path).unwrap();
    assert!(public.contains(&report.public_key_line));
    let opened =
        SigningKey::open(&fs::read_to_string(&report.key_path).unwrap(), PASSPHRASE).unwrap();
    assert_eq!(opened.public_key_line(), report.public_key_line);
    assert_eq!(opened.key_id_hex(), report.key_id);
}

#[test]
fn two_keys_made_in_one_folder_are_different() {
    let scratch = Scratch::new("two");
    let active = keygen_in(&scratch, "active");
    let reserve = keygen_in(&scratch, "reserve");
    assert_ne!(active.public_key_line, reserve.public_key_line);
    assert_ne!(active.key_id, reserve.key_id);
}

#[test]
fn keygen_creates_the_folder_when_it_is_missing() {
    let scratch = Scratch::new("mkdir");
    let nested = scratch.path("a").join("b");
    let mut prompts = Script::new(&[PASSPHRASE, PASSPHRASE]);
    keygen(&nested, "active", KdfCost::CHEAPEST, &mut prompts).unwrap();
    assert!(nested.join("active.key").is_file());
}

#[test]
fn keygen_writes_nothing_when_the_passphrases_differ() {
    let scratch = Scratch::new("mismatch");
    let mut prompts = Script::new(&[PASSPHRASE, "another long passphrase"]);
    assert!(matches!(
        error_of(keygen(
            &scratch.0,
            "active",
            KdfCost::CHEAPEST,
            &mut prompts
        )),
        WorkflowError::PassphraseMismatch
    ));
    assert_eq!(scratch.names(), Vec::<String>::new());
}

#[test]
fn keygen_writes_nothing_for_a_weak_passphrase_and_does_not_ask_twice() {
    let scratch = Scratch::new("weak");
    let mut prompts = Script::new(&["short", "short"]);
    assert!(matches!(
        error_of(keygen(
            &scratch.0,
            "active",
            KdfCost::CHEAPEST,
            &mut prompts
        )),
        WorkflowError::Sign(SignError::WeakPassphrase { .. })
    ));
    assert_eq!(prompts.asked, 1);
    assert_eq!(scratch.names(), Vec::<String>::new());
}

#[test]
fn keygen_never_replaces_an_existing_file_and_does_not_even_ask() {
    let scratch = Scratch::new("exists");
    keygen_in(&scratch, "active");
    let before: Vec<Vec<u8>> = ["active.key", "active.pub"]
        .iter()
        .map(|n| fs::read(scratch.path(n)).unwrap())
        .collect();

    let mut prompts = Script::new(&[PASSPHRASE, PASSPHRASE]);
    assert!(matches!(
        error_of(keygen(
            &scratch.0,
            "active",
            KdfCost::CHEAPEST,
            &mut prompts
        )),
        WorkflowError::AlreadyExists(_)
    ));
    assert_eq!(prompts.asked, 0);
    let after: Vec<Vec<u8>> = ["active.key", "active.pub"]
        .iter()
        .map(|n| fs::read(scratch.path(n)).unwrap())
        .collect();
    assert_eq!(before, after);

    // A lone leftover public file also blocks, and the private file is not made.
    fs::remove_file(scratch.path("active.key")).unwrap();
    let mut prompts = Script::new(&[PASSPHRASE, PASSPHRASE]);
    assert!(matches!(
        error_of(keygen(
            &scratch.0,
            "active",
            KdfCost::CHEAPEST,
            &mut prompts
        )),
        WorkflowError::AlreadyExists(_)
    ));
    assert_eq!(scratch.names(), ["active.pub"]);
}

#[test]
fn keygen_refuses_names_that_could_escape_the_folder_or_confuse_a_shell() {
    let scratch = Scratch::new("names");
    let long = "a".repeat(MAX_NAME_CHARS + 1);
    for name in [
        "", "Active", "a/b", "a\\b", "..", ".hidden", "-x", "a b", "a.b", "ñ", &long,
    ] {
        let mut prompts = Script::new(&[PASSPHRASE, PASSPHRASE]);
        assert!(
            matches!(
                error_of(keygen(&scratch.0, name, KdfCost::CHEAPEST, &mut prompts)),
                WorkflowError::BadName(_)
            ),
            "{name:?}"
        );
        assert_eq!(prompts.asked, 0, "{name:?}");
    }
    assert_eq!(scratch.names(), Vec::<String>::new());
    let longest = "a".repeat(MAX_NAME_CHARS);
    let mut prompts = Script::new(&[PASSPHRASE, PASSPHRASE]);
    assert!(keygen(&scratch.0, &longest, KdfCost::CHEAPEST, &mut prompts).is_ok());
}

#[test]
fn a_failing_prompt_is_reported_and_leaves_nothing() {
    let scratch = Scratch::new("prompt");
    let mut prompts = Script::new(&[]);
    assert!(matches!(
        error_of(keygen(
            &scratch.0,
            "active",
            KdfCost::CHEAPEST,
            &mut prompts
        )),
        WorkflowError::Prompt(_)
    ));
    assert_eq!(scratch.names(), Vec::<String>::new());
}

#[cfg(unix)]
#[test]
fn the_private_key_is_readable_by_its_owner_only() {
    use std::os::unix::fs::PermissionsExt as _;
    let scratch = Scratch::new("mode");
    let report = keygen_in(&scratch, "active");
    let mode = fs::metadata(&report.key_path).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
}

fn signed_setup(name: &str) -> (Scratch, KeygenReport, PathBuf) {
    let scratch = Scratch::new(name);
    let report = keygen_in(&scratch, "active");
    let manifest = scratch.path("release-manifest.json");
    fs::write(&manifest, MANIFEST).unwrap();
    (scratch, report, manifest)
}

/// Signs with the pair `keygen_in` made.
fn sign_with(
    report: &KeygenReport,
    manifest: &Path,
    comment: &str,
    prompts: &mut Script,
) -> Result<PathBuf, WorkflowError> {
    sign_manifest(
        &report.key_path,
        &report.public_path,
        manifest,
        comment,
        prompts,
    )
}

#[test]
fn a_manifest_is_signed_and_the_shipped_verifier_accepts_the_result() {
    let (_scratch, report, manifest) = signed_setup("sign");
    let mut prompts = Script::new(&[PASSPHRASE]);
    let written = sign_with(&report, &manifest, "timestamp:1 file:m", &mut prompts).unwrap();
    assert_eq!(prompts.asked, 1);

    assert_eq!(written, signature_path_for(&manifest));
    assert!(written.ends_with("release-manifest.json.minisig"));
    let signature = fs::read_to_string(&written).unwrap();
    let keys = TrustedKeys::from_base64(&[report.public_key_line.as_str()]).unwrap();
    let parsed = verify_manifest(MANIFEST.as_bytes(), &signature, &keys).unwrap();
    assert_eq!(parsed.channel, "stable");
}

#[test]
fn a_wrong_passphrase_signs_nothing() {
    let (scratch, report, manifest) = signed_setup("wrongpass");
    let mut prompts = Script::new(&["not the right passphrase"]);
    assert!(matches!(
        error_of(sign_with(&report, &manifest, "c", &mut prompts)),
        WorkflowError::Sign(SignError::WrongPassphraseOrDamaged)
    ));
    assert_eq!(prompts.asked, 1);
    assert!(!signature_path_for(&manifest).exists());
    assert_eq!(scratch.names().len(), 3);
}

#[test]
fn a_private_key_that_is_not_the_expected_one_signs_nothing() {
    let scratch = Scratch::new("mismatch-key");
    let active = keygen_in(&scratch, "active");
    let reserve = keygen_in(&scratch, "reserve");
    let manifest = scratch.path("release-manifest.json");
    fs::write(&manifest, MANIFEST).unwrap();

    // The reserve private key with the active public key: opens fine, is the wrong key.
    let mut prompts = Script::new(&[PASSPHRASE]);
    let error = error_of(sign_manifest(
        &reserve.key_path,
        &active.public_path,
        &manifest,
        "c",
        &mut prompts,
    ));
    assert!(
        matches!(
            &error,
            WorkflowError::KeyMismatch { opened, expected }
                if *opened == reserve.public_key_line && *expected == active.public_key_line
        ),
        "{error}"
    );
    assert!(!signature_path_for(&manifest).exists());

    // The matching pair still works, for each of the two keys.
    for pair in [&active, &reserve] {
        let _ = fs::remove_file(signature_path_for(&manifest));
        let mut prompts = Script::new(&[PASSPHRASE]);
        let written = sign_with(pair, &manifest, "c", &mut prompts).unwrap();
        let signature = fs::read_to_string(written).unwrap();
        let keys = TrustedKeys::from_base64(&[pair.public_key_line.as_str()]).unwrap();
        assert!(verify_manifest(MANIFEST.as_bytes(), &signature, &keys).is_ok());
    }
}

#[test]
fn an_unusable_public_key_file_is_refused_before_the_passphrase_is_asked() {
    let (scratch, report, manifest) = signed_setup("badpublic");
    let good = fs::read_to_string(&report.public_path).unwrap();
    let key_line = report.public_key_line.clone();
    let cases = [
        String::new(),
        "just notes\n".to_owned(),
        "untrusted comment: only a comment\n".to_owned(),
        "untrusted comment: x\n!!!not base64!!!\n".to_owned(),
        format!(
            "untrusted comment: x\n{}\n",
            &key_line[..key_line.len() - 4]
        ),
        format!("{good}extra line\n"),
        format!("not a comment\n{key_line}\n"),
    ];
    let public = scratch.path("other.pub");
    for text in cases {
        fs::write(&public, &text).unwrap();
        let mut prompts = Script::new(&[PASSPHRASE]);
        let error = error_of(sign_manifest(
            &report.key_path,
            &public,
            &manifest,
            "c",
            &mut prompts,
        ));
        assert!(
            matches!(error, WorkflowError::BadPublicKeyFile { .. }),
            "{text:?}: {error}"
        );
        assert_eq!(prompts.asked, 0, "{text:?}");
    }
    // Not valid text at all, and a missing file.
    fs::write(&public, [0xff, 0xfe, 0x00]).unwrap();
    assert!(matches!(
        error_of(sign_manifest(
            &report.key_path,
            &public,
            &manifest,
            "c",
            &mut Script::new(&[PASSPHRASE])
        )),
        WorkflowError::BadPublicKeyFile { .. }
    ));
    assert!(matches!(
        error_of(sign_manifest(
            &report.key_path,
            &scratch.path("missing.pub"),
            &manifest,
            "c",
            &mut Script::new(&[PASSPHRASE])
        )),
        WorkflowError::Io { .. }
    ));
    assert!(!signature_path_for(&manifest).exists());
}

#[test]
fn a_file_that_fails_while_being_written_is_removed() {
    let scratch = Scratch::new("partial");
    let path = scratch.path("half.key");
    let error = error_of(create_new_with(&path, true, |file| {
        file.write_all(b"half of a key")?;
        Err(io::Error::other("disk full"))
    }));
    assert!(matches!(error, WorkflowError::Io { .. }), "{error}");
    assert!(!path.exists(), "a truncated file was left behind");

    // A successful fill keeps the file.
    create_new_with(&path, true, |file| file.write_all(b"whole")).unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"whole");
}

#[test]
fn a_sealed_key_that_does_not_open_again_is_caught_before_writing() {
    let key = SigningKey::generate().unwrap();
    let other = SigningKey::generate().unwrap();
    let sealed = key.seal(PASSPHRASE, KdfCost::CHEAPEST).unwrap();
    assert!(confirm_sealed(&key, &sealed, PASSPHRASE).is_ok());
    // A different passphrase, and a sealed file of another key, both fail the check.
    assert!(matches!(
        confirm_sealed(&key, &sealed, "a different passphrase"),
        Err(WorkflowError::SealCheckFailed)
    ));
    assert!(matches!(
        confirm_sealed(&other, &sealed, PASSPHRASE),
        Err(WorkflowError::SealCheckFailed)
    ));
}

#[test]
fn a_manifest_the_program_would_refuse_is_not_signed_and_the_passphrase_is_not_asked() {
    let (_scratch, report, manifest) = signed_setup("badmanifest");
    for text in [
        "not json",
        "{}",
        &MANIFEST.replace("\"schema\":1", "\"schema\":9"),
    ] {
        fs::write(&manifest, text).unwrap();
        let mut prompts = Script::new(&[PASSPHRASE]);
        let error = error_of(sign_with(&report, &manifest, "c", &mut prompts));
        assert!(
            matches!(error, WorkflowError::ManifestRefused(_)),
            "{text}: {error}"
        );
        assert_eq!(prompts.asked, 0);
        assert!(!signature_path_for(&manifest).exists());
    }
}

#[test]
fn an_existing_signature_is_never_replaced() {
    let (_scratch, report, manifest) = signed_setup("keepsig");
    let signature_path = signature_path_for(&manifest);
    fs::write(&signature_path, "old signature").unwrap();
    let mut prompts = Script::new(&[PASSPHRASE]);
    assert!(matches!(
        error_of(sign_with(&report, &manifest, "c", &mut prompts)),
        WorkflowError::AlreadyExists(_)
    ));
    assert_eq!(
        fs::read_to_string(&signature_path).unwrap(),
        "old signature"
    );
    assert_eq!(prompts.asked, 0);
}

#[test]
fn a_bad_trusted_comment_signs_nothing() {
    let (_scratch, report, manifest) = signed_setup("badcomment");
    for comment in ["two\nlines", "reversed\u{202E}text"] {
        let mut prompts = Script::new(&[PASSPHRASE]);
        assert!(matches!(
            error_of(sign_with(&report, &manifest, comment, &mut prompts)),
            WorkflowError::Sign(SignError::BadTrustedComment)
        ));
        assert!(!signature_path_for(&manifest).exists());
    }
}

#[test]
fn oversized_files_are_refused_from_their_size_and_before_any_prompt() {
    let (scratch, report, manifest) = signed_setup("limits");

    // The size is read from the file's metadata; the error reports it exactly, and a
    // file one byte over the limit is refused while one at the limit is read.
    let big_manifest = scratch.path("big.json");
    fs::write(&big_manifest, vec![b' '; MAX_MANIFEST_BYTES + 1]).unwrap();
    let mut prompts = Script::new(&[PASSPHRASE]);
    let error = error_of(sign_with(&report, &big_manifest, "c", &mut prompts));
    assert!(
        matches!(
            error,
            WorkflowError::TooLarge { size, limit, .. }
                if size == MAX_MANIFEST_BYTES as u64 + 1 && limit == MAX_MANIFEST_BYTES as u64
        ),
        "{error}"
    );
    assert_eq!(prompts.asked, 0);

    let big_key = scratch.path("big.key");
    fs::write(
        &big_key,
        vec![b'a'; usize::try_from(MAX_KEY_FILE_BYTES).unwrap() + 1],
    )
    .unwrap();
    let mut prompts = Script::new(&[PASSPHRASE]);
    let error = error_of(sign_manifest(
        &big_key,
        &report.public_path,
        &manifest,
        "c",
        &mut prompts,
    ));
    assert!(matches!(error, WorkflowError::TooLarge { .. }), "{error}");
    assert_eq!(prompts.asked, 0);

    let exact = scratch.path("exact.bin");
    fs::write(&exact, vec![b'x'; 16]).unwrap();
    assert_eq!(read_limited(&exact, 16).unwrap().len(), 16);
    assert!(matches!(
        read_limited(&exact, 15),
        Err(WorkflowError::TooLarge {
            size: 16,
            limit: 15,
            ..
        })
    ));
}

#[test]
fn missing_files_are_reported_before_any_prompt() {
    let (scratch, report, manifest) = signed_setup("missing");
    for (key, public, file) in [
        (
            scratch.path("missing.key"),
            report.public_path.clone(),
            manifest.clone(),
        ),
        (
            report.key_path.clone(),
            report.public_path.clone(),
            scratch.path("missing.json"),
        ),
    ] {
        let mut prompts = Script::new(&[PASSPHRASE]);
        let error = error_of(sign_manifest(&key, &public, &file, "c", &mut prompts));
        assert!(matches!(error, WorkflowError::Io { .. }), "{error}");
        assert_eq!(prompts.asked, 0, "{key:?} {file:?}");
    }
}

#[test]
fn a_file_that_is_not_a_key_is_refused_before_the_passphrase_is_asked() {
    let (scratch, report, manifest) = signed_setup("notkey");
    let not_a_key = scratch.path("notes.txt");
    for content in [b"just notes\n".to_vec(), vec![0xff, 0xfe, 0x00]] {
        fs::write(&not_a_key, &content).unwrap();
        let mut prompts = Script::new(&[PASSPHRASE]);
        let error = error_of(sign_manifest(
            &not_a_key,
            &report.public_path,
            &manifest,
            "c",
            &mut prompts,
        ));
        assert!(
            matches!(error, WorkflowError::Sign(SignError::KeyFileFormat(_))),
            "{error}"
        );
        assert_eq!(prompts.asked, 0);
    }

    // A key file asking for more work than allowed is refused the same way.
    let text = fs::read_to_string(&report.key_path).unwrap();
    let mut body = STANDARD.decode(text.lines().nth(1).unwrap()).unwrap();
    body[5] = 40;
    fs::write(
        &not_a_key,
        format!(
            "{}\n{}\n",
            text.lines().next().unwrap(),
            STANDARD.encode(body)
        ),
    )
    .unwrap();
    let mut prompts = Script::new(&[PASSPHRASE]);
    assert!(matches!(
        error_of(sign_manifest(
            &not_a_key,
            &report.public_path,
            &manifest,
            "c",
            &mut prompts
        )),
        WorkflowError::Sign(SignError::KdfCostRefused)
    ));
    assert_eq!(prompts.asked, 0);
}

#[test]
fn the_signature_path_is_the_manifest_path_with_a_suffix() {
    assert_eq!(
        signature_path_for(Path::new("out/release-manifest.json")),
        Path::new("out/release-manifest.json.minisig")
    );
}

#[test]
fn the_file_writer_itself_refuses_to_replace_a_file_that_appeared_after_the_check() {
    // `keygen` and `sign_manifest` check first, but a file can appear between that
    // check and the write; the writer is the last line of defence.
    let scratch = Scratch::new("race");
    let path = scratch.path("taken.key");
    fs::write(&path, "someone else's file").unwrap();
    assert!(matches!(
        error_of(write_new(&path, b"new content", true)),
        WorkflowError::AlreadyExists(_)
    ));
    assert_eq!(fs::read_to_string(&path).unwrap(), "someone else's file");
}
