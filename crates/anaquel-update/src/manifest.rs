use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use semver::{Version, VersionReq};
use serde::{Deserialize, Deserializer};

use crate::error::UpdateError;
use crate::time::Timestamp;

/// The only manifest format this program reads.
pub const MANIFEST_SCHEMA: u32 = 1;

/// Largest manifest accepted. A real one lists a few hundred releases at most.
pub const MAX_MANIFEST_BYTES: usize = 1024 * 1024;

/// Largest program file accepted, as a sanity bound on what a manifest may promise.
pub const MAX_ASSET_BYTES: u64 = 256 * 1024 * 1024;

/// Most releases a manifest may list.
const MAX_RELEASES: usize = 10_000;

/// The signed description of every release and every known problem with one.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Manifest {
    /// Format number; only [`MANIFEST_SCHEMA`] is read.
    pub schema: u32,
    /// Release line the manifest describes, such as `stable`.
    pub channel: String,
    /// When the manifest was written. A later manifest never has an earlier time.
    pub issued: Timestamp,
    /// After this instant the manifest is no longer believed.
    pub expires: Timestamp,
    /// Every release, in no particular order; versions are unique.
    pub releases: Vec<Release>,
    /// Known problems with released versions.
    #[serde(default)]
    pub advisories: Vec<Advisory>,
}

/// One published version.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Release {
    /// Semantic version of the release.
    pub version: Version,
    /// When it was published.
    pub published: Timestamp,
    /// Present when the release was pulled and must not be installed.
    #[serde(default)]
    pub withdrawn: Option<Withdrawal>,
    /// Program file for each platform, keyed by target triple.
    pub assets: BTreeMap<String, Asset>,
}

/// Why a release was pulled.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Withdrawal {
    /// One sentence for the person reading it.
    pub reason: String,
}

/// A downloadable program file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Asset {
    /// Where to download it from; always `https`.
    pub url: String,
    /// SHA-256 of the file, 64 lowercase hexadecimal digits.
    pub sha256: String,
    /// Exact size of the file in bytes.
    pub size: u64,
}

/// How serious a known problem is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Minor or hard to reach.
    Low,
    /// Worth fixing soon.
    Moderate,
    /// Fix as soon as possible.
    High,
    /// Stop using the affected versions.
    Critical,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Low => "low",
            Self::Moderate => "moderate",
            Self::High => "high",
            Self::Critical => "critical",
        })
    }
}

/// A known problem with some versions.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Advisory {
    /// Stable identifier, such as `ANAQUEL-2026-001`.
    pub id: String,
    /// How serious it is.
    pub severity: Severity,
    /// The versions it affects, as a semantic version requirement such as `<0.1.2`.
    #[serde(deserialize_with = "version_req")]
    pub affects: VersionReq,
    /// One sentence for the person reading it.
    pub message: String,
}

fn version_req<'de, D: Deserializer<'de>>(deserializer: D) -> Result<VersionReq, D::Error> {
    let text = String::deserialize(deserializer)?;
    VersionReq::parse(&text).map_err(serde::de::Error::custom)
}

impl Manifest {
    /// Reads a manifest and checks everything a signature cannot: the bounds,
    /// the schema, unique versions and well-formed downloads.
    ///
    /// Call it only on text whose signature was verified; [`crate::verify_manifest`] does.
    ///
    /// # Errors
    /// Returns [`UpdateError`] when the text is too large, not valid JSON for this
    /// format, of another schema, or breaks one of the rules in the field documentation.
    pub fn parse(text: &[u8]) -> Result<Self, UpdateError> {
        if text.len() > MAX_MANIFEST_BYTES {
            return Err(UpdateError::ManifestTooLarge {
                size: text.len(),
                limit: MAX_MANIFEST_BYTES,
            });
        }
        let manifest: Self =
            serde_json::from_slice(text).map_err(|e| UpdateError::BadManifest(e.to_string()))?;
        if manifest.schema != MANIFEST_SCHEMA {
            return Err(UpdateError::UnsupportedSchema {
                found: manifest.schema,
                expected: MANIFEST_SCHEMA,
            });
        }
        manifest.check().map_err(UpdateError::BadManifest)?;
        Ok(manifest)
    }

    fn check(&self) -> Result<(), String> {
        if self.channel.is_empty() || !self.channel.bytes().all(|b| b.is_ascii_lowercase()) {
            return Err(format!(
                "channel `{}` must be lowercase letters",
                self.channel
            ));
        }
        if self.expires <= self.issued {
            return Err("`expires` must be later than `issued`".to_owned());
        }
        if self.releases.len() > MAX_RELEASES {
            return Err(format!("more than {MAX_RELEASES} releases"));
        }
        let mut seen = BTreeSet::new();
        for release in &self.releases {
            if !seen.insert(&release.version) {
                return Err(format!("version {} is listed twice", release.version));
            }
            for (platform, asset) in &release.assets {
                asset
                    .check(platform)
                    .map_err(|why| format!("release {}: {why}", release.version))?;
            }
            if release
                .withdrawn
                .as_ref()
                .is_some_and(|withdrawal| withdrawal.reason.trim().is_empty())
            {
                return Err(format!(
                    "release {} is withdrawn without a reason",
                    release.version
                ));
            }
        }
        Ok(())
    }
}

impl Asset {
    fn check(&self, platform: &str) -> Result<(), String> {
        if platform.is_empty()
            || !platform
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
        {
            return Err(format!("platform `{platform}` is not a target triple"));
        }
        let address = self.url.strip_prefix("https://").unwrap_or("");
        if address.is_empty()
            || self
                .url
                .bytes()
                .any(|b| b.is_ascii_control() || b == b' ' || !b.is_ascii())
        {
            return Err(format!(
                "the address for {platform} must be a plain https URL"
            ));
        }
        let hex_ok = self.sha256.len() == 64
            && self
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if !hex_ok {
            return Err(format!(
                "the sha256 for {platform} must be 64 lowercase hexadecimal digits"
            ));
        }
        if self.size == 0 || self.size > MAX_ASSET_BYTES {
            return Err(format!(
                "the size for {platform} must be between 1 and {MAX_ASSET_BYTES} bytes"
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
