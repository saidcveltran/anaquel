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

- `cargo-deny` rejected the path dependencies between workspace crates as wildcards; `deny.toml` now allows wildcard paths (the crates are not published).

- The theme toggle showed a moon in both colour schemes. It now shows a moon in the light scheme and a sun in the dark one, chosen by CSS only.

### Changed

- Two types were renamed because the naming gate refused them: `Shared` (preview server state) is now `ServedSite` and `Context` (template input) is now `TemplateInput`.
- Names that become part of an address are checked. A page or folder name, a `slug` and every segment of an alias may hold letters of any script, digits and `- _ . ~ + @`; a space, quote, `#`, `?`, `%`, `&`, `;`, bracket, control or invisible character is an error with the rename to make (a page may set `slug` instead of renaming the file). Before, a file called `c#.md` or `my page.md` was written under an address that browsers read differently, and an alias with a quote or `<` passed `check` and then failed to write on Windows. Static files may not have `#`, `%` or `?` in their names. This can turn a library that built before into one that does not; the messages say which name to change.
- A leading `# Title` that repeats the page title is left out of the page body, because the theme already prints the title. A `#` heading that says something else, or that is not the first block, stays.
- Only pages named `terms.md` or `glossary.md` define terms. A `###` heading in ordinary prose used to define one and made duplicates an error.
- `CHANGELOG.md` follows Keep a Changelog 1.1.

### Known limits

- First CI run: Linux and Windows pass the format, clippy, test and sample-library steps; the licence check failed on the wildcard rule (fixed above) and its other rules were not reported as failing. The macOS job was still queued when this was written.
- The theme was looked at in screenshots of one page at desktop width, in both colour schemes. A window 500 px wide (single column, menu button, toggle) was also looked at. Narrower widths were not: the headless browser used cannot go below 500 px, so 390 px is unchecked.
- The search UI was checked by script in a browser: results appear and there is no CSP violation.
- Search needs the external `pagefind` program; it is not linked into the binary (the library tree is large and contains C code).
- The `indexing_slicing` lint is not denied yet; fuzzing of the directive, compose and schema parsers is planned for M1.
- The embedded Literata font is a Latin subset (Western European); text in other scripts falls back to system serif fonts.
- Theme UI strings are English only; translation arrives with the language work of M2.
- The crates are not publishable yet: the theme files live outside the crate folders.
- The conduct contact is the private security channel of GitHub, because the project has no public mailbox.
- Publishing to Cloudflare Workers is not documented or tested.
- The release archives are not signed by Windows or Apple (no spending), so SmartScreen and Gatekeeper will warn. The macOS and Linux archives and the installers were planned by `dist` but only the Windows archive was built and smoke-tested locally; the CI job covers the other two systems on its first run. There are no minisign signatures, no `upgrade` command and no `version --verbose` with the tagged commit yet (M2).
- There is no Linux arm64 archive.
- Directive limits: the opening line must start at column 0 with the name right after the colons; a fenced code block indented by four spaces or more is not tracked, so a `:::` line inside one is read as a directive. `include`, `variables` and `updates` are not implemented. The tab script and the directive styles were looked at in screenshots at desktop width only; the embed frame was seen to load, but playing a video was not tested. Pages without `description` still get one made from the whole page text, which starts with the title.
- Highlighting uses the grammars bundled with `syntect`: there is none for TOML, TypeScript, JSX, PowerShell, Dockerfile, Kotlin, Swift or INI, so those blocks are shown plain. Adding grammars means bundling and licensing more files; not done.
- The attack suite checks the output with a scanner written for it, not with a browser. It does not run script and does not cover browsers' own parsing quirks beyond the cases listed in it.
- Images may come from any https address (`img-src` allows `https:`), so a page can make a reader's browser contact another site. An SVG file in a library is copied as it is; the site-wide Content-Security-Policy is what stops script in it, and only when the host serves the `_headers` file.
- The fuzz targets were not run on the machine that wrote them (Windows cannot link the libFuzzer runtime); only the smoke test ran there. The first real run is the CI job; it stopped the `url_policy` target after about six minutes on an input that was a false alarm of the check itself (a page that shows the words `formaction="vbscript:` as text), so the check now reads attributes only inside tags and that input is a seed. Ten minutes in all is a floor, not a guarantee: the parsers have not been fuzzed for hours.
- The public gate in CI cannot know the private names of a maintainer; those are checked only where the maintainer runs it with `--private`. The naming gate reads type names with one line-by-line pattern, not a Rust parser, so an item declared in an unusual layout is not seen.
