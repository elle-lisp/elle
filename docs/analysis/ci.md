# CI and Triage

<!-- audited: 2026-09-21 -->

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
| Boot Image Tests | ubuntu | `smoke-boot-image` — the corpus booted from an image | — |
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

Each job caches under its own key — set explicitly with
`Swatinem/rust-cache`'s `shared-key` where a job wants a stable one, and taken
from the per-job default otherwise. The pair builds different profiles, so one
shared key would make the two jobs overwrite each other's cache on alternating
runs.

### What each job builds

Every job that drives the Makefile builds `--release` and runs
`target/release/elle`, on every platform. The Makefile picks that under
`ifdef GITHUB_ACTIONS`, which is set on all GitHub runners, so the macOS and
AArch64 Smoke jobs are release runs exactly as the x86_64 ones are. The Rust
Tests jobs build the dev profile, also on every platform, because `cargo test`
does. No platform is quietly gated at a weaker optimization level.

Three jobs change the release profile they build. `VM+JIT Tests`, `Thread-Pool
I/O Tests` and `macOS Smoke` set `CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS`,
which is what compiles the region checks in. Those checks are
`#[cfg(debug_assertions)]`, so a corpus job without the flag drives the whole
corpus blind to every one of them.

The rule is one such job per I/O backend. `Thread-Pool I/O Tests` covers the
pool and `VM+JIT Tests` covers io_uring, so finding a region defect never
depends on the macOS runner — the slowest box in the workflow, and the one
whose failures read as flaky timeouts (§ "Runner capacity").
`tests/integration/workflows.rs` is the standing check that both backends keep
a job.

`macOS Smoke` sets `--trace=scrub` beside the flag, because the panic that
reads a scrubbed page needs both (docs/impl/region/diagnostics.md). Its binary
is a release build that also runs the debug-only checks, on the smallest runner
in the workflow. Read a macOS corpus timing against that, not against a Linux
one.

### The cross-checks mirror a local target

Two jobs compile for a platform no runner in the workflow executes. `QA` runs
clippy over `x86_64-apple-darwin`, and `Android Cross-Check` runs `cargo check`
over `aarch64-linux-android`. Neither step codegens or links, so neither needs
an SDK or an NDK — only the target's std.

`make crosscheck` runs both, and that is what the target is for. A `cfg` arm no
local command compiles has a runner for its first reader, and the report
arrives after the push rather than before it. The Android job has run since
#752, and the local target arrived later covering macOS alone — so an
Android-only break compiled everywhere a developer could look.

`tests/integration/workflows.rs` is the standing check that every target a job
cross-compiles is a target `make crosscheck` compiles too.

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

So `Plugin Tests` checks the submodule out at its recorded pointer, builds
every plugin in the workspace, asserts the portable artifacts, and runs
`plugins/tests/*.lisp` against the release binary. Building at the recorded
pointer is also what keeps the pointer fresh: a pointer left behind an ABI
change names plugins that no longer compile, and the job fails on them.

#### Why the job builds every plugin, not the portable set

`make plugins` builds the packages in `plugins/Makefile`'s `PORTABLE` list.
Five plugins sit outside that list — `elle-arrow`, `elle-polars`,
`elle-vulkan`, `elle-egui` and `elle-wayland` — so a job that stopped at `make
plugins` left the hole above open for them. The break this job exists to catch
is a compile error, so compiling every plugin is the whole gate. The job runs
`make plugins-all`, which builds the workspace.

The five cost nothing extra on the runner. Each reaches its system library
through `dlopen` rather than through the linker: `ash` is taken with the
`loaded` feature, and `wayland-client`, `winit` and `glutin` open theirs at run
time. `ldd` over the five artifacts names nothing outside libc, and the five
build from a cold target directory with `PKG_CONFIG` pointed at a program that
always fails. The install list below is therefore the same list `make plugins`
needed.

`PORTABLE` still decides what the artifact assertion demands and what the
corpus can rely on. It is a statement about what is safe to **run** on a
headless runner, not about what compiles.

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

The assertion names the portable set, not every plugin, because the two lists
answer different questions. Compiling is what the job builds all of them for.
Asserting an artifact matters where a corpus file would otherwise report
success over a plugin that never built, and `elle-vulkan`, `elle-egui` and
`elle-wayland` need a GPU or a display before they can do anything a corpus
file could assert.

