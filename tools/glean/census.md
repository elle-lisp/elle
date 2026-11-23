# The census

<!-- audited: 2026-11-23 -->

The census compares each spec fence's hand facts with glean's facts of its program, and counts each mismatch by its cause.

`make census` runs [census.lisp](census.lisp). It imports each document of
[the spec](../../docs/spec/AGENTS.md) in one process, as
[the specification](../../docs/spec.md) lays each one out, and skips a document
that holds no fence. It takes every fence whose `:source` is a string, compiles
that source, gleans the unit, and compares the hand facts with the facts of the
unit's function that bears the fence's name. A fence with no source is skipped.

| File | What it holds |
|------|---------------|
| [census.lisp](census.lisp) | The census, a program |
| [rows.lisp](rows.lisp) | A row: one fence's hand facts and gleaned facts, judged alike |
| [noise.lisp](noise.lisp) | The `:glean/noise` signal, and the mediator that hears it |
| [paths.lisp](paths.lisp) | The points from which every path to an exit passes an error emit |
| [causes.lisp](causes.lisp) | The causes that claim each name of a row |

## A row

One fixed judge runs over both fact sets: `released-all` and `refusals`, each
answer as the set of value names, points dropped. A glean symbol counts as the
keyword of the same spelling, and every other name is compared as it is. Each
mismatch is counted, not failed.

A row records the document and the fence's name. It holds the error that
stopped the fence, a compile, glean or judge error or no function of that name.
Otherwise it holds the marks per kind and each name in one answer and not the
other. `row` builds a row from a fence and the unit's glean answer, and
[glean-census](../../tests/lang/glean-census.lisp) tests it on hand-built
input.

## Causes

Most mismatches are not defects. A cause is a known difference between the hand
facts and glean's, and each mismatched name is claimed by every cause that
explains it:

| Cause | What the row shows |
|-------|--------------------|
| `:constructor` | `@[]`, `(array x)` and `(list …)` lower to a call of a native whose `:spec` result is `:fresh`. The hand facts write that value's `alloc` alone, and glean gives the call beside its `alloc` |
| `:hoisted` | A literal such as `(array 1 2)` arrives as a synthetic capture, so the function makes nothing |
| `:unruled` | A kind with no rule, or a call of a native with no `:spec`, gives no `store`, `read` or `remove` fact. `get`, `first`, `rest` and `length` declare nothing |
| `:error-path` | `push`, `put` and a compare inline into a type dispatch, whose error emit makes values the hand facts lack |
| `:grafted` | A function defined in the same source is inlined, so its call disappears, and the values its body makes are glean's alone |
| `:unnamed` | The hand facts name a value by a letter, and glean names it by its register or slot, so it counts in each answer alone |
| `:captures` | The judge runs no captures, as a fence's check does |
| `:closure-result` | The hand facts write the result of a closure call that makes it as an `alloc`, and glean gives the call alone |

The judge runs no `calls`, instance or crossing either. No cause claims what
those would change, so such a name stays unclaimed.

## Noise

Glean raises the user signal `:glean/noise` at each fact it gives where the hand
facts model the program otherwise. The payload is `{:cause c :id i :point p
:values [...]}`: the cause, the closure id of the function, the point, and the
glean names of the values the cause explains. Glean ignores the value the
resume answers. A caller of glean catches the signal, because one that does not
stops with an unhandled signal. `heard` is that mediator: it runs a thunk in a
fiber whose mask holds `:glean/noise`, keeps each payload, and resumes.

| Cause | Glean raises it at | Its values |
|-------|-------------------|------------|
| `:constructor` | each call of a native whose `:spec` result is `:fresh` | the result |
| `:hoisted` | the function, point nil, once per capture whose `:captures` entry is `:synthetic` | `[:capture i]` |
| `:unruled` | each instruction of a marked kind, and each call of a native with no `:spec` | the result and each register the instruction reads, those that name a value |
| `:error-path` | each `alloc` at an error-only point, and the LoadResumeValue that begins the resume block of an error Emit | the made value |

An error emit is an Emit whose signal holds `:error`, or a MatchFail. A point is
error-only when some path from it reaches an error emit and no path reaches an
exit without passing one. A MatchFail is an error emit, so its resume value is
made at an error-only point.

## Claims

A cause raised as noise claims a name when the name, spelled as a row spells it,
is one of the cause's values in the fence's function. It also claims each value
those values pass into, along glean's `pass` facts to a fixpoint, because a
hoisted literal reaches its binding by a pass. The census claims the rest
itself:

| Cause | The census claims |
|-------|-------------------|
| `:grafted` | the result and each argument of a hand `call` whose callee no glean `call` names. A glean callee `[:fn k]` names the closure's name. Also each value glean makes at a point whose `:at` line is a line of that callee's own function in the unit, and each value it passes into |
| `:unnamed` | a hand name no glean fact holds and no `:slots` or `:captures` entry of the unit binds. Also a glean `[:slot s]`, `[:reg r]` or `[:const p]` in a judgment where a hand name is unnamed |
| `:captures` | a name in one answer alone under the raw judge, and in neither alone under `judged` over `with-captures`, as [capture](../../docs/spec/capture.md) defines them |
| `:closure-result` | a hand `alloc(v, p)` whose `v` is the result of a glean `call` to a callee that is no native |

A grafted callee written on a line of the fence's own function lends that line
to both, so `:grafted` claims the values the function makes there too.

After the rows, the census prints for each cause the names it claims and the
names it alone claims, the names more than one cause claims, and then each name
no cause claims. An unclaimed name is a cause missing from this table or a
defect, and it is worth reading against the function's LIR.
