# Changelog

## [Unreleased]

### Added

- Engine core: schema, directive, catalog, compose, theme, assets, render, emit, search port and pipeline crates.
- `anaquel` command line: `new`, `build`, `check`, `serve`, `doctor`, `version`; `--strict` turns warnings into errors.
- Local preview server bound to `127.0.0.1`, with a `Host` check, path sanitising and reload on change.
- `folio` theme with a bounded navigation tree, dark and light tokens and a CSP without inline code.

- `examples/reference-library`: an invented library of 271 pages that must check with no diagnostics.
- The preview server applies the `/*` rule of the built `_headers` file, so the Content-Security-Policy is the same locally as in production.
- `anaquel serve --search` indexes on every rebuild.
- CI for Linux, Windows and macOS (format, clippy, tests, sample library) and a licence/advisory check with `cargo-deny`.
- Literata is bundled (WOFF2, 130 KB); stylesheets may name other theme assets with a placeholder that the build replaces with the fingerprinted URL.

### Changed

- A leading `# Title` that repeats the page title is left out of the page body, because the theme already prints the title. A `#` heading that says something else, or that is not the first block, stays.
- Only pages named `terms.md` or `glossary.md` define terms. A `###` heading in ordinary prose used to define one and made duplicates an error.

### Known limits

- The CI workflow and `deny.toml` were written but never run: nothing is pushed yet.
- The theme was inspected in one screenshot (dark scheme, desktop width) and by script elsewhere; light scheme and phone width have not been looked at.
- The search UI was checked by script in a browser: results appear and there is no CSP violation.
- Search needs the external `pagefind` program; it is not linked into the binary (the library tree is large and contains C code).
- The `indexing_slicing` lint is not denied yet; fuzzing of the directive, compose and schema parsers is planned for M1.
- The embedded Literata font is a Latin subset (Western European); text in other scripts falls back to system serif fonts.
- Theme UI strings are English only; translation arrives with the language work of M2.
- The crates are not publishable yet: the theme files live outside the crate folders.
