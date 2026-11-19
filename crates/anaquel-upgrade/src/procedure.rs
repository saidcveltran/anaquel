//! The order of an upgrade. The order is what makes it safe: nothing on the disk changes until
//! everything that can be checked beforehand has been.
//!
//! 1. Refuse at once when a package manager owns the program or its folder cannot be written to,
//!    before anything is downloaded.
//! 2. Download the release manifest and its signature, each with a size limit, and verify the
//!    signature with the keys the program carries before the manifest is read.
//! 3. Ask [`anaquel_update::decide`] what to do, with the newest manifest time seen before, and
//!    remember this manifest's time when the manifest is one that may be believed.
//! 4. Download the one archive the plan names beside the program, checking its size and SHA-256
//!    against the signed manifest as the bytes arrive.
//! 5. Take only the program out of the archive, run it to see that it starts and reports the
//!    version the manifest promised, and only then swap it in, keeping the one it replaces.
//!
//! Every input arrives through [`Context`]: the network, the clock, the keys, the addresses and
//! the paths. A test hands in a scripted network and a scratch folder; the program hands in the
//! real ones.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anaquel_update::{
    Advisory, DigestCheck, MAX_MANIFEST_BYTES, Outcome, Plan, Refusal, Request, Target, Timestamp,
    TrustedKeys, UpdateError, decide, verify_manifest,
};
use semver::Version;
use thiserror::Error;

use crate::archive::{ArchiveError, ArchiveKind, Limits, extract_program};
use crate::fetch::{Fetch, FetchError, fetch_to_vec};
use crate::state::{StateError, read_last_seen, record_last_seen};
use crate::swap::{
    PackageManager, SwapError, check_writable, download_path, install, managed_by, previous_path,
    rollback, staged_path,
};

/// Largest signature file read. A minisign signature is a few hundred bytes.
const MAX_SIGNATURE_BYTES: u64 = 4096;

/// Runs a program to see that it starts and says which version it is. The version is the one it
/// must report, or `None` when any version of the program will do. The error is a sentence.
pub type Probe<'a> = &'a dyn Fn(&Path, Option<&Version>) -> Result<(), String>;

/// Everything an upgrade depends on, passed in so that nothing is read from the environment.
pub struct Context<'a> {
    /// The network.
    pub fetch: &'a dyn Fetch,
    /// The public keys a manifest may be signed with.
    pub keys: &'a TrustedKeys,
    /// Where the signed manifest is; its signature is at this address followed by `.minisig`.
    pub manifest_address: &'a str,
    /// Every download address in the manifest must start with this.
    pub url_prefix: &'a str,
    /// The release line followed, such as `stable`.
    pub channel: &'a str,
    /// The target triple of the running program, such as `x86_64-unknown-linux-gnu`.
    pub platform: &'a str,
    /// The version of the running program.
    pub current: &'a Version,
    /// The time now.
    pub now: &'a Timestamp,
    /// The running program, which is the file that gets replaced.
    pub installed: &'a Path,
    /// The name the program has inside a release archive.
    pub program_name: &'a str,
    /// Where the newest manifest time is remembered, or `None` when the system does not say.
    pub state_dir: Option<&'a Path>,
    /// How a program is run to check it.
    pub probe: Probe<'a>,
}

/// What the person asked for.
#[derive(Debug, Clone)]
pub struct Want {
    /// The release wanted.
    pub target: Target,
    /// Whether a version older than the running one may be installed.
    pub allow_downgrade: bool,
}

/// Whether the upgrade is carried out or only described.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Report what would be done and change nothing on the disk but the remembered manifest time.
    Check,
    /// Do it.
    Install,
}

/// Why the program cannot be replaced by this command.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Blocker {
    /// A package manager owns the program and should be the one to update it.
    #[error("this program is managed by {0}; update it with that tool")]
    PackageManager(PackageManager),
    /// The folder of the program cannot be written to.
    #[error("{0}")]
    NotWritable(String),
}

/// What an upgrade found or did.
#[derive(Debug)]
pub struct Report {
    /// Known problems with the running version, most serious first.
    pub advisories: Vec<Advisory>,
    /// Whether the system said where to remember the newest manifest time. When it did not, an
    /// older manifest replayed later could not be told from a new one.
    pub replay_protection: bool,
    /// The result.
    pub verdict: Verdict,
}

/// The result of an upgrade.
#[derive(Debug)]
pub enum Verdict {
    /// The running version is already the wanted one, or newer than the newest release.
    UpToDate,
    /// Nothing will be installed, and why.
    Refused(Refusal),
    /// With [`Mode::Check`]: this would be installed, unless the blocker stops it.
    WouldInstall {
        /// What would be installed.
        plan: Plan,
        /// What would stop it, if anything.
        blocker: Option<Blocker>,
    },
    /// With [`Mode::Install`]: this was installed.
    Installed {
        /// What was installed.
        plan: Plan,
        /// Where the program it replaced is kept.
        kept: PathBuf,
    },
}

