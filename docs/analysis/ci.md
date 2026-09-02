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
| Plugin Tests | ubuntu | Builds the `plugins/` submodule, asserts its artifacts, runs its corpus | — |
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

### What each job builds

Every job that drives the Makefile builds `--release` and runs
`target/release/elle`, on every platform. The Makefile picks that under
`ifdef GITHUB_ACTIONS`, which is set on all GitHub runners, so the macOS and
AArch64 Smoke jobs are release runs exactly as the x86_64 ones are. The Rust
Tests jobs build the dev profile, also on every platform, because `cargo test`
does. No platform is quietly gated at a weaker optimization level.

One job changes the release profile it builds. `macOS Smoke` sets
`CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS`, because the panic that reads a
scrubbed page is `#[cfg(debug_assertions)]` and `--trace=scrub` is worthless
without it (docs/impl/region/diagnostics.md). Its binary is therefore a release
build that also runs the debug-only checks, on the smallest runner in the
workflow. Read a macOS corpus timing against that, not against a Linux one.

### Runner capacity

The corpus passes run one process per file, `parallel -j $(JOBS)`. On CI the
Makefile reads that count from the runner — `nproc`, or `getconf
_NPROCESSORS_ONLN` where `nproc` is absent, which is every macOS runner. It does
not write a number down.

GitHub does not give every runner the same machine, and it re-sizes them
without notice. Today the Linux x86_64 and AArch64 runners report four
processors and the macOS runner reports three. A constant chosen for one runner
over-subscribes the other.

Over-subscription does not fail the corpus, it stretches it. Every file still
passes its assertions, and the ones nearest the per-file budget get killed on
the way out. That failure is exit 124 with no output, which reads as a flaky
runner rather than as a job count that never fit. `JOBS` remains an override
for a job that needs a different number.

Outside CI the default stays the constant 16. A development box is not sized by
the runner, and its owner can pass `JOBS=`.

`tests/integration/capacity.rs` is the standing check that the CI count still
tracks the runner.

### The plugins job

The `plugins/` submodule is a separate cargo workspace. Every plugin in it
takes `elle-plugin` by path, so a rebuild moves the plugin and the SDK
together: a changed `elle_api!` declaration either compiles at every call site
or does not. #997 changed six declarations and stopped 17 plugins from
compiling, at about 90 call sites, and nothing went red. No job checked the
submodule out.

The ABI version guard does not close that hole. It compares `ABI_VERSION` when
a plugin loads, so it catches a stale `.so` built against an older SDK. A
source break never reaches a load. Only a job that compiles the submodule sees
one.

So `Plugin Tests` checks the submodule out at its recorded pointer, builds the
portable plugins, asserts the artifacts, and runs `plugins/tests/*.lisp`
against the release binary. Building at the recorded pointer is also what keeps
the pointer fresh: a pointer left behind an ABI change names plugins that no
longer compile, and the job fails on them.

#### What the runner has to install

Two plugins in the portable set are not pure Rust, and neither says so in its
own `Cargo.toml`. The system libraries arrive several crates deep:
`elle-oxigraph` reaches `oxrocksdb-sys`, which runs bindgen over vendored
RocksDB and needs libclang; `elle-plotters` reaches `font-kit`, which needs
fontconfig through pkg-config and pulls the freetype, expat and png headers
behind it.

A manifest scan finds none of that, which is how the job's first run failed on
a fontconfig nobody had declared. Two other readings do find it, and they
answer different questions.

For what the plugins **link**, read the built artifacts. After a local `make
plugins`, `ldd target/release/libelle_*.so` names every shared library and
which plugin needs it. Today that is `libfontconfig`, `libfreetype`,
`libexpat`, `libpng16` and their compression chain under
`libelle_plotters.so`, plus `libstdc++` under `libelle_oxigraph.so`, and
nothing under any other portable plugin.

For what the plugins **build with**, `ldd` says nothing: a build-time tool
leaves no trace in the artifact. `oxrocksdb-sys` runs bindgen, which dlopens
libclang at build time and links none of it — the job's second run failed
there, one layer past the first. That class shows up only in a build on a bare
runner, so read it out of the failure and set the variable the tool asks for.
The job locates `libclang.so` rather than naming an LLVM version, because the
version moves with the runner image.

#### Why the job asserts its build output

Each `plugins/tests/*.lisp` imports its `.so` under `protect` and exits 0 when
the import fails. That is deliberate, because the file has to run on a tree
where the submodule was never built. It also means a plugin that did not build
makes its own test report success: run from a directory where the paths did not
resolve, 13 of 19 files reported `ok` having executed nothing.

`make plugins-verify` asserts the build output separately. Every package in
`plugins/Makefile`'s `PORTABLE` list must have produced its cdylib under
`target/release`. The list is read back out of the submodule's own make rather
than copied, so a plugin added there is demanded here with no second edit. With
the artifacts asserted, the self-gating is harmless — the tests never have to be
the thing that detects a missing plugin. `make smoke-plugins` runs the
assertion before the corpus for the same reason.

The assertion names the portable set, not every plugin. `elle-polars` does not
build on the current toolchain: its `ethnum` dependency fails a
`mem::transmute` size check under this rustc, unrelated to elle. `elle-arrow`
carries heavy optional dependencies, and `elle-vulkan`, `elle-egui` and
`elle-wayland` need a GPU or a display to exercise. The tests for those plugins
gate themselves out of the run.

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
