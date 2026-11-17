# Changelog

All notable changes to this project are written here. The format follows
[Keep a Changelog 1.1](https://keepachangelog.com/en/1.1.0/) and the project will follow
[Semantic Versioning](https://semver.org/) from its first release. Nothing has been released yet.

## [Unreleased]

### Added

- Engine core: schema, directive, catalog, compose, theme, assets, render, emit, search port and pipeline crates.
- `anaquel` command line: `new`, `build`, `check`, `serve`, `doctor`, `version`; `--strict` turns warnings into errors.
- Local preview server bound to `127.0.0.1`, with a `Host` check, path sanitising and reload on change.
- The preview server applies the `/*` rule of the built `_headers` file, so the Content-Security-Policy is the same locally as in production.
- `anaquel serve --search` indexes on every rebuild.
- `folio` theme with a bounded navigation tree, dark and light tokens and a CSP without inline code.
- Literata is bundled (WOFF2, 130 KB); stylesheets may name other theme assets with a placeholder that the build replaces with the fingerprinted URL.
- `examples/reference-library`: an invented library of 271 pages that must check with no diagnostics.
- CI for Linux, Windows and macOS (format, clippy, tests, sample library, the five-minute example of the README) and a licence and advisory check with `cargo-deny`.
- `tools/check_images.py`: refuses image metadata (extra PNG chunks, editor data in SVG) and runs in CI; `--fix` rewrites a PNG without it.
- README in English and Spanish with banner, screenshots for both colour schemes, feature table, install table, comparison and a list of cases where Anaquel is the wrong choice.
- Issue forms (bug, feature, translation, documentation), a pull request template and the Contributor Covenant 2.1 as `CODE_OF_CONDUCT.md`.

### Changed

- A leading `# Title` that repeats the page title is left out of the page body, because the theme already prints the title. A `#` heading that says something else, or that is not the first block, stays.
- Only pages named `terms.md` or `glossary.md` define terms. A `###` heading in ordinary prose used to define one and made duplicates an error.
- `CHANGELOG.md` follows Keep a Changelog 1.1.

### Known limits

- The CI workflow and `deny.toml` were pushed but their first run has not been read yet.
- The theme was looked at in screenshots of one page at desktop width, in both colour schemes. Phone width has not been looked at. The theme toggle shows the same icon in both schemes.
- The search UI was checked by script in a browser: results appear and there is no CSP violation.
- Search needs the external `pagefind` program; it is not linked into the binary (the library tree is large and contains C code).
- The `indexing_slicing` lint is not denied yet; fuzzing of the directive, compose and schema parsers is planned for M1.
- The embedded Literata font is a Latin subset (Western European); text in other scripts falls back to system serif fonts.
- Theme UI strings are English only; translation arrives with the language work of M2.
- The crates are not publishable yet: the theme files live outside the crate folders.
- The conduct contact is the private security channel of GitHub, because the project has no public mailbox.
- Publishing to Cloudflare Workers is not documented or tested.
