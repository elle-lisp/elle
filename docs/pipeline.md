# Compilation pipeline

<!-- audited: 2026-10-23 -->

Which VM each entry point expands macros on, what a loader keeps a compile under, and how signal inference settles over a unit.

[The pipeline index](../src/pipeline/AGENTS.md) lists the entry points, the
compile context and the stages in order. [The HIR index](../src/hir/AGENTS.md)
owns the passes `regularize` runs.

## Which VM expands

Macro expansion runs Elle code, so it needs a VM. The entry points split on
whose VM that is, and a macro's side effects land on that VM.

**The compile context's macro VM.** `compile`, the file front end behind
`compile_file`, and `compile_file_to_lir` expand inside
`CompileCtx::with_macro_expansion`. It resets the macro VM's fiber and hands
the closure a clone of the expander and of the compile metadata. These entry
points need no VM from the caller, and a macro's side effects stay on the
macro VM. A primitive that compiles for a fiber, such as a loader, names that
fiber's withheld capabilities with `CompileCtx::on_behalf_of`, and the macro
VM's fresh fiber withholds them too.

**The caller's VM.** `eval`, `analyze` and `analyze_file` expand on the VM the
caller passes, with the clones `CompileCtx::expander_and_meta` answers. `eval`
then runs the bytecode on that VM as well, so a macro's side effects and the
program's share one VM.

**Compile on one, run on the other.** `eval_file` and `eval_all` compile
through `compile_file` on the macro VM, then run the bytecode on the caller's
VM. A macro's side effects therefore do not reach the caller's VM.

**The expander mid expansion.** `eval_syntax` compiles and runs a macro
transformer or a `begin-for-syntax` value. It takes the caller's expander as
well as the caller's VM, because it runs inside an expansion: a macro call in
the tree it compiles expands in the same context. Its compile metadata rides
on the expander (`eval_meta`), so it needs no borrow of the compile context
while the context is mid expansion.

Each clone is independent, so a compile nested inside an expansion, such as a
`begin-for-syntax` that imports a file, does not alias the compile context.

## The REPL layer

A REPL line compiles through `compile_file_repl`. Its clone also carries the
macros earlier lines defined, and its analysis the definitions they made. No
other entry point sees either, so a file a line imports compiles as it would
from any program. A macro one compile defines reaches a later compile only
when the REPL registers it with `register_repl_macros`.

## Units

Each clone has begun the unit its compile expands, so every scope the clone
mints names that unit ([the syntax tree](impl/syntax.md)). The VM's `eval`
instruction keeps one expander across calls instead, and begins a unit for each
datum. That drops the `begin-for-syntax` definitions the last datum made.
`eval_syntax` compiles a transformer in a unit of its own, and the caller's
unit resumes after.

Every transformer call runs through `VM::call_transformer`, and every
`begin-for-syntax` definition through `eval_syntax`. Both run the body under
the expansion fuel budget ([runtime signals](runtime.md)), and both turn a park
the body raises into a compile error, because an expansion cannot hold one.

## No compile reads another file

No entry point compiles a file other than the one it is given. The analyzer
reads nothing from the target of an `import-file`, so a cycle of imports does
not recurse. A file names its imports, and the loader compiles each one, or
runs the code the store keeps, when the import runs.

A compile may still run Elle code from the standard library. An `include`
calls `import/resolve` on the VM that expands it to find its file
([modules](modules.md)). An include reads a file and compiles none: its forms
become part of the unit that holds it ([include](include.md)).

## The store

`compile_file_stored` answers the `CodeUnit` the store keeps for a path while
the compile inputs still match, and otherwise compiles the file and keeps the
unit. An entry holds a digest of the inputs that can change within an
instance: the source text, the path, the capabilities `on_behalf_of` named,
and each include's resolved path and text. A lookup resolves each include again
on the macro VM, as the compile did, and reads it again. A compile that fails
keeps nothing. Each load runs a clone of the kept unit, so every load runs the
same code and builds values of its own ([modules](modules.md)).

Registering a host binding, a host primitive or the standard library's exports
empties the store, because every compile resolves those names. No other
compile input needs a place in the key:

- The epoch is in the source text. The build, the Unicode generation and the
  configuration are fixed when the instance is built.
- A loader's compile reads the instance's layer alone: no REPL layer, no
  `eval` environment and no test transform.
- A declared user signal keeps its bit for the life of the process, and a
  compile that names an undeclared one fails.
- A file's compile reads none of its imports, as the section above says.
- A transformer is pure, so it reads nothing outside these inputs
  ([macros](macros.md)).

`CompileCtx::release` empties the store before teardown counts the residue.

## Signal inference over a unit

A file compiles as one letrec, a local `letrec` form is the same shape, and a
function body is an implicit letrec ([bindings](bindings.md)). A definition may
call one defined after it, or call itself. The analyzer makes one pass over a
unit, and the signal solver then settles every signal in it at once
([the solver](impl/solver.md)).

### The problem

A single pass in order reads a sibling defined later before it is analyzed.
Below, the pass analyzes `foo` first, and `foo` reads `bar`'s seed. `foo` would
then carry whatever that seed said, not what `bar` turned out to be. The
algorithm below gives `foo` the yield `bar` settles on:

