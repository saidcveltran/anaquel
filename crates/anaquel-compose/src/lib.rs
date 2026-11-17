//! Markdown to HTML.
//!
//! One page of Markdown becomes an HTML fragment, its headings and its plain
//! text. Links and `[[term]]` references are checked against the catalog while
//! the page is composed, so a broken one is reported with its line. Raw HTML in
//! content is refused: it is the usual way for a documentation site to become
//! a cross-site-scripting hole. Directive blocks (`:::hint`, `:::tabs`, ...) are rendered to
//! fixed markup from [`anaquel_directive`]; content never supplies HTML of its own.

mod blocks;
mod heading;
mod markdown;

pub use heading::headings_of;
pub use markdown::{Composed, compose};

#[cfg(test)]
mod tests;
