# Modules: a proposal

<!-- audited: 2026-10-23 -->

The module system Elle is building toward: one pure form per file, evaluated once per instance, linked across files, shipped as images.

[modules.md](modules.md) describes the module system as it is. This document
and the two beside it state the design it becomes, and the argument for each
part. Six parts are implemented, and [compile.md](modules-proposal/compile.md)
states them: sound signals, the raw primitives, contained expansion,
deterministic expansion, one compile per file and a unit's signals solved
jointly. The shape a module takes is decided: one pure form, which
[shape.md](modules-proposal/shape.md) states with how the form is held pure,
why it does not suspend, and module images. [solver.md](impl/solver.md) owns
the signal solver, which the compiler runs on each unit and linking will run
across files. The decisions that still belong to the language's owner are
listed under Open decisions below.

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

**The standard library is close to the shape.** 85 of the 86 files in
[lib/](../lib/) end in a `(fn …)` that builds the export struct; `lua.lisp` is
the exception, and it does not compile (#1217). What sits outside that lambda
is what moves into it under [the shape](modules-proposal/shape.md):

| File | Top level outside the lambda |
|------|------------------------------|
| [lib/zmq.lisp](../lib/zmq.lisp) | `ffi/native` loads `libzmq.so` |
| [lib/aws.lisp](../lib/aws.lisp) | `sys/env` reads credentials and region; `@sigv4-mod` is mutable, and the lambda assigns it |
| [lib/dns.lisp](../lib/dns.lisp) | an imported module called at top level; `@next-txid` is a counter every caller shares |
| [lib/redis.lisp](../lib/redis.lisp) | two parameters, and an imported module called at top level |
| [lib/process.lisp](../lib/process.lisp), [lib/telemetry.lisp](../lib/telemetry.lisp) | imported modules called at top level |
| [lib/process/scheduler.lisp](../lib/process/scheduler.lisp) | imports at top level |

## Terms

**A signal is a possible transfer of control.** Every signal, `:error`
included, is a possible raise. Signals describe control flow and nothing else.

**Silent** means the inferred signal is empty and the inference is sound.

**Pure** means silent apart from `:error`, reading no ambient state, and
changing no state that outlives the evaluation. Signals decide the first two
parts: `sys/env`, `sys/args`, `sys/pid`, `backend?` and every other ambient
read carry `:ambient` ([capabilities.md](signals/capabilities.md)). Assigning a
captured binding, reading a mutable binding, and mutating a shared container
are silent, so no signal decides the third.

**Deterministic** means the same inputs give the same output, a raise included.
A compile is deterministic in its key and reads files, so determinism is
weaker than purity. A cache needs determinism.

**Instantiation** is a call of the function a module's value holds. It may be
impure: it imports other modules, loads libraries and allocates state.

## Principles

**A raw primitive does one thing and assumes nothing.** `import-file` and
`include-file` load the file their argument names. `import` and `include`
resolve a spec, and the rules they follow are Elle code a program may replace.

**A user could build it.** The runtime compiles a file, evaluates a form under
a capability mask, and keeps caches a program cannot see. Resolution, `import`
and the wiring of dependencies are Elle code over those, so a user reasons
about them as about any other code.

**A check is enforced, not trusted.** Where a property can be checked at run
time, it is checked there too. Capabilities already work this way.

**A cache is invisible.** A program cannot tell whether a module came from a
cache, except by time. Every input that can change a compile is in the key,
and every observable effect of an import still happens on a hit.

**A change to what a program means waits on a decision of its own.** A change
that only removes a defect lands first. A change to module identity or to the
language waits on its open decision, below.

## Linking

A call through `module:field` takes every bit today (#1232). Linking gives it
the signal solved in the module's file:

- Signals cross files as facts, not as a per-field summary. Each file states
  its signals over free variables: its own parameters, captured parameters,
  fields of parameters, and the exports of its imports. Linking joins the files
  through their literal imports and solves the whole graph at once
  ([solver.md](impl/solver.md)).
- A map from field to signal cannot hold the answer: it has no way to say
  "field `:connect` of parameter 0". The `propagates` mask in `Signal` becomes
  the special case where every free variable is one of the function's own
  parameters.
- A struct literal at the end of a module's value is what linking reads
  precisely, and `compile/exports`, the semver surface and IDE completion read
  it the same way. A module whose value is built another way still works, and
  a call through it takes every bit. No rule requires the literal.
- A cycle between function bodies is legal, and its files solve jointly. A
  cycle of imports during instantiation reaches the loader, which names it.
- A closure that captures no instantiation argument can be inlined into its
  importer, as stdlib bodies already are.
- The WASM backend links an imported module in ahead of time, instead of
  running it on the host VM (#924).

Under [the shape](modules-proposal/shape.md), an import runs inside the
function a module returns, and `import` computes its path when it runs. So the
compiler sees a literal spec, and the file that spec names is known only at run
time. When linking joins the files is the open decision "linking through
import".

## Open decisions

- **Linking through import**: how a call through an imported module gets its
  solved signal, now that an import runs at instantiation and `import`
  computes its path when it runs.
- **Exported macros**: a macro exported as a value would join its importer's
  macro environment, so the importer's compile would evaluate another module.
  That is the compile-time import [shape.md](modules-proposal/shape.md)
  rejects. `include` stays the way to share macros, and [warts.md](warts.md)
  keeps its entry.
- **Native libraries**: how a native library's init runs once per instance,
  and whether `ffi/on-unload` keeps one teardown for each library and symbol.
  [lib/git/core.lisp](../lib/git/core.lisp) registers its teardown again on
  every instantiation.
