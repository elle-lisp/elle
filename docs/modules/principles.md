# Modules: terms and principles

<!-- audited: 2026-11-21 -->

The words the module documents use, and the rules every part of the module system follows, built or planned.

[modules.md](../modules.md) states what the module system does.
[compile.md](compile.md) and [shape.md](shape.md) state why the built parts
have their shape, and [plan.md](plan.md) states the parts still to build.

## Terms

**A signal is a possible transfer of control.** Every signal, `:error`
included, is a possible raise. Signals describe control flow and nothing else.

**Silent** means the inferred signal is empty and the inference is sound.

**Pure** means silent apart from `:error`, reading no ambient state, and
changing no state that outlives the evaluation. Signals decide the first two
parts: `sys/env`, `sys/args`, `sys/pid`, `backend?` and every other ambient
read carry `:ambient` ([bits.md](../signals/bits.md)). Assigning a
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
language waits on its open decision, which [plan.md](plan.md) lists.
