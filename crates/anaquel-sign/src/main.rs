//! `anaquel-sign`: the maintainer's tool for release signing keys.

use std::io::{self, Write as _};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use anaquel_sign::KdfCost;
use anaquel_sign::release_manifest::{
    ManifestBuildError, ReleaseInput, TimeChoice, default_base_url, resolve_times,
    write_release_manifest,
};
use anaquel_sign::workflow::{self, PassphraseSource, WorkflowError};
use clap::{Parser, Subcommand};
use zeroize::Zeroizing;

/// Makes release signing keys and signs the release manifest. No network access.
#[derive(Parser)]
#[command(name = "anaquel-sign", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Make a key pair; the passphrase is asked twice on the terminal.
    Keygen {
        /// Name of the key, such as `active` or `reserve`.
        #[arg(long)]
        name: String,
        /// Folder that receives NAME.key (private) and NAME.pub.
        #[arg(long)]
        out: PathBuf,
    },
    /// Sign a release manifest; writes MANIFEST.minisig beside it.
    Sign {
        /// The private key file made by `keygen`.
        #[arg(long)]
        key: PathBuf,
        /// The matching public key file (`NAME.pub`); signing stops if the private key is another one.
        #[arg(long)]
        public: PathBuf,
        /// The trusted comment; defaults to the time and the file name.
        #[arg(long)]
        comment: Option<String>,
        /// The `release-manifest.json` to sign.
        manifest: PathBuf,
    },
    /// Write the release manifest from the archives `dist` built; never replaces a file.
    Manifest {
        /// Folder with the `anaquel-TARGET.tar.gz` or `.zip` archives and their `.sha256` files.
        #[arg(long)]
        dir: PathBuf,
        /// The version being released, such as `0.2.0`.
        #[arg(long)]
        version: semver::Version,
        /// Release line of the manifest.
        #[arg(long, default_value = "stable")]
        channel: String,
        /// The manifest of the previous release, whose releases and advisories are kept.
        #[arg(long)]
        previous: Option<PathBuf>,
        /// Publication time, `YYYY-MM-DDTHH:MM:SSZ`; the clock by default.
        #[arg(long)]
        published: Option<String>,
        /// Issue time; the clock by default. Needs `--expires`.
        #[arg(long, requires = "expires")]
        issued: Option<String>,
        /// Expiry time; `--valid-days` after the clock by default.
        #[arg(long)]
        expires: Option<String>,
        /// Days the manifest stays valid when `--expires` is not given.
        #[arg(long, default_value_t = 60)]
        valid_days: u64,
        /// Folder address of the archives; defaults to the project's release for the version.
        #[arg(long)]
        base_url: Option<String>,
        /// The file to write; it must not exist yet.
        #[arg(long, default_value = "release-manifest.json")]
        out: PathBuf,
    },
}

/// Why a command did not finish.
#[derive(Debug, thiserror::Error)]
enum CliError {
    #[error(transparent)]
    Workflow(#[from] WorkflowError),
    #[error(transparent)]
    Manifest(#[from] ManifestBuildError),
}

/// Reads passphrases from the terminal without echoing them.
struct Terminal;

impl PassphraseSource for Terminal {
    fn ask(&mut self, prompt: &str) -> io::Result<Zeroizing<String>> {
        rpassword::prompt_password(prompt).map(Zeroizing::new)
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli.command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(io::stderr(), "error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(command: Command) -> Result<(), CliError> {
    let mut out = io::stdout();
    match command {
        Command::Keygen { name, out: dir } => {
            let report = workflow::keygen(&dir, &name, KdfCost::STANDARD, &mut Terminal)?;
            let _ = writeln!(out, "key id:     {}", report.key_id);
            let _ = writeln!(out, "public key: {}", report.public_key_line);
            let _ = writeln!(out, "private:    {}", report.key_path.display());
            let _ = writeln!(out, "public:     {}", report.public_path.display());
            if cfg!(windows) {
                // No access list is set on Windows; the file has its folder's permissions.
                let _ = writeln!(
                    out,
                    "note: on Windows the private key file keeps the permissions of its folder; \
                     choose a folder only you can read, and keep a copy offline."
                );
            }
        }
        Command::Sign {
            key,
            public,
            comment,
            manifest,
        } => {
            let comment = comment.unwrap_or_else(|| default_comment(&manifest));
            let written =
                workflow::sign_manifest(&key, &public, &manifest, &comment, &mut Terminal)?;
            let _ = writeln!(out, "signature written to {}", written.display());
        }
        Command::Manifest {
            dir,
            version,
            channel,
            previous,
            published,
            issued,
            expires,
            valid_days,
            base_url,
            out: out_path,
        } => {
            let choice = TimeChoice {
                published: published.as_deref(),
                issued: issued.as_deref(),
                expires: expires.as_deref(),
                valid_days,
            };
            let (published, issued, expires) = resolve_times(now_unix(), &choice)?;
            let input = ReleaseInput {
                base_url: base_url.unwrap_or_else(|| default_base_url(&version)),
                channel,
                version,
                published,
                issued,
                expires,
            };
            write_release_manifest(previous.as_deref(), &input, &dir, &out_path)?;
            let _ = writeln!(
                out,
                "manifest for {} written to {} (valid until {})",
                input.version,
                out_path.display(),
                input.expires
            );
        }
    }
    Ok(())
}

/// Seconds since 1970-01-01T00:00:00Z on the system clock; zero if the clock is before it.
fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn default_comment(manifest: &std::path::Path) -> String {
    let seconds = now_unix();
    let file = manifest
        .file_name()
        .map_or_else(|| "manifest".into(), |name| name.to_string_lossy());
    // Control characters would be refused by the signer; keep the default always valid.
    let file = anaquel_sign::plain_comment_text(&file);
    format!("timestamp:{seconds} file:{file}")
}
