# Anaquel

Anaquel builds a static documentation site from a folder of Markdown files. A
site is a **Library** made of **Volumes**; each Volume holds sections and pages.
The output is plain files you can host anywhere, with a ready `_headers` and
`_redirects` for Cloudflare.

Status: early development (0.1, not released). Spanish version: [README.es.md](README.es.md).

## What it does

- CommonMark and GFM Markdown with YAML front matter.
- `[[term]]` links to a glossary entry in the same volume.
- Broken links, broken anchors, duplicate URLs and unknown terms are errors that
  stop the build, so a published site has none.
- A page that disappears without an alias is an error: no silent dead URLs.
- Raw HTML in content is refused; only `http`, `https` and `mailto` links pass.
- The site ships a strict Content-Security-Policy with no inline script or style.
- Full-text search through [Pagefind](https://pagefind.app) (optional).
- A local preview server on `127.0.0.1` that rebuilds and reloads on change.

## Quick start

```text
anaquel new my-docs --title "My docs"
anaquel serve my-docs
anaquel build my-docs
```

Commands: `new`, `build`, `check`, `serve`, `doctor`, `version`.
`check --strict` and `build --strict` treat warnings as errors.

## Layout of a library

```text
anaquel.toml              schema, engine, title, base_url, language
volumes/<slug>/volume.toml
volumes/<slug>/_index.md  front page of the volume (required)
volumes/<slug>/**/*.md    pages; a folder's `_index.md` is its section page
volumes/<slug>/terms.md   glossary: each `###` heading is a term
```

Only pages named `terms.md` or `glossary.md` define terms. Links are written
to the source file (`[setup](setup.md)`) and resolved by the engine.

## Build from source

Rust 1.99 or newer.

```text
cargo build --release
cargo test --workspace
cargo run -p anaquel -- check examples/reference-library --strict
```

`examples/reference-library` is an invented library of 270+ pages used to test
the engine.

## Known limits

See [CHANGELOG.md](CHANGELOG.md).

## Licence

Apache-2.0. See [LICENSE](LICENSE).
