# Contributing

Thank you for helping. A few rules keep the project small and predictable.

## Before you open a pull request

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p anaquel -- check examples/reference-library --strict
```

All four must pass on Linux, Windows and macOS (CI runs them).

## Code rules

- Rust 1.99, edition 2024. `unsafe` is forbidden.
- No `unwrap`, `expect`, `panic!`, `todo!` or `dbg!` outside tests.
- No `println!` or `eprintln!`; the command line writes through its own helpers.
- Every public item has a doc comment.
- A new diagnostic gets an `AQ####` code in `anaquel-diagnostics` and a test.
- Dependencies are pinned with `=`. A new one needs a reason in the pull request,
  and its licence must pass `cargo deny check` (nothing copyleft).
- The product contains no AI: no model calls, no generated content at build time.

## Commits

Write the message in English, in the imperative, starting with a type:
`feat:`, `fix:`, `docs:`, `test:`, `refactor:`, `chore:`. Explain why, not what.

## Contributions

By submitting a contribution you agree it is licensed under Apache-2.0, as in
section 5 of the licence.

## Changelog

Add each user-visible change under `[Unreleased]` in `CHANGELOG.md`, with its
known limits.
