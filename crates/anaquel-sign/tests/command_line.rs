//! Smoke tests of the real program. Everything that needs a passphrase is covered
//! through `workflow` with a scripted source; these cover what runs before a prompt.

use support::{run, scratch, stderr};

/// Helpers that may panic on a broken environment; marked as test code so the
/// workspace's `expect` lint, which exempts tests, applies the same way here.
#[cfg(test)]
mod support {
    use std::path::PathBuf;
    use std::process::{Command, Output, Stdio};

    pub fn run(arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_anaquel-sign"))
            .args(arguments)
            .stdin(Stdio::null())
            .output()
            .expect("the program starts")
    }

    pub fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("anaquel-sign-cli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch folder");
        dir
    }

    pub fn stderr(output: &Output) -> String {
        String::from_utf8_lossy(&output.stderr).into_owned()
    }
}

#[test]
fn help_lists_both_commands_and_exits_cleanly() {
    let output = run(&["--help"]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("keygen") && text.contains("sign"), "{text}");
}

#[test]
fn an_unknown_command_is_a_usage_error() {
    let output = run(&["publish"]);
    assert!(!output.status.success());
}

#[test]
fn a_bad_key_name_fails_before_any_prompt_and_writes_nothing() {
    let dir = scratch("badname");
    let out = dir.to_string_lossy().into_owned();
    let output = run(&["keygen", "--name", "../escape", "--out", &out]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("lowercase letters"),
        "{}",
        stderr(&output)
    );
    assert_eq!(std::fs::read_dir(&dir).expect("folder").count(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn signing_a_missing_manifest_fails_before_any_prompt() {
    let dir = scratch("missing");
    let manifest = dir.join("release-manifest.json");
    let key = dir.join("active.key");
    let output = run(&[
        "sign",
        "--key",
        &key.to_string_lossy(),
        &manifest.to_string_lossy(),
    ]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).starts_with("error: "),
        "{}",
        stderr(&output)
    );
    assert!(!dir.join("release-manifest.json.minisig").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
