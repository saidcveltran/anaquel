//! Theme package of a library: manifest, templates and assets.
//!
//! The default theme, `folio`, is compiled into the program so a library
//! builds the same way on every machine. Themes loaded from a folder, with a
//! lock file and a version check, arrive with milestone 4.

use anaquel_diagnostics::{Code, Diagnostic};
use serde::Deserialize;

/// A static file shipped by a theme, served under `/assets/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    /// Path inside the theme's `assets/` folder, with `/`.
    pub path: String,
    /// File contents.
    pub bytes: Vec<u8>,
}

/// A template file, addressed by its path inside `templates/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    /// Name used by `extends` and `include`, e.g. `partials/nav.html`.
    pub name: String,
    /// Template source.
    pub source: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeManifest {
    name: String,
    version: String,
    anaquel: String,
}

/// A theme ready to render.
#[derive(Debug, Clone)]
pub struct Theme {
    /// Theme name from `theme.toml`.
    pub name: String,
    /// Theme version from `theme.toml`.
    pub version: String,
    /// Engine line the theme was written for.
    pub engine: String,
    /// Templates.
    pub templates: Vec<Template>,
    /// Static files.
    pub assets: Vec<Asset>,
}

macro_rules! template {
    ($name:literal) => {
        Template {
            name: $name.to_owned(),
            source: include_str!(concat!("../../../themes/folio/templates/", $name)).to_owned(),
        }
    };
}

macro_rules! asset {
    ($path:literal) => {
        Asset {
            path: $path.to_owned(),
            bytes: include_bytes!(concat!("../../../themes/folio/assets/", $path)).to_vec(),
        }
    };
}

impl Theme {
    /// The default theme, compiled into the program.
    ///
    /// # Errors
    /// Returns a diagnostic when the embedded `theme.toml` is invalid, which
    /// would be a bug of this build.
    pub fn folio() -> Result<Self, Diagnostic> {
        let manifest: ThemeManifest =
            toml::from_str(include_str!("../../../themes/folio/theme.toml")).map_err(|e| {
                Diagnostic::error(Code::THEME, format!("theme.toml: {}", e.message()))
            })?;
        Ok(Self {
            name: manifest.name,
            version: manifest.version,
            engine: manifest.anaquel,
            templates: vec![
                template!("base.html"),
                template!("page.html"),
                template!("home.html"),
                template!("not_found.html"),
                template!("partials/mark.html"),
                template!("partials/nav.html"),
            ],
            assets: vec![
                asset!("styles/main.css"),
                asset!("scripts/init.js"),
                asset!("scripts/main.js"),
                asset!("brand/mark.svg"),
            ],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folio_loads_with_its_files() {
        let theme = Theme::folio().unwrap();
        assert_eq!(theme.name, "folio");
        assert!(theme.templates.iter().any(|t| t.name == "base.html"));
        assert!(
            theme
                .assets
                .iter()
                .any(|a| a.path == "styles/main.css" && !a.bytes.is_empty())
        );
    }

    #[test]
    fn folio_assets_stay_inside_their_budgets() {
        let theme = Theme::folio().unwrap();
        let size = |path: &str| {
            theme
                .assets
                .iter()
                .find(|a| a.path == path)
                .map_or(0, |a| a.bytes.len())
        };
        assert!(size("styles/main.css") < 40 * 1024);
        assert!(size("scripts/main.js") + size("scripts/init.js") < 30 * 1024);
    }
}
