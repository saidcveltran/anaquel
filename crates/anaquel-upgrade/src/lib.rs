//! The work of `anaquel upgrade` that touches the outside: the disk, and in [`fetch`] alone the
//! network. Nothing here decides what to trust (that is `anaquel-update`).
//!
//! - [`archive`] takes the program out of a release archive without unpacking the rest;
//! - [`fetch`] downloads a file, and is the only piece that touches the network;
//! - [`swap`] puts a new program in place, keeps the one before it, and goes back;
//! - [`state`] remembers, per user, the newest manifest time ever accepted, so that an older
//!   manifest replayed later is refused.
//!
//! Each piece takes its inputs as arguments (paths, readers, an environment lookup), so the
//! tests run it in a scratch folder and never read the real environment.

pub mod archive;
pub mod fetch;
pub mod state;
pub mod swap;

#[cfg(test)]
mod scratch;
