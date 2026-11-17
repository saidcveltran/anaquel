# Changelog

## [Unreleased]

### Added

- Engine core: schema, directive, catalog, compose, theme, assets, render, emit, search port and pipeline crates.
- `anaquel` command line: `new`, `build`, `check`, `serve`, `doctor`, `version`; `--strict` turns warnings into errors.
- Local preview server bound to `127.0.0.1`, with a `Host` check, path sanitising and reload on change.
- `folio` theme with a bounded navigation tree, dark and light tokens and a CSP without inline code.

### Known limits

- Search needs the external `pagefind` program; it is not linked into the binary (the library tree is large and contains C code).
- The `indexing_slicing` lint is not denied yet; fuzzing of the directive, compose and schema parsers is planned for M1.
- The Literata font is not bundled yet; the theme falls back to system serif fonts.
- Theme UI strings are English only; translation arrives with the language work of M2.
- The crates are not publishable yet: the theme files live outside the crate folders.
