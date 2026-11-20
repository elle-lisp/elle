# The runner's acceptance tests

<!-- audited: 2026-11-20 -->

Three Elle programs that drive `elle test` as a subprocess and assert on the
session DB each run writes.

Together they are the runner's contract, scenario by scenario
([test-runner](../../docs/test-runner.md)). Each assertion's exact text is part
of that contract, so change the design document before an assertion.

- [tiers.lisp](tiers.lisp) holds what a run records per tier: the rows a
  form records on each tier, the assertion payload a failure records, a gated
  skip and its reason, and a whole-file script under each JIT policy, atomic,
  in a worker and in-process.
- [agreement.lisp](agreement.lisp) holds when two tiers' values agree, and the
  `diverge` row they record when they do not.
- [acceptance.lisp](acceptance.lisp) holds the rest: ad-hoc forms and their
  promotion, a file that will not compile, the captured output, the timeout, a
  gated shared setup, a form whose value cannot leave its worker, the printed
  summary, the `--host` child, and a file with a sidecar under `elle-rig test`.

All three splice [lib/harness.lisp](lib/harness.lisp), the helpers that start
a run and read its DB.

The implementation suite runs all three on the rig (`make smoke-impl`). Each file
spawns the binary `ELLE` names, which the Makefile exports, and reads each DB
through [sqlite.lisp](../../lib/sqlite.lisp). The sidecar scenario spawns the
binary that runs it, which is the rig under the suite, and gates itself out
under any other binary. Every scenario writes under one scratch directory, so a
run leaves nothing behind. By hand:

```sh
ELLE=./target/debug/elle ./target/debug/elle tests/runner/tiers.lisp
ELLE=./target/debug/elle ./target/debug/elle tests/runner/agreement.lisp
ELLE=./target/debug/elle ./target/debug/elle tests/runner/acceptance.lisp
```

The files under `fixtures/` are their inputs, and no suite runs them on their
own.
