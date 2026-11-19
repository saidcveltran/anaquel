//! The work of `anaquel upgrade` that touches the outside: the disk, the network and the programs
//! it runs. Nothing here decides what to trust (that is `anaquel-update`).
//!
//! - [`procedure`] is the order of an upgrade and of a rollback, over everything below;
//! - [`report`] puts what the procedure found into words;
//! - [`archive`] takes the program out of a release archive without unpacking the rest;
//! - [`fetch`] downloads a file, and is the only piece that touches the network;
//! - [`probe`] runs a program to see that it starts and says its version;
//! - [`swap`] puts a new program in place, keeps the one before it, and goes back;
//! - [`state`] remembers, per user, the newest manifest time ever accepted, so that an older
//!   manifest replayed later is refused.
//!
//! Each piece takes its inputs as arguments (paths, readers, an environment lookup, the network),
//! so the tests run it in a scratch folder and never read the real environment.

pub mod archive;
pub mod fetch;
pub mod probe;
pub mod procedure;
pub mod report;
pub mod state;
pub mod swap;

/// Where the signed release manifest is: the file of that name of the newest published release,
/// which GitHub answers with a redirect to its download host. Its signature is at this address
/// followed by `.minisig`. The manifest lists every release, so the newest one is the one to ask.
pub const RELEASE_MANIFEST_ADDRESS: &str =
    "https://github.com/saidcveltran/anaquel/releases/latest/download/release-manifest.json";

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod scratch;