```lisp
(def pair (compile/analyze "(def foo (fn [] (bar)))
                            (def bar (fn [] (yield 1) (foo)))"
                           {:file "pair.lisp"}))
(assert (get (compile/signal pair :bar) :yields) "bar yields")
(assert (get (compile/signal pair :foo) :yields) "foo, analyzed first, yields too")
```

A function that refers to itself reads its own seed the same way. Below, the
lambda inside `walk` calls `walk` while `walk` is still being analyzed. The
pass gives that lambda the seed's signal, silent, and its yield would then
break the proof at run time.

```lisp
(defn outer []
  (defn walk [n]
    (if (= n 0)
      (yield :leaf)
      (list :back ((fn [k] (walk k)) (- n 1)))))
  walk)

(def walker (fiber/new (fn [] ((outer) 3)) |:yield|))
(assert (= :leaf (fiber/resume walker)) "the yield reaches the resume")
(fiber/resume walker)
(assert (= '(:back (:back (:back nil))) (fiber/value walker)))
```

### The algorithm

1. **Seed** every name before any initializer is analyzed. A file, a `letrec`
   and a function body each bind every name they define first. A name bound to
   `(fn …)` syntax is seeded as silent, with the arity its parameter list
   gives and whether it collects keywords. A `def` or `var` of a lambda
   anywhere else seeds its own name just before its initializer.
2. **Analyze once**, in order. A call reads its callee's signal as it stands,
   which is the seed for a callee the pass has not reached. The call checks
   its argument count against the seeded arity.
3. **Solve.** The entry point, `Analyzer::analyze` for a form and
   `Analyzer::analyze_file` for a file, states facts about the HIR the pass
   built and computes their least model.
4. **Raise.** Each call takes the bits the model answers for it, and each node
   takes its children's bits, up to the enclosing lambda. Each lambda takes
   the signal the model answers for it. A signal only rises, so nothing the
   pass inferred is lost.
5. **Check** each ceiling, and each bound at a call, against the final
   signals. A seed is optimistic, so a check during the pass would accept a
   body that raises. The messages are the ones
   [signals/inference.md](signals/inference.md) shows.

### Why it ends

The pass reads each form once. The solver only adds a bit or a free variable
to an answer, and a unit's facts name finitely many of each, so it stops.
Nothing caps either step.

The model is the least one the solver's rules allow, and each rule is sound,
so a final signal is never lower than what the code can raise. A defect that
makes one too low still meets the boundary of a silent function, which reports
it as the terminal `:broken-proof` error
([signals/inference.md](signals/inference.md)). The boundary is the backstop,
not the guarantee.

### Scope

The solve is per unit: a file, or a form compiled alone. No signal crosses a
file boundary ([signal inference](signals/inference.md)), so a mutual
recursion across one does not settle.

Within a unit, the order of the definitions changes no signal. A call to a
sibling defined later in a function body reads that sibling's signal, as a
call to an earlier one does, and mutual recursion between siblings settles:

```lisp
(defn outer-bits [src]
  (get (compile/signal (compile/analyze src) :outer) :bits))

(def later (outer-bits "(defn outer [] (defn f [] (g)) (defn g [] (yield 1)) (f))"))
(def earlier (outer-bits "(defn outer [] (defn g [] (yield 1)) (defn f [] (g)) (f))"))
(def mutual (outer-bits "(defn outer [] (defn f [n] (if n (g false) 0)) (defn g [n] (yield n) (f n)) (f true))"))
(assert (= later |:yield|) "a call to a later sibling reads its signal")
(assert (= earlier |:yield|) "so does a call to an earlier one")
(assert (= mutual |:yield|) "mutual recursion between siblings settles")
```

The seeded arity checks a call to a later sibling, so a call with the wrong
count does not compile:

```lisp
(def [ok? err] (protect (eval '(defn outer [] (defn f [] (g 1)) (defn g [] 0) (f)))))
(assert (not ok?) "the call to g passes one argument too many")
(assert (string/contains? (get err :message) "arity error: g expects 0 arguments, got 1"))
```

A ceiling is checked after the solve, so a lambda inside any form meets it with
its final signal. Below, the lambda in `k` reaches `g`'s yield through `f`,
and the pass analyzes it before it reaches `g`:

```lisp
(def src "(def f (fn [] (g)))
(def k (fiber/new (fn [] (silent!) (f)) |:yield|))
(def g (fn [] (yield 1) 2))")
(def [ok? err] (protect (compile/whole-module src "<doc>")))
(assert (not ok?) "the silent lambda reaches a yield")
(assert (string/contains? (get err :message) "body may emit {:yield}"))
```

A call through a binding that an `assign` reaches takes every bit, wherever the
call stands, because the solver cannot know which value the call finds
([the solver](impl/solver.md)):

```lisp
(def [ok? err]
  (protect (eval '(fn []
                    (var h (fn [] 1))
                    (defn f [] (silent!) (h))
                    (assign h (fn [] (yield 1)))
                    f))))
(assert (not ok?) "f may call the lambda that yields")
(assert (string/contains? (get err :message) "function restricted to {} but body may emit"))
```

The inferred signals are data a program can query: `compile/signal`, the
[portrait](analysis/portrait.md), and the [MCP server](mcp.md) read them.
