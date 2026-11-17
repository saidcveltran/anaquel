//! `anaquel new`: a minimal library that builds without errors.

use std::fs;
use std::path::Path;

use anaquel_diagnostics::{Code, Diagnostic};

const MANIFEST: &str = "schema = 1\nengine = \"{ENGINE}\"\ntitle = \"{TITLE}\"\nlanguage = \"en\"\ndescription = \"Documentation built with Anaquel.\"\n";

const VOLUME: &str = "title = \"Guide\"\ndescription = \"A first volume.\"\n";

const INDEX: &str =
    "# Guide\n\nWelcome. Start with [the first page](first-steps.md), or look up a [[shelf]].\n";

const FIRST: &str = "---\ndescription: Where to begin.\n---\n# First steps\n\nEvery `.md` file under `volumes/guide/` is a page. Links point to the source file, for example [the index](_index.md).\n\n## Add a page\n\nCreate a file next to this one and link to it.\n";

const TERMS: &str = "# Terms\n\n### Shelf\n\nA group of volumes. Write `[[shelf]]` anywhere in the volume to link here.\n";

/// Writes the files and returns how many.
///
/// # Errors
/// Fails when the folder exists and is not empty, or a file cannot be written.
pub fn create(folder: &Path, title: Option<&str>) -> Result<usize, Diagnostic> {
    if folder.exists()
        && fs::read_dir(folder)
            .map_err(|e| io(folder, &e))?
            .next()
            .is_some()
    {
        return Err(Diagnostic::error(
            Code::IO,
            format!("{} already exists and is not empty", folder.display()),
        ));
    }
    let title = title
        .unwrap_or("My library")
        .replace(['"', '\\', '\n'], " ");
    let manifest = MANIFEST
        .replace("{ENGINE}", anaquel_pipeline::ENGINE_VERSION)
        .replace("{TITLE}", &title);
    let files = [
        ("anaquel.toml", manifest.as_str()),
        ("volumes/guide/volume.toml", VOLUME),
        ("volumes/guide/_index.md", INDEX),
        ("volumes/guide/first-steps.md", FIRST),
        ("volumes/guide/terms.md", TERMS),
        (".gitignore", "dist/\n.anaquel/\n"),
    ];
    for (path, text) in files {
        let full = folder.join(path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).map_err(|e| io(parent, &e))?;
        }
        fs::write(&full, text).map_err(|e| io(&full, &e))?;
    }
    Ok(files.len())
}

fn io(path: &Path, error: &std::io::Error) -> Diagnostic {
    Diagnostic::error(Code::IO, format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_library_builds_clean() {
        let dir = std::env::temp_dir().join(format!("anaquel-new-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(create(&dir, Some("Demo")).unwrap(), 6);
        let outcome = anaquel_pipeline::check(&dir);
        assert!(!outcome.report.has_errors(), "{:?}", outcome.report.items());
        assert_eq!(outcome.pages, 3);
        assert!(create(&dir, None).is_err(), "refuses a non-empty folder");
        fs::remove_dir_all(&dir).unwrap();
    }
}
