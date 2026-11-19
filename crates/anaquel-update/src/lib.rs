//! The trust core of `anaquel upgrade`: what may be believed and what may be installed.
//!
//! This crate never touches the network, the clock or the file system. A caller
//! downloads the release manifest and its signature, passes the bytes and the
//! current time in, and gets back either an installable plan or a refusal with a
//! reason. The steps are, in this order:
//!
//! 1. [`verify_manifest`] checks the detached minisign signature against the keys
//!    the program carries and only then reads the JSON, so unsigned text is never parsed.
//! 2. [`decide`] chooses a release for a platform and applies the rules that keep an
//!    upgrade safe: an expired manifest, an older manifest than one already seen, a
//!    withdrawn release, a downgrade nobody asked for and a download address outside
//!    the project are all refused.
//! 3. [`DigestCheck`] compares the downloaded file with the SHA-256 and the size the
//!    signed manifest promised.
//!
//! The manifest is JSON, schema 1, signed as `release-manifest.json` with a detached
//! `release-manifest.json.minisig`. Times are UTC and written `YYYY-MM-DDTHH:MM:SSZ`.

mod decide;
mod digest;
mod error;
mod manifest;
mod release_keys;
mod time;
mod trust;

pub use decide::{Decision, Outcome, Plan, PlanKind, Refusal, Request, Target, decide};
pub use digest::DigestCheck;
pub use error::UpdateError;
pub use manifest::{
    Advisory, Asset, MANIFEST_SCHEMA, MAX_ASSET_BYTES, MAX_MANIFEST_BYTES, Manifest, Release,
    Severity, Withdrawal,
};
pub use release_keys::{ACTIVE_RELEASE_KEY, RESERVE_RELEASE_KEY};
pub use time::Timestamp;
pub use trust::{TrustedKeys, verify_manifest};
