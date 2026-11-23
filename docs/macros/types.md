# Macros: types and signals

<!-- audited: 2026-11-23 -->

A macro reads no inferred type or signal: it places each argument once, writes the question as a dispatch, and the fold answers.

[overview.md](overview.md) states the rule this document applies.

## A macro reads no inferred fact

A type and a signal are least fixpoints over the expanded unit
([typeinfer.md](../impl/typeinfer.md), [solver.md](../impl/solver.md)). An
expansion that read one would change the program the fixpoint runs over. An
estimate read part-way is worse: it sits below the fixpoint, where it proves
what is false.

A macro may still check a fact that settles after it. It emits `silent!`,
`attune!` or a `%`-intrinsic, and the compiler rejects the unit when the fact
does not hold ([compile-time.md](../compile-time.md)).

## The fold answers a dispatch

A macro that needs a type writes the question into the program as a `type-of`
dispatch. The graft puts a small dispatching function in place of its call,
and the fold deletes each arm the proven types rule out
([graft.md](../impl/graft.md)). A proven type leaves one arm and no test.

## A template places each argument once

A dispatch whose every arm holds the body multiplies the body by the number of
arms, at every level of nesting. Each pass before the fold pays for every
copy, and every copy feeds the type ascent before the fold deletes it
([the report](duplication.md)). So a template places each argument once, and
the dispatch goes around the small parts that differ.

A loop keeps its body in one `while`, and reads the sequence through four
functions that each dispatch on its type: one to start, one to test for more,
one to read the element and one to advance. `each` takes this shape. An
unproven type then costs a dispatch for each turn of the loop, where one
dispatch for the loop was the price before.

Arms that compute one value for a shared tail leave through a `break`, and the
tail follows the block:

```lisp
(defn pick [v]
  (block :done
    (let [x (block :body
              (match (type-of v)
                :integer (break :body (+ v 1))
                :string (break :body (length v))
                _ (break :done :other)))]
      (* x 2))))
(assert (= [4 6 :other] [(pick 1) (pick "abc") (pick :k)]))
```

A lint reports a template that places one argument in two positions that run.
An argument inside a `quote` does not run, so `assert` may quote its predicate
beside the one place that evaluates it.

## Declared types

A literal's kind, a primitive's declared result and a declared parameter are
known where a form expands. Each is an upper bound on the type, and a bound
from above proves nothing false. This design gives a macro no way to read one,
because no macro measured so far needs it ([plan.md](plan.md)).

## Rejected alternatives

**A macro that reads the estimate while inference runs.** The expansion would
depend on an estimate below the fixpoint, and the fixpoint on the expansion.

**A second transformer after inference.** It would rewrite analyzed code and
would have to prove that it only narrows what it replaces. The fold already
derives the narrow form from the general one, with no proof left to an author.

**An arm for each type around the whole body.** `each` has this shape today.
