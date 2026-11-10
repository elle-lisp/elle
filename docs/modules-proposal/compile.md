# Modules: how a file compiles

<!-- audited: 2026-10-26 -->

The module proposal's implemented parts: sound and joint signals, raw loaders in Elle, contained and deterministic expansion.

[The proposal](../modules-proposal.md) defines the terms this document uses
and states the principles it follows. Each part below is implemented, and every
later part of the design reads it.

## Silent is sound

Every construct that can raise carries `:error`, a bound's entry check and
`eval` included. A ceiling checks the inferred signal and does not replace it,
and `muffle` is gone. A signal that leaves a silent function is a broken
proof, a terminal error ([inference.md](../signals/inference.md)).

Every later part reads an inferred signal, so each one depends on this.

## The raw primitives are raw

`import/load-file` and `include-file` load exactly the file named (#1325).
Each loader does one thing, and all but one are Elle functions over the
primitives every program has. `import/load-file` reads a file and hands the
text on, `import/load-source` reads one form out of text, and
`import/load-syntax` evaluates one form as a module. `import/load-plugin` is
the primitive, because it runs foreign code, so `:ffi` sits on its declaration
instead of being read off a path. [modules.md](../modules.md) states the rules.

**A spec that names code resolves against the file that writes it.** A
program then means the same thing wherever the process starts, and a library
moves as one unit with its private modules. `include-file` already worked this
way, and so do ES modules, Python's relative imports and Rust's `mod` paths. A
path that names data, given to `file/read` or `port/open`, stays relative to
the working directory. So there are two rules: code is relative to its file,
and data to the process.

Only the compiler knows which file wrote a form, so `(meta/location)` hands
that fact to Elle code, and the `import` macro reads it. A form a macro builds
carries the location of the macro call, so a macro learns the file of the code
that called it. `import/load-file` takes a path as `file/read` takes one; the
writer's directory is `import`'s business.

**`import` is a library, not a primitive.** It is a prelude macro that emits
`:import`, and the rules a request follows are `import/load` and
`import/resolve`, functions in the standard library. It shows a user how to
build a module system, and a user may replace it for a scope. Two costs
follow:

- A macro is not a value, so `import` cannot be handed to `map`.
- `(import spec)` resolves its spec when the code runs, so the compiler knows
  no file from it. A literal `(import/load-file "path")` is the one dependency
  the compiler reads before anything runs, and [linking](../modules-proposal.md)
  inherits that limit.

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
  capabilities that fiber withholds. Every expansion also withholds the effect
  capabilities, `:ambient`, which names the reads of state no argument
  carries, and `:import`. A denied primitive in a transformer fails the
  compile, naming the macro and the primitive (#1321,
  [capabilities.md](../signals/capabilities.md), [macros.md](../macros.md)).
- An expansion runs under a fuel budget the VM counts, which silent code, the
  fibers the expansion resumes and the expansions it starts all spend.
  Exhausting it fails the compile, naming the macro (#1322,
  [runtime.md](../runtime.md)).
- A file compiles against the prelude, the standard library, the bindings an
  embedder registers, its own definitions and its includes. REPL macros and
  REPL definitions reach later REPL lines, not a file a line imports
  ([pipeline](../../src/pipeline/AGENTS.md)).
- A file's analysis never compiles another file. It states facts about its own
  code, and [linking](../modules-proposal.md) joins them. A module's form
  imports nothing, so no load starts another (#1323,
  [shape.md](shape.md)).

Containment is what makes it safe to expand a file its reader did not write,
which the LSP, the linter and `compile/analyze` do. A pure transformer is what
lets a cache keep a compiled unit: an input outside the key cannot change the
expansion.

## Expansion is deterministic

A unit expands to the same tree in every process. Its scope ids and its
expansion-time gensym names come from its own tag and its own count
([macros.md](../macros.md)). State kept in `begin-for-syntax` resets on every
compile, so the unit's source fixes it too.

A user signal's bit still follows declaration order, so a compile takes the
declarations as an input. Within one process a bit never changes, and an
in-instance cache needs nothing more. Code that crosses processes needs the
signal's name instead, and module images ([shape.md](shape.md)) are the first
such code.

The compile inputs:

| Input | Why it changes a compile |
|-------|--------------------------|
| The source text, and every included file's text | The forms themselves |
| The unit's own path | Its spans, `(meta/location)`, and the directory a relative `import` spec joins |
| The epoch and the Unicode generation | Migration rules and string semantics |
| The binary's build identity and the primitive table | Codegen, primitive ids, the prelude and stdlib, the stdlib bodies a call grafts, and the expansion budget |
| The bindings an embedder registers | Every compile in the instance resolves them, as it resolves a primitive |
| The macro environment | The prelude, the unit's own macros, and the macros of its includes |
| The REPL layer, for a REPL line | The macros and definitions earlier lines made |
| The `env` struct an `eval` takes | Its entries bind as immutable names |
| The capabilities the compiling fiber withholds | A macro that calls a denied primitive fails the compile |
| The `elle test` transforms | The runner wraps each test form before analysis |
| The resolved path of every `include` | The writer's directory and the search paths decide which file a spec names |
| User signal declarations | Bit numbers, until signals relocate by name |

This table is what the key of a compile cache covers, and the digest of a
module image, as the source digest is for the boot image
([boot.md](../impl/image/boot.md)). Containment comes first, because it fixes
the macro environment.

A file's compile reads none of its imports, so no import is in its key. An
import runs when the code that holds it runs, and reads its file then.

## A unit's signals are solved jointly

After the analyzer's one pass over a unit, the signal solver settles all of the
unit's signals together ([pipeline.md](../pipeline.md),
[solver.md](../impl/solver.md)). A function body therefore infers what a file
infers: a call to a sibling defined later, mutual recursion included, takes
that sibling's signal. Every ceiling meets the solved signals.

[The shape](shape.md) moves all module code into a body, so it reads this part.

## Rejected alternatives

**Resolving a relative path against the working directory.** It is one rule
for code and data, and every library pays for it: its imports break when a
user runs from another directory. Passing the caller's directory at run time
fails too, because a function that holds an import may run from any file.

**One loader that sorts its input.** A loader that picks source or library by
extension, or by whether the file is UTF-8, must read its argument to know
what it needs. `import/load` picks by name, and each loader declares its need.

**A store of compiled units inside the runtime.** The Rust loader kept each
file's unit under its path, keyed by the compile inputs, so a second load of a
file compiled nothing (#881). It went with that loader: a cache the loader
cannot see is one the loader cannot replace, and the loader is Elle now. A
cache returns as the loader's own code.

**Impure macros.** A transformer allowed to read ambient state makes its unit
one that no cache may keep, so every file whose macros read the environment
compiles on every load. Recording each read in the unit's key, as `include`
records its file, was the other way to keep such a unit. Transformers are
pure instead, and `include` stays the one read a compile records.
