//! `anaquel upgrade` on the real program. Nothing here reaches the network: the release address
//! is a constant of the program, so what is tested is the command line, the rollback (which works
//! on files only) and that the program answers the check an upgrade runs on a new program.

#[cfg(test)]
mod support {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Mutex, MutexGuard, PoisonError};

    /// Lets one test at a time copy the program and start it.
    ///
    /// On Linux a program that was copied a moment ago cannot be started while another thread's
    /// `fork` still holds a copy of the descriptor it was written through ("Text file busy"). The
    /// tests copy and start the program in parallel threads, so each one holds this lock from the
    /// creation of its folder until the folder is removed.
    pub fn serial_access() -> MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The program under test.
    pub const PROGRAM: &str = env!("CARGO_BIN_EXE_anaquel");

    pub fn run(program: &Path, arguments: &[&str]) -> Output {
        Command::new(program).args(arguments).output().unwrap()
    }

    pub fn stdout(output: &Output) -> String {
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    pub fn stderr(output: &Output) -> String {
        String::from_utf8_lossy(&output.stderr).into_owned()
    }

    /// A folder under the system's temporary folder, removed when it goes out of scope.
    pub struct Folder {
        path: PathBuf,
        _serial: MutexGuard<'static, ()>,
    }

    impl Folder {
        pub fn new(name: &str) -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let serial = serial_access();
            let path = std::env::temp_dir().join(format!(
                "anaquel-upgrade-command-{name}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self {
                path,
                _serial: serial,
            }
        }

        /// A copy of the program in this folder, so that replacing it touches nothing real.
        pub fn copy_of_program(&self) -> PathBuf {
            let name = if cfg!(windows) {
                "anaquel.exe"
            } else {
                "anaquel"
            };
            let copy = self.path.join(name);
            fs::copy(PROGRAM, &copy).unwrap();
            copy
        }

        pub fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

use std::fs;

use anaquel_upgrade::probe::run_version_probe;
use semver::Version;
use support::{Folder, PROGRAM, run, serial_access, stderr, stdout};

#[test]
fn the_help_lists_the_command_and_its_options() {
    let folder = Folder::new("help");
    let copy = folder.copy_of_program();

    let top = run(&copy, &["--help"]);
    assert!(stdout(&top).contains("upgrade"), "{}", stdout(&top));

    let help = run(&copy, &["upgrade", "--help"]);
    assert!(help.status.success());
    let text = stdout(&help);
    for option in ["--check", "--version", "--allow-downgrade", "--rollback"] {
        assert!(text.contains(option), "`{option}` is missing from:\n{text}");
    }
}

#[test]
fn options_that_contradict_each_other_are_refused_before_anything_happens() {
    let folder = Folder::new("usage");
    let copy = folder.copy_of_program();
    for arguments in [
        &["upgrade", "--rollback", "--check"][..],
        &["upgrade", "--rollback", "--version", "0.1.0"],
        &["upgrade", "--allow-downgrade"],
        &["upgrade", "--version", "not-a-version"],
        &["upgrade", "--version"],
    ] {
        let output = run(&copy, arguments);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{arguments:?}: {}",
            stderr(&output)
        );
    }
    // Nothing was written beside the program.
    assert_eq!(fs::read_dir(folder.path()).unwrap().count(), 1);
}

#[test]
fn a_rollback_puts_the_previous_program_back_and_keeps_the_one_it_replaced() {
    let folder = Folder::new("rollback");
    let installed = folder.copy_of_program();
    let original = fs::read(&installed).unwrap();
    // A previous program that still runs (a program tolerates bytes after its end) and that is
    // told apart from the installed one by its length.
    let mut previous_bytes = original.clone();
    previous_bytes.push(0);
    let previous = installed.with_file_name(format!(
        "{}.previous",
        installed.file_name().unwrap().to_string_lossy()
    ));
    fs::write(&previous, &previous_bytes).unwrap();
    // A program renamed to `.previous` keeps its permissions; a file written anew has none to run.
    fs::set_permissions(&previous, fs::metadata(&installed).unwrap().permissions()).unwrap();

    let output = run(&installed, &["upgrade", "--rollback"]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stdout(&output).starts_with("Restored the previous program."),
        "{}",
        stdout(&output)
    );
    assert_eq!(fs::read(&installed).unwrap(), previous_bytes);
    assert_eq!(fs::read(&previous).unwrap(), original);
}

#[test]
fn a_rollback_with_nothing_to_go_back_to_fails_and_changes_nothing() {
    let folder = Folder::new("rollback-nothing");
    let installed = folder.copy_of_program();
    let original = fs::read(&installed).unwrap();

    let output = run(&installed, &["upgrade", "--rollback"]);

    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("no earlier program"),
        "{}",
        stderr(&output)
    );
    assert_eq!(fs::read(&installed).unwrap(), original);
    assert_eq!(fs::read_dir(folder.path()).unwrap().count(), 1);
}

#[test]
fn a_previous_program_that_does_not_run_is_not_put_back() {
    let folder = Folder::new("rollback-broken");
    let installed = folder.copy_of_program();
    let original = fs::read(&installed).unwrap();
    let previous = installed.with_file_name(format!(
        "{}.previous",
        installed.file_name().unwrap().to_string_lossy()
    ));
    fs::write(&previous, b"not a program").unwrap();

    let output = run(&installed, &["upgrade", "--rollback"]);

    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("failed its check"),
        "{}",
        stderr(&output)
    );
    assert_eq!(fs::read(&installed).unwrap(), original);
    assert_eq!(fs::read(&previous).unwrap(), b"not a program");
}

#[test]
fn the_program_answers_the_check_an_upgrade_runs_on_a_new_program() {
    // Starting a process while another test writes a program can make that program busy.
    let _serial = serial_access();
    let own_version = Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
    let program = std::path::Path::new(PROGRAM);

    assert_eq!(run_version_probe(program, Some(&own_version)), Ok(()));
    assert_eq!(run_version_probe(program, None), Ok(()));
    let other = Version::parse("99.0.0").unwrap();
    assert!(run_version_probe(program, Some(&other)).is_err());
}
