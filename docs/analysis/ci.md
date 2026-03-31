# CI and Triage

CI structure, local workflow, and failure diagnosis.

## CI structure


| Job | Trigger | What | Proptest cases |
|-----|---------|------|----------------|
| examples | Always | All `.lisp` files in `examples/` | — |
| test-rust | After examples | Unit + integration tests (skip property) | — |
| test-property | After test-rust | All property tests | 8 (PR) / 16 (merge queue) |
| toolchain-check | Weekly | Full suite on beta/nightly | 128 |

Examples gate everything. Fast tier (PR): ~5 minutes. Thorough tier (merge
queue): ~30 minutes. Weekly: full coverage on beta/nightly.


## Local development workflow


```bash
# Smoke test (what agents should run first)
make smoke

# Fast feedback (examples + elle scripts + unit tests)
make smoke

# Run only Elle scripts
cargo test elle::

# Run only property tests, reduced
PROPTEST_CASES=8 cargo test property::

# Run a specific Elle test script
cargo run -- tests/elle/core.lisp

# Full suite (before opening PR, or let CI handle it)
cargo test --workspace
```


## Failure triage


| Failure | Symptom | Likely cause | Fix |
|---------|---------|--------------|-----|
| **elle-doc generation** | `docs` job fails on `./target/release/elle elle-doc/generate.lisp` | Using `nil?` to check end-of-list. Lists terminate with `EMPTY_LIST`, not `NIL`. | Use `empty?` for list termination checks. Check `elle-doc/generate.lisp` and `elle-doc/lib/`. |
| **Examples fail** | `examples` job fails | Runtime error in `.lisp` file. Assertions use `assert-eq`, `assert-true`, etc. from `examples/assertions.lisp`. | Run `cargo run -- examples/failing.lisp` locally. Check assertion message. |
| **Elle scripts fail** | `examples` job fails on Elle script tests | Runtime error in `tests/elle/*.lisp`. | Run `cargo run -- tests/elle/failing.lisp` locally. Check assertion message. |
| **Property tests fail** | `test-property` job fails with shrunk counterexample | The shrunk output shows the *minimal* failing input. | Reproduce with the exact shrunk values as a unit test. Check `proptest-regressions/` files. |
| **Integration tests fail** | `test-rust` job fails | Tests use `eval_source()` which runs the full pipeline. | Read the assertion. Check whether the test expects `.unwrap()` (success) or `.is_err()` (error). |
| **Clippy** | `clippy` job fails | Any Rust warning. CI runs with `-D warnings`. | Run `cargo clippy --workspace --all-targets -- -D warnings` locally. |
| **Formatting** | `fmt` job fails | Unformatted Rust code. | Run `cargo fmt`. |
| **Rustdoc** | `docs` job fails on `cargo doc` step | Broken intra-doc links or malformed doc comments. | Run `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` locally. |


---

## See also

- [Analysis index](index.md)
