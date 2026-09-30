# The runner's acceptance tests

<!-- audited: 2026-09-29 -->

Two Elle programs that drive `elle test` as a subprocess and assert on the
session DB each run writes.

Together they are the runner's contract, scenario by scenario
([test-runner](../../docs/test-runner.md)). Each assertion's exact text is part
of that contract, so change the design document before an assertion.

- [tiers.lisp](tiers.lisp) holds what a run records per tier: the rows a
  form records on each tier, the assertion payload a failure records, a gated
  skip and its reason, the `diverge` row, and a whole-file script under each
  JIT policy, atomic, in a worker and in-process.
- [acceptance.lisp](acceptance.lisp) holds the rest: ad-hoc forms and their
  promotion, a file that will not compile, the captured output, the timeout, a
  gated shared setup, an unsendable capture, the printed summary, and the
  `--host` child.

Both splice [lib/harness.lisp](lib/harness.lisp), the helpers that start a run
and read its DB.

The implementation suite runs both on the rig (`make smoke-impl`). Each file
spawns the binary `ELLE` names, which the Makefile exports, and reads each DB
through [sqlite.lisp](../../lib/sqlite.lisp). Every scenario writes under one
scratch directory, so a run leaves nothing behind. By hand:

```sh
ELLE=./target/debug/elle ./target/debug/elle tests/runner/tiers.lisp
ELLE=./target/debug/elle ./target/debug/elle tests/runner/acceptance.lisp
```

The files under `fixtures/` are their inputs, one per scenario, and no suite
runs them on their own.
