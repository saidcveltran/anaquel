use anaquel_diagnostics::{Code, Diagnostic};
use serde::Deserialize;

use crate::SCHEMA;

/// Contents of `anaquel.toml`, the root of a library.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryManifest {
    /// Format number of this file. Only `1` exists.
    pub schema: u32,
    /// Engine line this library was written for, as `MAJOR.MINOR`.
    pub engine: String,
    /// Name of the library, shown in the header and in the page title.
    pub title: String,
    /// Absolute URL the library will be served from, without a trailing path.
    #[serde(default)]
    pub base_url: Option<String>,
    /// BCP 47 language tag of the content, written to `<html lang>`.
    #[serde(default = "default_language")]
    pub language: String,
    /// One sentence about the library, used in metadata.
    #[serde(default)]
    pub description: Option<String>,
}

fn default_language() -> String {
    "en".to_owned()
}

/// Contents of `volume.toml`, the manifest of one book.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VolumeManifest {
    /// Name of the volume.
    pub title: String,
    /// One sentence about the volume, shown on the library home.
    #[serde(default)]
    pub description: Option<String>,
    /// Position among the volumes of the library; lower comes first.
    #[serde(default)]
    pub weight: i64,
}

impl LibraryManifest {
    /// Parses `anaquel.toml` and checks its schema and engine line.
    ///
    /// # Errors
    /// Returns a diagnostic when the text is not valid TOML, has unknown keys,
    /// asks for another schema or needs a different engine line.
    pub fn parse(text: &str, engine: &str) -> Result<Self, Diagnostic> {
        let manifest: Self = toml::from_str(text).map_err(|e| {
            Diagnostic::error(Code::MANIFEST, e.message().to_owned()).at("anaquel.toml")
        })?;
        if manifest.schema != SCHEMA {
            return Err(Diagnostic::error(
                Code::SCHEMA_VERSION,
                format!(
                    "schema {} is not supported; this engine reads schema {SCHEMA}",
                    manifest.schema
                ),
            )
            .at("anaquel.toml"));
        }
        check_engine(&manifest.engine, engine).map_err(|message| {
            Diagnostic::error(Code::SCHEMA_VERSION, message)
                .at("anaquel.toml")
                .with_help(
                    "install a matching engine or update `engine` after reading the changelog",
                )
        })?;
        Ok(manifest)
    }
}

impl VolumeManifest {
    /// Parses a `volume.toml`; `path` is only used to locate errors.
    ///
    /// # Errors
    /// Returns a diagnostic when the text is invalid or has unknown keys.
    pub fn parse(text: &str, path: &str) -> Result<Self, Diagnostic> {
        toml::from_str(text)
            .map_err(|e| Diagnostic::error(Code::MANIFEST, e.message().to_owned()).at(path))
    }
}

/// Decides whether a library written for `required` (`MAJOR.MINOR`) can be
/// built by `current` (`MAJOR.MINOR.PATCH`).
///
/// Before 1.0 a new minor line may break the format, so `0.y` requires the
/// same `y`. From 1.0 on, the same major and an equal or newer minor is enough.
///
/// # Errors
/// Returns a message when either string is malformed or the engine does not fit.
pub fn check_engine(required: &str, current: &str) -> Result<(), String> {
    let req = major_minor(required)
        .ok_or_else(|| format!("`engine = \"{required}\"` must look like \"0.1\""))?;
    let cur =
        major_minor(current).ok_or_else(|| format!("engine version \"{current}\" is malformed"))?;
    let fits = if req.0 == 0 {
        cur == req
    } else {
        cur.0 == req.0 && cur.1 >= req.1
    };
    if fits {
        Ok(())
    } else {
        Err(format!(
            "this library needs engine {required}, but this is {current}"
        ))
    }
}

fn major_minor(text: &str) -> Option<(u64, u64)> {
    let mut parts = text.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

#[cfg(test)]
mod tests {
    use super::*;

    const OK: &str = "schema = 1\nengine = \"0.1\"\ntitle = \"Demo\"\n";

    #[test]
    fn parses_minimal_library() {
        let m = LibraryManifest::parse(OK, "0.1.0").expect("valid");
        assert_eq!(m.title, "Demo");
        assert_eq!(m.language, "en");
    }

    #[test]
    fn rejects_unknown_key() {
        let text = format!("{OK}colour = \"red\"\n");
        let err = LibraryManifest::parse(&text, "0.1.0").expect_err("unknown key");
        assert_eq!(err.code, Code::MANIFEST);
    }

    #[test]
    fn rejects_other_schema() {
        let text = OK.replace("schema = 1", "schema = 2");
        let err = LibraryManifest::parse(&text, "0.1.0").expect_err("schema");
        assert_eq!(err.code, Code::SCHEMA_VERSION);
    }

    #[test]
    fn engine_before_one_point_zero_needs_same_minor() {
        assert!(check_engine("0.1", "0.1.9").is_ok());
        assert!(check_engine("0.1", "0.2.0").is_err());
        assert!(check_engine("0.2", "0.1.0").is_err());
    }

    #[test]
    fn engine_after_one_point_zero_accepts_newer_minor() {
        assert!(check_engine("1.2", "1.5.0").is_ok());
        assert!(check_engine("1.5", "1.2.0").is_err());
        assert!(check_engine("1.0", "2.0.0").is_err());
    }

    #[test]
    fn engine_must_be_well_formed() {
        assert!(check_engine("latest", "0.1.0").is_err());
    }
}
