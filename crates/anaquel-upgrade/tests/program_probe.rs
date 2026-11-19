//! The program check against real programs: each test copies a fixture program and writes beside
//! it what the copy is to do, so the check starts, waits for, reads and kills real processes.

#[cfg(test)]
mod support {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Mutex, MutexGuard, PoisonError};

    /// Lets one test at a time write a program and start it.
    ///
    /// On Linux a program that was written a moment ago cannot be started while another thread's
    /// `fork` still holds a copy of the descriptor it was written through ("Text file busy"). The
    /// tests write programs and start them in parallel threads, so each one holds this lock from
    /// the creation of its folder until the folder is removed.
    fn serial_access() -> MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(PoisonError::into_inner)
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
                "anaquel-probe-{name}-{}-{}",
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

        /// A copy of the fixture program named `file_name`, told to behave as `behaviour` says.
        pub fn program(&self, file_name: &str, behaviour: &str) -> PathBuf {
            let program = self.path.join(file_name);
            fs::copy(env!("CARGO_BIN_EXE_probe_fixture"), &program).unwrap();
            let mut sidecar = program.clone().into_os_string();
            sidecar.push(".behaviour");
            fs::write(sidecar, behaviour).unwrap();
            program
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

    pub fn version(text: &str) -> semver::Version {
        semver::Version::parse(text).unwrap()
    }
}

use std::fs;
use std::time::{Duration, Instant};

use anaquel_upgrade::probe::{run_version_probe, run_version_probe_within};
use support::{Folder, version};

#[test]
fn a_program_that_says_its_version_passes_when_the_version_is_the_one_expected_or_none_is() {
    let folder = Folder::new("says");
    let program = folder.program("anaquel", "say anaquel 0.2.0");

    assert_eq!(run_version_probe(&program, None), Ok(()));
    assert_eq!(run_version_probe(&program, Some(&version("0.2.0"))), Ok(()));
    let error = run_version_probe(&program, Some(&version("0.3.0"))).unwrap_err();
    assert_eq!(error, "it reports version 0.2.0 but 0.3.0 was expected");
}

#[test]
fn a_program_whose_name_ends_in_new_runs_like_any_other() {
    // The new program is checked under the name it waits with, beside the installed one.
    let folder = Folder::new("staged-name");
    let program = folder.program("anaquel.exe.new", "say anaquel 1.0.0");

    assert_eq!(run_version_probe(&program, Some(&version("1.0.0"))), Ok(()));
}

#[test]
fn a_program_that_exits_with_an_error_fails_even_if_it_printed_the_right_words() {
    let folder = Folder::new("exit-code");
    let program = folder.program("anaquel", "exit 3");

    let error = run_version_probe(&program, None).unwrap_err();

    assert!(error.starts_with("it exited with"), "{error}");
}

#[test]
fn a_program_that_exits_normally_but_says_nothing_or_something_else_fails() {
    let folder = Folder::new("wrong-words");
    for (behaviour, word) in [("exit 0", "silent"), ("say hello there", "other")] {
        let program = folder.program(&format!("anaquel-{word}"), behaviour);

        let error = run_version_probe(&program, None).unwrap_err();

        assert!(
            error.starts_with("it did not say `anaquel` and a version"),
            "{word}: {error}"
        );
    }
}

#[test]
fn a_program_that_does_not_answer_is_stopped_at_the_time_limit_and_not_waited_for() {
    let folder = Folder::new("hang");
    let program = folder.program("anaquel", "hang");
    let started = Instant::now();

    let error = run_version_probe_within(&program, None, Duration::from_secs(1)).unwrap_err();

    assert_eq!(error, "it did not answer within 1 seconds");
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "the check waited {:?} for a program that hangs for a minute",
        started.elapsed()
    );
}

#[test]
fn a_program_that_prints_without_end_is_refused_and_does_not_fill_the_memory() {
    let folder = Folder::new("flood");
    let program = folder.program("anaquel", "flood");
    let started = Instant::now();

    let error = run_version_probe_within(&program, None, Duration::from_secs(10)).unwrap_err();

    assert!(error.starts_with("it "), "{error}");
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "took {:?}",
        started.elapsed()
    );
}

#[test]
fn a_file_that_is_not_a_program_is_reported_as_not_started() {
    let folder = Folder::new("not-a-program");
    let file = folder.path().join("anaquel");
    fs::write(&file, b"this is only text").unwrap();

    let error = run_version_probe(&file, None).unwrap_err();

    assert!(error.starts_with("it could not be started: "), "{error}");
}

#[test]
fn a_fixture_with_no_instructions_fails_the_check_and_so_does_every_unknown_one() {
    let folder = Folder::new("no-instructions");
    let program = folder.program("anaquel", "dance wildly");

    let error = run_version_probe(&program, None).unwrap_err();

    assert!(error.starts_with("it exited with"), "{error}");
}
