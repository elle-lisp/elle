# The runner's acceptance test

<!-- audited: 2026-09-28 -->

An Elle program that drives `elle test` as a subprocess and asserts on the
session DB each run writes.

[acceptance.lisp](acceptance.lisp) is the runner's contract, scenario by
scenario: one `worker` row per file, the assertion payload a failure records,
a gated skip and its reason, whole-file atomicity, the timeout, the captured
output and the `--host` child ([test-runner](../../docs/test-runner.md)). Each
assertion's exact text is part of that contract, so change the design document
before an assertion.

The implementation suite runs it on the rig (`make smoke-impl`). The file
spawns the binary `ELLE` names, which the Makefile exports, and reads each DB
through [sqlite.lisp](../../lib/sqlite.lisp). Every scenario writes under one
scratch directory, so a run leaves nothing behind. By hand:

```sh
ELLE=./target/debug/elle ./target/debug/elle tests/runner/acceptance.lisp
```

The files under `fixtures/` are its inputs, one per scenario, and no suite runs
them on their own.
