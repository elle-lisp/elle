# Modules: one form

<!-- audited: 2026-11-19 -->

The decided shape of a module: one form per file, evaluated by an Elle loader the importing fiber calls, and shipped as a compiled form.

[The proposal](../modules-proposal.md) defines the terms this document uses,
and [compile.md](compile.md) states the implemented parts the shape builds on.
[modules.md](../modules.md) states what the system does; this document says
why it has this shape, and what it rejected. [macros.md](macros.md) states
how a module exports a macro.

## A module is one form

A module file holds one form, after its `(elle/epoch N)` line, which the
reader consumes. A load evaluates the form where the import stands and answers
its value. The runtime adds nothing else: the loader is Elle code, and a loader
of the user's own may cache what it compiles.

No implicit letrec remains. A module with several definitions wraps them in
`letrec`, `let`, `fn` or a struct literal. `defmacro` works inside a body, and
so does `include-file`, which splices into the form that holds it
([include.md](../include.md)).

The form may build any value. Work it does outside a lambda runs once per
load. Effects and dependencies go inside a function the value holds, and a
call of it runs them. [lib/git.lisp](../../lib/git.lisp) has this shape: its
lambda imports `std/git/core`, and the core's lambda loads `libgit2.so`. A
module with no dependency and no effect may return its struct directly.

The rule binds what a loader loads. A program that `elle` runs keeps many
forms and may be impure, as the files under [tests/lang](../../tests/lang)
are.

Three consequences follow:

- **Identity.** Whatever the form builds belongs to the load that built it, a
  parameter, a trait table or a mutable table included. Two imports build two
  values. A program that wants one instance imports once and hands the value
  on, or installs a loader that caches.
- **Plugins.** `import/load-plugin` signals `:ffi`, so the form cannot load a
  plugin. A module loads one in the function it returns, or takes it as a
  parameter, as [modules.md](../modules.md) shows. No rule names plugins.
- **Raises.** A raise in the form fails the load with the error the form
  raised.

A module that returns a function and one that returns a struct look alike at
the import. So `get` on a closure says that a module which returns a function
is called before its fields are read.

All module code sits in a body, and a body's signals are solved jointly, as a
file's are ([compile.md](compile.md)).

## Loading is a call

`import` is a prelude macro. `(import spec)` expands to a call of the loader
`*loader*` holds, with the request `{:spec spec :dir dir}`, where `dir` is the
directory of the file that wrote the import. The call runs on the importing
fiber, where the import stands. So the loader sees the dynamic bindings, the
capabilities and the scheduler that the import sees, and its `file/read` parks
the importer as any I/O does.

`parameterize` replaces the loader for a dynamic extent, and a spawned fiber
starts with its spawner's bindings, so the loader is as local as any other
parameter. No signal, no handler and no scheduler arm takes part. An import
therefore works wherever a call works: in a bare fiber, in a process, in an
embedder's VM, and under a scheduler of the user's own.

A function that imports takes every bit, as any function that calls what a
parameter holds does. The loader is arbitrary code, and nothing the compiler
could read says less.

A sandbox closes imports by denying what a loader spends. A loader that reads
spends `:fs`, one that loads a plugin spends `:ffi`, and one that fetches
spends `:net`. A loader that spends nothing answers from memory, and refusing
that protects nobody. So no bit names importing, and a denial names the
primitive the loader called.

The loader is Elle code, and so is every loader but `import/load-plugin`,
which runs foreign code. `import/load-file` is `file/read`, then
`compile/read-forms`, then `eval`, whose closure a fiber under the deny set
below calls. A user can therefore build a module system of their own from the
same parts, over an object store, cached syntax, or an image read off the
network, and ours is the one installed first. The compiler keeps no capability
a user's loader lacks, and it reads no import: a literal path in a program is
a string like any other.

## How the form is held pure

The default loader compiles the form on the calling fiber, and runs it in a
fiber that withholds every effect capability and `:ambient`. A form that loads
a library, reads a file, prints or reads ambient state is denied, and the load
fails naming the primitive. An import at the top of a form is such a read, so
a load never starts another load, and no list of loads in progress exists
anywhere. The compile reads the form's includes as the caller may
([include.md](../include.md)). A loader of the user's own picks its own deny
set, in the one argument `fiber/new` takes for it.

