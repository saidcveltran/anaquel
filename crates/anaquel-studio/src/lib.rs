//! The core of `anaquel studio`, the local editor of a library.
//!
//! Three pieces, none of which knows about HTTP:
//!
//! - [`LibraryPath`]: a path inside a library, checked once when it is built, so that no other code
//!   handles a raw string path;
//! - [`list_pages`]: the pages of a library with their volume, status and last change;
//! - [`read_page`] and [`save_page`]: reading a page and replacing it atomically, refusing to
//!   overwrite a file that changed since it was read.
//!
//! The editor changes pages that exist. Creating, renaming and deleting a page are not part of
//! this core; a rename is `anaquel relocate`, which keeps the links and leaves a redirect.

mod api;
mod guard;
mod library_path;
mod page_store;
mod pages;
mod server;
mod ui;

pub use library_path::{LibraryPath, PathError};
pub use page_store::{MAX_PAGE_BYTES, PageText, Revision, SaveError, read_page, save_page};
pub use pages::{PageList, PageStatus, PageSummary, list_pages};
pub use server::{Config as StudioConfig, Studio};

#[cfg(test)]
mod test_folder;
