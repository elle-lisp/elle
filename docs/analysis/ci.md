# CI and Triage

<!-- audited: 2026-10-21 -->

CI structure, local workflow, and failure diagnosis.

## CI structure

[pr.yml](../../.github/workflows/pr.yml) runs on every pull request. `QA` runs
first, on every pull request, and every other job waits for it to pass. `Detect
Changes` reads the diff and sets `source`, which gates every job after QA except
two. `Documentation Build` runs on a Markdown-only PR too, because a renamed
heading breaks the site generator. `Plugin Tests` reads `plugins`, which is also
set when the diff moves only the submodule pointer.

| Job | Runner | What | Proptest cases |
|-----|--------|------|----------------|
| Detect Changes | ubuntu | Sets `source` from the changed paths | — |
| QA | ubuntu | The prose checks, `cargo fmt`, the indexes, the tree's ledgers, then clippy, the macOS cross-check, rustdoc, the Rust doctests | — |
| Documentation Build | ubuntu | `make docs` and the Elle doc site, minus the publish | — |
| Default Build Tests | ubuntu | The producer pass, `doctest`, then each other suite pass of the default build and its rig, one step each; `signal-solve`, the cross-file signal solver ([solver](../impl/solver.md)) | — |
| No-JIT Build Tests | ubuntu | `smoke-nojit` — the language suite on the interpreter alone | — |
| Boot Image Tests | ubuntu | `smoke-boot-image` — bytecode parity between the two boots, then the language suite booted from an image | — |
| Rust Tests | ubuntu | Integration tests, the rig's tests, then property tests, under the `ci` profile | 16 |
| Thread-Pool I/O Tests | ubuntu | `smoke-pool-producers`, `smoke-pool`, `smoke-pool-impl` — both suites on a build without `uring` and its rig | — |
| MLIR Tests | ubuntu | `smoke-mlir-producers`, `doctest`, `smoke-mlir`, `smoke-mlir-impl` — the language suite on the MLIR build, the implementation suite on its rig | — |
| WASM Build | ubuntu | `check-wasm` — the feature compiles, the tier boots | — |
| Plugin Tests | ubuntu | Builds the `plugins/` submodule, asserts its artifacts, runs its corpus | — |
| AArch64 Smoke | ubuntu-arm | Each pass of `make smoke`, one step each | — |
| AArch64 Rust Tests | ubuntu-arm | As Rust Tests | 8 |
| AArch64 No-Features | ubuntu-arm | `smoke-noffi` — the language suite on a build with no features, under the default build's runner | — |
| Android Cross-Check | ubuntu | `cargo check` for `aarch64-linux-android` | — |
| macOS Smoke | macos | clippy, then each pass of `make smoke`, one step each, then `smoke-scrub` | — |
| macOS Rust Tests | macos | As Rust Tests | 8 |
| All Checks Passed | ubuntu | The one status check branch protection requires | — |

The merge queue ([merge-queue.yml](../../.github/workflows/merge-queue.yml))
runs `make smoke`, split into steps: `make qa`, then each pass `SMOKE_PASSES`
names, with `PROPTEST_CASES=1`. So the queue runs QA on the merged result,
which no pull-request job sees. Its toolchain therefore carries what the `QA`
job's does: rustfmt, clippy, both cross-check targets and LLVM 22. Between QA
and the first pass, the queue builds the plugins the language suite loads
([plugins](plugins.md)). The weekly schedule
([weekly.yml](../../.github/workflows/weekly.yml)) runs the whole workspace
suite on beta and nightly at 128 cases, plus a dependency audit.

### QA runs first, and every other job waits for it

QA fails in minutes, and the suites and the Rust tests take up to an hour. So
no other job starts until QA passes, and a change that fails QA costs one short
job instead of the whole matrix.

The prose checks run first inside QA. [prose.rs](../../tests/prose.rs) holds
every stamped file to the parts of [the policy](../../DOCUMENTATION.md) a
machine can decide, so an over-long call-out fails in minutes rather than
inside the Rust Tests jobs. The checks have a binary of their own, so QA does
not wait for the shared test binary to compile. `make prose` runs them, and
`make qa` runs `make prose` before anything else.

