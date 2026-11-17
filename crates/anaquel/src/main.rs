//! `anaquel`: command line of the Anaquel documentation engine.

mod scaffold;

use std::fmt::Write as _;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anaquel_diagnostics::{Diagnostic, Severity};
use anaquel_pipeline::{ENGINE_VERSION, Options, Outcome, build, check};
use anaquel_preview::{Config, Hooks};
use clap::{Parser, Subcommand};

/// Exit code when the library has errors (or warnings with `--strict`).
const EXIT_PROBLEMS: u8 = 1;
/// Exit code when the command itself could not run.
const EXIT_FAILED: u8 = 2;

#[derive(Parser)]
#[command(
    name = "anaquel",
    version,
    about = "Builds static documentation libraries"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Build the site into `dist`.
    Build {
        /// Library folder (the one with `anaquel.toml`).
        #[arg(default_value = ".")]
        root: PathBuf,
        /// Output folder; defaults to `<root>/dist`.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Skip the search index even if `pagefind` is available.
        #[arg(long)]
        no_search: bool,
        /// Treat warnings as errors.
        #[arg(long)]
        strict: bool,
    },
    /// Check a library and report every problem; writes nothing.
    Check {
        /// Library folder.
        #[arg(default_value = ".")]
        root: PathBuf,
        /// Treat warnings as errors.
        #[arg(long)]
        strict: bool,
    },
    /// Create a new library with one volume.
    New {
        /// Folder to create.
        folder: PathBuf,
        /// Title of the library.
        #[arg(long)]
        title: Option<String>,
    },
    /// Build, serve on 127.0.0.1 and rebuild when a file changes.
    Serve {
        /// Library folder.
        #[arg(default_value = ".")]
        root: PathBuf,
        /// Port on 127.0.0.1.
        #[arg(long, default_value_t = 1111)]
        port: u16,
    },
    /// Show what this installation can and cannot do.
    Doctor,
    /// Print the version.
    Version,
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Build {
            root,
            out,
            no_search,
            strict,
        } => {
            let dist = out.unwrap_or_else(|| root.join("dist"));
            let outcome = build(&Options {
                root,
                dist: dist.clone(),
                search: !no_search,
            });
            summarize(&outcome, Some(&dist), strict)
        }
        Command::Check { root, strict } => summarize(&check(&root), None, strict),
        Command::New { folder, title } => match scaffold::create(&folder, title.as_deref()) {
            Ok(files) => {
                say(&format!(
                    "created {} with {files} files; next: `anaquel serve {}`",
                    folder.display(),
                    folder.display()
                ));
                ExitCode::SUCCESS
            }
            Err(diagnostic) => fail(&diagnostic),
        },
        Command::Serve { root, port } => serve(&root, port),
        Command::Doctor => doctor(),
        Command::Version => {
            say(&format!("anaquel {ENGINE_VERSION}"));
            ExitCode::SUCCESS
        }
    }
}

fn say(line: &str) {
    let _ = writeln!(std::io::stdout(), "{line}");
}

fn complain(line: &str) {
    let _ = writeln!(std::io::stderr(), "{line}");
}

fn fail(diagnostic: &Diagnostic) -> ExitCode {
    complain(&diagnostic.to_string());
    ExitCode::from(EXIT_FAILED)
}

fn print_report(outcome: &Outcome) {
    for item in outcome.report.items() {
        complain(&item.to_string());
    }
}

fn summarize(outcome: &Outcome, dist: Option<&Path>, strict: bool) -> ExitCode {
    print_report(outcome);
    let errors = outcome.report.count(Severity::Error);
    let warnings = outcome.report.count(Severity::Warning);
    let mut line = format!(
        "{} pages, {errors} errors, {warnings} warnings",
        outcome.pages
    );
    if let (Some(stats), Some(dist)) = (&outcome.written, dist) {
        let _ = write!(
            line,
            "; wrote {} files, removed {} in {}{}",
            stats.written,
            stats.removed,
            dist.display(),
            if outcome.searched {
                " (search indexed)"
            } else {
                ""
            }
        );
    }
    say(&line);
    if errors > 0 || (strict && warnings > 0) {
        ExitCode::from(EXIT_PROBLEMS)
    } else {
        ExitCode::SUCCESS
    }
}

fn status_line(root: &Path, dist: &Path) -> String {
    let outcome = build(&Options {
        root: root.to_path_buf(),
        dist: dist.to_path_buf(),
        search: false,
    });
    print_report(&outcome);
    let errors = outcome.report.count(Severity::Error);
    if errors > 0 {
        format!("build failed with {errors} errors; the previous site stays in place")
    } else {
        format!("built {} pages", outcome.pages)
    }
}

fn serve(root: &Path, port: u16) -> ExitCode {
    let root = match root.canonicalize() {
        Ok(root) => root,
        Err(e) => {
            return fail(&Diagnostic::error(
                anaquel_diagnostics::Code::IO,
                format!("{}: {e}", root.display()),
            ));
        }
    };
    let dist = root.join(".anaquel").join("preview");
    let rebuild_root = root.clone();
    let rebuild_dist = dist.clone();
    let config = Config {
        root: root.clone(),
        dir: dist.clone(),
        ignored: vec![root.join(".anaquel"), root.join("dist")],
        port,
    };
    let hooks = Hooks {
        rebuild: Box::new(move || status_line(&rebuild_root, &rebuild_dist)),
        log: Box::new(say),
    };
    match anaquel_preview::serve(&config, hooks) {
        Ok(()) => ExitCode::SUCCESS,
        Err(diagnostic) => fail(&diagnostic),
    }
}

fn doctor() -> ExitCode {
    say(&format!("anaquel {ENGINE_VERSION}"));
    say(&format!(
        "platform: {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    match anaquel_search::ExternalPagefind::locate() {
        Some(found) => say(&format!(
            "search: pagefind found at {}",
            found.program().display()
        )),
        None => say(
            "search: pagefind not found; sites build without search (install it next to `anaquel`, on PATH, or set ANAQUEL_PAGEFIND)",
        ),
    }
    ExitCode::SUCCESS
}
