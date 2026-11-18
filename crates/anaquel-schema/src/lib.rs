//! Typed file formats of a library: manifests and page front matter.
//!
//! Everything read from disk that has a fixed shape is parsed here, with
//! unknown keys rejected, so a typo never turns into silent behaviour.

mod front_matter;
mod manifest;
mod names;

pub use front_matter::{FrontMatter, Parsed, parse_page};
pub use manifest::{LibraryManifest, ShelfManifest, VolumeManifest, check_engine};
pub use names::url_segment_problem;

/// Schema number this build understands for `anaquel.toml`.
pub const SCHEMA: u32 = 1;
