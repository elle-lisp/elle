# Modules: one form

<!-- audited: 2026-11-21 -->

Why a module is one form per file, evaluated in a fiber that withholds every effect, and the shapes it rejected.

[principles.md](principles.md) defines the terms this document uses, and
[compile.md](compile.md) states how a file compiles. [modules.md](../modules.md)
states what a load does; this document says why it has this shape, and what it
rejected. [plan.md](plan.md) states the parts of the design not yet built.

## A module is one form

A module file holds one form, after its `(elle/epoch N)` line, which the
reader consumes. A load evaluates the form and answers its value. The runtime
adds nothing else: the loader is Elle code, and the default loader keeps the
compiled form of each file it loads ([modules.md](../modules.md)).

No implicit letrec remains. A module with several definitions wraps them in
`letrec`, `let`, `fn` or a struct literal. `defmacro` works inside a body, and
so does `include-file`, which splices into the form that holds it
([include.md](../include.md)).

The form may build any value. Work it does outside a lambda runs once per
load. Effects and dependencies go inside a function the value holds, and a
call of it runs them. [lib/git.lisp](../../lib/git.lisp) has this shape: its
lambda imports `std/git/core`, and the core's lambda loads `libgit2.so`. A
module with no dependency and no effect may return its struct directly. Every
file in [lib/](../../lib/) holds one form.

The rule binds what a loader loads. A program that `elle` runs keeps many
forms and may be impure, as the files under [tests/lang](../../tests/lang)
are.

Three consequences follow:

- **Identity.** Whatever the form builds belongs to the load that built it, a
  parameter or a trait table included. Two imports build two values. A program
  that wants one instance imports once and hands the value on, or installs a
  loader that keeps values.
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

## How the form is held pure

The default loader compiles the form on the calling fiber, and runs it in a
fiber that withholds every effect capability, `:ambient`, `:import` and
`:inspect`. A form that loads a library, reads a file, prints, reads ambient
state, imports or reads what a closure captured is denied, and the load fails
naming the primitive. So a load never starts
another load, and no list of loads in progress exists anywhere. The compile
reads the form's includes as the caller may ([include.md](../include.md)). A
loader of the user's own picks its own deny set, in the one argument
`fiber/new` takes for it.

The default loader also walks the value and refuses mutable state
([modules.md](../modules.md)). [plan.md](plan.md) drops that walk.

## A module's form does not suspend

The fiber that runs the form catches a yield, and the loader refuses the load,
because a load answers one value. A form under the deny set signals nothing
else but `:error`. The function a module's value holds is an ordinary closure,
and it may suspend when called directly and inside `ev/spawn`. So a module is
already asynchronous where its work runs.

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
