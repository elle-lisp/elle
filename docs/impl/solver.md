# The signal solver

<!-- audited: 2026-10-05 -->

Signal inference across files: facts per file, a link through literal imports, and the least model of a few Datalog rules.

The [module design](../modules-proposal.md) links literal imports, and a
linked call needs the callee's signal from another file. This document owns
how that signal is computed. The spike
[examples/signal_solve](../../examples/signal_solve/main.rs) builds it beside
the compiler, and nothing in the compiler uses it yet.

## What the analyzer cannot answer

The analyzer infers one file at a time ([signals/inference.md](../signals/inference.md)):

- A file's top level converges by a fixpoint, and a function body runs none
  ([pipeline.md](../pipeline.md)). A forward reference inside a body gets every
  bit, and a lambda that calls its enclosing definition reads the silent seed.
- A projection maps each exported field to a signal. It cannot say "field
  `:connect` of parameter 0", a parameter captured from an enclosing function,
  or a dependency on another module's export.
- A call through `module:field` never uses the projection (#1232).

A call to a field of a parameter, such as a plugin a module is given, has a
call expression for its callee, and the analyzer answers it with every bit.

## A signal is bits and free variables

The solver gives each function a signal of two parts: the bits it raises by
itself, and the free variables whose signal it includes. A free variable is
one of these:

| Variable | Stands for |
|----------|------------|
| a parameter | what the caller passes at that position |
| a captured parameter | a parameter of an enclosing function |
| a field of a parameter | `p:connect`, where `p` is a parameter |
| an import's export | `m:f`, where `m` is a literal import this run did not analyze |
| a module instance | the value an instantiation of such an import returns |

A call replaces the callee's free variables with what the call passes. A
function whose answer still names a free variable depends on its caller, as a
polymorphic function does today. The `propagates` mask of `Signal` is the
special case where every free variable is one of the function's own
parameters.

Two values carry fixed signals. A literal is data, so calling it can only
raise `:error`. A value nothing is known about raises every bit. One bit value,
`TOP`, stands for every user-facing bit at once, so the unknown value costs one
fact instead of fifty.

## Facts

[extract.rs](../../examples/signal_solve/extract.rs) walks each file's analyzed
HIR and states facts about it. The file is analyzed without compiling its
imports (`analyze_file_detached`, [pipeline.md](../pipeline.md)), so the walk
never enters another file.

- An `emit` adds its bits to the enclosing function. A `match` with no total
  arm adds `:error`.
- A primitive's bits and propagated parameters come from the compile
  metadata.
- A call states the context it runs in, the callee, the function whose
  parameters it binds, and each argument.
- A lambda's declared ceiling and muffle bits come from the analyzer
  (`LambdaDecl`), because the HIR keeps only the signal that results from
  them.
- A literal-mask `squelch` or `attune` becomes a variable that filters the
  bits of its target.

## Linking

[link.rs](../../examples/signal_solve/link.rs) joins the files of one run
through their literal imports.

- A module instance's fields are the exports of the lambda the file returns.
- A call of an import's lambda binds that lambda's parameters to the call's
  arguments.
- A call of an export binds the export's parameters.
- A field read through a parameter that a call passes on needs a variable on
  the argument too, and the link adds one until no call wants another.

An import this run did not analyze stays open. Each export read from it is a
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
bits(c, b)      :- raw(c, b), !muf(c, b), !ceiled(c).
bits(c, b)      :- ceil(c, b).
dep(c, w)       :- rdep(c, w), !ceiled(c).
bits(q, b)      :- sqof(q, f), bits(f, b), !sqm(q, b).
bits(q, :error) :- sqof(q, f), bits(f, b), sqm(q, b).
dep(q, w)       :- sqof(q, f), dep(f, w).
viol(c, b)      :- raw(c, b), ceiled(c), !ceil(c, b), !muf(c, b).
```

`c` is a context, `g` a callee, `o` the function whose parameters a call binds,
`w` a free variable, and `a` what the call binds it to. `sub` says the call
binds `w` to `a`, `miss` that it binds `w` to nothing known, and `flow` that an
open export may call `a`. A ceiling replaces what the body raises, and `viol`
names the bits a body raises past its ceiling.

The input facts also seed `raw`, `bits` and `dep`. Every negation names an
input relation, so the program is Datalog with negation on its inputs alone.
It has one least model, and every correct engine computes that model, whatever
the order of its facts. The model is the answer the design needs, because a
least fixpoint never claims a bit nothing raises.

## Two engines, and why these two

[fixpoint.rs](../../examples/signal_solve/fixpoint.rs) solves the rules with a
worklist: each context recomputes its bits and free variables from what it
reads, and a change queues its readers.
[crosscheck.rs](../../examples/signal_solve/crosscheck.rs) solves the same
rules with the `datafrog` crate, each rule written as a join over sorted
relations. Every run computes both models and fails when they differ. The two
share the input facts and nothing else, so a dropped or wrong rule in either
shows as a difference.

Five engines were measured at commit `73a50c636`, on all of `lib/`: 80 files,
115,000 facts. Each in-process engine reports its fastest of five solves, over
five runs on a loaded machine.

| Engine | Solve | Crates it adds | Agrees |
|--------|-------|----------------|--------|
| datafrog | 2.2–3.9 ms | 1 | yes |
| ascent | 3.9–6.6 ms | 23 | yes |
| worklist | 6.7–16.6 ms | 0 | yes |
| crepe | 10–21.5 ms | 6 | yes |
| z3 | 1–2 s | an external binary | yes |

The analysis the engines read takes 323–374 ms, so each in-process engine
costs under 5% of it. Removing the `owner` check from the datafrog rules made
its model differ from the others on `lib/` and on one fixture, so the
comparison catches a dropped rule.

- **The worklist solves.** It adds no dependency, and its cost is small beside
  the analysis.
- **datafrog checks.** It adds one crate, as a development dependency, and its
  joins are a second, independent reading of the rules.
- **z3** ran the rules in its Datalog engine and used no SMT reasoning. It is
  a process and a text format away, it costs a binary on every machine that
  runs the tests, and it was about 100 times slower than the worklist. A model
  from its SMT engine would not be unique; the rules need Datalog semantics.
  z3 earns a place only for a constraint a fixpoint cannot solve.
- **ascent** reads as the rules, line for line, and adds 23 crates.
- **crepe** was the slowest in-process engine, and it adds
  `proc-macro-error`, which RustSec lists as unmaintained.

## What the fixtures show

The fixtures in [examples/signal_solve/fixtures](../../examples/signal_solve/fixtures)
call only callees whose signals are exact: `yield`, `port/open`, literals, and
parameters. The standard library's signals are too coarse to judge precision
by; stdlib `map` alone carries nearly every bit. A fixture states its expected
answers in `# expect` lines, which `--expect` checks:

```text
# expect NAME |BITS| [prop I] [field I KEY] [import KEY] [instantiate]
```

`prop I` names an own parameter, `field I KEY` a field of parameter `I`,
`import KEY` an open export, and `instantiate` an open module instance. The
bits, parameters and free variables must match exactly.

A `# violates NAME |BITS|` line states the bits the body of `NAME` raises past
its ceiling. Under `--expect`, every violation the model finds in a fixture
must be stated, and every stated one found.

Where the analyzer answers with every bit, the solver is exact:

- a forward or mutual reference inside a function body
- a parameter called from a nested lambda
- a parametric module whose exports call fields of the plugin it is given:
  `go` solves to `port/open`'s `|:error :io :fs|`
- a higher-order export, which solves to `|:yield|` or `||` by its argument
- two modules that import each other inside function bodies, which solve
  jointly

`ceiling.lisp` holds a `(silence)` function the analyzer accepts. It calls a
function that passes its parameter to a callee which also yields by itself, and
the analyzer keeps the parameter and drops the callee's own `:yield`. The solver
keeps both, so it reports the ceiling exceeded by `:yield`.

`sep.lisp` runs with `--no-follow`, so its imports stay open. Each function
names the exports it calls, and a literal argument to an open export adds
`:error`.

## What the solver does not cover

- `(f g)` with `f` a parameter: both the analyzer and the solver drop `g`.
- A mutable binding, a spliced argument, and `eval`.
- A native plugin's signals at compile time.
- The purity facts the design needs beside signals.

## Running it

`make signal-solve` builds the spike, runs every fixture with `--expect`, and
runs it over all of `lib/`. It fails when an expectation fails or when the two
engines disagree. `make test` runs it, and so does the `Default Build Tests`
job ([ci.md](../analysis/ci.md)).

An import spec resolves against the working directory, so run a fixture from
its own directory:

```sh
cd examples/signal_solve/fixtures
../../../target/release/examples/signal_solve --expect app.lisp
../../../target/release/examples/signal_solve --expect --no-follow sep.lisp
```
