# Modules: one pure form

<!-- audited: 2026-11-09 -->

The decided shape of a module: one pure form per file, loaded by a request that a loader written in Elle answers, and shipped as an image.

[The proposal](../modules-proposal.md) defines the terms this document uses,
and [compile.md](compile.md) states the implemented parts the shape builds on.
[modules.md](../modules.md) states what the system does; this document says
why it has this shape, and what it rejected.

## A module is one pure form

A module file holds one form, after its `(elle/epoch N)` line, which the
reader consumes. A load evaluates the form and answers its value. The runtime
adds nothing else: a loader may cache, and an image can carry the value.

No implicit letrec remains. A module with several definitions wraps them in
`letrec`, `let`, `fn` or a struct literal. `defmacro` works inside a body, and
so does `include`, which splices into the form that holds it
([include.md](../include.md)).

The form may build any value. Pure work it does outside a lambda runs once per
load, and a loader that caches shares the result. Effects and dependencies go
inside a function the value holds, and a call of it runs them.
[lib/git.lisp](../../lib/git.lisp) has this shape today: its lambda imports
`std/git/core`, and the core's lambda loads `libgit2.so`. A module with no
dependency and no effect may return its struct directly.

The rule binds what a loader loads. A program that `elle` runs keeps many
forms and may be impure, as the files under [tests/lang](../../tests/lang)
are.

Four consequences follow:

- **Identity.** Whatever the form builds belongs to the load, a parameter or a
  trait table included. A module that wants one per importer builds it in the
  function it returns, and a loader that caches shares what the form built.
- **Plugins.** `import/load-plugin` signals `:ffi`, so a pure form cannot load
  a plugin. A module loads one in the function it returns, or takes it as a
  parameter, as [modules.md](../modules.md) shows. No rule names plugins.
- **Raises.** A raise in the form is deterministic, so it fails every load of
  the file while the inputs match.
- **Migration.** Every top level outside a module's lambda moves into it: the
  files the [proposal](../modules-proposal.md) lists, and the fixtures under
  [tests/modules](../../tests/modules) that held several forms.
  [captures.rs](../../tests/integration/file_scope/captures.rs) imports a
  module that kept a counter at its top level twice, and checks that the two
  counters are independent. The counter moves into the function, and the
  assertion holds unchanged.

A module that returns a function and one that returns a struct look alike at
the import. So `get` on a closure says that a module which returns a function
is called before its fields are read.

All module code sits in a body, and a body's signals are solved jointly, as a
file's are ([compile.md](compile.md)).

## Loading is a request

`import` emits `:import`, bit 20, with a request, and the nearest fiber whose
mask names the bit answers it with the module's value. A fiber's mask is what
makes the handler ambient and hierarchical: the innermost handler wins, a
child fiber's request climbs to it, and `protect` works the same way. No
parameter is needed for the routing. One, `*loader*`, names the loader the
scheduler should run, and a spawned fiber inherits it in its baseline.

The request is a signal and not a call because the module's form runs in a
fiber that withholds `:fs`. A loader called through a parameter would run
inside that fiber and be denied too. The signal lets the request leave the
fiber and be answered on one that has `:fs`. The same route lets a sandbox
deny imports outright: a denied emit of `:import` is a `:capability-denied`
like any other.

The loader is Elle code, and so is every loader but `import/load-plugin`,
which runs foreign code. `import/load-file` is `file/read`, then
`compile/read-forms`, then `eval`, whose function a fiber under the module
deny set calls. A user
can therefore build a module system of their own from the same parts, over an
object store, cached syntax, or an image read off the network, and ours is the
one installed first. The compiler keeps no capability a user's loader lacks.
The literal edge the solver reads from `(import/load-file "path")` is a
tooling convenience and no rule of the language: a computed path loads with
the same meaning ([solver.md](../impl/solver.md)).

The scheduler answers a program's requests, and its fibers' masks name
`:import`. On a request from fiber F, it spawns a loader fiber running the
loader F was spawned under, parks F on it as a join, and resumes F with the
value. The loader's `file/read` parks like any fiber's I/O. A request costs a
fiber per import and nothing per spawn ([scheduler.md](../scheduler.md)).

## How the form is held pure

The loader compiles the form on the calling fiber, and runs it in a fiber that
withholds the module deny set: every effect capability, `:ambient`, and
`:import`. A form that loads a library, reads a file, prints, imports or reads
ambient state is denied, and the load fails naming the primitive, or the signal
for an emit. The compile reads the form's includes as the caller may
([include.md](../include.md)). Ambient state
is what `:ambient` names ([capabilities.md](../signals/capabilities.md)). A
user could write the same evaluation with `fiber/new` and `:deny`.

