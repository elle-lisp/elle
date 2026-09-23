# Elle

<!-- audited: 2026-09-22 -->

A Lisp whose compiler infers what each function does and where each value dies,
and runs on those facts.

[![CI](https://github.com/elle-lisp/elle/actions/workflows/main.yml/badge.svg)](https://github.com/elle-lisp/elle/actions/workflows/main.yml)

Before any code runs, Elle's compiler resolves every binding, finds what every
closure captures, infers the signals every function can raise, and infers the
point where every value dies. Most Lisps discover these facts at run time or
never. Elle uses them for four things: typed control flow, capability
sandboxing, cooperative concurrency, and memory management with no garbage
collector.

This file explains those ideas, what works today, and what we are building
toward. Elle is under active development. Its syntax changes between
[epochs](docs/epochs.md), and its performance is not yet a focus. The
[open issues](https://github.com/elle-lisp/elle/issues) list what is broken.

## Contents

- [The compiler's facts are data](#the-compilers-facts-are-data)
- [Signals flow up](#signals-flow-up)
- [Capabilities flow down](#capabilities-flow-down)
- [Fibers](#fibers)
- [Memory without a collector](#memory-without-a-collector)
- [Images](#images)
- [Hygienic macros](#hygienic-macros)
- [Epochs](#epochs)
- [Where Elle is going](#where-elle-is-going)
- [Using Elle](#using-elle)

## The compiler's facts are data

A program can ask the compiler what it inferred about source text, without
running that text:

```lisp
(def src "(defn pick [b x y] (if b x y))
          (defn fetch [url] (emit :yield url))
          (defn apply-to [f x] (f x))")
(def facts (compile/analyze src {:file "example.lisp"}))

(assert (get (compile/signal facts :pick) :silent))    # raises nothing
(assert (get (compile/signal facts :fetch) :yields))   # suspends its caller
(assert (not (get (compile/signal facts :apply-to) :silent)))
# apply-to raises whatever its argument f raises
```

The same queries answer which bindings a closure captures, what a function
calls, and the whole call graph ([portrait](docs/analysis/portrait.md)). An
[MCP server](docs/mcp.md), in the `mcp` submodule, loads these facts for a
whole codebase into an RDF graph that a coding agent can query with SPARQL.

## Signals flow up

A signal is a keyword a function raises to its caller: `:error`, `:yield`,
`:io`, or one the program declares. The compiler infers, for every function,
the set of signals it can raise. A function is **silent**, **yields**, or is
**polymorphic**, which means it raises whatever its function arguments raise.
Nobody writes these annotations.

The inference is what makes functions colorless. A function that suspends
needs no `async` marker, and neither do its callers. The compiler knows which
calls can suspend and compiles each call to match.

A fiber catches the signals its mask names. A generator is a fiber that
yields:

```lisp
(defn produce [] (emit :yield 1) (emit :yield 2) (emit :yield 3))
(def gen (fiber/new produce |:yield|))

(fiber/resume gen)
(assert (= (fiber/value gen) 1))
(fiber/resume gen)
(assert (= (fiber/value gen) 2))
```

`(silence)` declares a ceiling, and the compiler enforces it. A function that
promises silence cannot call an argument that might yield, unless it bounds
that argument too:

```lisp
(def refused (protect (eval '(defn tight [f x] (silence) (f x)))))
(assert (not (first refused)))                 # a compile error

(def bounded (protect (eval '(defn tight [f x] (silence f) (f x)))))
(assert (first bounded))                       # f is bounded, so it compiles
```

[docs/signals/](docs/signals/index.md) covers inference, user signals, and the
protocol.

## Capabilities flow down

A parent fiber can withhold capabilities from a child: `:io`, `:fs`, `:ffi`,
`:exec` and the rest. When the child calls a primitive that needs a withheld
capability, the primitive does not run. The child raises a denial instead, and
the parent decides what happens next. The parent can perform the operation on
the child's behalf, or it can refuse, and the child sees an ordinary error at
its own call site:

```lisp
(with-temp-dir dir
  (let [target (path/join dir "notes")
        child (fiber/new
                (fn []
                  (let [[ok? err] (protect (file/write target "x"))]
                    [:refused (not ok?) err]))
                |:fs :error|
                :deny |:fs|)]
    (fiber/resume child)                # the child asks to write; it is denied
    (fiber/refuse child :not-permitted)
    (assert (= (fiber/value child) [:refused true :not-permitted]))))
```

A child inherits every restriction its parent carries, and threads inherit
them too. [docs/signals/capabilities.md](docs/signals/capabilities.md) states
what a denial confines, and what it does not.

## Fibers

A fiber is an execution context with its own stack and signal mask. Fibers
are cooperative: one runs at a time, so shared data needs no locks. Top-level
code runs inside an async scheduler, so I/O suspends only the fiber that asked.
The scheduler uses io_uring on Linux.

Structured concurrency sits on top of the scheduler:

```lisp
(defn load-users [] [:ada :grace])
(defn load-settings [] {:theme :dark})

(def page
  (ev/scope (fn [spawn]
    (let [users (spawn load-users)
          settings (spawn load-settings)]
      {:users (ev/join users) :settings (ev/join settings)}))))

(assert (= (get page :users) [:ada :grace]))
```

Janet's fibers are the model we started from. [lib/process.lisp](lib/process.lisp)
builds Erlang-style processes from the same fibers, in Elle alone: mailboxes,
links, monitors, GenServers and supervisors. The process layer is young. Links,
supervisor restarts and timeouts have open defects under the
[processes](https://github.com/elle-lisp/elle/issues?q=is%3Aopen+label%3Aprocesses)
label. See [docs/concurrency.md](docs/concurrency.md) and
[docs/processes.md](docs/processes.md).

## Memory without a collector

Elle has no tracing collector and no GC pause. Every value is born in a
*region*: a set of pages the compiler assigns from its analysis. The compiler
emits the release of each region at the point where its values die. Nobody
calls `free`.

- **Immutable values follow the Tofte–Talpin region calculus.** Escape analysis
  bounds each value's lifetime statically, and the compiler frees it at its last
  use.
- **Mutation adds reference counts.** A store into a mutable container can
  point anywhere, at a time no static analysis can see. So a store counts a
  reference to the stored value's region, at run time.
- **Ownership reclaims whole subtrees.** A region the compiler proves has one
  owner joins an ownership forest rooted at an activation or a fiber. It frees
  with its owner, interior cycles included.

Freed pages return to a small per-thread cache, and pages past that cache go
back to the operating system at once. So a long-running program's resident
memory falls when its live data falls.

This is where most of the current work goes. The project measures leaks with
gauges ([tests/elle/oracle.lisp](tests/elle/oracle.lisp)) and closes them one
class at a time. Two costs remain visible today:

- A region owns at least one 4 KiB page. A value that keeps a region of its own
  (for example, each element a loop pushes into an `@array`) therefore costs
  about 4 KB. Region merging, which gives values with one lifetime one region,
  is the next step.
- The region table grows to the largest number of regions alive at once, and
  does not shrink.

[docs/regions.md](docs/regions.md) is the model to write code against, and
[docs/impl/memory.md](docs/impl/memory.md) is the implementor's map.

## Images

A region is a set of pages, so a region can be written to a file and mapped
back. An *image* is a compacted region plus a relocation table. Loading one
maps its pages privately and rewrites its pointers in one pass. No value is
decoded, and pages that nothing touches are never read from disk.

The boot image holds the whole standard library as one region. From a warm
image, Elle starts in under 10 ms with 12 MB resident. Loading the library from
the default bytecode cache takes 40 ms and 39 MB, and compiling it from source
takes 230 ms and 100 MB. Try the image with a cache directory:

```sh
elle --boot-image=$HOME/.cache/elle script.lisp
```

The boot image is opt-in for now. A closure loaded from an image carries no
LIR yet, so the JIT cannot compile the library's functions. Saving and loading
a user session as an image comes next, and then [fleet](docs/impl/fleet.md):
running a function on other machines by shipping its image.
[docs/impl/image.md](docs/impl/image.md) holds the design.

## Hygienic macros

Macros transform syntax objects that carry scope sets, as in Racket. A name a
macro introduces cannot capture a name at its call site:

```lisp
(defmacro my-swap (a b)
  `(let [tmp ,a] (assign ,a ,b) (assign ,b tmp)))

(def @tmp 100)
(def @x 1)
(def @y 2)
(my-swap x y)
(assert (= [x y tmp] [2 1 100]))   # the macro's tmp is not the caller's tmp
```

See [docs/macros.md](docs/macros.md).

## Epochs

A breaking change to the language gets an epoch number. A file can declare the
epoch it was written for with `(elle/epoch N)`, and the compiler migrates older
syntax before it expands macros. `elle rewrite` applies the same migration to
the source file. See [docs/epochs.md](docs/epochs.md).

## Where Elle is going

- **Region merging.** Collapse regions that die together, so a collection and
  its elements share pages. This removes most of the per-value page cost.
- **Performance.** Performance has not been a focus since the memory rewrite.
  Today `+`, `-` and `<` are variadic library functions that the JIT cannot
  compile. Plain numeric code runs 100 to 2,000 times slower than Janet or Lua,
  and code written with `%` [intrinsics](docs/intrinsics.md) about 4 to 50 times
  slower. Type inference and [dissolution](docs/impl/dissolution.md) are the
  way back. Dissolution already fuses chains of `map`, `filter` and `fold` over
  proven arrays into single loops. The plan lowers proven calls to intrinsics
  and VM instructions, and restores the JIT's speed.
- **Ownership in place of counting.** Extend the ownership forest, in the
  direction of Project Verona's regions, so fewer mutable stores need a
  reference count.
- **Heterogeneous execution.** Lower dissolved kernels through MLIR
  automatically, keep device data in region-based device pools, and migrate
  regions between host and GPU. This replaces today's hand-driven Vulkan path
  ([docs/impl/gpu.md](docs/impl/gpu.md)).
- **Environment images and fleet**, described under [Images](#images).
- **WASM.** The WebAssembly backend was set aside during the memory rewrite.
  Under `--wasm=full` it frees no memory and fails about half of the test
  corpus ([docs/impl/wasm.md](docs/impl/wasm.md)).

## Using Elle

Build with a stable Rust toolchain. [INSTALL.md](INSTALL.md) lists the system
libraries.

```sh
cargo build --release -p elle
./target/release/elle script.lisp       # run a file
./target/release/elle notes.md          # run the lisp blocks of a markdown file
./target/release/elle -e '(+ 1 2)'      # evaluate an expression
./target/release/elle                   # start the REPL
```

Read [QUICKSTART.md](QUICKSTART.md) before you write Elle. It lists the traps:
`nil` is not `()`, `#` starts a comment, and `assign` mutates while `set` builds
a set. [docs/coming-from.md](docs/coming-from.md) maps Elle onto the language
you know.

The tools ship in the same binary: `elle fmt` formats source, `elle lint` runs
the compiler's diagnostics, `elle lsp` serves an editor over the Language
Server Protocol, and `elle rewrite` migrates source between epochs.

The standard library ([docs/libraries.md](docs/libraries.md)) and the Rust
plugins ([docs/plugins.md](docs/plugins.md)) are written against the same
primitives. The plugins and the MCP server live in submodules:

```sh
git submodule update --init plugins mcp
```

[CONTRIBUTING.md](CONTRIBUTING.md) describes how to work on Elle and which
tests to run. The full corpus takes about 30 minutes on a release build.

## License

MIT
