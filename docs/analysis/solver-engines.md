# Five engines on the signal solver's rules (2026-10-05)

<!-- audited: 2026-10-24 -->

How fast five Datalog engines solve the signal solver's rules over `lib/`, and whether their models agree.

**Verdict: all five engines compute the same model. datafrog solves in under
4 ms, the worklist, which adds no crate, in under 17 ms, and z3 in 1–2 s.**

[The signal solver](../impl/solver.md) states the rules, and which engine
solves and which checks.

## What is measured

The subject is all of `lib/` at `73a50c636`: 80 files, 115,000 facts. The
spike at that commit solves the same rules with all five engines. Each
in-process engine reports its fastest of five solves, over five runs on a
loaded machine.

| Engine | Solve | Crates it adds | Agrees |
|--------|-------|----------------|--------|
| datafrog | 2.2–3.9 ms | 1 | yes |
| ascent | 3.9–6.6 ms | 23 | yes |
| worklist | 6.7–16.6 ms | 0 | yes |
| crepe | 10–21.5 ms | 6 | yes |
| z3 | 1–2 s | an external binary | yes |

The analysis the engines read takes 323–374 ms, so each in-process engine
costs under 5% of it.

z3 ran the rules in its Datalog engine and used no SMT reasoning. It runs as
a separate process and reads the facts as text. crepe adds
`proc-macro-error`, which RustSec lists as unmaintained.

## The comparison catches a dropped rule

Removing the `owner` check from the datafrog rules made its model differ from
the others on `lib/` and on the `app.lisp` fixture.