One part needs more than that fiber: no mutable state reaches the value. A
mutable container in the value, or a closure that assigns a binding the form
made, would be shared by every importer a cache serves. The loader walks the
value and refuses both. The walk is Elle code over `fn/captures`, which
answers each captured slot of a closure with its value and whether code
assigns the binding. The compiler records, on each function's code, every
binding the function captures, by name, and whether code assigns it.

The record sits on the code and not on the cell because the form compiles as
the body of a function. A cell made in a body carries no name and no mark, so
a walk that read cells would refuse the `letrec` over a table that
[modules.md](../modules.md) shows. The record counts every assign in the form,
so a closure over a binding the form assigns before the capture is refused
too. `lib/aws.lisp` assigns its top-level `@sigv4-mod` inside its lambda, so
the binding moves inside too.

The walk stops at a parameter, and never calls it or reads its default. A
call answers what the calling fiber binds, so it says nothing about the value.
So `{:say println}` loads: `println` captures `*stdout*`, and the walk ends
there. A mutable default is the author's choice, and every importer a cache
serves shares it, as it shares any parameter the form builds.

A raise is allowed: a raise is deterministic, and a cache tolerates it.

The form imports nothing, so a load never starts another load, and no list of
loads in progress exists anywhere. A cycle between two modules' functions is
the program's own recursion, and nothing names it as a cycle.

## A module's form does not suspend

The fiber that runs the form catches a yield, and the loader refuses the load,
because a load answers one value. A pure form signals nothing but `:error`, so
it never suspends. The function a module's value
holds is an ordinary closure, and it may suspend when called directly and
inside `ev/spawn`. So a module is already asynchronous where its work runs.

## Images

A module's value is pure, so it can be saved as an image and mapped at load
([image.md](../impl/image.md)). A module image is a configuration of the image
mechanism, layered over the boot image as an environment image is. Its digest
is the compile-input table in [compile.md](compile.md).

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
impurity went. A pure form may hold a lambda already, so requiring one buys
nothing, and the implicit letrec stays a rule a reader cannot see.

**Hoisting.** The compiler would move pure bindings out of a module's lambda
to share them. One pure form lets the author write that sharing, and a pass
the program cannot see would decide what the author can write.

**A loader that loads a module's literal imports before its form runs.** The
form would then receive values instead of reading files. The same
`import/load-file` would mean one thing in a module and another in a program.

**A compile-time import.** A new form, like `include`, would read another
module and evaluate its form while the importer compiles. A compile would
then run code from other files, a special case no user could build. It would
also bring back compile-time recursion between files.

**A declaration of dependencies.** A module's value would name its imports as
data, and the loader would apply the module to their values. A function that
imports when called gives the same result without a protocol.

**A form that imports, with the handler answering.** The handler could answer
a request the form raises, and the form would stay pure. It was rejected to
keep a load from starting another load, so that no loader needs a list of
loads in progress.

**Cycle tracking in the VM.** The VM kept the loads in progress and named a
cycle. Under the shape no load nests, so the list has no job. Marking each
activation of a module's function, so that a cycle of calls could be named,
was rejected too. It is a mark on every call, it conflicts with fibers, and it
is one more obstacle for the JIT and WASM tiers.

**The form compiled in the fiber that runs it.** A compile reads an include
under the capabilities of the fiber it runs for, so no module could include a
file.

**A loader reached through a parameter alone.** It runs inside the fiber that
imports, which is denied `:fs` when that fiber runs a module's form.

**A handler fiber per spawn.** `ev/spawn` would run each thunk inside a
`with-loader` fiber. That is one more fiber per spawn; a loader fiber per
import costs nothing per spawn.

**The scheduler loading on its own fiber.** Its `file/read` would then run
unparked, on the fiber that drives every other fiber's I/O.

**A disk cache of compiled code beside each source file.** Key a file's
bytecode on its compile inputs, store it, and load it in the next process. The
[stdlib disk cache](../impl/stdlib-cache.md) works that way, and it is a
stopgap. A hit rebuilds every object through the send codec, which
[image.md](../impl/image.md) rejects as the CAS cache: an image maps a region and
decodes no value. A module disk cache would repeat the rejected design for
user code, so module images replace it.
