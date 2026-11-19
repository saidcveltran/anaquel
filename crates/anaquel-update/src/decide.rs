use std::fmt;

use semver::Version;

use crate::manifest::{Advisory, Asset, Manifest, Release};
use crate::time::Timestamp;

/// Which release the caller wants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// The newest release that is neither withdrawn nor a prerelease.
    Latest,
    /// This exact version, which may be older or a prerelease.
    Exact(Version),
}

/// What the caller knows; everything the decision depends on is passed in.
#[derive(Debug, Clone)]
pub struct Request {
    /// Version of the running program.
    pub current: Version,
    /// Target triple of the running program, such as `x86_64-unknown-linux-gnu`.
    pub platform: String,
    /// Release line the program follows; a manifest of another line is refused.
    pub channel: String,
    /// The release the caller wants.
    pub target: Target,
    /// The current time, taken once by the caller.
    pub now: Timestamp,
    /// `issued` of the newest manifest ever accepted on this machine, if any.
    pub last_seen_issued: Option<Timestamp>,
    /// Permits installing a version older than `current`.
    pub allow_downgrade: bool,
    /// Every download address must start with this. It must start with `https://`
    /// and end with `/`, so that `https://host/project` cannot match `https://host/project-fork/`.
    pub url_prefix: String,
}

/// The decision and what the caller should tell the person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    /// What to do.
    pub outcome: Outcome,
    /// Advisories that affect the running version, most serious first. Empty when
    /// the manifest itself was not believed.
    pub advisories: Vec<Advisory>,
}

/// What to do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The running version is already the wanted one, or newer than the newest release.
    UpToDate,
    /// Download and install this.
    Install(Plan),
    /// Do nothing and say why.
    Refused(Refusal),
}

/// A release chosen for installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Version to install.
    pub version: Version,
    /// Whether it is newer, older or the same as the running one.
    pub kind: PlanKind,
    /// The file to download and verify with [`crate::DigestCheck`].
    pub asset: Asset,
}

/// How the chosen release compares with the running one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanKind {
    /// A newer version.
    Upgrade,
    /// An older version, which the caller explicitly allowed.
    Downgrade,
    /// The same version again.
    Reinstall,
}

/// Why nothing will be installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The manifest describes another release line.
    WrongChannel {
        /// Line the program follows.
        wanted: String,
        /// Line the manifest describes.
        found: String,
    },
    /// The manifest is past its expiry, so it may hide newer withdrawals.
    Expired {
        /// When it stopped being believed.
        expires: Timestamp,
    },
    /// The manifest is older than one already accepted: a replay of old data.
    Stale {
        /// When this manifest was written.
        issued: Timestamp,
        /// When the newest accepted manifest was written.
        last_seen: Timestamp,
    },
    /// The manifest does not list the requested version.
    UnknownVersion(Version),
    /// No release is both stable and installable.
    NoStableRelease,
    /// The release was pulled.
    Withdrawn {
        /// The pulled version.
        version: Version,
        /// Why, in the maintainers' words.
        reason: String,
    },
    /// The release is older than the running one and nobody allowed that.
    DowngradeNotAllowed {
        /// Running version.
        current: Version,
        /// Requested version.
        target: Version,
    },
    /// The release has no file for this platform.
    NoAssetForPlatform {
        /// The release.
        version: Version,
        /// The platform asked for.
        platform: String,
    },
    /// The download address is outside the project or not a plain address.
    ForeignUrl {
        /// The address in the manifest.
        url: String,
    },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongChannel { wanted, found } => {
                write!(f, "the manifest is for channel `{found}`, not `{wanted}`")
            }
            Self::Expired { expires } => {
                write!(f, "the manifest expired at {expires}; try again later")
            }
            Self::Stale { issued, last_seen } => write!(
                f,
                "the manifest was issued at {issued}, before one already seen at {last_seen}"
            ),
            Self::UnknownVersion(version) => write!(f, "version {version} is not in the manifest"),
            Self::NoStableRelease => {
                f.write_str("the manifest lists no installable stable release")
            }
            Self::Withdrawn { version, reason } => {
                write!(f, "version {version} was withdrawn: {reason}")
            }
            Self::DowngradeNotAllowed { current, target } => write!(
                f,
                "version {target} is older than the running {current}; downgrading needs an explicit request"
            ),
            Self::NoAssetForPlatform { version, platform } => {
                write!(f, "version {version} has no file for {platform}")
            }
            Self::ForeignUrl { url } => {
                write!(f, "the download address {url} is outside the project")
            }
        }
    }
}