That leaves one thing for a test to hold. `make plugins-all` compiles the
members of `plugins/Cargo.toml`'s workspace, so a plugin directory missing from
that list is compiled by nothing and asserted by nothing, and the job stays
green over it. `tests/integration/plugins.rs` pins every plugin directory to a
workspace member.

### The boot-image job

`--boot-image=` is off by default, and stays off until the encoded-LIR
side-stream and the two cross-unit registries land
([boot.md](../impl/image/boot.md)). No other job boots from an image, so a
change that breaks hydration passes every gate in the workflow.

`Boot Image Tests` runs `make smoke-boot-image`. The target stores an image,
proves the next start hydrates it, and runs the corpus through that instance.
It sets `CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS` for the reason the two backend
jobs do: a hydrated region is a region, and the region checks are compiled out
of a release build without the flag.

### Adding a job

`All Checks Passed` is the only status check branch protection requires
(`.github/BRANCH_PROTECTION.md`). A new job that is missing from that job's
`needs` list runs, reports, and cannot block a merge. Add the job to `needs`
and give it a check step. `tests/integration/workflows.rs` fails when either
is missing.


## Local development workflow


```bash
# The fast inner loop
cargo test -p elle --lib

# The QA job, locally — run it before every push
make qa

# The corpus, plus the doctests and the embedding demo
make smoke

# One corpus file
./target/release/elle tests/elle/core.lisp

# Property tests, reduced
PROPTEST_CASES=8 cargo test --test lib property::

# What the PR gate runs, before opening a pull request
make test
```


## Failure triage


| Failure | Symptom | Likely cause | Fix |
|---------|---------|--------------|-----|
| **Documentation site** | `Documentation Build` fails on `./target/release/elle demos/docgen/generate.lisp` | Using `nil?` to check end-of-list. Lists terminate with `EMPTY_LIST`, not `NIL`. | Use `empty?` for list termination checks. Check `demos/docgen/generate.lisp` and `demos/docgen/lib/`. |
| **Elle scripts fail** | `VM+JIT Tests` fails on a corpus pass | Runtime error in `tests/elle/*.lisp`. | Run `./target/release/elle tests/elle/failing.lisp` locally. Check the assertion message. |
| **Boot from an image fails** | `Boot Image Tests` fails and every other corpus job passes | The corpus file answers differently under a hydrated boot, or the image no longer hydrates. | Run `make smoke-boot-image` locally. Read the hydration proof first: a target that fails there never reached the corpus. |
| **Property tests fail** | `Rust Tests` fails with a shrunk counterexample | The shrunk output shows the *minimal* failing input. | Reproduce with the exact shrunk values as a unit test. Check `proptest-regressions/` files. |
| **Integration tests fail** | `Rust Tests` fails | Tests use `eval_source()` which runs the full pipeline. | Read the assertion. Check whether the test expects `.unwrap()` (success) or `.is_err()` (error). |
| **Clippy** | `QA` fails on the clippy step | Any Rust warning. CI runs with `-D warnings`. | Run `cargo clippy --workspace --all-targets -- -D warnings` locally. |
| **Formatting** | `QA` fails on `cargo fmt --check` | Unformatted Rust code. | Run `cargo fmt`. |
| **Rustdoc** | `QA` fails on the `cargo doc` step | Broken intra-doc links or malformed doc comments. CI documents private items, so a link into a `pub(crate)` item counts. A `#[cfg(test)]` item is absent from a doc build — gate it `#[cfg(any(test, doc))]` if the docs link to it. | Run `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items` locally. |
| **macOS cross-check** | `qa` job fails on `Cross-check macOS`, or `macOS Smoke` fails on `Run clippy` | A binding or method used only by the io_uring backend reads as dead code on the thread-pool platform. The Linux clippy gate compiles only the `cfg(target_os = "linux")` arms and cannot see it. | Run `make crosscheck` locally. Gate the binding with `#[cfg(target_os = "linux")]`, or narrow the allow with `#[cfg_attr(not(target_os = "linux"), allow(dead_code))]`. |
| **Android cross-check** | `android` job fails on `Cross-check Android` | A `not(target_os = "linux")` arm that assumed the other side was a desktop unix. Android is neither: `target_os` is `"android"`, so it takes the else-arm, and its libc is missing what a BSD or macOS arm reaches for. | Run `make crosscheck` locally — it covers both cross-targets. Split the arm by the call the platform has (`any(target_os = "linux", target_os = "android")`), not by the name. |


---

## See also

- [Analysis index](index.md)
