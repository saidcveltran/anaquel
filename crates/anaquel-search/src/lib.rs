//! Search index port.
//!
//! The engine asks a [`SearchIndexer`] to index a finished output folder. The
//! only adapter today runs the `pagefind` program. Linking Pagefind into the
//! engine pulls in about 245 more crates (a web server, C code, unmaintained
//! parsers) for a step that needs no more than a command, so it stays outside
//! the binary until that cost changes. See `docs/decisions/0001-search.md`.

use std::path::{Path, PathBuf};
use std::process::Command;

use anaquel_diagnostics::{Code, Diagnostic};

/// Name of the folder the index is written to, inside the output folder.
pub const BUNDLE_DIR: &str = "_pagefind";

/// Something that can index a built site.
pub trait SearchIndexer {
    /// Indexes the pages under `dist`, writing the bundle to `dist/_pagefind/`.
    ///
    /// # Errors
    /// Returns a diagnostic when the index cannot be built.
    fn index(&self, dist: &Path) -> Result<(), Diagnostic>;
}

/// Runs the `pagefind` program on the output folder.
#[derive(Debug, Clone)]
pub struct ExternalPagefind {
    program: PathBuf,
}

impl ExternalPagefind {
    /// Looks for `pagefind` next to the engine, then on `PATH`.
    ///
    /// `ANAQUEL_PAGEFIND` may name the program explicitly.
    pub fn locate() -> Option<Self> {
        if let Some(explicit) = std::env::var_os("ANAQUEL_PAGEFIND") {
            let program = PathBuf::from(explicit);
            return program.is_file().then_some(Self { program });
        }
        let file = format!("pagefind{}", std::env::consts::EXE_SUFFIX);
        let beside_engine = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|d| d.join(&file)));
        let on_path = std::env::var_os("PATH")
            .map(|paths| {
                std::env::split_paths(&paths)
                    .map(|d| d.join(&file))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        beside_engine
            .into_iter()
            .chain(on_path)
            .find(|p| p.is_file())
            .map(|program| Self { program })
    }

    /// The program that will be run, for `doctor`.
    pub fn program(&self) -> &Path {
        &self.program
    }
}

impl SearchIndexer for ExternalPagefind {
    fn index(&self, dist: &Path) -> Result<(), Diagnostic> {
        let fail = |message: String| {
            Diagnostic::warning(Code::SEARCH, message).with_help(
                "the site was built without search; run `anaquel doctor` to check the setup",
            )
        };
        let output = Command::new(&self.program)
            .arg("--site")
            .arg(dist)
            .arg("--output-subdir")
            .arg(BUNDLE_DIR)
            .output()
            .map_err(|e| fail(format!("cannot run {}: {e}", self.program.display())))?;
        if output.status.success() {
            Ok(())
        } else {
            let detail = String::from_utf8_lossy(&output.stderr);
            Err(fail(format!(
                "pagefind failed: {}",
                detail.lines().last().unwrap_or("no message")
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_program_is_a_warning_not_a_panic() {
        let indexer = ExternalPagefind {
            program: PathBuf::from("definitely-not-installed-pagefind"),
        };
        let err = indexer.index(Path::new(".")).unwrap_err();
        assert_eq!(err.code, Code::SEARCH);
        assert_eq!(err.severity, anaquel_diagnostics::Severity::Warning);
    }
}
