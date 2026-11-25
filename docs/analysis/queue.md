# The merge queue in CI

<!-- audited: 2026-11-25 -->

The merge queue checks that a branch still passes once it merges with the main it lands on.

A pull request reaches the queue after every job in
[pr.yml](../../.github/workflows/pr.yml) passed on its branch ([ci](ci.md)).
Two branches that each passed can break each other only where their changes
meet: in a compile, in a ledger row that both move, or in a test that one
branch wrote against code that the other changed. So
[merge-queue.yml](../../.github/workflows/merge-queue.yml) runs the checks
that read those places, on the default build alone.

## What the queue runs

| Job | Runs | What a merge can break there |
|-----|------|------------------------------|
| QA | `make qa` | A call-out, a link, an audit count, a size row, an index, the format, and clippy and rustdoc over code that both branches changed |
| Producers | `smoke-producers` on the rig | A build ledger row that both branches move |
| Charge | `smoke-charge` on the rig | A file's charge under the other branch's runtime, read against its row |
| Language suite | `smoke-lang`, `doctest` and `semver-check` on `elle` | A language test or a document example against the merged code, and a library surface |
| Implementation suite | `smoke-impl` on the rig | An implementation test, the runner's tests and the command line's tests |

Each job builds what it runs, and no job waits for another. So the queue costs
its slowest job, not the sum of its passes. Every branch in the queue passed
QA already, so a QA job that the others waited for would add its own length to
every merge and rarely save a runner minute.

The QA job's toolchain carries what the pull request's `QA` job carries:
rustfmt, clippy, both cross-check targets and LLVM 22. `make crosscheck`
prints SKIPPED and passes over a target the toolchain lacks, so a missing
target fails nothing. Each job that records a run uploads its store, under a
name of its own ([ci](ci.md)).

## What the queue leaves out

The queue runs no variant build, no other platform, no eager profile, no
embedding demo and no Rust suite. Each of those ran on both branches before
the queue. A merge breaks one of them alone only when the two changes meet in
code that a single build, platform or profile reaches. That case is rare, and
the next pull request's full run on the merged main reports it. Running every
pull-request job again would make each merge wait for the slowest runner in
the workflow.

[merge_queue.rs](../../tests/integration/merge_queue.rs) is the standing check
that the queue runs QA and each of these passes once, that no job waits for
another, and that the gate waits for and fails on every job.
