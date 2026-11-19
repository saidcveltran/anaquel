//! `anaquel upgrade`: puts the real network, clock, keys and paths into `anaquel-upgrade` and
//! turns what comes back into output and an exit code. The order of the steps and every rule
//! about what to believe live in `anaquel-upgrade` and `anaquel-update`; this file only connects
//! them to the machine the program runs on.

use std::path::Path;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use anaquel_pipeline::ENGINE_VERSION;
use anaquel_update::{RELEASE_DOWNLOAD_PREFIX, Target, Timestamp, TrustedKeys};
use anaquel_upgrade::RELEASE_MANIFEST_ADDRESS;
use anaquel_upgrade::fetch::HttpFetch;
use anaquel_upgrade::probe::run_version_probe;
use anaquel_upgrade::procedure::{Mode, UpgradeEnvironment, Verdict, Want, roll_back, upgrade};
use anaquel_upgrade::report::describe;
use anaquel_upgrade::state::{Platform, state_dir};
use semver::Version;

use super::{EXIT_FAILED, EXIT_PROBLEMS, complain, say};

/// The release line this program follows.
const CHANNEL: &str = "stable";

/// The target triple this program was built for, which is also the key of its file in the
/// manifest. `build.rs` records it.
const PLATFORM: &str = env!("ANAQUEL_TARGET_TRIPLE");

/// What the person typed after `anaquel upgrade`.
#[derive(Debug, Clone)]
pub struct Args {
    /// Only report what would be done.
    pub check: bool,
    /// Install this version and not the newest.
    pub version: Option<Version>,
    /// Allow a version older than the running one.
    pub allow_downgrade: bool,
    /// Go back to the program the last upgrade replaced.
    pub rollback: bool,
}

/// Runs the command.
pub fn run(args: &Args) -> ExitCode {
    let installed = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => return fail(&format!("could not find where this program is: {error}")),
    };
    if args.rollback {
        return rollback_program(&installed);
    }
    upgrade_program(args, &installed)
}

fn rollback_program(installed: &Path) -> ExitCode {
    match roll_back(installed, &|path, expected| {
        run_version_probe(path, expected)
    }) {
        Ok(()) => {
            say(
                "Restored the previous program. The one it replaced is kept, so running this again goes forward.",
            );
            ExitCode::SUCCESS
        }
        Err(error) => fail(&error.to_string()),
    }
}

fn upgrade_program(args: &Args, installed: &Path) -> ExitCode {
    let keys = match TrustedKeys::release() {
        Ok(keys) => keys,
        Err(error) => {
            return fail(&format!(
                "the release keys of this program are damaged: {error}"
            ));
        }
    };
    let current = match Version::parse(ENGINE_VERSION) {
        Ok(version) => version,
        Err(error) => return fail(&format!("this program's own version is not valid: {error}")),
    };
    let now = match now() {
        Ok(now) => now,
        Err(reason) => {
            return fail(&format!(
                "the clock of this system cannot be used: {reason}"
            ));
        }
    };
    let state = state_dir(Platform::current(), &|name| std::env::var_os(name));
    let fetch = HttpFetch::release();
    let program_name = if cfg!(windows) {
        "anaquel.exe"
    } else {
        "anaquel"
    };
    let context = UpgradeEnvironment {
        fetch: &fetch,
        keys: &keys,
        manifest_address: RELEASE_MANIFEST_ADDRESS,
        url_prefix: RELEASE_DOWNLOAD_PREFIX,
        channel: CHANNEL,
        platform: PLATFORM,
        current: &current,
        now: &now,
        installed,
        program_name,
        state_dir: state.as_deref(),
        probe: &|path, expected| run_version_probe(path, expected),
    };
    let want = Want {
        target: args.version.clone().map_or(Target::Latest, Target::Exact),
        allow_downgrade: args.allow_downgrade,
    };
    let mode = if args.check {
        Mode::Check
    } else {
        Mode::Install
    };
    match upgrade(&context, &want, mode) {
        Ok(report) => {
            for line in describe(&report, &current) {
                say(&line);
            }
            exit_code_of(&report.verdict)
        }
        Err(error) => fail(&error.to_string()),
    }
}

/// A refusal is a normal answer that leaves the program as it was, and says so with the code
/// that tells a script the library or the request has a problem; everything else is success.
fn exit_code_of(verdict: &Verdict) -> ExitCode {
    match verdict {
        Verdict::Refused(_) => ExitCode::from(EXIT_PROBLEMS),
        Verdict::UpToDate | Verdict::WouldInstall { .. } | Verdict::Installed { .. } => {
            ExitCode::SUCCESS
        }
    }
}

fn fail(message: &str) -> ExitCode {
    complain(message);
    ExitCode::from(EXIT_FAILED)
}

/// The time now, in the form a manifest writes it.
fn now() -> Result<Timestamp, String> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_secs();
    Timestamp::from_unix_seconds(seconds)
}
