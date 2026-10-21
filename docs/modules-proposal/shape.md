# Modules: one pure form

<!-- audited: 2026-10-21 -->

The decided shape of a module: one pure form per file, evaluated once per instance, held pure by a full deny, and shipped as an image.

[The proposal](../modules-proposal.md) defines the terms this document uses,
and [compile.md](compile.md) states the implemented parts the shape builds on.

## A module is one pure form

A module file holds one form, after its `(elle/epoch N)` line, which the
reader consumes. `import/load-file` evaluates the form once per instance and
keeps its value. Every later load of the file returns that value while its
compile inputs match, and an image can carry it. The runtime adds nothing
else: it caches and hydrates.

No implicit letrec remains. A module with several definitions wraps them in
`letrec`, `let`, `fn` or a struct literal. `defmacro` already works inside a
body. `include` splices only at a file's top level today, so it must splice
into the body that holds it.

The form may build any value. Pure work it does outside a lambda runs once per
instance, and every importer shares the result. Effects and dependencies go
inside a function the value holds, and instantiation runs them:

```lisp
# lib/x.lisp, as one form:
# (letrec [table (build-table)]            # pure: built once per instance
#   (fn [&named level]                     # instantiation: may be impure
#     (def b64 ((import "std/base64")))    # a dependency, imported when called
#     {:encode (fn [s] (b64:encode (lookup table s level)))}))
```

[lib/git.lisp](../../lib/git.lisp) has this shape today: its lambda imports
`std/git/core`, and the core's lambda loads `libgit2.so`. A module with no
dependency and no effect may return its struct directly.

The rule binds what `import/load-file` loads. A program that `elle` runs keeps
many forms and may be impure, as the files under [tests/lang](../../tests/lang)
are.

Four consequences follow:

- **Identity.** Whatever the form builds exists once per instance, a parameter
  or a trait table included. A module that wants one per importer builds it in
  the function it returns.
- **Plugins.** `import/load-plugin` signals `:ffi`, so a pure form cannot load
  a plugin. A module loads one in the function it returns, or takes it as a
  parameter, as [modules.md](../modules.md) shows. No rule names plugins.
- **Raises.** A raise in the form is deterministic, so it fails every load of
  the file while the inputs match.
- **Migration.** The top levels the [proposal](../modules-proposal.md) lists
  move into their functions.
  [captures.rs](../../tests/integration/file_scope/captures.rs) imports a module
  that keeps a counter at its top level twice, and checks that the two
  counters are independent. That fixture moves its counter into a function,
  and the assertion holds unchanged.

A module that returns a function and one that returns a struct look alike at
the import. `(def b (import "std/base64"))` followed by `(b:encode "x")`
reports "get: expected collection …, got closure" when the module returns a
function. That case needs an error of its own.

All module code moves into a body, which infers less than a file
([the proposal](../modules-proposal.md)). The solver solves a body's
definitions jointly, forward and mutual references included
([solver.md](../impl/solver.md)), so it lands before the shape.

## How the form is held pure

`import/load-file` evaluates the form in a fiber that withholds every
capability. A form that loads a library, reads a file or prints is denied, and
the load fails naming the primitive. A user could write the same evaluation
with `fiber/new` and `:deny`.

Two parts need more than that fiber:

- **Ambient reads gain a capability bit.** `sys/env`, `sys/args`, `sys/pid`,
  the clocks, `backend?` and the other primitives an expansion refuses carry a
  mark today, and no bit. Each gains one, so the fiber's mask denies them, and
  `:deny` withholds them from any fiber.
- **No mutable state reaches the value.** A mutable container in the value, or
  a closure that assigns a binding the form made, would be shared by every
  importer. The loader walks the value and refuses both. The walk enforces at
  run time what an analysis may report earlier. `lib/aws.lisp` assigns its
  top-level `@sigv4-mod` inside its lambda, so the binding moves inside too.

A raise is allowed: a raise is deterministic, and a cache tolerates it.

The form imports nothing. An import reads a file, on a hit too, because the
store reads the file and its includes to check the key. A program that denies
`:fs` sees the same denial whether or not the file was cached before.

## A module's form does not suspend

A loader runs a module's form on the current fiber and refuses a suspension,
because it cannot hold one ([park.md](../impl/region/park.md)). A pure form
signals nothing but `:error`, so it never suspends. The function a module's
value holds is an ordinary closure, and it may suspend when called directly
and inside `ev/spawn`. So a module is already asynchronous where its work runs.

An asynchronous read would buy little. The file read and path resolution block
briefly. The compile is the cost, and all of it is CPU work on the one
scheduler thread. One compile per file pays it once per instance.

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

**A file as an implicit letrec of any value.** This is the shape today: the
top level runs on every import, and nothing it builds is shared. A rule a
reader cannot see decides the file's scope, and its value cannot be cached.

**An implicit letrec that must return a lambda.** The lambda was where
impurity went. A pure form may hold a lambda already, so requiring one buys
nothing, and the implicit letrec stays a rule a reader cannot see.

**Hoisting.** The compiler would move pure bindings out of a module's lambda
to share them. One pure form lets the author write that sharing, and a pass
the program cannot see would decide what the author can write.

**A loader that loads a module's literal imports before its form runs.** The
form would then receive values instead of reading files. The same
`import-file` would mean one thing in a module and another in a program.

**A compile-time import.** A new form, like `include`, would read another
module and evaluate its form while the importer compiles. A compile would
then run code from other files, a special case no user could build. It would
also bring back compile-time recursion between files (#1323).

**A declaration of dependencies.** A module's value would name its imports as
data, and the loader would apply the module to their values. A function that
imports when called gives the same result without a protocol.

**A disk cache of compiled code beside each source file.** Key a file's
bytecode on its compile inputs, store it, and load it in the next process. The
[stdlib disk cache](../impl/stdlib-cache.md) works that way, and it is a
stopgap. A hit rebuilds every object through the send codec, which
[image.md](../impl/image.md) rejects as the CAS cache: an image maps a region and
decodes no value. A module disk cache would repeat the rejected design for
user code, so module images replace it.
