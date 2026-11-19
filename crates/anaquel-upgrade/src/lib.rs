//! The file work of `anaquel upgrade`: everything that touches the disk and nothing that
//! touches the network or decides what to trust (that is `anaquel-update`).
//!
//! - [`archive`] takes the program out of a release archive without unpacking the rest;
//! - [`swap`] puts a new program in place, keeps the one before it, and goes back;
//! - [`state`] remembers, per user, the newest manifest time ever accepted, so that an older
//!   manifest replayed later is refused.
//!
//! Each piece takes its inputs as arguments (paths, readers, an environment lookup), so the
//! tests run it in a scratch folder and never read the real environment.

pub mod archive;
pub mod state;
pub mod swap;

#[cfg(test)]
mod scratch;
