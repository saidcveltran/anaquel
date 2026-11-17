## What and why

<!-- One paragraph. Link the issue if there is one. -->

## Checklist

- [ ] `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` pass.
- [ ] There is a test for the change, or I say here why there is none.
- [ ] `CHANGELOG.md` has an entry under `[Unreleased]`, with the known limits of the change.
- [ ] Images carry no metadata: `python -I tools/check_images.py` passes.
- [ ] The diff holds no secrets, no personal paths, no private names and no generated-by trailers.
- [ ] No dependency was added, or `deny.toml` and `THIRD-PARTY.md` were updated with it.
