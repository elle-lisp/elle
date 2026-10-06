# Modules: a proposal

<!-- audited: 2026-10-06 -->

The module system Elle is building toward: compiled once per instance, linked across files, and shipped as images.

[modules.md](modules.md) describes the module system as it is. This document
states the design it becomes, and the argument for each part. Two parts are
implemented: [the raw primitives](#the-raw-primitives-are-raw) and
[contained expansion](#expansion-is-contained).
[solver.md](impl/solver.md) owns the cross-file signal solver that linking
uses. The decisions that belong to the language's owner are
listed under [Open decisions](#open-decisions).

## Contents

- [Where the module system stands](#where-the-module-system-stands)
- [Terms](#terms)
- [Principles](#principles)
- [Silent is sound](#silent-is-sound)
- [The raw primitives are raw](#the-raw-primitives-are-raw)
- [Expansion is contained](#expansion-is-contained)
- [Expansion is deterministic](#expansion-is-deterministic)
- [One compile per file](#one-compile-per-file)
- [The module shape](#the-module-shape)
- [A top level does not suspend](#a-top-level-does-not-suspend)
- [What is shared is pure](#what-is-shared-is-pure)
- [Linking](#linking)
- [Images and exported macros](#images-and-exported-macros)
- [Rejected alternatives](#rejected-alternatives)
- [Open decisions](#open-decisions)
- [Measuring](#measuring)

## Where the module system stands

The defects below carry issue numbers. The measurements were taken on a
release build at `8fe781a03`; [Measuring](#measuring) gives the method.
[modules.md](modules.md) states two more under its architectural
constraints: every load compiles its file again (#881), and no signal crosses a
file boundary, so a call into another file takes every bit (#1232).

**A function body infers less than a file.** A file's top level converges by a
fixpoint, and a function body runs none ([pipeline.md](pipeline.md)). A
forward reference inside a body gets every bit, and a lambda that calls its
enclosing definition reads the silent seed.

**Silent does not mean silent.** Strict destructuring, qualified access, a
spliced call to a known function, and `parameterize` on a non-parameter all
infer silent and raise at run time (#1320). So do `eval` (#1243), a muffled
signal (#1236), and the `(silence p)` entry check (#1233). An uncaught raise
from such a function aborts the process as a silence violation.

**A transformer may still reach outside its unit.** Where no fiber withholds
the capability, a transformer may read files and load shared libraries.
Scheduler I/O fails inside one as an unexpected signal. `eval` and `import`
fail because expansion has no compile context, not because a rule forbids
them. Which of these a macro may do is an [open decision](#open-decisions).

**Expansion depends on more than its input.**

- State kept in `begin-for-syntax` resets on every compile, so it is fixed by
  the unit's source and is not an input of its own.
- `gensym` draws from a process-global counter, and scope ids from a
  per-instance counter.
- A user signal's bit is assigned in declaration order, process-wide:
  `:alpha` is bit 32 or 33 depending on which declaration ran first. Compiled
  code carries the bit number.

**The standard library already has the shape.** 69 of the 70 files in
[lib/](../lib/) end in a `(fn …)` that builds the export struct; `lua.lisp` is
the exception, and it does not compile (#1217). What sits outside that lambda
is what any sharing rule has to settle:

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
Today it is not sound (#1320).

**Pure** means silent, reading no ambient state, and changing no state that
outlives the evaluation. Signals decide only the first part. `sys/env`,
`sys/args`, `sys/pid` and `backend?` transfer no control, so they are silent,
and they are still ambient reads. Assigning a captured binding, reading a
mutable binding, and mutating a shared container are silent too. Purity needs
an analysis of its own beside signal inference.

**Deterministic** means the same inputs give the same output, a raise included.
A deterministic expression may raise, so it is weaker than pure. A cache needs
determinism. Moving code to another point in time needs more, because a moved
raise happens at a different time, or happens where it never did.

**Hoisting** moves a binding out of a module's lambda to the file's top level,
so it is computed once when the file loads instead of on every call. The
example runs: both shapes answer the same, and the proposal is that the
compiler turns the first into the second.

```lisp
(defn build-huffman-table [] {})
(defn lookup [table s level] s)

# Today: the table is built on every call.
(def before (fn [&named level]
              (def table (build-huffman-table))
              (defn encode [s] (lookup table s level))
              {:encode encode}))

# Proposed: the compiler hoists the table to the file's top level.
(def table (build-huffman-table))
(def after (fn [&named level]
             (defn encode [s] (lookup table s level))
             {:encode encode}))

(assert (= ((get (before) :encode) "x") ((get (after) :encode) "x")))
```

A binding may move only when it reads nothing bound inside the lambda, it is
deterministic, and it allocates nothing mutable. A hoisted `@[]` is shared by
every instance. Whether a hoisted binding may also raise is an
[open decision](#open-decisions): a raise that moves to load time fails the
import instead of the call. Hoisting is not inlining. Inlining puts an imported
closure's body at a call site in another file.

## Principles

**A raw primitive does one thing and assumes nothing.** `import-file` and
`include-file` load the file their argument names. `import` and `include`
resolve a spec, and the rules they follow are Elle code a program may replace.

**A check is enforced, not trusted.** Where a property can be checked at run
time, it is checked there too. Capabilities already work this way.

**A cache is invisible.** A program cannot tell whether a module came from a
cache, except by time. Every input that can change a compile is in the key,
and every observable effect of an import still happens on a hit.

**A change to what a program means waits on a decision of its own.** A change
that only removes a defect lands first. A change to module identity or to the
language waits on its [open decision](#open-decisions).

## Silent is sound

A construct that can raise carries `:error`, the `(silence p)` entry check
included (#1320, #1233). `eval` reports its signal to the enclosing function
(#1243). `muffle` is checked at the function boundary as `squelch` is, so a
muffled signal becomes `:signal-violation` instead of leaving the function
(#1236). Removing `muffle` would change the language, which the fourth
principle puts behind a decision.

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
- The compiler does not assume what `import` resolves to. A literal
  `import-file` is the one dependency it knows before anything runs, and
  [linking](#linking) inherits that limit.

`include` keeps one copy of the rules by calling `import/resolve` at compile
time.

**The search visits neither the working directory nor the writer's
directory.** A spec that starts with `./` or `../` names a file relative to
the writer. A bare spec searches `--path`, then home. A file beside a program
therefore cannot shadow a library of the same name, the defect that Python 2's
implicit relative imports had and PEP 328 removed. A user who wants the working
directory searched sets `ELLE_PATH=.`.

## Expansion is contained

- A compile that code on a fiber starts expands every macro with the
  capabilities that fiber withholds. A denied primitive in a transformer fails
  the compile, naming the macro and the primitive (#1321,
  [capabilities.md](signals/capabilities.md)).
- Each transformer call and each `begin-for-syntax` definition runs under a
  fuel budget of its own. Exhausting it fails the compile, naming the macro
  (#1322, [runtime.md](runtime.md)).
- A file compiles against the prelude, the standard library, the bindings an
  embedder registers, its own definitions and its includes. REPL macros and
  REPL definitions reach later REPL lines, not a file a line imports
  ([pipeline](../src/pipeline/AGENTS.md)).
- A file's analysis never compiles another file. It states facts about its own
  code, and [linking](#linking) joins them. A cycle of literal imports
  therefore reaches the loader, which names the cycle (#1323,
  [modules.md](modules.md)).

Containment is what makes it safe to expand a file its reader did not write,
which the LSP, the linter and `compile/analyze` do.

## Expansion is deterministic

- Number gensyms and intro scopes per unit, so an expansion depends on its
  unit and not on what the process expanded before it.
- Give a user signal a stable identity in compiled code: its name, relocated
  to the process's bit when the code loads. The image documents disagree
  today: [image.md](impl/image.md) replays a signal table at hydration, and
  [sealing.md](impl/image/sealing.md) says no image carries one and refuses
  the dump. Relocation by name replaces both.
- Write down the complete set of compile inputs.

The compile inputs, as far as they are known:

| Input | Why it changes a compile |
|-------|--------------------------|
| The source text, and every included file's text | The forms themselves |
| The epoch and the Unicode generation | Migration rules and string semantics |
| The binary's build identity and the primitive table | Codegen, primitive ids, the prelude and stdlib |
| The bindings an embedder registers | Every compile in the instance resolves them, as it resolves a primitive |
| The macro environment | Prelude, plus the unit's own macros; plus the macros of every literal import once modules export them |
| The resolved path of every literal `import-file` and every `include` | The writer's directory and the search paths decide which file a path names |
| The facts of every literal import | Linking reads them to solve the file's signals |
| User signal declarations | Bit numbers, until signals relocate by name |

This table is the key of [one compile per file](#one-compile-per-file), and
the digest of a module image, as the source digest is for the boot image
([boot.md](impl/image/boot.md)). Containment comes first, because it fixes the
macro environment.

## One compile per file

Keep the bytecode a file's compile produced, keyed in the instance by its
compile inputs, and let the runtime `import` run it instead of compiling
again. This closes #881. No chdir primitive exists, so a resolved path is
stable within an instance. The key follows the content, so a REPL that edits a
module gets the new code.

An importer's key includes the keys of its literal imports. Once a call into
another file uses the linked signal, the importer's bytecode depends on what
its imports state. The store keeps each file's facts beside its bytecode, not
the signals solved from them. A file solved with its imports left open is sound
but loose ([solver.md](impl/solver.md)), and linking closes it.

A compiled template may run twice: constants materialize per execution, so two
runs share code and nothing else. Every import still runs the file, so module
meaning does not change.

## The module shape

This is where module meaning changes. Three shapes are on the table:

| Shape | Top level | Instantiation | Purity check |
|-------|-----------|---------------|--------------|
| (a) Today: an implicit letrec, any value | runs on every import | not separate | none |
| (b) An implicit letrec that must return a lambda | pure; evaluated once and shared | the lambda call; may be impure | needed |
| (c) A single `(fn …)` expression | a closure with an empty environment | the lambda call; may be impure | deferred to hoisting |

Shape (b) splits link time from instantiation. Impurity moves into the lambda,
so a stateful or effectful module stays possible: it opens its library, reads
its environment and allocates its state when called. Within an instance, (b)
evaluates the top level once; across processes, the speed comes from
[module images](#images-and-exported-macros).

Shape (c) is pure by construction, because a lambda literal is a value. A
function body is already an implicit letrec, so the letrec moves inside the
lambda instead of disappearing. Four costs follow:

- Work that every instance could share is repeated on each call until the
  compiler hoists it.
- Hoisting needs the purity analysis below, so (c) moves the check into the
  compiler rather than removing it.
- `include` is spliced only at a file's top level today, so it would have to
  work inside the body.
- All module code moves into a function body, which infers less than a file
  ([where it stands](#where-the-module-system-stands)). So (c) needs
  [linking](#linking) first, or a fixpoint over function bodies. The solver
  solves a body's definitions jointly, forward and mutual references included.

Both (b) and (c) change identity. Today every import makes new closures,
parameters and trait tables. Under (b) the top level is shared by every
importer in an instance, as plugins already are, so one parameter or trait
table exists per instance. Under (c) nothing is shared until hoisting shares
it.

Either shape moves the impure top levels in the table above into their
lambdas. [captures.rs](../tests/integration/file_scope/captures.rs) imports a
module that keeps a counter at its top level twice, and checks that the two
counters are independent. The purity check rejects that fixture under (b).
Once the counter moves inside the lambda, the assertion holds unchanged.

The lambda rule makes one mistake common. `(def b (import "std/base64"))`
followed by `(b:encode "x")` reports "get: expected collection …, got
closure". That case needs an error of its own.

## A top level does not suspend

A loader runs a module's top level on the current fiber and refuses a
suspension, because it cannot hold one ([park.md](impl/region/park.md)). A top
level that runs `ev/sleep` or `port/read-all` fails with `import/load-file:
unexpected signal`. The lambda a module returns is an ordinary closure, and it may suspend
when called directly and inside `ev/spawn`. So a module is already
asynchronous where its work runs.

The top level needs no rule of its own. Shape (b) makes it pure, shape (c)
makes it one lambda literal, and neither suspends. Three reasons keep it from
suspending:

1. The set of imports in progress belongs to the VM, not to a fiber. A
   suspended top level would keep its mark, so a second fiber importing the
   same file would get "circular dependency".
2. Under (b), a shared top level that runs interleaved with other fibers could
   build a different value on each run.
3. An asynchronous read buys little. The file read and path resolution block
   briefly, and the compile is the cost: 0.29 s for `std/http2`, all of it CPU
   on the one scheduler thread. One compile per file and linking remove the
   runtime compile for a literal import. `lib/`, `tests/`, `demos/` and
   `tools/` hold 439 literal specs and about 15 computed ones.

## What is shared is pure

Under (b), the top level must be pure. Signals answer whether it can raise.
A second analysis answers whether it reads ambient state or changes shared
state:

- Mark the primitives that read process state: the environment, the
  arguments, the pid, the clock, configuration, and identity hashes.
- Reject a top level that assigns a captured binding, mutates a container it
  did not allocate, or lets a mutable allocation escape into the value.
- Reject a lambda that assigns a binding of the top level. `lib/aws.lisp`
  allocates its mutable binding at the top level and assigns it inside the
  lambda, so under (b) a second instantiation would overwrite the first's.
- Enforce it at run time as well, by evaluating the top level in a fiber that
  withholds every capability. That catches the effects that carry a bit. The
  ambient reads carry none, so whether they gain one is an
  [open decision](#open-decisions); until then the analysis alone covers them.

Literal imports at the top level need a rule of their own. Once imports link,
a literal import of a pure module is a dependency, not an effect. A top level
that calls an imported lambda is pure only if that lambda's body is.

The check reads sound signals, and it needs a decided shape.

## Linking

Treat `((import-file "literal"))` as a dependency the compiler knows before
anything runs:

- The export shape becomes part of the rule: the lambda's body ends in a
  struct literal. `compile/exports`, the semver surface and IDE completion then
  work for every module.
- Signals cross files as facts, not as a per-field summary. Each file states
  its signals over free variables: its own parameters, captured parameters,
  fields of parameters, and the exports of its imports. Linking joins the files
  through their literal imports and solves the whole graph at once
  ([solver.md](impl/solver.md)).
- A call through `module:field` uses the solved signal, which closes #1232.
  A map from field to signal cannot hold that answer: it has no way to say
  "field `:connect` of parameter 0". The `propagates` mask in `Signal` becomes
  the special case where every free variable is one of the function's own
  parameters.
- A cycle between function bodies is legal, and its files solve jointly. A
  cycle of top-level imports is an instantiation cycle, and the runtime check
  reports it by name.
- A closure that captures no instantiation argument can be inlined into its
  importer, as stdlib bodies already are.
- The WASM backend links an imported module in ahead of time, instead of
  running it on the host VM (#924).
- Under shape (c), hoisting gives back the sharing that (b) writes by hand.

Linking needs the purity check under (b), and sound signals under either
shape. It gives a module value an image can carry.

## Images and exported macros

A pure, linked module value can be saved as an image and mapped at load
([image.md](impl/image.md)). A module image is a configuration of the image
mechanism, layered over the boot image as an environment image is. Its digest
is the compile-input table above, with each literal import's digest folded in.

The rules images already state carry over:

- Loading a module image still requires `:fs`, so a sandboxed fiber sees the
  same denial as for a source import.
- A directory of module images is a code-execution surface. Trust an image as
  you would a `.so` ([image.md](impl/image.md)).
- A closure that carries a process-declared signal bit refuses the dump until
  signals relocate by name.

[image.md](impl/image.md) describes the boot and environment configurations
only. Module images wait on two image milestones
([plan.md](impl/image/plan.md)): the environment configuration, and LIR that
lives in regions. Until the second lands, an image-loaded closure never
reaches the JIT.

A module whose top level is pure can be evaluated while its importer compiles.
Its macros can then be exported as values, and they join the macro environment
in the compile-input table. That removes the [warts](warts.md) entry saying a
macro cannot be exported, and leaves `include` without its main use.

## Rejected alternatives

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

## Open decisions

- **Macro purity: strict or deterministic.** A transformer builds its result
  with `list`, `concat` and quasiquote, which may raise `:error`, so under
  strict purity few of the 31 prelude macro bodies pass. Determinism is
  enough for a code cache, because a deterministic raise is a compile error
  the cache can store. Strictness matters only where expansion is reordered
  or hoisted.
- **Impure macros.** Either allow them and never cache their unit, or let a
  macro read only through primitives that record what they read in the unit's
  key, as `include` already does. The second keeps file embedding,
  environment-driven configuration and `feature?` possible.
- **The module shape**: (a), (b) or (c).
- **Identity under (b)**: whether one parameter and trait table per instance
  is the intended meaning.
- **Raises at a shared top level**: whether a top level, or a hoisted
  binding, may raise `:error`. A cache tolerates it. A hoisted raise fails
  the import instead of the call.
- **Ambient reads**: whether `sys/env`, `sys/args`, `sys/pid`, the clock and
  `backend?` gain a capability bit, so the run-time purity check covers them.
- **Linking through `import`**: how `((import "std/x"))` becomes a dependency
  the compiler knows, now that `import` is a macro over a resolver a program
  may replace.
- **Plugins**: whether `(import "plugin/x")` keeps returning a struct while a
  source module returns a lambda.

## Measuring

The repeated compile is visible per file and per phase. Run this from the
repository root:

```sh
echo '(def a ((import-file "lib/base64.lisp"))) (def b ((import-file "lib/base64.lisp")))' | elle --trace=compile -
```

`lib/base64.lisp` appears twice, once for each load. One compile per file is
done when it appears once.

On a release build at `8fe781a03`, where an importer's analysis compiled each
literal import for its projection, and so did the analysis of each import
inside `std/http2`:

| Measure | Value |
|---------|-------|
| Files `std/http2` compiles twice | 9 |
| Time the import adds | 0.29 s |
| The second compiles, summed | 105 ms |

The numbers move with the machine and with every front-end pass. Re-measure
rather than read them.