Two producers read the tree and not the build:
[audit.lisp](../../tests/ratchet/audit.lisp) counts the audit queue, and
[size.lisp](../../tests/ratchet/size.lisp) prices each file over its cap. A
change to any document can move either reading, and a build cannot. So QA
builds the release rig and runs `make tree-producers`, which judges both
against their ledgers before any suite starts, and no build's producer pass
runs them again. That step records a run, so QA uploads its store (§ A corpus
job's results leave the runner).

QA runs on every pull request, with no `source` gate. A Markdown-only change
can break a call-out or a link, and those are what the prose checks read.
[qa_gate.rs](../../tests/integration/qa_gate.rs) is the standing check that QA
runs first, runs always, and gates every other job.

### Each build is an implementation

The corpus jobs differ in the build they make, not in flags they pass. A build
carries one optimizing tier and one I/O backend ([config](../config.md)), and
every build runs the language suite with no flag ([spec](../spec.md)). So the
job list above is the implementation matrix. It holds the default build, a
build with no JIT, a thread-pool build, an MLIR build, a build with no
features, and the default build on AArch64 and macOS. A language test that
passes on one and fails on another has found a defect in the build that fails.

The build with no features cannot host the runner: the runner's store reaches
SQLite and zstd through FFI. So `smoke-noffi` runs `elle test` on the default
build and each file's child on the no-features binary, `ELLE_NOFFI`, through
`--host`. Each child is still the build under test, run with no flag, and
`AArch64 No-Features` builds both binaries.

The implementation suite runs where the default build runs: `Default Build
Tests`, `AArch64 Smoke` and `macOS Smoke` each build the rig beside `elle`, and
each runs every suite pass of `make smoke`. `Thread-Pool I/O Tests` runs the
implementation suite on the pool build's rig too, because some resources of this
implementation exist only on the pool. A file that counts worker threads reads
zero on io_uring and gates itself there. `MLIR Tests` runs it on the MLIR
build's rig, the one rig that carries the MLIR tier.
[workflows.rs](../../tests/integration/workflows.rs) is the standing check that
every implementation keeps its job.

### Why each platform has two test jobs

The corpus and the Rust suite share no work. The corpus drives the release
binary through `elle test`; the Rust suite
builds separate test binaries under the `ci` profile. A job that runs both pays
the sum of two build trees and two run times, in series, and the pull request
waits for whichever platform does that.

So each platform splits them: a Smoke job for the corpus, a Rust Tests job for
`cargo test`. The two jobs run at the same time, and the platform costs the
slower of the pair instead of the sum. The split doubles the runner minutes the
platform spends and roughly halves the wall clock, which is the trade the merge
gate cares about.

A Smoke job runs each pass of `make smoke` as a step of its own, not `make
smoke` itself, for the reason the next section gives. The Makefile names the passes once, in
`SMOKE_PASSES`. Each pass runs once. A `make smoke` step after the job's
`doctest` step would run the doctests twice. A Smoke job runs no `make qa`: the
`QA` job is the QA gate for every platform.

### One step, one pass

A make target runs at most one `elle test` pass, and a job runs each such
target as a step of its own. So a step's time is one pass's time, a red step
names the pass that failed, and a pass's log starts where its step starts.

A pass deals its files into batches and runs them side by side, and each batch
prints its own summary. Each pass is a target of its own, and
[the rig](../../rig/overview.md) names the ones that run on a rig.

[Testing](../testing.md) describes what each batch prints. What a batch
records leaves the runner in the store the job uploads, as a later section
describes.

A job runs its build's producer pass right after the build, before `doctest`
and every other pass. The pass is short, and a stale ledger row fails it as
surely as a broken file fails a suite. `SMOKE_PASSES` names it first, so
`make smoke` and the merge queue keep that order.

[ci_steps.rs](../../tests/integration/ci_steps.rs) is the standing check that
no step runs a target of more than one pass and that the producer pass leads,
and [suites.rs](../../tests/integration/suites.rs) that every suite target runs
exactly one.

Each job caches under its own key — set explicitly with
`Swatinem/rust-cache`'s `shared-key` where a job wants a stable one, and taken
from the per-job default otherwise. The pair builds different profiles, so one
shared key would make the two jobs overwrite each other's cache on alternating
runs.

### What each job builds

Every job that drives the Makefile builds `--release` and runs its binaries
from `target/release`, on every platform: `elle` and `elle-rig`, or a variant's
own ([bins](../../bins/overview.md)). The Makefile picks that under
`ifdef GITHUB_ACTIONS`, which is set on all GitHub runners, so the macOS and
AArch64 Smoke jobs are release runs exactly as the x86_64 ones are.

The Rust Tests jobs build the `ci` profile on every platform, and `make test`
builds it too. The profile is the dev profile with the workspace crates at
`opt-level = 2`. Most of the Rust suite's time goes to the `elle` processes its
tests start, and each one at `opt-level = 0` runs about ten times slower. The
profile keeps debug assertions, so the region checks still run. Each test
runtime and each process it starts caches its standard library under the
runner's `HOME` ([config](../config.md)).

A release `make elle` embeds a boot image in the binaries it builds, so every
corpus job's `elle` and `elle-rig` hydrate it at every start
([blob.md](../impl/image/blob.md)).

Four jobs change the release profile they build. `Default Build Tests`, `Boot
Image Tests`, `Thread-Pool I/O Tests` and `macOS Smoke` set
`CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS`, which is what compiles the region
checks in. Those checks are
`#[cfg(debug_assertions)]`, so a corpus job without the flag drives the whole
corpus blind to every one of them.

`Thread-Pool I/O Tests` also builds without the `uring` feature. That binary
runs every I/O operation on the thread pool, the backend every non-Linux build
runs, so a pool-only defect fails on a Linux runner before it reaches the Mac.

The rule is one such job per I/O backend. `Thread-Pool I/O Tests` covers the
pool and `Default Build Tests` covers io_uring, so finding a region defect never
depends on the macOS runner. That runner is the slowest box in the workflow, and
its failures read as flaky timeouts, as the runner capacity section explains.
[workflows.rs](../../tests/integration/workflows.rs) is the standing check that
both backends keep a job.

`macOS Smoke` runs the rig's scrub profile beside the flag, because the panic
that reads a scrubbed page needs both
([diagnostics](../impl/region/diagnostics.md)). Its binary is a release build
that also runs the debug-only checks, on the smallest runner in the workflow. Read a macOS corpus timing against that, not against a Linux
one.

### Every job that runs `cargo test` installs rustfmt and clippy

The Rust suite runs the real pre-commit hook in a scratch repository
([pre_commit.rs](../../tests/integration/pre_commit.rs)). On a staged `.rs`
file the hook calls `rustfmt`, then `cargo clippy`. So every job that runs
`cargo test` names both in its toolchain step's `components`.

A toolchain the runner image does not carry arrives in rustup's minimal
profile, which holds neither tool. The weekly beta and nightly jobs are of that
kind. Stable jobs find both tools in the image, and naming them takes that
dependency away. On nightly, the toolchain action takes the newest build that
carries every named component.

[toolchains.rs](../../tests/integration/toolchains.rs) is the standing check
that every such job, in every workflow, names both.

### The cross-checks mirror a local target

Two jobs compile for a platform no runner in the workflow executes. `QA` runs
clippy over `x86_64-apple-darwin`, and `Android Cross-Check` runs `cargo check`
over `aarch64-linux-android`. Neither step codegens or links, so neither needs
an SDK or an NDK — only the target's std.

`make crosscheck` runs both, and that is what the target is for. A `cfg` arm no
local command compiles has a runner for its first reader, and the report
arrives after the push rather than before it.

[crosscheck.rs](../../tests/integration/crosscheck.rs) is the standing check
that every target a job cross-compiles is a target `make crosscheck` compiles
too.

### Runner capacity

The suites run in `$(JOBS)` batches side by side. On CI the
Makefile reads that count from the runner — `nproc`, or `getconf
_NPROCESSORS_ONLN` where `nproc` is absent, which is every macOS runner. It does
not write a number down.

GitHub does not give every runner the same machine, and it re-sizes them
without notice. A constant chosen for one runner over-subscribes another.

Over-subscription does not fail the corpus, it stretches it. Every file still
passes its assertions, and the ones nearest the per-file budget get killed on
the way out. That failure is a `timeout` row that names the budget, which
reads as a flaky runner rather than as a job count that never fit. `JOBS` remains an override
for a job that needs a different number.

Outside CI the default stays the constant 16. A development box is not sized by
the runner, and its owner can pass `JOBS=`.

No workflow sets `RUST_TEST_THREADS`, for the same reason. The test harness
then runs one test per processor the runner reports.

[capacity.rs](../../tests/integration/capacity.rs) is the standing check that
the CI count still tracks the runner, and that no workflow sets a test thread
count.

### Corpus batch size

The suite targets deal each suite into batches of `CORPUS_BATCH` files, one
`elle test` process per batch, and run `$(JOBS)` batches side by side. A
language pass runs its files inside the runner process, so there a batch bounds
the heap one runner holds. An implementation pass runs each file as its own
child, so there a batch bounds only how many files one runner records.

The default is 25 files. `macOS Smoke` and `AArch64 Smoke` use 10, because a
25-file batch stalled on macOS when a batch ran all its files inside the runner
process. AArch64 never stalled, and takes the smaller batch to shorten its wall
clock. Measure again before you change either.

The working theory for the macOS stall is page size. Pages on the macOS runner
(arm64) are 16 KiB, and pages on the x86_64 Linux runner are 4 KiB. Nobody has
confirmed the theory, so the smaller batch is a mitigation and not a diagnosis.

The Makefile reads the platform from `HOST_OS` and `HOST_ARCH`, which default to
`uname -s` and `uname -m`. A batch of 10 applies on Darwin and on `aarch64` or
`arm64`. A test sets both variables to present another platform. Pass
`CORPUS_BATCH=` to override the choice for one run.

### The plugins submodule

`Plugin Tests` compiles every plugin in the `plugins/` submodule, and every job
that runs the language suite builds the plugins that suite loads first.
[The plugins submodule in CI](plugins.md) holds the argument for both.

### The boot-image job

Every release `make elle` proves that its embedded blob hydrates. A start that
cannot hydrate boots from source, so the suites pass either way. No other job
proves that the warm cache hydrates, and none compares the two boots' bytecode.
So `Boot Image Tests` runs `make smoke-boot-image`
([boot.md](../impl/image/boot.md) owns the gate). It sets
`CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS` for the reason the two backend jobs
do: a hydrated region is a region, and the region checks are compiled out of a
release build without the flag.

### A corpus job's results leave the runner

A job that runs `elle test` records every form's verdict in a session DB, with
stdout, stderr and the rest of each result's assets in the CAS beside it
([test-store](../test-store.md)). The runner exists so that a failure is a
query rather than a log. On a runner you cannot reach, that holds only once
the store is uploaded.

So each job that records runs points `ELLE_STATE` at a directory in the
workspace, and uploads the session DB and the CAS beside it as an artifact,
pass or fail, under a retention window. The scratch directory stays behind: a
worker's redirect files are disposable, and the two the artifact carries are
what `--import` reads. The state directory is named rather than left to
default because the default sits under `HOME`, which moves with the runner
image. A fresh runner has no history to accumulate, so nothing is lost by
moving it.

Read one back by merging it into local history:

```bash
gh run download <run-id> --name <artifact> --dir far
elle test --import far/elle-tests.db
elle test --query "SELECT f.file, r.tier, r.reason FROM result r
  JOIN form f ON f.hash = r.form_hash
  WHERE r.run_id = (SELECT max(id) FROM run) AND r.status = 'fail'"
```

The imported run keeps the commit, the worktree, the host and the build it ran
under, so it answers beside every local run rather than in a database of its
own. [run_artifacts.rs](../../tests/integration/run_artifacts.rs) is the
standing check that every job recording a run uploads its store.

### Adding a job

`All Checks Passed` is the only status check branch protection requires
([BRANCH_PROTECTION.md](../../.github/BRANCH_PROTECTION.md)). A new job that
is missing from that job's `needs` list runs, reports, and cannot block a
merge. Add the job to `needs` and give it a check step. Name `qa` in the new
job's own `needs`, so it waits for QA like every other job.
[workflows.rs](../../tests/integration/workflows.rs) fails when either of the
first two is missing, and [qa_gate.rs](../../tests/integration/qa_gate.rs) when
the third is.


## Local development workflow


```bash
# The fast inner loop
cargo test -p elle --lib

# The prose checks, the first step of QA
make prose

# The QA job, locally
make qa

# QA, then the corpus, the doctests, the embedding demo and the surface gate
make smoke

# One corpus file
./target/release/elle tests/lang/core.lisp

# Property tests, reduced
PROPTEST_CASES=8 cargo test --test lib property::

# What the PR gate runs, before opening a pull request
make test
```


## Failure triage


| Failure | Symptom | Likely cause | Fix |
|---------|---------|--------------|-----|
| **Documentation site** | `Documentation Build` fails on `./target/release/elle demos/docgen/generate.lisp` | Using `nil?` to check end-of-list. Lists terminate with `EMPTY_LIST`, not `NIL`. | Use `empty?` for list termination checks. Check [generate.lisp](../../demos/docgen/generate.lisp) and [lib](../../demos/docgen/lib). |
| **Language suite fails** | A corpus job fails `smoke-lang` | A language test answers differently on that build. | Run `./target/release/elle tests/lang/failing.lisp` on the same build. Check the assertion message; a file that passes on one build and fails on another has found a defect in the build that fails. |
| **Implementation suite fails** | `Default Build Tests` fails `smoke-impl` or `smoke-eager` | An implementation test on the rig, or a suite file under the eager profile. | Run `./target/release/elle-rig tests/impl/failing.lisp`; the file's sidecar sets its mode. For a profile failure, pass the same `--profile`. |
| **The embedded blob fails** | A corpus job fails on `make elle` after the second build | The second build's dump differs from the blob it carries, so the two builds compiled the library differently. Or a start did not map the blob from its own executable. | Run `make elle CARGO_PROFILE=--release` locally. A dump difference keeps both files to `cmp`. Otherwise read the `--trace=boot` lines it prints: `blob-copy` names a blob the start could not find in its own file. |
| **Boot from an image fails** | `Boot Image Tests` fails and every other corpus job passes | A corpus file compiles to other bytecode or answers differently under a hydrated boot, or the image no longer hydrates. | Run `make smoke-boot-image` locally. A parity failure names the files that differ and keeps both dumps of each under `target/bytecode-parity`; diff a pair. Otherwise read the hydration proof first: a target that fails there never reached the corpus. |
| **Signal solver** | `Default Build Tests` fails `signal-solve` | A fixture's `# expect` line no longer holds, or the two engines solved different models. | Run `make signal-solve` locally. A disagreement names the engines; [solver.md](../impl/solver.md) states the rules both must solve. |
| **Property tests fail** | `Rust Tests` fails with a shrunk counterexample | The shrunk output shows the *minimal* failing input. | Reproduce with the exact shrunk values as a unit test. Check the files under `tests/proptest-regressions/`. |
| **Integration tests fail** | `Rust Tests` fails | Tests use `eval_source()` which runs the full pipeline. | Read the assertion. Check whether the test expects `.unwrap()` (success) or `.is_err()` (error). |
| **Prose** | `QA` fails on `make prose` | A stamped file breaks a rule a machine can check: a call-out over 140 characters, a broken link, a bare filename, or banned wording. | Run `make prose` and repair the file the message names ([DOCUMENTATION.md](../../DOCUMENTATION.md)). |
| **The tree's ledgers** | `QA` fails `make tree-producers` on a row of [size.lisp](../../tests/ratchet/size.lisp) or [audit.lisp](../../tests/ratchet/audit.lisp) | A file grew past its bucket or shrank under it, a file new to the size ledger raised the excess, or a stamp moved an audit count. | Shed the growth or split the file ([DOCUMENTATION.md](../../DOCUMENTATION.md)). Re-pin a stale row with `elle-rig test --repin` and the producer's path, and delete a missing one. |
| **Clippy** | `QA` fails on the clippy step | Any Rust warning. CI runs with `-D warnings`. | Run `cargo clippy --workspace --all-targets -- -D warnings` locally. |
| **Formatting** | `QA` fails on `cargo fmt --check` | Unformatted Rust code. | Run `cargo fmt`. |
| **Rustdoc** | `QA` fails on the `cargo doc` step | Broken intra-doc links or malformed doc comments. CI documents private items, so a link into a `pub(crate)` item counts. A `#[cfg(test)]` item is absent from a doc build — gate it `#[cfg(any(test, doc))]` if the docs link to it. | Run `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items` locally. |
| **macOS cross-check** | `qa` job fails on `Cross-check macOS`, or `macOS Smoke` fails on `Run clippy` | A binding or method used only by the io_uring backend reads as dead code on the thread-pool platform. The Linux clippy gate compiles only the `cfg(target_os = "linux")` arms and cannot see it. | Run `make crosscheck` locally. Gate the binding with `#[cfg(target_os = "linux")]`, or narrow the allow with `#[cfg_attr(not(target_os = "linux"), allow(dead_code))]`. |
| **Android cross-check** | `android` job fails on `Cross-check Android` | A `not(target_os = "linux")` arm that assumed the other side was a desktop unix. Android is neither: `target_os` is `"android"`, so it takes the else-arm, and its libc is missing what a BSD or macOS arm reaches for. | Run `make crosscheck` locally — it covers both cross-targets. Split the arm by the call the platform has (`any(target_os = "linux", target_os = "android")`), not by the name. |


---

## See also

- [Analysis index](index.md)
