# Modules: the plan

<!-- audited: 2026-11-26 -->

The parts of the module design not yet built: loading as a call, no walk of the value, module images, linking, and the open decisions.

[principles.md](principles.md) defines the terms this document uses and the
rules it follows. [shape.md](shape.md) states the shape of a module that these
parts build on, and [macros.md](macros.md) states how a module will export a
macro. [modules.md](../modules.md) states what the system does today.

## Loading is a call

Today `import` emits `:import`, and the scheduler answers the request on a
fiber of its own ([modules.md](../modules.md)). The plan replaces the request
with a call. `(import spec)` expands to a call of the loader `*loader*` holds,
with the request `{:spec spec :dir dir}`, where `dir` is the directory of the
file that wrote the import. The call runs on the importing fiber, where the
import stands. So the loader sees the dynamic bindings, the capabilities and
the scheduler that the import sees, and its `file/read` parks the importer as
any I/O does.

`parameterize` replaces the loader for a dynamic extent, and a spawned fiber
starts with its spawner's bindings, so the loader is as local as any other
parameter. No signal, no handler and no scheduler arm takes part. An import
therefore works wherever a call works: in a bare fiber, in a process, in an
embedder's VM, and under a scheduler of the user's own.

The form then sees the dynamic bindings where the import stands, because the
fiber that runs it starts with the bindings of the fiber that resumes it. A
parameter is an argument the caller binds and scopes. Ambient state is what no
argument carries ([bits.md](../signals/bits.md)). So the two
rules agree: the form reads what its caller handed it, and nothing else.

A function that imports takes every bit, as any function that calls what a
parameter holds does. The loader is arbitrary code, and nothing the compiler
could read says less.

A sandbox closes imports by denying what a loader spends. A loader that reads
spends `:fs`, one that loads a plugin spends `:ffi`, and one that fetches
spends `:net`. A loader that spends nothing answers from memory, and refusing
that protects nobody. So no bit names importing, and a denial names the
primitive the loader called.

## No walk of the value

Today the default loader walks each value and refuses mutable state
([modules.md](../modules.md)). The plan drops the walk from the default
loader. A load answers its value to the one importer that asked, and the
runtime never shares it, so a mutable table in a module's value is that
importer's own. A loader that keeps values does share them, so the walk
becomes a function such a loader calls, `import/shareable?`.

## Images

A module image carries the compiled form: the closure over nothing that the
loader builds from the form before it runs it. The image is a configuration of
the image mechanism, layered over the boot image as an environment image is
([image.md](../impl/image.md)). Its digest is the compile-input table in
[compile.md](compile.md). A loader that holds an image hydrates the closure and
calls it as it would call a fresh compile, so every load still evaluates the
form where the import stands. The image is the compile cache a loader keeps,
and the two are one decision.

The rules images already state carry over:

- Loading a module image still requires `:fs`, so a sandboxed fiber sees the
  same denial as for a source import.
- A directory of module images is a code-execution surface. Trust an image as
  you would a `.so` ([image.md](../impl/image.md)).
- A closure that carries a process-declared signal bit refuses the dump until
  signals relocate by name.

A module image gives a user signal a stable identity in compiled code: its
name, relocated to the process's bit when the code loads. The image documents
disagree today: [image.md](../impl/image.md) replays a signal table at
hydration, and [sealing.md](../impl/image/sealing.md) says no image carries one
and refuses the dump. Relocation by name replaces both.

[image.md](../impl/image.md) describes the boot and environment configurations
only. Module images wait on the environment configuration
([plan.md](../impl/image/plan.md)).

## Linking

A call through `module:field` takes every bit today (#1232). Linking gives it
the signal of the closure the field holds, filled in when the calling closure
is made:

- A function's code carries its signal as bits and free variables: its own
  parameters, fields of them, and the captured bindings whose value the
  compiler cannot fix ([solver.md](../impl/solver.md)). The `propagates` mask in
  `Signal` is the special case where every free variable is one of the
  function's own parameters.
- A closure's captures fill the free variables when the closure is made. That
  substitution is the link. An import's answer is a value a binding holds, and
  nothing privileges it: a field of a captured struct resolves to the closure
  the field holds, however the struct was built.
- Two captures stay open after the closure is made, and each keeps every bit.
  A binding that code assigns holds a cell whose value changes. A `letrec`
  binding read before its initializer ran holds nothing yet, and
  [bindings.md](../bindings.md) owns the rule a read of it follows.
- A struct literal at the end of a module's value is what `compile/exports`,
  the semver surface and IDE completion read. No rule requires the literal.
- A cycle between function bodies is legal, and its files solve jointly. A
  cycle of imports at run time is the program's own recursion
  ([modules.md](../modules.md)).

## Open decisions

- **Signals as a type.** A signal is a process-wide bit today, so a user
  signal's identity depends on declaration order, and an image cannot carry
  one by name. Whether a signal becomes a value of its own type, and what a
  mask then holds, is undecided.
- **The WASM backend's imports** (#924): whether a WASM build reads an import
  ahead of time, which no other compile does.
- **Native libraries**: how a native library's init runs once per instance,
  and whether `ffi/on-unload` keeps one teardown for each library and symbol.
  [lib/git/core.lisp](../../lib/git/core.lisp) registers its teardown again on
  every instantiation.

## Rejected alternatives

**Loading as a signal.** This is the design today. `import` emits a bit with
the request, and the nearest fiber whose mask names the bit answers it: the
scheduler, on a loader fiber it spawns and joins the importer to. That fiber
starts with the scheduler's bindings, so the request carries `*loader*` to the
loader, and no other parameter reaches it. An import needs a handler above
it, which a process fiber, a bare fiber and an embedder's VM lack. And the bit
names nothing a loader spends that `:fs`, `:ffi` and `:net` do not name
already. A handler fiber per spawn, and the scheduler loading on its own fiber,
are variants of it.

**The loader a fiber was spawned under.** The scheduler kept each fiber's
loader from its spawn, so a `parameterize` around an import did not reach
that import, unlike any other parameter. It also cost a record per fiber.

**A value shared by a cache.** The default loader walks the value and refuses
mutable state, so that a cache could hand one value to every importer. The
default loader keeps compiled forms and shares no value, so the refusal
protects nobody and refuses a table an author meant to keep.

**An image of the form's value.** The value depends on the dynamic bindings
where the import stands. An image of it would need a fiber with no inherited
bindings, and a rule that the form reads none. The image carries the compiled
form instead.

**A disk cache of compiled code beside each source file.** Key a file's
bytecode on its compile inputs, store it, and load it in the next process. A
hit would rebuild every object through a codec, which
[image.md](../impl/image.md) rejects as the CAS cache: an image maps a region and
decodes no value. So module images replace it.
