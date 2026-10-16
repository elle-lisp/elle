# Modules: a proposal

<!-- audited: 2026-10-16 -->

The module system Elle is building toward: one pure form per file, evaluated once per instance, linked across files, shipped as images.

[modules.md](modules.md) describes the module system as it is. This document
states the design it becomes, and the argument for each part. Five parts are
implemented: [sound signals](#silent-is-sound),
[the raw primitives](#the-raw-primitives-are-raw),
[contained expansion](#expansion-is-contained),
[deterministic expansion](#expansion-is-deterministic) and
[one compile per file](#one-compile-per-file). The shape a module takes is
decided: [one pure form](#a-module-is-one-pure-form).
[solver.md](impl/solver.md) owns the cross-file signal solver that linking
uses. The decisions that still belong to the language's owner are listed under
[Open decisions](#open-decisions).

## Contents

- [Where the module system stands](#where-the-module-system-stands)
- [Terms](#terms)
- [Principles](#principles)
- [Silent is sound](#silent-is-sound)
- [The raw primitives are raw](#the-raw-primitives-are-raw)
- [Expansion is contained](#expansion-is-contained)
- [Expansion is deterministic](#expansion-is-deterministic)
- [One compile per file](#one-compile-per-file)
- [A module is one pure form](#a-module-is-one-pure-form)
- [How the form is held pure](#how-the-form-is-held-pure)
- [A module's form does not suspend](#a-modules-form-does-not-suspend)
- [Linking](#linking)
- [Images](#images)
- [Rejected alternatives](#rejected-alternatives)
- [Open decisions](#open-decisions)
- [Measuring](#measuring)

## Where the module system stands

A defect below carries its issue number where it has one.
[modules.md](modules.md) states one more under its architectural constraints:
no signal crosses a file boundary, so a call into another file takes every bit
(#1232).

**A function body infers less than a file.** A file converges its definitions
jointly. A body converges a definition only against itself, so a call to a
later sibling, mutual recursion included, takes every bit
([pipeline.md](pipeline.md)).

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
is what moves into it under [the shape](#a-module-is-one-pure-form):

| File | Top level outside the lambda |
|------|------------------------------|
| [lib/zmq.lisp](../lib/zmq.lisp) | `ffi/native` loads `libzmq.so` |
| [lib/aws.lisp](../lib/aws.lisp) | `sys/env` reads credentials and region; `@sigv4-mod` is mutable, and the lambda assigns it |
| [lib/dns.lisp](../lib/dns.lisp) | `@next-txid` is a counter every caller shares |
| [lib/redis.lisp](../lib/redis.lisp) | two parameters, and an imported module called at top level |
| [lib/process.lisp](../lib/process.lisp), [lib/telemetry.lisp](../lib/telemetry.lisp) | imported modules called at top level |
| [lib/process/scheduler.lisp](../lib/process/scheduler.lisp) | imports at top level |

## Terms

**A signal is a possible transfer of control.** Every signal, `:error`
included, is a possible raise. Signals describe control flow and nothing else.

**Silent** means the inferred signal is empty and the inference is sound.

**Pure** means silent apart from `:error`, reading no ambient state, and
changing no state that outlives the evaluation. Signals decide only the first
part. Today `sys/env`, `sys/args`, `sys/pid` and `backend?` transfer no
control, so they are silent, and they are still ambient reads. Assigning a
captured binding, reading a mutable binding, and mutating a shared container
are silent too.

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
language waits on its [open decision](#open-decisions).

## Silent is sound

Every construct that can raise carries `:error`, a bound's entry check and
`eval` included. A ceiling checks the inferred signal and does not replace it,
and `muffle` is gone. A signal that leaves a silent function is a broken
proof, a terminal error ([inference.md](signals/inference.md)).

Every later part reads an inferred signal, so each one depends on this.

## The raw primitives are raw

`import-file` and `include-file` load exactly the file named (#1325). Three
loaders do the work, one for each kind of input: `import/load-file` for
source, `import/load-plugin` for a shared library, and `import/load-syntax`
for a form. No loader decides what its input is, so `:ffi` sits on the plugin
loader's declaration instead of being read off a path.
[modules.md](modules.md) states the rules.

**A path that names code resolves against the file that writes it.** A
program then means the same thing wherever the process starts, and a library
moves as one unit with its private modules. `include-file` already worked this
way, and so do ES modules, Python's relative imports and Rust's `mod` paths. A
path that names data, given to `slurp` or `port/open`, stays relative to the
working directory. So there are two rules: code is relative to its file, and
data to the process.

Only the compiler knows which file wrote a form, so `import-file` is a special
form, and `(meta/location)` hands the same fact to Elle code. A form a macro
builds carries the location of the macro call, so a macro learns the file of
the code that called it.

**`import` is a library, not a primitive.** It is a prelude macro over
`import-file`, and the rules it follows are `import/resolve`, a function in the
standard library. It shows a user how to build a module system, and a user
may replace it. Two costs follow:

- A macro is not a value, so `import` cannot be handed to `map`.
- `(import spec)` expands to `(import-file (import/resolve spec dir))`, so the
  path is computed when the code runs. A literal `import-file` is the one
  dependency the compiler knows before anything runs, and
  [linking](#linking) inherits that limit.

`include` keeps one copy of the rules: the compiler, not a macro, calls
`import/resolve` while it compiles, and records the path it answers.

**The search visits neither the working directory nor the writer's
directory.** A spec that starts with `./` or `../` names a file relative to
the writer. A bare spec searches `--path`, then home. A file beside a program
therefore cannot shadow a library of the same name, the defect that Python 2's
implicit relative imports had and PEP 328 removed. A user who wants the working
directory searched sets `ELLE_PATH=.`.

## Expansion is contained

- A transformer is deterministic and changes no state outside its unit. It
  may raise, because a raise is a compile error the unit's inputs decide, and
  `list`, `concat` and quasiquote all raise.
- A compile that code on a fiber starts expands every macro without the
  capabilities that fiber withholds, and every expansion withholds the effect
  capabilities as well. A denied primitive in a transformer fails the compile,
  naming the macro and the primitive (#1321,
  [capabilities.md](signals/capabilities.md)).
- A primitive that reads or changes ambient state through no capability bit
  carries a mark, and an expansion refuses it. The refusal halts the
  expansion, so no handler in the transformer catches it, and the compile
  fails naming the macro and the primitive ([macros.md](macros.md)).
- An expansion runs under a fuel budget the VM counts, which silent code, the
  fibers the expansion resumes and the expansions it starts all spend.
  Exhausting it fails the compile, naming the macro (#1322,
  [runtime.md](runtime.md)).
- A file compiles against the prelude, the standard library, the bindings an
  embedder registers, its own definitions and its includes. REPL macros and
  REPL definitions reach later REPL lines, not a file a line imports
  ([pipeline](../src/pipeline/AGENTS.md)).
- A file's analysis never compiles another file. It states facts about its own
  code, and [linking](#linking) joins them. A cycle of literal imports
  therefore reaches the loader, which names the cycle (#1323,
  [modules.md](modules.md)).

Containment is what makes it safe to expand a file its reader did not write,
which the LSP, the linter and `compile/analyze` do. A pure transformer is what
lets [one compile per file](#one-compile-per-file) keep a unit: an input
outside the key cannot change the expansion.

## Expansion is deterministic

A unit expands to the same tree in every process. Its scope ids and its
expansion-time gensym names come from its own tag and its own count
([macros.md](macros.md)). State kept in
`begin-for-syntax` resets on every compile, so the unit's source fixes it too.

A user signal's bit still follows declaration order, so a compile takes the
declarations as an input. Within one process a bit never changes, and an
in-instance cache needs nothing more. Code that crosses processes needs the
signal's name instead, and [module images](#images) are the first such code.

The compile inputs:

| Input | Why it changes a compile |
|-------|--------------------------|
| The source text, and every included file's text | The forms themselves |
| The unit's own path | Its spans, `(meta/location)`, and the directory a relative `import-file` joins |
| The epoch and the Unicode generation | Migration rules and string semantics |
| The binary's build identity and the primitive table | Codegen, primitive ids, the prelude and stdlib, the registries the stdlib compile fills, and the expansion budget |
| The bindings an embedder registers | Every compile in the instance resolves them, as it resolves a primitive |
| The macro environment | The prelude, the unit's own macros, and the macros of its includes |
| The REPL layer, for a REPL line | The macros and definitions earlier lines made |
| The `env` struct an `eval` takes | Its entries bind as immutable names |
| The capabilities the compiling fiber withholds | A macro that calls a denied primitive fails the compile |
| The `elle test` transforms | The runner wraps each test form before analysis |
| The resolved path of every literal `import-file` and every `include` | The writer's directory and the search paths decide which file a path names |
| User signal declarations | Bit numbers, until signals relocate by name |

This table is what the key of [one compile per file](#one-compile-per-file)
covers, and the digest of a module image, as the source digest is for the boot
image ([boot.md](impl/image/boot.md)). Containment comes first, because it fixes
the macro environment.

## One compile per file

`import/load-file` keeps the code a file's compile produced in the instance's
store, keyed by its compile inputs, and runs it instead of compiling again
(#881, [modules.md](modules.md)). No chdir primitive exists, so a resolved path
is stable within an instance. The key follows the content, so a REPL that edits
a module gets the new code.

The key is a digest of the inputs in the table above that can change within an
instance: the file's text, its path, the capabilities the compiling fiber
withholds, and each include's resolved path and text. A lookup reads the file
and resolves and reads every include again, so a hit reads what a miss reads. A
host that registers a binding or a primitive empties the store, and
[pipeline.md](pipeline.md) says why no other input needs a place in the key.

A file's compile reads none of its imports, so no import is in its key. An
import runs when the code that holds it runs, and reads its file then.

Today every import runs the kept code, so module meaning has not changed. The
[shape](#a-module-is-one-pure-form) below changes it.

## A module is one pure form

A module file holds one form, after its `(elle/epoch N)` line, which the
reader consumes. `import/load-file` evaluates the form once per instance and
keeps its value. Every later load of the file returns that value while its
compile inputs match, and an [image](#images) can carry it. The runtime adds
nothing else: it caches and hydrates.

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

[lib/git.lisp](../lib/git.lisp) has this shape today: its lambda imports
`std/git/core`, and the core's lambda loads `libgit2.so`. A module with no
dependency and no effect may return its struct directly.

The rule binds what `import/load-file` loads. A program that `elle` runs keeps
many forms and may be impure, as the files under [tests/lang](../tests/lang)
are.

Four consequences follow:

- **Identity.** Whatever the form builds exists once per instance, a parameter
  or a trait table included. A module that wants one per importer builds it in
  the function it returns.
- **Plugins.** `import/load-plugin` signals `:ffi`, so a pure form cannot load
  a plugin. A module loads one in the function it returns, or takes it as a
  parameter, as [modules.md](modules.md) shows. No rule names plugins.
- **Raises.** A raise in the form is deterministic, so it fails every load of
  the file while the inputs match.
- **Migration.** The top levels in the
  [table](#where-the-module-system-stands) move into their functions.
  [captures.rs](../tests/integration/file_scope/captures.rs) imports a module
  that keeps a counter at its top level twice, and checks that the two
  counters are independent. That fixture moves its counter into a function,
  and the assertion holds unchanged.

A module that returns a function and one that returns a struct look alike at
the import. `(def b (import "std/base64"))` followed by `(b:encode "x")`
reports "get: expected collection …, got closure" when the module returns a
function. That case needs an error of its own.

All module code moves into a body, which infers less than a file
([where it stands](#where-the-module-system-stands)). The solver solves a
body's definitions jointly, forward and mutual references included
([solver.md](impl/solver.md)), so it lands before the shape.

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
because it cannot hold one ([park.md](impl/region/park.md)). A pure form
signals nothing but `:error`, so it never suspends. The function a module's
value holds is an ordinary closure, and it may suspend when called directly
and inside `ev/spawn`. So a module is already asynchronous where its work runs.

An asynchronous read would buy little. The file read and path resolution block
briefly, and the compile is the cost: 0.29 s for `std/http2` at `8fe781a03`,
all of it CPU on the one scheduler thread. One compile per file pays it once
per instance. `lib/`, `tests/`, `demos/` and `tools/` hold 317 literal specs
and 15 computed ones.

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

An import runs inside the function a module returns, and `import` computes its
path when it runs. So the compiler sees a literal spec, and the file that spec
names is known only at run time. When linking joins the files is the open
decision [Linking through import](#open-decisions).

## Images

A module's value is pure, so it can be saved as an image and mapped at load
([image.md](impl/image.md)). A module image is a configuration of the image
mechanism, layered over the boot image as an environment image is. Its digest
is the compile-input table above.

The rules images already state carry over:

- Loading a module image still requires `:fs`, so a sandboxed fiber sees the
  same denial as for a source import.
- A directory of module images is a code-execution surface. Trust an image as
  you would a `.so` ([image.md](impl/image.md)).
- A closure that carries a process-declared signal bit refuses the dump until
  signals relocate by name.

A module image gives a user signal a stable identity in compiled code: its
name, relocated to the process's bit when the code loads. The image documents
disagree today: [image.md](impl/image.md) replays a signal table at
hydration, and [sealing.md](impl/image/sealing.md) says no image carries one
and refuses the dump. Relocation by name replaces both.

[image.md](impl/image.md) describes the boot and environment configurations
only. Module images wait on the environment configuration
([plan.md](impl/image/plan.md)).

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
[stdlib disk cache](impl/stdlib-cache.md) works that way, and it is a
stopgap. A hit rebuilds every object through the send codec, which
[image.md](impl/image.md) rejects as the CAS cache: an image maps a region and
decodes no value. A module disk cache would repeat the rejected design for
user code, so module images replace it.

**Resolving a relative path against the working directory.** It is one rule
for code and data, and every library pays for it: its imports break when a
user runs from another directory. Passing the caller's directory at run time
fails too, because a function that holds an import may run from any file.

**One loader that sorts its input.** A loader that picks source or library by
extension, or by whether the file is UTF-8, must read its argument to know
what it needs. `import-file` picks by name, and each loader declares its need.

**Impure macros.** A transformer allowed to read ambient state makes its unit
one that no store may keep, so every file whose macros read the environment
compiles on every load. Recording each read in the unit's key, as `include`
records its file, was the other way to keep such a unit. Transformers are
pure instead, and `include` stays the one read a compile records.

## Open decisions

- **Linking through import**: how a call through an imported module gets its
  solved signal, now that an import runs at instantiation and `import`
  computes its path when it runs.
- **Exported macros**: a macro exported as a value would join its importer's
  macro environment, so the importer's compile would evaluate another module.
  That is the compile-time import rejected above. `include` stays the way to
  share macros, and [warts.md](warts.md) keeps its entry.
- **Native libraries**: how a native library's init runs once per instance,
  and whether `ffi/on-unload` keeps one teardown for each library and symbol.
  [lib/git/core.lisp](../lib/git/core.lisp) registers its teardown again on
  every instantiation.

## Measuring

Run this from the repository root:

```sh
echo '(def a ((import-file "lib/base64.lisp"))) (def b ((import-file "lib/base64.lisp")))' | elle --trace=compile -
```

`lib/base64.lisp` appears once: the second load runs the code the first one
compiled. Before the store, each load compiled it, and at `8fe781a03` the
import of `std/http2` compiled nine files twice.