The form sees the dynamic bindings where the import stands, because the fiber
that runs it starts with the bindings of the fiber that resumes it. A
parameter is an argument the caller binds and scopes. Ambient state is what no
argument carries ([capabilities.md](../signals/capabilities.md)). So the two
rules agree: the form reads what its caller handed it, and nothing else.

Nothing refuses mutable state in the value. A load answers its value to the
one importer that asked, and the runtime never shares it, so a mutable table
in a module's value is that importer's own. A loader that caches values does
share them, and the walk that refuses mutable state is a function such a
loader calls, `import/shareable?`. The walk reads a closure through
`fn/captures`, which answers each captured slot with its value and whether
code assigns the binding, from the record the compiler keeps on each
function's code ([modules.md](../modules.md)).

A cycle between two modules' functions is the program's own recursion, and
nothing names it as a cycle.

## A module's form does not suspend

The fiber that runs the form catches a yield, and the loader refuses the load,
because a load answers one value. A form under the deny set signals nothing
else but `:error`. The function a module's value holds is an ordinary closure,
and it may suspend when called directly and inside `ev/spawn`. So a module is
already asynchronous where its work runs.

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

## Rejected alternatives

**A file as an implicit letrec of any value.** This was the shape: the top
level ran on every import, and nothing it built could be shared. A rule a
reader cannot see decided the file's scope, and its value could not be cached.

**An implicit letrec that must return a lambda.** The lambda was where
impurity went. A form may hold a lambda already, so requiring one buys
nothing, and the implicit letrec stays a rule a reader cannot see.

**Hoisting.** The compiler would move pure bindings out of a module's lambda
to share them. One form lets the author write that sharing, and a pass the
program cannot see would decide what the author can write.

**A loader that loads a module's literal imports before its form runs.** The
form would then receive values instead of reading files. The same
`import/load-file` would mean one thing in a module and another in a program.

**A compile-time import.** A new form, like `include-file`, would read
another module and evaluate its form while the importer compiles. A compile
would then run code from other files, a special case no user could build. It
would also bring back compile-time recursion between files.

**A declaration of dependencies.** A module's value would name its imports as
data, and the loader would apply the module to their values. A function that
imports when called gives the same result without a protocol.

**Loading as a signal.** `import` emitted a bit with the request, and the
nearest fiber whose mask named the bit answered it: the scheduler, on a loader
fiber it spawned and joined the importer to. That fiber started with the
scheduler's bindings, so `*loader*` alone reached the loader and every other
parameter did not. An import needed a handler above it, which a process fiber,
a bare fiber and an embedder's VM lacked. And the bit named nothing a loader
spends that `:fs`, `:ffi` and `:net` do not name already. A handler fiber per
spawn, and the scheduler loading on its own fiber, were variants of it.

**The loader a fiber was spawned under.** The scheduler kept each fiber's
loader from its spawn, so a `parameterize` around an import did not reach
that import, unlike any other parameter. It also cost a record per fiber.

**A form that imports.** A loader could lift the denial for the one import a
form makes. It was rejected so that no load starts another, and no loader
keeps a list of loads in progress.

**Cycle tracking in the VM.** The VM kept the loads in progress and named a
cycle. Under the shape no load nests, so the list has no job. Marking each
activation of a module's function, so that a cycle of calls could be named,
was rejected too. It is a mark on every call, it conflicts with fibers, and it
is one more obstacle for the JIT and WASM tiers.

**The form compiled in the fiber that runs it.** A compile reads an include
under the capabilities of the fiber it runs for, so no module could include a
file.

**A value shared by a cache.** The loader walked the value and refused
mutable state, so that a cache could hand one value to every importer. The
runtime shares nothing, so the refusal protected nobody and refused a table
an author meant to keep.

**An image of the form's value.** The value depends on the dynamic bindings
where the import stands. An image of it would need a fiber with no inherited
bindings, and a rule that the form reads none. The image carries the compiled
form instead.

**A disk cache of compiled code beside each source file.** Key a file's
bytecode on its compile inputs, store it, and load it in the next process. The
[stdlib disk cache](../impl/stdlib-cache.md) works that way, and it is a
stopgap. A hit rebuilds every object through the send codec, which
[image.md](../impl/image.md) rejects as the CAS cache: an image maps a region and
decodes no value. A module disk cache would repeat the rejected design for
user code, so module images replace it.
