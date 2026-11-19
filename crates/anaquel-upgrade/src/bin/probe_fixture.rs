//! A program for the tests of the program check, never shipped: it behaves as the file next to it
//! says. The file has the name of this program followed by `.behaviour`, so a test makes a copy of
//! this program for each behaviour and nothing is passed through the environment.
//!
//! The behaviours, one per file: `say TEXT` prints TEXT and ends normally; `exit CODE` ends with
//! that status and prints nothing; `hang` does not answer for a minute; `flood` prints a megabyte.

use std::io::Write;
use std::process::ExitCode;
use std::time::Duration;

/// How long `hang` waits: far longer than any test lets a program take to answer.
const HANG: Duration = Duration::from_secs(60);
/// How much `flood` prints.
const FLOOD_BYTES: usize = 1 << 20;
/// The status when no behaviour file is found or understood.
const NO_BEHAVIOUR: u8 = 64;

fn main() -> ExitCode {
    let Ok(program) = std::env::current_exe() else {
        return ExitCode::from(NO_BEHAVIOUR);
    };
    let mut file = program.into_os_string();
    file.push(".behaviour");
    let Ok(behaviour) = std::fs::read_to_string(&file) else {
        return ExitCode::from(NO_BEHAVIOUR);
    };
    let behaviour = behaviour.trim_end();
    let mut out = std::io::stdout();
    match behaviour.split_once(' ').unwrap_or((behaviour, "")) {
        ("say", text) => {
            let _ = writeln!(out, "{text}");
            ExitCode::SUCCESS
        }
        ("exit", code) => ExitCode::from(code.parse().unwrap_or(NO_BEHAVIOUR)),
        ("hang", _) => {
            std::thread::sleep(HANG);
            ExitCode::SUCCESS
        }
        ("flood", _) => {
            let _ = out.write_all(&vec![b'x'; FLOOD_BYTES]);
            ExitCode::SUCCESS
        }
        _ => ExitCode::from(NO_BEHAVIOUR),
    }
}
