//! Plans and writes the output of a build.
//!
//! Files are written through a temporary name and a rename, so a reader never
//! sees a half-written file, and only when their content changed. Up to eight
//! files are written at once; the outcome is the one of writing them in order
//! (see the `concurrent_write` module). A ledger records what a build produced:
//! the next build uses it to remove files that no longer exist and to notice
//! pages whose URL vanished.

mod concurrent_write;
mod ledger;
mod site_files;
mod writer;

pub use ledger::{Ledger, lost_urls};
pub use site_files::{LlmsLink, LlmsSection, headers_file, llms_txt, redirects_file, sitemap};
pub use writer::{OutputFile, WriteStats, write_ledger, write_outputs};
