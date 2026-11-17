# 0001: Search runs through the external Pagefind program

Status: accepted (2026-11-17)

## Context

A library of books needs full-text search that works on a static host with no
server. Pagefind builds a static index and ships a small browser UI.

Pagefind is also published as a Rust library. Measured on 2026-11-17:

- the dependency tree has 276 packages, including C code and crates that are
  no longer maintained;
- a clean build takes about 1 minute 46 seconds.

## Decision

Anaquel does not link Pagefind. The `anaquel-search` crate defines a
`SearchIndexer` trait and one adapter, `ExternalPagefind`, that runs the
`pagefind` command on the built site and writes the index to `_pagefind/`.

The program is found through `ANAQUEL_PAGEFIND`, then next to the `anaquel`
executable, then on `PATH`. When it is missing the site still builds; the build
reports a warning (`AQ0701`) and the theme shows no search box.

## Consequences

- The engine's own dependency tree and build time stay small.
- A different indexer can replace Pagefind by implementing the trait.
- Release bundles must ship a Pagefind binary next to `anaquel`, with its
  licence in `THIRD-PARTY`.
- Search is verified by running the real program in tests and in a browser;
  it is not covered by `cargo test` alone.
- The preview server reads the built `_headers` file and applies it, so the
  Content-Security-Policy that Pagefind's UI must satisfy is the same locally
  and in production.
