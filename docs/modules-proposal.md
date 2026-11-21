# Modules: a proposal

<!-- audited: 2026-11-21 -->

Where modules are going: one form per file, loaded by Elle code a program may replace, linked when a closure is made, shipped as images.

[modules.md](modules.md) describes the module system as it is. This document
and the three beside it state the design it becomes, and the argument for each
part. Five parts are implemented, and [compile.md](modules-proposal/compile.md)
states them: sound signals, the raw primitives, contained expansion,
deterministic expansion and a unit's signals solved jointly. The shape a module
takes is decided: one form, evaluated by a loader the importing fiber calls,
which [shape.md](modules-proposal/shape.md) states with how the form is held
pure and what a module image carries. [macros.md](modules-proposal/macros.md)
states how a module exports a macro. [solver.md](impl/solver.md) owns the
signal solver, whose model of a signal is what linking fills in. The decisions
that still belong to the language's owner are listed under Open decisions
below.

## Where the module system stands

A defect below carries its issue number where it has one.
[modules.md](modules.md) states one more under its architectural constraints:
no signal crosses a file boundary, so a call into another file takes every bit
(#1232).

**A transformer still reads two things outside its unit.** Under `eval`, a
parameter it calls answers the caller's dynamic binding. A closure, a syntax
object and a parameter order by their address, so a transformer that sorts
them may expand differently in two processes (#1387). Every other read and
change outside the unit is refused ([macros.md](macros.md)). `eval` inside a
transformer fails because the macro VM has no compile context.

**A user signal's bit depends on the process.** A bit is assigned in
declaration order, process-wide: `:alpha` is bit 32 or 33 depending on which
declaration ran first. Compiled code carries the bit number.

**An import is a request a handler answers.** `import` emits a bit, and the
scheduler answers it on a fiber of its own, which starts with the scheduler's
bindings. [shape.md](modules-proposal/shape.md) replaces the request with a
call on the importing fiber.

**The standard library has the shape.** Every file in [lib/](../lib/) holds
one form.

## Terms

**A signal is a possible transfer of control.** Every signal, `:error`
included, is a possible raise. Signals describe control flow and nothing else.

**Silent** means the inferred signal is empty and the inference is sound.

**Pure** means silent apart from `:error`, reading no ambient state, and
changing no state that outlives the evaluation. Signals decide the first two
parts: `sys/env`, `sys/args`, `sys/pid`, `backend?` and every other ambient
read carry `:ambient` ([bits.md](signals/bits.md)). Assigning a
captured binding, reading a mutable binding, and mutating a shared container
are silent, so no signal decides the third.

**Deterministic** means the same inputs give the same output, a raise included.
A compile is deterministic in its key and reads files, so determinism is
weaker than purity. A cache needs determinism.

**Instantiation** is a call of the function a module's value holds. It may be
impure: it imports other modules, loads libraries and allocates state.

## Principles

**A raw primitive does one thing and assumes nothing.** `import/load-file`
and `include-file` load the file their argument names. `import` resolves a
spec, and the rules it follows are Elle code a program may replace.

**The compiler has no capability a user's loader lacks.** `(import "std/x")`
and a loader a program wrote give the same value from the same source, and
the loader may take that source from a filesystem, an object store, cached
syntax or an image. The runtime offers the parts: `compile/read-forms`,
`eval`, a fiber under a deny set, `fn/captures`, and the `*loader*` parameter
`import` reads. The compiler keeps nothing for itself: it reads no import, and
a literal path in a program is a string like any other.

**A check is enforced, not trusted.** Where a property can be checked at run
time, it is checked there too. Capabilities already work this way.

**A cache is invisible.** A program cannot tell whether a module came from a
cache, except by time, by the calls the VM counts, and by the address of a
constant. Every input that can change a compile is in the key, and every
observable effect of an import still happens on a hit.

**A change to what a program means waits on a decision of its own.** A change
that only removes a defect lands first. A change to module identity or to the
language waits on its open decision, below.

## Linking

A call through `module:field` takes every bit today (#1232). Linking gives it
the signal of the closure the field holds, filled in when the calling closure
is made:

- A function's code carries its signal as bits and free variables: its own
  parameters, fields of them, and the captured bindings whose value the
  compiler cannot fix ([solver.md](impl/solver.md)). The `propagates` mask in
  `Signal` is the special case where every free variable is one of the
  function's own parameters.
- A closure's captures fill the free variables when the closure is made. That
  substitution is the link. An import's answer is a value a binding holds, and
  nothing privileges it: a field of a captured struct resolves to the closure
  the field holds, however the struct was built.
- Two captures stay open after the closure is made, and each keeps every bit.
  A binding that code assigns holds a cell whose value changes. A `letrec`
  binding read before its initializer ran holds nothing yet, and
  [bindings.md](bindings.md) owns the rule a read of it follows.
- A struct literal at the end of a module's value is what `compile/exports`,
  the semver surface and IDE completion read. No rule requires the literal.
- A cycle between function bodies is legal, and its files solve jointly. A
  cycle of imports at run time is the program's own recursion
  ([shape.md](modules-proposal/shape.md)).

## Open decisions

- **Signals as a type.** A signal is a process-wide bit today, so a user
  signal's identity depends on declaration order, and an image cannot carry
  one by name. Whether a signal becomes a value of its own type, and what a
  mask then holds, is undecided.
- **The WASM backend's imports** (#924): whether a WASM build reads an import
  ahead of time, which no other compile does.
- **Native libraries**: how a native library's init runs once per instance,
  and whether `ffi/on-unload` keeps one teardown for each library and symbol.
  [lib/git/core.lisp](../lib/git/core.lisp) registers its teardown again on
  every instantiation.
