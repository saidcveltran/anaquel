//! The `manifest` command run as a program, the way the maintainer runs it.

use std::fs;

use anaquel_update::Manifest;
use support::{ARCHIVE, SUM_OF_ABC, Scratch, manifest_command, stderr};

/// Helpers that may panic on a broken environment; marked as test code so the
/// workspace's `unwrap` lint, which exempts tests, applies the same way here.
#[cfg(test)]
mod support {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output, Stdio};
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub const ARCHIVE: &str = "anaquel-x86_64-unknown-linux-gnu.tar.gz";
    pub const SUM_OF_ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    /// A scratch folder removed when the test ends, even if it fails.
    pub struct Scratch(PathBuf);

    impl Scratch {
        /// A folder holding `archives/` with one archive and its `.sha256` file.
        pub fn with_archive(name: &str) -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "anaquel-sign-cli-{name}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&root);
            let archives = root.join("archives");
            fs::create_dir_all(&archives).unwrap();
            fs::write(archives.join(ARCHIVE), b"abc").unwrap();
            fs::write(
                archives.join(format!("{ARCHIVE}.sha256")),
                format!("{SUM_OF_ABC}  {ARCHIVE}\n"),
            )
            .unwrap();
            Self(root)
        }

        pub fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    pub fn manifest_command(scratch: &Scratch, arguments: &[&str], out: &Path) -> Output {
        Command::new(env!("CARGO_BIN_EXE_anaquel-sign"))
            .arg("manifest")
            .arg("--dir")
            .arg(scratch.path("archives"))
            .arg("--out")
            .arg(out)
            .args(arguments)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    pub fn stderr(output: &Output) -> String {
        String::from_utf8_lossy(&output.stderr).into_owned()
    }
}

#[test]
fn the_command_writes_a_manifest_the_program_accepts() {
    let scratch = Scratch::with_archive("ok");
    let out = scratch.path("release-manifest.json");
    let output = manifest_command(&scratch, &["--version", "0.2.0"], &out);

    assert!(output.status.success(), "{}", stderr(&output));
    let manifest = Manifest::parse(&fs::read(&out).unwrap()).unwrap();
    assert_eq!(manifest.channel, "stable");
    assert_eq!(manifest.releases.len(), 1);
    let asset = &manifest.releases[0].assets["x86_64-unknown-linux-gnu"];
    assert_eq!(asset.sha256, SUM_OF_ABC);
    assert!(asset.url.ends_with(&format!("/v0.2.0/{ARCHIVE}")));
    // The default validity is 60 days from the clock: the manifest is not already expired.
    assert!(manifest.expires > manifest.issued);
}

#[test]
fn the_command_refuses_to_replace_a_file_and_leaves_it_alone() {
    let scratch = Scratch::with_archive("exists");
    let out = scratch.path("release-manifest.json");
    fs::write(&out, b"keep me").unwrap();

    let output = manifest_command(&scratch, &["--version", "0.2.0"], &out);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("already exists"),
        "{}",
        stderr(&output)
    );
    assert_eq!(fs::read(&out).unwrap(), b"keep me");
}

#[test]
fn the_command_refuses_what_is_not_the_projects_release_address() {
    let scratch = Scratch::with_archive("foreign");
    let out = scratch.path("release-manifest.json");
    let output = manifest_command(
        &scratch,
        &["--version", "0.2.0", "--base-url", "https://example.com/"],
        &out,
    );
    assert!(!output.status.success());
    assert!(!out.exists());
}

#[test]
fn the_command_refuses_an_issue_time_without_an_expiry_and_a_bad_version() {
    let scratch = Scratch::with_archive("times");
    let out = scratch.path("release-manifest.json");

    let output = manifest_command(
        &scratch,
        &["--version", "0.2.0", "--issued", "2026-01-01T00:00:00Z"],
        &out,
    );
    assert!(!output.status.success());

    let output = manifest_command(&scratch, &["--version", "two"], &out);
    assert!(!output.status.success());

    let output = manifest_command(
        &scratch,
        &["--version", "0.2.0", "--valid-days", "366"],
        &out,
    );
    assert!(!output.status.success());
    assert!(!out.exists(), "no refused run leaves a file");
}

#[test]
fn a_chosen_expiry_and_issue_time_are_written_as_given() {
    let scratch = Scratch::with_archive("chosen");
    let out = scratch.path("release-manifest.json");
    let output = manifest_command(
        &scratch,
        &[
            "--version",
            "0.2.0",
            "--published",
            "2026-11-20T08:00:00Z",
            "--issued",
            "2026-11-20T09:00:00Z",
            "--expires",
            "2027-01-01T00:00:00Z",
        ],
        &out,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let text = fs::read_to_string(&out).unwrap();
    for expected in [
        "\"published\": \"2026-11-20T08:00:00Z\"",
        "\"issued\": \"2026-11-20T09:00:00Z\"",
        "\"expires\": \"2027-01-01T00:00:00Z\"",
    ] {
        assert!(text.contains(expected), "{expected} in {text}");
    }
}
