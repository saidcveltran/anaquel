# Fuzzing

Four targets feed random bytes to the parts of Anaquel that read untrusted text:

| Target | What it reads | What must hold |
| --- | --- | --- |
| `directive_grammar` | a page body, split into `:::` blocks | no panic; depth and block limits kept; offsets inside the text |
| `front_matter` | the YAML block at the top of a page | no panic; an accepted `slug` is a safe address segment |
| `url_policy` | a whole page, composed to HTML | no script, style, frame (outside `:::embed`) or dangerous URL in the output |
| `link_resolver` | one link target | only http, https, mailto or a site URL comes out; never a panic |

The checks are in [`crates/anaquel-fuzzcheck`](../crates/anaquel-fuzzcheck/src/lib.rs). The same checks
run over the seed files in `corpus/` and a few thousand mutations of them in the ordinary test run
(`cargo test -p anaquel-fuzzcheck`), on Linux and Windows. That is a regression net, not fuzzing.

Real fuzzing needs a nightly compiler and Linux (the libFuzzer runtime does not link on Windows):

```sh
cargo install cargo-fuzz --locked --version 0.13.2
cargo +nightly fuzz run directive_grammar -- -max_total_time=150
```

CI does this for 150 seconds per target, ten minutes in all. When a target fails, the input is in
`fuzz/artifacts/<target>/`. Copy it into `corpus/<target>/` with a name that says what it was, fix the
bug, and the smoke test keeps it fixed.
