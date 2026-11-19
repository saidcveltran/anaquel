//! `anaquel-sign`: the maintainer's tool for release signing keys.

use std::io::{self, Write as _};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use anaquel_sign::KdfCost;
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
        /// The trusted comment; defaults to the time and the file name.
        #[arg(long)]
        comment: Option<String>,
        /// The `release-manifest.json` to sign.
        manifest: PathBuf,
    },
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

fn run(command: Command) -> Result<(), WorkflowError> {
    let mut out = io::stdout();
    match command {
        Command::Keygen { name, out: dir } => {
            let report = workflow::keygen(&dir, &name, KdfCost::STANDARD, &mut Terminal)?;
            let _ = writeln!(out, "key id:     {}", report.key_id);
            let _ = writeln!(out, "public key: {}", report.public_key_line);
            let _ = writeln!(out, "private:    {}", report.key_path.display());
            let _ = writeln!(out, "public:     {}", report.public_path.display());
        }
        Command::Sign {
            key,
            comment,
            manifest,
        } => {
            let comment = comment.unwrap_or_else(|| default_comment(&manifest));
            let written = workflow::sign_manifest(&key, &manifest, &comment, &mut Terminal)?;
            let _ = writeln!(out, "signature written to {}", written.display());
        }
    }
    Ok(())
}

fn default_comment(manifest: &std::path::Path) -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let file = manifest
        .file_name()
        .map_or_else(|| "manifest".into(), |name| name.to_string_lossy());
    // Control characters would be refused by the signer; keep the default always valid.
    let file: String = file.chars().filter(|c| !c.is_control()).collect();
    format!("timestamp:{seconds} file:{file}")
}
