# Modules: a proposal

<!-- audited: 2026-10-04 -->

The ordered changes that take Elle's modules from re-compiling every import to compiled, cacheable, linkable units.

[modules.md](modules.md) describes the module system as it is. This document
proposes what it becomes, step by step. Nothing here is implemented. Each step
names what it needs from the steps before it, and what it unlocks. The
decisions that belong to the language's owner are listed last, under
[Open decisions](#open-decisions).

## Contents

- [Where the module system stands](#where-the-module-system-stands)
- [Terms](#terms)
- [Principles](#principles)
- [The steps](#the-steps)
- [Open decisions](#open-decisions)
- [Measuring](#measuring)

## Where the module system stands

The defects below carry issue numbers. The measurements were taken on a
release build at `8fe781a03`; [Measuring](#measuring) gives the method.

**Every literal import compiles its target twice.** The analyzer compiles the
target of each `((import "literal"))` to read its signal projection
([call.rs](../src/hir/analyze/call.rs), [cache.rs](../src/pipeline/cache.rs)).
The runtime `import` then compiles it again (#881). Importing `std/http2`
compiles each of its nine files twice, and the second compile is about a third
of the time the import adds.

**The first compile buys nothing yet.** A call through `module:field` never
uses the projection, so the analyzer treats it as a call to an unknown
function (#1232).

**Silent does not mean silent.** Strict destructuring, qualified access, a
spliced call to a known function, and `parameterize` on a non-parameter all
infer silent and raise at run time (#1320). So do `eval` (#1243), a muffled
signal (#1236), and the `(silence p)` entry check (#1233). An uncaught raise
from such a function aborts the process as a silence violation.

**Expansion is not contained.**

- A macro run on behalf of a fiber ignores the capabilities the fiber
  withholds (#1321).
- A transformer runs with no fuel budget, so a looping macro hangs every tool
  that compiles the file (#1322).
- `compile/analyze` of an importer runs the macros of each file it imports.
- A transformer may read files and load shared libraries. Scheduler I/O
  fails inside one as an unexpected signal. `eval` and `import` fail because
  expansion has no compile context, not because a rule forbids them.

**Expansion depends on more than its input.**

- A macro typed at the REPL reaches the compile of every later import in the
  session ([pipeline](../src/pipeline/AGENTS.md)). So does a `def` typed
  there: a module that names a binding only the REPL defined compiles under
  the REPL and fails as a file.
- State kept in `begin-for-syntax` resets on every compile, so it is fixed by
  the unit's source and is not an input of its own.
- `gensym` draws from a process-global counter, and scope ids from a
  per-instance counter.
- A user signal's bit is assigned in declaration order, process-wide:
  `:alpha` is bit 32 or 33 depending on which declaration ran first. Compiled
  code carries the bit number.
- The projection cache is keyed by path and never invalidated.

**Resolution is loose where it should be strict.** `import-file` is an alias of
`import`, so it resolves `std/`, `plugin/`, the search paths and suffixes
(#1325). A relative spec resolves against the working directory, never against
the importing file.

**A cycle of literal imports overflows the stack** at compile time, before the
runtime cycle check can run (#1323).

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
`include-file` load the file their argument names. `import` and `include` are
the only forms that resolve a spec.

**A check is enforced, not trusted.** Where a property can be checked at run
time, it is checked there too. Capabilities already work this way.

**A cache is invisible.** A program cannot tell whether a module came from a
cache, except by time. Every input that can change a compile is in the key,
and every observable effect of an import still happens on a hit.

**A step that changes what a program means is a decision of its own.** The
steps that only remove defects come first. The steps that change module
identity or the language come after, each behind an open decision.

## The steps

### 1. Make silent sound

Fix #1320, #1243, #1236 and #1233. A construct that can raise carries
`:error`, and `eval` reports its signal to the enclosing function. `muffle` is
checked at the function boundary as `squelch` is, so a muffled signal becomes
`:signal-violation` instead of leaving the function. Removing `muffle` would
change the language, which the fourth principle puts behind a decision.

Needs: nothing. Unlocks: every later step that reads an inferred signal.

### 2. Make the raw primitives raw

Fix #1325. `import-file` and `include-file` load exactly the file named.
`import` and `include` resolve `std/`, `plugin/`, the search paths, `.lisp`
probing, and the platform's shared-object name and suffix.

Which directory a relative spec resolves against stays an
[open decision](#open-decisions). Resolving against the importing file, as
`include-file` does, needs a mechanism `import` lacks, because it is a runtime
primitive given a string. Either the call site's location reaches it at run
time, or `import` becomes a form that bakes the directory in at compile time.

Needs: nothing. Unlocks: two resolution rules a reader can state, one for the
raw primitives and one for the resolving forms.

### 3. Contain expansion

- A fiber's withheld capabilities reach every macro expanded on its behalf
  (#1321).
- Each transformer call runs under a fuel budget; exhausting it is a compile
  error naming the macro (#1322).
- The projection compile tracks the files in progress and reports a cycle by
  name (#1323).
- A module compiles against the prelude, its own definitions and its
  includes. REPL macros and REPL definitions reach the REPL's own lines, not
  an imported file. This rewrites the sixth invariant in
  [pipeline](../src/pipeline/AGENTS.md), and that file changes with it.

Needs: nothing. Unlocks: running another file's macros safely, which the LSP,
the linter and `compile/analyze` already do.

### 4. Make expansion deterministic

- Number gensyms and intro scopes per unit, so an expansion depends on its
  unit and not on what the process expanded before it.
- Give a user signal a stable identity in compiled code: its name, relocated
  to the process's bit when the code loads. The image documents disagree
  today: [image.md](impl/image.md) replays a signal table at hydration, and
  [sealing.md](impl/image/sealing.md) says no image carries one and refuses
  the dump. Relocation by name replaces both.
- Write down the complete set of compile inputs. This is the cache key of
  step 6.

The compile inputs, as far as they are known:

| Input | Why it changes a compile |
|-------|--------------------------|
| The source text, and every included file's text | The forms themselves |
| The epoch and the Unicode generation | Migration rules and string semantics |
| The binary's build identity and the primitive table | Codegen, primitive ids, the prelude and stdlib |
| The macro environment | Prelude, plus the unit's own macros; plus the macros of every literal import once step 10 exports them |
| The resolved path of every import | The search paths decide which file a spec names |
| The projection of every literal import | Signal inference reads them |
| User signal declarations | Bit numbers, until signals relocate by name |

Needs: step 3, which fixes the macro environment. Unlocks: step 6.

### 5. Reuse the projection compile

Keep the bytecode the projection compile produced, keyed in the instance by
resolved path and content hash, and let the runtime `import` run it instead
of compiling again. This closes #881 and removes the second compile. No chdir
primitive exists, so a resolved path is stable within an instance, and the key
follows the content, so a REPL that edits a module gets the new code and the
new projection.

A compiled template may run twice: constants materialize per execution, so two
runs share code and nothing else. Every import still runs the file, so module
meaning does not change.

Needs: step 3, which keeps REPL state out of a module's compile. Unlocks: fast
imports in one instance, and the store step 6 writes to disk.

### 6. Cache compiled code on disk

Key the compiled bytecode of a file on step 4's inputs, with each imported
file's key folded in, so the key covers the whole import graph. Resolution
runs before the lookup, so a hit still pays the search-path walk.

The cache stores code, not values. Every import still runs the file, so
stateful modules keep independent state. A hit still requires `:fs`, so a
sandboxed fiber sees the same denial either way.

The [stdlib disk cache](impl/stdlib-cache.md) is the model: it already carries
LIR for the JIT, symbol spellings for the display memo, and the registries
later compiles read. Its store discipline carries over too: a temporary file
renamed into place, pruning after the rename, and a key that follows the
binary. So does its warning: a writable cache directory is a code-execution
surface, and here it holds user modules. Source positions name files by
process-local ids, so `meta/origin` is lost across processes unless file
spellings travel too.

Needs: steps 2, 4 and 5. Unlocks: fast imports across processes, and a store
later steps build on.

### 7. Decide the module shape

This is where module meaning changes. Three shapes are on the table:

| Shape | Top level | Instantiation | Purity check |
|-------|-----------|---------------|--------------|
| (a) Today: an implicit letrec, any value | runs on every import | not separate | none |
| (b) An implicit letrec that must return a lambda | pure; evaluated once and shared | the lambda call; may be impure | needed |
| (c) A single `(fn …)` expression | a closure with an empty environment | the lambda call; may be impure | deferred to hoisting |

Shape (b) splits link time from instantiation. Impurity moves into the lambda,
so a stateful or effectful module stays possible: it opens its library, reads
its environment and allocates its state when called.

Shape (c) is pure by construction, because a lambda literal is a value. A
function body is already an implicit letrec, so the letrec moves inside the
lambda instead of disappearing. Three costs follow. Work that every instance
could share is repeated on each call until the compiler hoists it. Hoisting
needs the purity analysis of step 8, so (c) moves the check into the compiler
rather than removing it. `include` is spliced only at a file's top level
today, so it would have to work inside the body.

Both (b) and (c) change identity. Today every import makes new closures,
parameters and trait tables. Under (b) the top level is shared by every
importer in an instance, as plugins already are, so one parameter or trait
table exists per instance. Under (c) nothing is shared until hoisting shares
it.

Either shape moves the impure top levels in the table above into their
lambdas. [captures.rs](../tests/integration/file_scope/captures.rs) imports a
module that keeps a counter at its top level twice, and checks that the two
counters are independent. Step 8 rejects that fixture under (b). Once the
counter moves inside the lambda, the assertion holds unchanged.

The lambda rule makes one mistake common. `(def b (import "std/base64"))`
followed by `(b:encode "x")` reports "get: expected collection …, got
closure". That case needs an error of its own.

Needs: step 6 for the speed that makes the change worth it. Unlocks: steps 8
to 10.

### 8. Check purity of what is shared

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

Literal imports at the top level need a rule of their own. Under step 9 a
literal import of a pure module is a dependency, not an effect. A top level
that calls an imported lambda is pure only if that lambda's body is.

Needs: steps 1 and 7. Unlocks: step 9.

### 9. Link literal imports

Treat `((import "literal"))` as a dependency the compiler knows before
anything runs:

- The export shape becomes part of the rule: the lambda's body ends in a
  struct literal. Projection, `compile/exports`, the semver surface and IDE
  completion then work for every module.
- A call through `module:field` uses the projected signal, which closes
  #1232.
- A closure that captures no instantiation argument can be inlined into its
  importer, as stdlib bodies already are.
- The WASM backend links an imported module in ahead of time, instead of
  running it on the host VM (#924).
- Under shape (c), hoisting gives back the sharing that (b) writes by hand.

Needs: step 8. Unlocks: step 10, and a module value an image can carry.

### 10. Images and exported macros

A pure, linked module value can be saved as an image and mapped at load
([image.md](impl/image.md)). Two image constraints apply to modules. A closure
that carries a process-declared signal bit refuses the dump until signals
relocate by name (step 4). An image-loaded closure reaches the JIT only after
LIR becomes region-native.

A module whose top level is pure can be evaluated while its importer compiles.
Its macros can then be exported as values, and they join the macro environment
in step 4's table. That removes the [warts](warts.md) entry saying a macro
cannot be exported, and leaves `include` without its main use.

Needs: steps 8 and 9. Unlocks: modules that ship as compiled assets.

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
- **The module shape**: (a), (b) or (c) in step 7.
- **Identity under (b)**: whether one parameter and trait table per instance
  is the intended meaning.
- **Raises at a shared top level**: whether a top level, or a hoisted
  binding, may raise `:error`. A cache tolerates it. A hoisted raise fails
  the import instead of the call.
- **Ambient reads**: whether `sys/env`, `sys/args`, `sys/pid`, the clock and
  `backend?` gain a capability bit, so the run-time check in step 8 covers
  them.
- **The raw primitives** (#1325). Which directory a relative spec resolves
  against. Whether `import-file` loads a shared object given its exact path.
  Whether a file that is not valid UTF-8 is still tried as a plugin. Whether
  `module/import` stays an alias.
- **Plugins**: whether `(import "plugin/x")` keeps returning a struct while a
  source module returns a lambda.

## Measuring

The double compile is visible per file and per phase:

```sh
echo '(def h ((import "std/http2")))' | elle --trace=compile -
```

Each module file appears twice. The first appearance is nested inside the
importing file's analysis, and the second is the runtime import. Step 5 is
done when each file appears once.

On a release build at `8fe781a03`:

| Measure | Value |
|---------|-------|
| Files `std/http2` compiles twice | 9 |
| Time the import adds | 0.29 s |
| The second compiles, summed | 105 ms |

The numbers move with the machine and with every front-end pass. Re-measure
rather than read them.
