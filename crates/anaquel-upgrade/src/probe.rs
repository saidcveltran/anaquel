//! Running a program to see that it starts and which version it says it is. A new program is run
//! this way before it replaces the installed one, and the previous program before it is put back,
//! so a download that cannot run never takes the place of one that can.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use semver::Version;

/// How long a program has to answer.
const ANSWER_TIME: Duration = Duration::from_secs(20);
/// How often a program that has not answered yet is looked at.
const POLL_INTERVAL: Duration = Duration::from_millis(20);
/// The most of its answer that is read. `anaquel version` prints one short line.
const MAX_ANSWER_BYTES: u64 = 4096;

/// Runs `program version` and checks that it answers `anaquel X.Y.Z` and exits normally. When
/// `expected` is given, the version must be that one.
///
/// # Errors
/// A sentence saying how the program failed the check.
pub fn run_version_probe(program: &Path, expected: Option<&Version>) -> Result<(), String> {
    run_version_probe_within(program, expected, ANSWER_TIME)
}

/// As [`run_version_probe`], with the time the program has to answer chosen by the caller.
///
/// # Errors
/// A sentence saying how the program failed the check.
pub fn run_version_probe_within(
    program: &Path,
    expected: Option<&Version>,
    answer_time: Duration,
) -> Result<(), String> {
    let mut child = Command::new(program)
        .arg("version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("it could not be started: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "its output could not be read".to_owned())?;
    let reader = thread::spawn(move || {
        let mut answer = Vec::new();
        // A read error leaves what was read; the answer is checked as it is.
        let _ = stdout.take(MAX_ANSWER_BYTES).read_to_end(&mut answer);
        answer
    });

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= answer_time => {
                // The reader is left to end on its own: a program that started others may still
                // hold the pipe open.
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "it did not answer within {} seconds",
                    answer_time.as_secs()
                ));
            }
            Ok(None) => thread::sleep(POLL_INTERVAL),
            Err(error) => return Err(format!("it could not be waited for: {error}")),
        }
    };
    if !status.success() {
        return Err(format!("it exited with {status}"));
    }
    let answer = reader
        .join()
        .map_err(|_| "its output could not be read".to_owned())?;
    check_answer(&String::from_utf8_lossy(&answer), expected)
}

/// Checks the text `anaquel version` printed.
fn check_answer(answer: &str, expected: Option<&Version>) -> Result<(), String> {
    let line = answer.trim();
    let reported = line
        .strip_prefix("anaquel ")
        .and_then(|text| Version::parse(text).ok())
        .ok_or_else(|| format!("it did not say `anaquel` and a version (it said `{line}`)"))?;
    match expected {
        Some(wanted) if *wanted != reported => Err(format!(
            "it reports version {reported} but {wanted} was expected"
        )),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests;
