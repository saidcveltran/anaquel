//! Release signing keys and manifest signatures for the maintainer of Anaquel.
//!
//! This crate makes the keys that `anaquel-update` trusts and signs the release
//! manifest with them. It never touches the network and is not shipped to readers.
//! The private key lives in a passphrase-protected file that only this crate reads;
//! public keys and signatures use the minisign formats, so any minisign reader can
//! check them. The private key file is this crate's own format and is **not**
//! compatible with the official minisign program.
//!
//! The pieces:
//! - [`SigningKey`] makes, seals, opens and uses a key;
//! - [`workflow`] runs the two maintainer commands on files with a passphrase source,
//!   so the same code is tested without a terminal.

mod error;
mod key;
mod signature;
pub mod workflow;

pub use error::SignError;
pub use key::{KdfCost, MIN_PASSPHRASE_CHARS, SigningKey, check_new_passphrase};
pub use signature::plain_comment_text;
