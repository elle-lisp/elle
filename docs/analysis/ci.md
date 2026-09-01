# CI and Triage

CI structure, local workflow, and failure diagnosis.

## CI structure

`.github/workflows/pr.yml` runs on every pull request. `Detect Changes` reads
the diff and sets `source`, which gates every job below it except
`Documentation Build` — that one runs on a Markdown-only PR too, because a
renamed heading breaks the site generator.

| Job | Runner | What | Proptest cases |
|-----|--------|------|----------------|
| Detect Changes | ubuntu | Sets `source` from the changed paths | — |
| QA | ubuntu | `cargo fmt`, clippy, the macOS cross-check, rustdoc | — |
| Documentation Build | ubuntu | `make docs` and the Elle doc site, minus the publish | — |
| VM+JIT Tests | ubuntu | `doctest`, `smoke-vm`, `smoke-jit` | — |
| Rust Tests | ubuntu | Integration tests, then property tests | 16 |
| Thread-Pool I/O Tests | ubuntu | The corpus on the thread-pool I/O backend | — |
| MLIR Tests | ubuntu | `smoke-mlir` | — |
| WASM Build | ubuntu | `check-wasm` — the feature compiles, the tier boots | — |
| AArch64 Smoke | ubuntu-arm | `make smoke` | — |
| AArch64 Rust Tests | ubuntu-arm | Integration tests, then property tests | 8 |
| AArch64 No-Features | ubuntu-arm | `smoke-noffi` | — |
| Android Cross-Check | ubuntu | `cargo check` for `aarch64-linux-android` | — |
| macOS Smoke | macos | clippy, then `make smoke` under `--trace=scrub` | — |
| macOS Rust Tests | macos | Integration tests, then property tests | 8 |
| All Checks Passed | ubuntu | The one status check branch protection requires | — |

The merge queue (`merge-queue.yml`) runs `make smoke` alone, with
`PROPTEST_CASES=1`. The weekly schedule (`weekly.yml`) runs the whole workspace
suite on beta and nightly at 128 cases, plus a dependency audit.

### Why each platform has two test jobs

The corpus and the Rust suite share no work. The corpus drives the release
binary through `elle test` and through one process per file; the Rust suite
builds separate test binaries under the dev profile. A job that runs both pays
the sum of two build trees and two run times, in series, and the pull request
waits for whichever platform does that.

So each platform splits them: a Smoke job for the corpus, a Rust Tests job for
`cargo test`. The two jobs run at the same time, and the platform costs the
slower of the pair instead of the sum. The split doubles the runner minutes the
platform spends and roughly halves the wall clock, which is the trade the merge
gate cares about.

Each job keeps its own `Swatinem/rust-cache` `shared-key`. The pair builds
different profiles, so one shared key would make the two jobs overwrite each
other's cache on alternating runs.

### Adding a job

`All Checks Passed` is the only status check branch protection requires
(`.github/BRANCH_PROTECTION.md`). A new job that is missing from that job's
`needs` list runs, reports, and cannot block a merge. Add the job to `needs`
and give it a check step. `tests/integration/workflows.rs` fails when either
is missing.


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
| **Rustdoc** | `docs` job fails on `cargo doc` step | Broken intra-doc links or malformed doc comments. CI documents private items, so a link into a `pub(crate)` item counts. A `#[cfg(test)]` item is absent from a doc build — gate it `#[cfg(any(test, doc))]` if the docs link to it. | Run `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items` locally. |
| **macOS cross-check** | `qa` job fails on `Cross-check macOS`, or `macOS Smoke` fails on `Run clippy` | A binding or method used only by the io_uring backend reads as dead code on the thread-pool platform. The Linux clippy gate compiles only the `cfg(target_os = "linux")` arms and cannot see it. | Run `make crosscheck` locally. Gate the binding with `#[cfg(target_os = "linux")]`, or narrow the allow with `#[cfg_attr(not(target_os = "linux"), allow(dead_code))]`. |


---

## See also

- [Analysis index](index.md)
