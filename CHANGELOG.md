# Changelog

All notable changes to this project are written here. The format follows
[Keep a Changelog 1.1](https://keepachangelog.com/en/1.1.0/) and the project will follow
[Semantic Versioning](https://semver.org/) from its first release.

## [Unreleased]

### Added

- Shelves: a folder `shelves/NAME/` with a `shelf.toml` (`title`, `description`, `weight`) holds volumes under `shelves/NAME/volumes/`, and each volume is served at `/NAME/VOLUME/`. Volumes in `volumes/` still work, and the home page lists them first, then each shelf with its title. `examples/multi-shelf` shows both. Limits: there is no page at `/NAME/` (only the volumes under it have pages); a shelf and a loose volume with one name are an error; a shelf with no volumes is not shown; the page and link rules are those of volumes, so a shelf name follows the same letters, digits, `-` and `_`.
- `llms.txt`: every build writes a plain Markdown index of the library (title, description, then one `##` section per volume with its pages in reading order, shelved volumes headed `Shelf: Volume`). Links are absolute when `base_url` is set and start with `/` otherwise. Limits: it lists titles and one-sentence descriptions only, not page text (no `llms-full.txt`); drafts are left out; there is no way yet to switch it off or to hide a page from it.
- Crate `anaquel-update`, the trust core of the coming `anaquel upgrade`: it verifies a detached minisign signature of `release-manifest.json` before reading it, then chooses a release and refuses an expired manifest, one older than a manifest already seen, a withdrawn release, a downgrade nobody asked for, a missing file for the platform and a download address outside the project; it also checks a download's SHA-256 and size. Limits: a library only, not called by any command yet; no network, clock or file access; it embeds no keys (the real keys are not generated yet); only prehashed signatures are accepted; times compare as text in UTC `Z` form.
- Design page `docs/decisions/0003-release-signing-keys.md`: who makes the release signing keys, the `anaquel-sign` tool and where the keys live. Limit: nothing is implemented; compatibility of its private key file with the official minisign program is not claimed.
- Design page `docs/decisions/0002-relocate-and-aliases.md` for `anaquel relocate` and aliases. Limit: nothing is implemented; the page is a proposal.
- Languages: `volume.toml` takes `language` (a BCP 47 tag; the library's `language` when absent) and `translation_of` (the address of the original volume, `slug` or `shelf/slug`). Pages of a translation are paired with the original by their path inside the volume; each page then lists its other-language pages as `<link rel="alternate" hreflang>` (only with a `base_url`, since the links must be absolute) and as a language list in the sidebar. `<html lang>` follows the volume. The texts of the `folio` theme (skip link, menu, previous, next, on this page, 404) come in English and Spanish by the primary subtag of the page language; any other language reads English. A bad tag, an unknown or self-referencing `translation_of`, a translation of a translation, or two volumes of one book in one language is error AQ0101 at the `volume.toml`. Limits: no `x-default`; a page missing from a translation simply lists no alternates; the home page and the 404 page follow the library language; the theme texts exist only in English and Spanish; whether the search index separates pages by language was not checked.

## [0.1.1] - 2026-11-18

Theme changes and the end of macOS support. The engine has no code change.

### Changed

- Theme `folio`: a jump to a heading on the same page scrolls smoothly; it stays instant under reduced motion or with `data-motion="off"`.
- Theme `folio`: jumps to a heading last 200 to 400 ms in every browser, move focus to the heading, and Back and Forward restore the position at once. Limits: the script adds about 9 KB before compression; Firefox and Safari were not run; the cases are in `docs/folio-browser-cases.md`.
- Theme `folio`: every page writes an animated logo and a greeting to the browser console. Limits: the script grows to about 19 KB before compression (about 5.5 KB after); Firefox and Safari were not run; the cases are C1 and C2 in `docs/folio-browser-cases.md`.

### Fixed

- The public gate in CI read the co-author trailer that GitHub adds to a squash merge as a trace of an AI tool and failed every pull request. It now accepts that trailer when its address is a public GitHub identity. Limit: the line that caused it stays in the history of `main`, which was not rewritten.

### Removed

- macOS support. CI no longer runs on macOS (its runners often got no machine and the jobs were cancelled after 15 minutes), and the release workflow builds archives for Windows x86_64 and Linux x86_64 only. Known limits: nobody builds or tests Anaquel on macOS or on any arm64 system; 0.1.0 keeps its two macOS archives but they are no longer kept up; the installer scripts of later releases will offer no macOS archive.

## [0.1.0] - 2026-11-18

First release.

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

- Release setup with `dist` 0.34.0 (`dist-workspace.toml`, `.github/workflows/release.yml`): archives for Windows x86_64, Linux x86_64 and macOS (Intel and Apple Silicon), a shell and a PowerShell installer, a SHA-256 sum per archive and a GitHub build attestation. The workflow builds only when a version tag is pushed; on pull requests it only plans. No tag has been pushed and nothing has been released.
- `tools/install_smoke.py` and an `install smoke` CI job on three systems: build the release archive, check its SHA-256, unpack it into an empty folder and run `version`, `new`, `check --strict` and `build` with the unpacked program.

- Directives: `:::name attributes` ... `:::` blocks, nested by using fewer colons inside. The set is closed: `hint` (info, success, warning, danger), `details` (title, open), `tabs` with `tab` (title; `sync` makes groups follow each other and remembers the choice), `cards` with `card` (title, href, image; `cols` 1 to 4), `steps` with `step`, `columns` with `column` (`width` 1 to 6) and `embed` (YouTube and Vimeo over https, rewritten to the privacy-friendly player in a sandboxed frame). An unknown, unclosed, misplaced or malformed directive is error AQ0801 with its line; nothing inside a fenced code block is read as a directive; limits are 6 levels deep and 500 blocks per page. Markup is fixed in the engine and every value that content supplies is escaped.
- The built `_headers` file forbids all frames (`frame-src 'none'`) unless a page embeds a video, and then allows only the origins that pages use.
- `examples/directives`: one page per directive with its source and its result; CI checks it with `--strict`.
- Theme styles for every directive in both colour schemes, and a small script that turns the tabs into an accessible tab list (arrow keys, Home, End). Without the script every tab is shown under its title.

- Headings inside directive blocks get their anchors; the heading list is read in the same order the page is composed.
- Syntax highlighting of fenced code with `syntect` 5.3.0 (pure Rust regex engine, no C code). The output uses class names only, so the Content-Security-Policy is unchanged; the theme colours them in both schemes. Blocks over 100 KB, unknown languages and grammar failures stay plain escaped code. `shell`, `console` and `csharp` are accepted as names.
- `tools/third_party.py` writes `THIRD-PARTY.md` from `cargo tree`; CI fails when the file is out of date.

- A hand-written attack suite (`crates/anaquel-pipeline/src/xss.rs`): hostile text in every place that accepts text must build into pages that a tag scanner finds clean (only listed tags, no event or style attributes, no script with a body, no dangerous URL, frames only from the sandboxed video players); about 100 payloads (script, data, file and protocol-relative links, entity and tab tricks, raw HTML of every kind, directive abuse, bad embed hosts, hostile front matter) must each be refused with nothing written.
- Fuzzing: four `cargo-fuzz` targets in `fuzz/` (`directive_grammar`, `front_matter`, `url_policy`, `link_resolver`) and a CI job that runs each for 150 seconds, ten minutes in all, on Linux with a pinned nightly compiler. The checks live in the new crate `anaquel-fuzzcheck`, which also runs them over the seed files and about 20 000 mutations on every system in the ordinary test run.
- `tools/naming_gate.py` refuses crate, folder, file, module and type names that say nothing about their responsibility (`utils`, `helpers`, `common`, `shared`, `types`, `context`, `item`, a `new_` or `old_` prefix, a version suffix, an `impl` suffix); `tools/public_gate.py` looks in the tree and in the commit history for user folders, keys, e-mail addresses that are not public GitHub identities, traces of AI tools and editor metadata in binaries, and takes a private list of names with `--private FILE`; `tools/test_gates.py` tests both. CI runs the three in a `gates` job with the full history.

### Fixed

- The `gates` CI job checked the merge commit that GitHub builds for a pull request, which carries the author's profile e-mail, and failed a clean change. It now checks the pull request at its own head.

- `cargo-deny` rejected the path dependencies between workspace crates as wildcards; `deny.toml` now allows wildcard paths (the crates are not published).

- The theme toggle showed a moon in both colour schemes. It now shows a moon in the light scheme and a sun in the dark one, chosen by CSS only.

- `anaquel serve` rebuilt the site about once a second on Linux with no file changes: `inotify` reports that a file was opened or read, and every build reads all sources. The watcher now ignores access events and acts only on creation, change and removal. Found by a teammate's test on Zorin OS; limit: checked by a unit test of the event filter on Windows, not yet by a long run on Linux.

- Theme `folio`: `@view-transition` sat inside a style rule, where it is dropped, so page transitions never ran; it is now a direct child of the media query. `data-motion="off"` now also turns off transitions, animations and the view transition. Escape closes the menu and returns focus to its button. Tab panels are focusable (`tabindex="0"`). Every code block is a tab stop (`<pre tabindex="0">`), so a keyboard can scroll one that is wider than the screen (found and fixed by a teammate). Limits: nothing in the theme sets `data-motion` yet (no visible switch); the page transition was seen working in Brave on Linux, the other changes were not checked by hand in a browser.

### Changed

- Two types were renamed because the naming gate refused them: `Shared` (preview server state) is now `ServedSite` and `Context` (template input) is now `TemplateInput`.
- Names that become part of an address are checked. A page or folder name, a `slug` and every segment of an alias may hold letters of any script, digits and `- _ . ~ + @`; a space, quote, `#`, `?`, `%`, `&`, `;`, bracket, control or invisible character is an error with the rename to make (a page may set `slug` instead of renaming the file). Before, a file called `c#.md` or `my page.md` was written under an address that browsers read differently, and an alias with a quote or `<` passed `check` and then failed to write on Windows. Static files may not have `#`, `%` or `?` in their names. This can turn a library that built before into one that does not; the messages say which name to change.
- A leading `# Title` that repeats the page title is left out of the page body, because the theme already prints the title. A `#` heading that says something else, or that is not the first block, stays.
- Only pages named `terms.md` or `glossary.md` define terms. A `###` heading in ordinary prose used to define one and made duplicates an error.
- `CHANGELOG.md` follows Keep a Changelog 1.1.

### Known limits

- CI runs on Linux, Windows and macOS: format, clippy, tests, the sample library, licences and advisories, an install smoke test of the release archive, a 10-minute fuzz run on Linux, and the naming and public gates.
- The theme was looked at in screenshots at desktop width in both colour schemes, at 500 px by the author, and at 390 px by a teammate in Chromium on Linux (light scheme on every directive page; dark scheme only for hint and code blocks). Other browsers, Safari on iOS and dark-scheme cards, columns, tabs, steps and details at 390 px were not checked. The keyboard and reduced-motion review is written but not run.
- The search UI was checked by script in a browser: results appear and there is no CSP violation.
- Search needs the external `pagefind` program; it is not linked into the binary (the library tree is large and contains C code).
- The `indexing_slicing` lint is not denied yet.
- The embedded Literata font is a Latin subset (Western European); text in other scripts falls back to system serif fonts.
- Theme UI strings are English only; translation arrives with the language work of M2.
- The crates are not publishable yet: the theme files live outside the crate folders.
- The conduct contact is the private security channel of GitHub, because the project has no public mailbox.
- Publishing to Cloudflare Workers is not documented or tested.
- The release archives are not signed by Windows or Apple (no spending), so SmartScreen and Gatekeeper will warn. `dist` builds the archives and installers when a tag is pushed; the CI install smoke test builds, unpacks and runs the archive of each system, but the installer scripts and `gh attestation verify` have not been checked against real release files. There are no minisign signatures, no `upgrade` command and no `version --verbose` with the tagged commit yet (M2).
- There is no Linux arm64 archive.
- Directive limits: the opening line must start at column 0 with the name right after the colons; a fenced code block indented by four spaces or more is not tracked, so a `:::` line inside one is read as a directive. `include`, `variables` and `updates` are not implemented. The tab script and the directive styles were looked at in screenshots at desktop width only; the embed frame was seen to load, but playing a video was not tested. Pages without `description` still get one made from the whole page text, which starts with the title.
- Highlighting uses the grammars bundled with `syntect`: there is none for TOML, TypeScript, JSX, PowerShell, Dockerfile, Kotlin, Swift or INI, so those blocks are shown plain. Adding grammars means bundling and licensing more files; not done.
- The attack suite checks the output with a scanner written for it, not with a browser. It does not run script and does not cover browsers' own parsing quirks beyond the cases listed in it.
- Images may come from any https address (`img-src` allows `https:`), so a page can make a reader's browser contact another site. An SVG file in a library is copied as it is; the site-wide Content-Security-Policy is what stops script in it, and only when the host serves the `_headers` file.
- The fuzz targets run only in CI on Linux (Windows cannot link the libFuzzer runtime); on the other systems a smoke test replays the corpus. The first CI runs stopped the `url_policy` target twice on inputs that were false alarms of the check itself (a page that shows the words `formaction="vbscript:` as text, and a code fence whose information string ends up quoted in a `class` attribute); the check now reads real attributes inside tags, and both inputs are seeds. The last run of ten minutes passed. Ten minutes is a floor, not a guarantee: the parsers have not been fuzzed for hours.
- The public gate in CI cannot know the private names of a maintainer; those are checked only where the maintainer runs it with `--private`. The naming gate reads type names with one line-by-line pattern, not a Rust parser, so an item declared in an unusual layout is not seen.
