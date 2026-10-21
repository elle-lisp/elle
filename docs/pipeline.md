# Compilation pipeline

<!-- audited: 2026-10-21 -->

Which VM each entry point expands macros on, what a loader keeps a compile under, and how signal inference settles over a letrec.

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
calls `import/resolve` on the macro VM to find its file
([modules](modules.md)).

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

## Signal inference over a letrec

A file compiles as one letrec, and a local `letrec` form is the same shape. A
`def` or `var` whose lambda refers to itself is a letrec of one binding. So one
mechanism, `converge_signals` in [fixpoint.rs](../src/hir/analyze/fixpoint.rs),
infers the signals of mutually recursive definitions in all three. Its callers
are `analyze_file_letrec` in
[fileletrec/letrec.rs](../src/hir/analyze/fileletrec/letrec.rs) for a file,
[letrec.rs](../src/hir/analyze/letrec.rs) for the form, and
[binding.rs](../src/hir/analyze/binding.rs) for a `def` in a function body or
in a form compiled alone.

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
lambda inside `walk` calls `walk` while `walk` is still being analyzed. One
pass gives that lambda the seed's signal, silent, and its yield then breaks
the proof at run time.

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

1. **Pre-bind** every name, so each initializer resolves its siblings. A name
   bound to `(fn …)` syntax is seeded as silent, with the arity its parameter
   list gives. A `def` or `var` of a lambda seeds its own name the same way,
   just before its initializer.
2. **Analyze the initializers** in order. Each lambda records its
   `inferred_signals` and its arity as the call signal of its binding.
3. **Iterate** over the lambda bindings, at most 10 times. Re-analyze each one
   against the current signals, and record any signal that changed. Stop at the
   first iteration that changes nothing. A `def` iterates only when its lambda
   refers to itself and its signal left the seed. The lambda of any other `def`
   never read its own seed, so its first analysis stands.
4. **Analyze the body** only after the loop settles, so a call into the cycle
   reads the settled signals. For a `def`, the body is the forms after it. A
   file-letrec has no separate body: step 2 analyzes its expression entries in
   order, and the loop does not analyze them again.

For a file, step 3 re-analyzes against the scope as it stood after step 1. An
expression entry such as a `parameterize` body may define names in step 2, and
a lambda re-analyzed with those visible would take a wrong capture set.

Re-analysis only adds binding flags, such as the ones `mark_captured` sets, and
never clears one. An extra iteration can make the result more conservative,
never wrong. Re-analysis reports again every error the first analysis
reported. The loop keeps the errors of its last iteration alone, and drops each
one that step 2 already reported, so a body that fails to analyze reports each
error once.

### Why it ends

Signals form a lattice, and each iteration only moves a signal upward from its
silent seed. A signal that stops moving is at its least fixed point, so the
lattice height bounds the loop. The limit of 10 guards against a defect in the
lattice; the argument does not need it.

The seed is optimistic, so a loop cut short reports signals that are too low.
That is the unsafe direction: a function that can raise `:error` but is
inferred silent passes a `(silent!)` check at compile time, and raises at run
time instead. That raise leaves a silent function, so its boundary reports it
as the terminal `:broken-proof` error
([signals/inference.md](signals/inference.md)). The boundary is the backstop,
not the guarantee.

### Scope

The fixpoint is per file. No signal crosses a file boundary
([signal inference](signals/inference.md)), so a mutual recursion across one
does not settle.

A function body seeds a `def` only as the `def` is reached. A lambda that
calls a sibling defined later in the same body reads no seed for it. It takes
the unknown-callee signal instead, which is sound and may be wider than the
sibling's. Mutual recursion between siblings takes it at the call to the later
one. A call to an earlier sibling reads that sibling's signal:

```lisp
(defn outer-bits [src]
  (get (compile/signal (compile/analyze src) :outer) :bits))

(def later (outer-bits "(defn outer [] (defn f [] (g)) (defn g [] (yield 1)) (f))"))
(def earlier (outer-bits "(defn outer [] (defn g [] (yield 1)) (defn f [] (g)) (f))"))
(def mutual (outer-bits "(defn outer [] (defn f [n] (if (= n 0) 0 (g (- n 1)))) (defn g [n] (yield n) (f n)) (f 3))"))
(assert (has? later :io) "a call to a later sibling takes every bit")
(assert (has? mutual :io) "mutual recursion between siblings takes every bit")
(assert (= earlier |:yield|) "a call to an earlier sibling reads its signal")
```

The inferred signals are data a program can query: `compile/signal`, the
[portrait](analysis/portrait.md), and the [MCP server](mcp.md) read them.