/// Why an upgrade did not happen. When one of these is returned the installed program is the
/// one that was there.
#[derive(Debug, Error)]
pub enum UpgradeError {
    /// The program cannot be replaced by this command.
    #[error(transparent)]
    Blocked(#[from] Blocker),
    /// A file could not be downloaded.
    #[error("could not download {what}: {source}")]
    Download {
        /// Which file.
        what: &'static str,
        /// Why.
        source: FetchError,
    },
    /// The signature file is not text.
    #[error("the signature file is not text")]
    SignatureNotText,
    /// The manifest is not signed by a key the program trusts, or is not a manifest.
    #[error("the release information cannot be trusted: {0}")]
    Untrusted(UpdateError),
    /// The remembered manifest time could not be read or written.
    #[error(transparent)]
    State(#[from] StateError),
    /// The release file is not an archive kind this program reads.
    #[error("`{0}` is not an archive this program can read")]
    UnknownArchive(String),
    /// The downloaded file is not the one the signed manifest describes.
    #[error("the download is not what the signed manifest describes: {0}")]
    Mismatch(UpdateError),
    /// The program could not be taken out of the archive.
    #[error(transparent)]
    Archive(#[from] ArchiveError),
    /// The new program did not start, or is not the version promised.
    #[error("the new program was not installed because it failed its check: {0}")]
    NewProgramRejected(String),
    /// The previous program did not start.
    #[error("the previous program was not restored because it failed its check: {0}")]
    PreviousRejected(String),
    /// The swap or the rollback failed.
    #[error(transparent)]
    Swap(#[from] SwapError),
    /// A file next to the program could not be made, read or removed.
    #[error("{}: {source}", path.display())]
    Io {
        /// The file concerned.
        path: PathBuf,
        /// What the system said.
        source: io::Error,
    },
}

/// Finds the release wanted and, in [`Mode::Install`], installs it.
///
/// # Errors
/// See [`UpgradeError`]. A refusal of the trust rules (expired, older than one seen, withdrawn,
/// downgrade, foreign address, no file for the platform) is not an error but a [`Verdict`].
pub fn upgrade(context: &Context<'_>, want: &Want, mode: Mode) -> Result<Report, UpgradeError> {
    // Whether the folder can be written to is tested by writing a file, which a check does not do.
    let blocker = blocker_of(context.installed, mode == Mode::Install);
    if mode == Mode::Install
        && let Some(blocker) = &blocker
    {
        return Err(UpgradeError::Blocked(blocker.clone()));
    }
    let manifest = download_manifest(context)?;
    let last_seen_issued = match context.state_dir {
        Some(dir) => read_last_seen(dir)?,
        None => None,
    };
    let decision = decide(
        &manifest,
        &Request {
            current: context.current.clone(),
            platform: context.platform.to_owned(),
            channel: context.channel.to_owned(),
            target: want.target.clone(),
            now: context.now.clone(),
            last_seen_issued,
            allow_downgrade: want.allow_downgrade,
            url_prefix: context.url_prefix.to_owned(),
        },
    );
    if let (Some(dir), true) = (
        context.state_dir,
        manifest_may_be_remembered(&decision.outcome),
    ) {
        record_last_seen(dir, &manifest.issued)?;
    }
    let verdict = match (decision.outcome, mode) {
        (Outcome::UpToDate, _) => Verdict::UpToDate,
        (Outcome::Refused(refusal), _) => Verdict::Refused(refusal),
        (Outcome::Install(plan), Mode::Check) => Verdict::WouldInstall { plan, blocker },
        (Outcome::Install(plan), Mode::Install) => {
            let kept = install_plan(context, &plan)?;
            Verdict::Installed { plan, kept }
        }
    };
    Ok(Report {
        advisories: decision.advisories,
        replay_protection: context.state_dir.is_some(),
        verdict,
    })
}

/// Puts the previous program back, after checking that it starts.
///
/// # Errors
/// See [`UpgradeError`]; a missing previous program is [`SwapError::NothingToRestore`].
pub fn roll_back(installed: &Path, probe: Probe<'_>) -> Result<(), UpgradeError> {
    if let Some(blocker) = blocker_of(installed, true) {
        return Err(UpgradeError::Blocked(blocker));
    }
    let previous = previous_path(installed);
    if !previous.is_file() {
        return Err(SwapError::NothingToRestore(previous).into());
    }
    probe(&previous, None).map_err(UpgradeError::PreviousRejected)?;
    rollback(installed)?;
    Ok(())
}

/// What stops the program at `installed` from being replaced, if anything.
fn blocker_of(installed: &Path, test_writing: bool) -> Option<Blocker> {
    if let Some(manager) = managed_by(installed) {
        return Some(Blocker::PackageManager(manager));
    }
    if test_writing {
        return check_writable(installed)
            .err()
            .map(|error| Blocker::NotWritable(error.to_string()));
    }
    None
}

/// Whether a manifest may have its time remembered. One for another release line says nothing
/// about the line followed, and one that is expired or older than one seen is not believed.
fn manifest_may_be_remembered(outcome: &Outcome) -> bool {
    !matches!(
        outcome,
        Outcome::Refused(
            Refusal::WrongChannel { .. } | Refusal::Expired { .. } | Refusal::Stale { .. }
        )
    )
}

/// Downloads the manifest and its signature and verifies the one with the other.
fn download_manifest(context: &Context<'_>) -> Result<anaquel_update::Manifest, UpgradeError> {
    let download = |what, address: &str, limit| {
        fetch_to_vec(context.fetch, address, limit)
            .map_err(|source| UpgradeError::Download { what, source })
    };
    let manifest_limit = u64::try_from(MAX_MANIFEST_BYTES).unwrap_or(u64::MAX);
    let manifest = download(
        "the release manifest",
        context.manifest_address,
        manifest_limit,
    )?;
    let signature_address = format!("{}.minisig", context.manifest_address);
    let signature = download("its signature", &signature_address, MAX_SIGNATURE_BYTES)?;
    let signature = String::from_utf8(signature).map_err(|_| UpgradeError::SignatureNotText)?;
    verify_manifest(&manifest, &signature, context.keys).map_err(UpgradeError::Untrusted)
}

/// Downloads, checks and swaps in the release `plan` names, and returns where the program it
/// replaced is kept.
fn install_plan(context: &Context<'_>, plan: &Plan) -> Result<PathBuf, UpgradeError> {
    let file_name = plan.asset.url.rsplit('/').next().unwrap_or_default();
    let kind = ArchiveKind::of_file_name(file_name)
        .ok_or_else(|| UpgradeError::UnknownArchive(file_name.to_owned()))?;
    let download = download_path(context.installed);
    let staged = staged_path(context.installed);
    // Whatever happens below, neither file is left behind. A crash of an earlier run may have
    // left them, so they are cleared first as well.
    let leftovers = Leftovers([download.clone(), staged.clone()]);
    leftovers
        .clear()
        .map_err(|(path, source)| UpgradeError::Io { path, source })?;

    download_archive(context, plan, &download)?;
    let archive = File::open(&download).map_err(io_error(&download))?;
    extract_program(
        archive,
        kind,
        context.program_name,
        &staged,
        Limits::for_release(),
    )?;
    (context.probe)(&staged, Some(&plan.version)).map_err(UpgradeError::NewProgramRejected)?;
    install(context.installed, &staged)?;
    Ok(previous_path(context.installed))
}

/// Downloads the archive to `destination` and checks it against the signed manifest.
fn download_archive(
    context: &Context<'_>,
    plan: &Plan,
    destination: &Path,
) -> Result<(), UpgradeError> {
    let file = File::create_new(destination).map_err(io_error(destination))?;
    let mut sink = CheckedFile {
        file,
        check: DigestCheck::new(&plan.asset),
    };
    // The manifest promised an exact size, so that is the limit, and not the general one.
    context
        .fetch
        .fetch(&plan.asset.url, plan.asset.size, &mut sink)
        .map_err(|source| UpgradeError::Download {
            what: "the release archive",
            source,
        })?;
    let CheckedFile { file, check } = sink;
    file.sync_all().map_err(io_error(destination))?;
    drop(file);
    check.finish().map_err(UpgradeError::Mismatch)
}

/// A file that checksums what is written to it.
struct CheckedFile {
    file: File,
    check: DigestCheck,
}

impl Write for CheckedFile {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let written = self.file.write(bytes)?;
        self.check.update(&bytes[..written]);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

/// Files that must not outlive an upgrade, removed when this is dropped.
struct Leftovers([PathBuf; 2]);

impl Leftovers {
    /// Removes the files, ignoring those that are not there.
    fn clear(&self) -> Result<(), (PathBuf, io::Error)> {
        for path in &self.0 {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err((path.clone(), error)),
            }
        }
        Ok(())
    }
}

impl Drop for Leftovers {
    fn drop(&mut self) {
        // Nothing can be done about a failure here, and the next upgrade clears them again.
        let _ = self.clear();
    }
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> UpgradeError {
    let path = path.to_owned();
    move |source| UpgradeError::Io { path, source }
}

#[cfg(test)]
mod tests;
