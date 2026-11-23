# The runner's acceptance tests

<!-- audited: 2026-11-23 -->

Elle programs that drive `elle test` and `elle` as subprocesses, and assert on
what each run leaves behind.

Together they are the runner's contract, scenario by scenario
([test-runner](../../docs/test-runner.md)), with the process-level behaviors
around it: the store a run writes ([test-store](../../docs/test-store.md)),
the flags it takes ([test-cli](../../docs/test-cli.md)), and the Makefile
recipes that hand it a suite. Each assertion's exact text is part of that
contract, so change the design document before an assertion.

A test that starts `elle test`, or reads a store one left, belongs here. Write
it in Rust only when it needs a Rust type
([testing](../../docs/analysis/testing.md)).

Each file opens with a call-out that names its subject. The implementation
suite runs every `.lisp` file at the top of this directory on the rig (`make
smoke-impl`), so a new file joins the pass with no list to edit. Each file
spawns the `elle` beside the executable that runs it, or the `elle-rig` there
for a scenario that needs a rig primitive ([rig](../../rig/overview.md)). It
reads each DB through [sqlite.lisp](../../lib/sqlite.lisp). Every scenario
writes under one scratch directory, so a run leaves nothing behind.

Each file splices the helpers it needs with `include-file`
([include](../../docs/include.md)):

- [lib/harness.lisp](lib/harness.lisp) starts a run, writes a fixture, and
  reads a DB back.
- [lib/make.lisp](lib/make.lisp) asks `make` what a variable expands to and
  what a target will run.

The files under `fixtures/` are inputs that several scenarios share, and no
suite runs them on their own. A scenario whose input names a scratch path
writes that input into its scratch directory.

Run one file by hand on the release rig. Each scenario starts the runner at
least once, and a debug start takes about twenty times as long:

```sh
./target/release/elle-rig tests/runner/tiers.lisp
```
