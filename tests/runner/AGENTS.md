# tests/runner — acceptance tests for `elle test`

Acceptance tests for the agent-first test runner specified in
[`docs/test-runner.md`](../../docs/test-runner.md). They drive `elle test` as a
subprocess and assert on the SQLite **session DB** it writes, reading it with
`lib/sqlite.lisp` — the same machinery the runner uses.

## Status: tests-first

The runner does not exist yet. These tests are written **before** the code (the
house process is docs → tests → code) and are **expected to fail** until:

- the `elle test` subcommand exists,
- `assert` is the macro that records `:syntax`/`:value`/`:message`,
- the compile-time gate `gate!` and predicate `backend?` exist.

Today the first failure is `elle test` being an unknown subcommand
(`test: No such file or directory`). That the harness fails for that reason is
what makes it a valid counter-factual.

## Why quarantined here (not tests/elle/)

`make smoke` globs `tests/elle/*.lisp` and must stay green. These tests fail by
design until the runner lands, and they spawn subprocesses, so they live here and
are run explicitly:

```sh
ELLE=./target/debug/elle ./target/debug/elle tests/runner/acceptance.lisp
```

When the runner is green, wire `acceptance.lisp` into CI as the runner's gate.

## Layout

| Path | Role |
|------|------|
| `acceptance.lisp` | driver: runs `elle test`, queries the DB, asserts rows |
| `fixtures/pass.lisp` | a form that passes on every tier |
| `fixtures/fail.lisp` | a failing assert — exercises the macro payload (`:syntax`, label, expected/actual) |
| `fixtures/gated.lisp` | `gate!`-d to JIT — `skip` on vm, `pass` on jit |
| `fixtures/diverge.lisp` | tier-dependent return value — recorded as `diverge` |

Each assertion in `acceptance.lisp` is **contract-defining**: exact statuses,
signal spelling, and predicate rendering are the spec the implementation must
satisfy. Change them only by changing the design doc first.
