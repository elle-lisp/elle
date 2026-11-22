# The signal solver

<!-- audited: 2026-11-21 -->

Signal inference as the least model of a few Datalog rules: how the compiler raises each unit's signals to it, and how a run links files.

The analyzer makes one pass over a unit, and the compiler then solves the
unit's signals with the rules below ([pipeline.md](../pipeline.md)). The
[module plan](../modules/plan.md) links literal imports, and a linked
call needs the callee's signal from another file. This document owns how both
are computed. [The solver](../../src/hir/solve/mod.rs) lives in the compiler,
and nothing in the compiler links across files yet.

## What one unit cannot answer

No signal crosses a file boundary, so a call into another file takes every bit
(#1232). A map from each exported field to a signal would not be enough: it
cannot say "field `:connect` of parameter 0", a parameter captured from an
enclosing function, or a dependency on another module's export.

A call to a field of a parameter, such as a plugin a module is given, has a
call expression for its callee, and the pass answers it with every bit. So
does a call to a parameter of an enclosing function. A signal only rises when
the compiler solves a unit, so the compiler keeps those answers, and nothing
reads the tighter ones the model holds.

## A signal is bits and free variables

The solver gives each function a signal of two parts: the bits it raises by
itself, and the free variables whose signal it includes. A free variable is
one of these:

| Variable | Stands for |
|----------|------------|
| a parameter | what the caller passes at that position |
| a captured parameter | a parameter of an enclosing function |
| a field of a parameter | `p:connect`, where `p` is a parameter |
| an import's export | `m:f`, where `m` is a literal import of a file with no shape |
| a module instance | the value an instantiation of such an import returns |

A call replaces the callee's free variables with what the call passes. A
function whose answer still names a free variable depends on its caller, as a
polymorphic function does today. The `propagates` mask of `Signal` is the
special case where every free variable is one of the function's own
parameters.

Two values carry fixed signals. A literal is data, so calling it can only
raise `:error`. A value nothing is known about raises every bit, and so does a
binding that an `assign` reaches. One bit value, `TOP`, stands for every
user-facing bit at once, so the unknown value costs one fact instead of fifty.

## Facts

[extract.rs](../../src/hir/solve/extract.rs) walks each unit's analyzed HIR
and states facts about it. A file's analysis never compiles its imports
([pipeline.md](../pipeline.md)), so the walk never enters another file.

- What a lambda raises by itself comes from the analyzer (`LambdaDecl`): its
  emits, a `match` with no total arm, a bound's entry check, and every
  construct that [inference.md](../signals/inference.md) lists as one that
  raises. The code outside every lambda takes the same bits from
  `AnalyzeResult`. The solver states none of these rules again, so it cannot
  drift from the analyzer on them.
- A binding the compile starts with, such as a primitive or an export of the
  standard library, carries the signal the analyzer recorded for it: its bits
  and the parameters it propagates.
- A parameter under a `bound` raises the bits the bound admits, whatever the
  caller passes.
- A call states the context it runs in, the callee, the function whose
  parameters it binds, and each argument. Each fact keeps the call it came
  from, so the model answers each call as well as each context.
- A lambda's declared ceiling comes from the analyzer (`LambdaDecl`), because
  the HIR keeps only the inferred signal.
- A literal-mask `squelch` or `attune` becomes a variable that filters the
  bits of its target. The mask is literal where the analyzer finds it literal.
  It holds the bits the boundary blocks, and never `:error` or `:halt`, which
  cross every boundary ([inference.md](../signals/inference.md)).

The walk keeps every refinement the pass makes. A collection literal and a
qualified name build calls the pass does not charge, and a call of `emit`
whose first argument is a literal raises the bits that argument names.

## Linking

[link.rs](../../src/hir/solve/link.rs) joins files through their literal
imports, by the shape it is given for each file: the exports of the lambda the
file returns. A literal import is `((import/load-file
"path"))` with a string path, which the solver resolves against the file that
holds it. An `import` is a macro over a resolver the program may replace, so
the solver treats `((import "std/x"))` as an ordinary call
([modules.md](../modules.md)). The edge is the solver's reading and no rule of
the language: a computed path loads with the same meaning.

- A module instance's fields are the exports of the lambda the file returns.
- A call of an import's lambda binds that lambda's parameters to the call's
  arguments.
- A call of an export binds the export's parameters.
- A field read through a parameter that a call passes on needs a variable on
  the argument too, and the link adds one until no call wants another.

An import of a file with no shape stays open. Each export read from it is a
free variable, which a later link binds, so the result is a summary of the
file. A call to an open export lets its arguments flow into the caller, because
the export may call them. That is sound and loose: a literal argument adds
`:error`, because calling a literal raises.

Lowering then joins the call facts over the parameter and field tables, so the
rules below read only joined relations. Every join there is over input facts,
so it runs once, before the solve.

## The rules

```text
raw(c, b)       :- call(c, g, _), bits(g, b).
rdep(c, w)      :- call(c, g, o), dep(g, w), !owner(w, o).
raw(c, b)       :- sub(c, g, w, a), dep(g, w), bits(a, b).
rdep(c, x)      :- sub(c, g, w, a), dep(g, w), dep(a, x).
raw(c, TOP)     :- miss(c, g, w), dep(g, w).
raw(c, b)       :- flow(c, a), bits(a, b).
rdep(c, x)      :- flow(c, a), dep(a, x).
bits(c, b)      :- raw(c, b).
dep(c, w)       :- rdep(c, w).
bits(q, b)      :- sqof(q, f), bits(f, b), b != TOP, !sqm(q, b).
bits(q, b)      :- sqof(q, f), bits(f, TOP), cap(b), !sqm(q, b).
bits(q, :error) :- sqof(q, f), bits(f, b), sqm(q, b).
bits(q, :error) :- sqof(q, f), bits(f, TOP), cap(b), sqm(q, b).
dep(q, w)       :- sqof(q, f), dep(f, w).
viol(c, b)      :- raw(c, b), ceiled(c), !ceil(c, b).
vdep(c, w)      :- rdep(c, w), ceiled(c).
```

`c` is a context, `g` a callee, `o` the function whose parameters a call binds,
`w` a free variable, and `a` what the call binds it to. `sub` says the call
binds `w` to `a`, `miss` that it binds `w` to nothing known, and `flow` that an
open export may call `a`. A call's own answer is `raw` and `rdep` over the
tuples that call contributes to `c`.

`cap` holds each user-facing bit, the bits `TOP` stands for. A mask never
holds `TOP`, so a squelch over a target that raises `TOP` raises, one by one,
each user-facing bit its mask does not hold, and `:error` when it holds one.

A ceiling checks and does not replace, as the analyzer's does: a reader of `c`
sees what the body raises. `viol` names the bits a body raises past its
ceiling. `vdep` names each free variable a ceiled body depends on. No ceiling
admits one, because the analyzer rejects a ceiled body that passes on the
signal of its parameters.

The input facts also seed `raw`, `bits` and `dep`. Every negation names an
input relation, so the program is Datalog with negation on its inputs alone.
It has one least model, and every correct engine computes that model, whatever
the order of its facts. The model is the answer the design needs, because a
least fixpoint never claims a bit nothing raises.

## In the compiler

`Analyzer::analyze` and `Analyzer::analyze_file` solve the unit they analyzed
before they return ([settle.rs](../../src/hir/analyze/settle.rs)). Every import
of the unit stays open, so the compile reads no other file, and a call into
one takes every bit, as above. The answers then raise the HIR, and never lower
it:

| Node | What it takes |
|------|---------------|
| A call | The bits the model answers for the call. A free variable that is a parameter of the enclosing lambda adds `:yield` and `:error`, as the pass's call to a parameter does. Any other free variable adds every bit |
| Any other node | The bits of each child but a lambda, because a lambda node is silent |
| A lambda | The model's bits for it. Each of its own parameters joins `:propagates`, and any other free variable adds every bit |

The ceilings, and the bounds a call meets, are checked against the raised
signals, with the messages [inference.md](../signals/inference.md) shows.

## The engine

[fixpoint.rs](../../src/hir/solve/fixpoint.rs) solves the rules with a
worklist: each context recomputes its bits and free variables from what it
reads, and a change queues its readers. It adds no dependency, and its cost is
small beside the analysis. datafrog, ascent and crepe each add crates, and z3
needs a binary on every machine and solves far slower.
[The engine measurements](../analysis/solver-engines.md) compare all five.

## What the solver does not cover

- `(f g)` with `f` a parameter: both the analyzer and the solver drop `g`.
- The position a spliced argument binds: from the first splice on, a call
  binds each parameter to nothing known.
- A native plugin's signals at compile time.
- The purity facts the design needs beside signals.
