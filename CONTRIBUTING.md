# Contributing

Thank you for helping. A few rules keep the project small and predictable.

## Before you open a pull request

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p anaquel -- check examples/reference-library --strict
```

All four must pass on Linux and Windows (CI runs them). CI also runs three small scripts
that need only Python 3:

```text
python3 -I tools/naming_gate.py     # no crate, file, folder or type called utils, helpers, common...
python3 -I tools/public_gate.py     # no private paths, keys, e-mail addresses or traces of tools
python3 -I tools/test_gates.py      # tests of the two scripts above
```

The names that `naming_gate.py` refuses are listed at the top of that file. A file that cannot be
renamed (Cargo names `lib.rs` and `main.rs`) is already allowed.

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