/// Decides what to install, if anything, from a manifest whose signature was verified.
///
/// The checks run in a fixed order: channel, expiry, replay, selection, then the
/// download address. Advisories for the running version are reported with every
/// decision made on a believed manifest, including refusals.
#[must_use]
pub fn decide(manifest: &Manifest, request: &Request) -> Decision {
    if let Some(refusal) = distrust(manifest, request) {
        return Decision {
            outcome: Outcome::Refused(refusal),
            advisories: Vec::new(),
        };
    }
    let mut advisories: Vec<Advisory> = manifest
        .advisories
        .iter()
        .filter(|advisory| advisory.affects.matches(&request.current))
        .cloned()
        .collect();
    advisories.sort_by(|a, b| b.severity.cmp(&a.severity).then_with(|| a.id.cmp(&b.id)));
    Decision {
        outcome: choose(manifest, request),
        advisories,
    }
}

/// Reasons not to believe the manifest at all.
fn distrust(manifest: &Manifest, request: &Request) -> Option<Refusal> {
    if manifest.channel != request.channel {
        return Some(Refusal::WrongChannel {
            wanted: request.channel.clone(),
            found: manifest.channel.clone(),
        });
    }
    if request.now >= manifest.expires {
        return Some(Refusal::Expired {
            expires: manifest.expires.clone(),
        });
    }
    match &request.last_seen_issued {
        Some(last_seen) if manifest.issued < *last_seen => Some(Refusal::Stale {
            issued: manifest.issued.clone(),
            last_seen: last_seen.clone(),
        }),
        _ => None,
    }
}

fn choose(manifest: &Manifest, request: &Request) -> Outcome {
    let release = match &request.target {
        Target::Latest => match latest(manifest) {
            Some(release) => release,
            None => return Outcome::Refused(Refusal::NoStableRelease),
        },
        Target::Exact(version) => match manifest.releases.iter().find(|r| r.version == *version) {
            Some(release) => release,
            None => return Outcome::Refused(Refusal::UnknownVersion(version.clone())),
        },
    };
    if let Some(withdrawal) = &release.withdrawn {
        return Outcome::Refused(Refusal::Withdrawn {
            version: release.version.clone(),
            reason: withdrawal.reason.clone(),
        });
    }
    let kind = match release.version.cmp(&request.current) {
        std::cmp::Ordering::Greater => PlanKind::Upgrade,
        std::cmp::Ordering::Equal => PlanKind::Reinstall,
        std::cmp::Ordering::Less => PlanKind::Downgrade,
    };
    if matches!(request.target, Target::Latest) && kind != PlanKind::Upgrade {
        return Outcome::UpToDate;
    }
    if kind == PlanKind::Downgrade && !request.allow_downgrade {
        return Outcome::Refused(Refusal::DowngradeNotAllowed {
            current: request.current.clone(),
            target: release.version.clone(),
        });
    }
    let Some(asset) = release.assets.get(&request.platform) else {
        return Outcome::Refused(Refusal::NoAssetForPlatform {
            version: release.version.clone(),
            platform: request.platform.clone(),
        });
    };
    if !url_is_inside(&asset.url, &request.url_prefix) {
        return Outcome::Refused(Refusal::ForeignUrl {
            url: asset.url.clone(),
        });
    }
    Outcome::Install(Plan {
        version: release.version.clone(),
        kind,
        asset: asset.clone(),
    })
}

/// The newest release that is stable and not withdrawn.
fn latest(manifest: &Manifest) -> Option<&Release> {
    manifest
        .releases
        .iter()
        .filter(|r| r.version.pre.is_empty() && r.withdrawn.is_none())
        .max_by(|a, b| a.version.cmp(&b.version))
}

/// Whether `url` lies under `prefix` without escaping it by dot segments or encoding.
fn url_is_inside(url: &str, prefix: &str) -> bool {
    if !prefix.starts_with("https://") || !prefix.ends_with('/') {
        return false;
    }
    let Some(rest) = url.strip_prefix(prefix) else {
        return false;
    };
    let lowered = rest.to_ascii_lowercase();
    !rest.is_empty()
        && !rest.contains(['\\', '#', '?'])
        && !lowered.contains("%2e")
        && !lowered.contains("%2f")
        && !lowered.contains("%5c")
        && !rest
            .split('/')
            .any(|segment| segment == "." || segment == "..")
}

#[cfg(test)]
mod tests;
