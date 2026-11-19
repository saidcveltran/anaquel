use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};

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

#[test]
fn a_manifest_is_signed_and_the_shipped_verifier_accepts_the_result() {
    let (_scratch, report, manifest) = signed_setup("sign");
    let mut prompts = Script::new(&[PASSPHRASE]);
    let written = sign_manifest(
        &report.key_path,
        &manifest,
        "timestamp:1 file:m",
        &mut prompts,
    )
    .unwrap();

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
        error_of(sign_manifest(
            &report.key_path,
            &manifest,
            "c",
            &mut prompts
        )),
        WorkflowError::Sign(SignError::WrongPassphraseOrDamaged)
    ));
    assert!(!signature_path_for(&manifest).exists());
    assert_eq!(scratch.names().len(), 3);
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
        let error = error_of(sign_manifest(
            &report.key_path,
            &manifest,
            "c",
            &mut prompts,
        ));
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
        error_of(sign_manifest(
            &report.key_path,
            &manifest,
            "c",
            &mut prompts
        )),
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
    let mut prompts = Script::new(&[PASSPHRASE]);
    assert!(matches!(
        error_of(sign_manifest(
            &report.key_path,
            &manifest,
            "two\nlines",
            &mut prompts
        )),
        WorkflowError::Sign(SignError::BadTrustedComment)
    ));
    assert!(!signature_path_for(&manifest).exists());
}

#[test]
fn oversized_and_missing_files_are_refused_before_anything_is_read_into_memory() {
    let (scratch, report, manifest) = signed_setup("limits");

    let big_manifest = scratch.path("big.json");
    fs::write(&big_manifest, vec![b' '; MAX_MANIFEST_BYTES + 1]).unwrap();
    let mut prompts = Script::new(&[PASSPHRASE]);
    assert!(matches!(
        error_of(sign_manifest(
            &report.key_path,
            &big_manifest,
            "c",
            &mut prompts
        )),
        WorkflowError::TooLarge { .. }
    ));

    let big_key = scratch.path("big.key");
    fs::write(
        &big_key,
        vec![b'a'; usize::try_from(MAX_KEY_FILE_BYTES).unwrap() + 1],
    )
    .unwrap();
    assert!(matches!(
        error_of(sign_manifest(
            &big_key,
            &manifest,
            "c",
            &mut Script::new(&[PASSPHRASE])
        )),
        WorkflowError::TooLarge { .. }
    ));

    for (key, file) in [
        (scratch.path("missing.key"), manifest.clone()),
        (report.key_path.clone(), scratch.path("missing.json")),
    ] {
        assert!(matches!(
            error_of(sign_manifest(
                &key,
                &file,
                "c",
                &mut Script::new(&[PASSPHRASE])
            )),
            WorkflowError::Io { .. }
        ));
    }
}

#[test]
fn a_file_that_is_not_a_key_is_refused() {
    let (scratch, _report, manifest) = signed_setup("notkey");
    let not_a_key = scratch.path("notes.txt");
    fs::write(&not_a_key, "just notes\n").unwrap();
    assert!(matches!(
        error_of(sign_manifest(
            &not_a_key,
            &manifest,
            "c",
            &mut Script::new(&[PASSPHRASE])
        )),
        WorkflowError::Sign(SignError::KeyFileFormat(_))
    ));
    fs::write(&not_a_key, [0xff, 0xfe, 0x00]).unwrap();
    assert!(matches!(
        error_of(sign_manifest(
            &not_a_key,
            &manifest,
            "c",
            &mut Script::new(&[PASSPHRASE])
        )),
        WorkflowError::Sign(SignError::KeyFileFormat(_))
    ));
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
