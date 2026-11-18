//! Plans and writes the output of a build.
//!
//! Files are written one at a time through a temporary name and a rename, so a
//! reader never sees a half-written file, and only when their content changed.
//! A ledger records what a build produced: the next build uses it to remove
//! files that no longer exist and to notice pages whose URL vanished.

mod ledger;
mod site_files;
mod writer;

pub use ledger::{Ledger, lost_urls};
pub use site_files::{LlmsLink, LlmsSection, headers_file, llms_txt, redirects_file, sitemap};
pub use writer::{OutputFile, WriteStats, write_ledger, write_outputs};
