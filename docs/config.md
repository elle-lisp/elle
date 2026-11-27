# Runtime Configuration (`vm/config`)

<!-- audited: 2026-11-27 -->

What a build decides, what the `elle` command line sets, and what a running
program reads and changes through `vm/config`.

A build fixes the runtime a program runs on: which optimizing tier it carries,
and which I/O backend. The command line sets what a user may reasonably
change per run — tracing, dumps, paths and caches — and no flag chooses a
tier. A running program reads the rest through `vm/config` and
changes a few settings through `vm/config-set`.

## Builds

A build carries at most one optimizing tier, and the cargo features pick it:

| Feature | Default | What it adds |
|---------|---------|--------------|
| `jit` | on | The Cranelift JIT, the tier of the default build |
| `mlir` | off | The MLIR tier. It replaces the JIT: an `mlir` build runs no JIT |
| `wasm` | off | The WebAssembly backend. It replaces both, and adds `--wasm=` |
| `ffi` | on | C interop through libffi |
| `uring` | on | io_uring on Linux. Without it, and on every other platform, I/O goes through the thread pool |
| `ablation` | off | The knob the fuel bench turns: the shape of a silent fuel charge ([benches](../benches/README.md)). No program reads it |

The precedence is `wasm`, then `mlir`, then `jit`: a build with more than one
of the three runs the first it carries. A tier compiles a function once the
function has been called ten times; `vm/config` reads and sets that threshold
(below).

A build is one implementation of Elle, and every build must pass the language
suite ([spec](spec.md)). There is therefore no flag that chooses a tier or a
backend. A user who wants the interpreter alone builds without the `jit`
feature. Two programs can switch a tier off or make it eager: the rig, for one
implementation test ([rig](../rig/overview.md)), and `elle test`
([test-runner](test-runner.md)). A user program cannot.

No primitive of a user build names the build; the rig's `(elle/build)` does
([rig](../rig/overview.md)).

## CLI flags

### The program and its arguments

The first argument that is not a flag names the program, and `elle` runs
exactly one. Every argument after it belongs to the program, reachable
through `sys/args` — no separator is needed:

```bash
elle server.lisp 8080      # the program reads ("8080") from (sys/args)
elle walk.lisp data.lisp   # data.lisp is an argument, not a second program
```

So a shell glob does not run every match: the first match is the program
and the rest are its arguments. The pinning test is
[argv.lisp](../tests/cli/argv.lisp).

A program name that starts with `--` is a flag `elle` does not know, and
`elle` refuses it rather than looking for a file of that name.

### Where elle's flags stop

The program name ends them. Elle reads the flags below before the program name,
and hands the program every argument after it through `sys/args`, a `--`
included.

```bash
elle --trace=call script.lisp -- --trace=call   # elle takes the first, the script the rest
```

`--help` and `--version` obey the same boundary, so a script is free to carry
flags of those names.

### Version

```bash
elle --version                       # print the version and exit
```

`--version` prints the banner — `Elle v` and the version — then exits 0. It
answers before the VM starts, so it works in a tree where the stdlib or a
plugin is broken.

One number feeds every surface that names a version: `[package] version` in
[the root manifest](../Cargo.toml), read at build time into `elle::VERSION`.
`--version`, the `--help` banner, the REPL greeting and the language server's
`serverInfo` all render that constant.

### Tracing

`--trace=` and its `--debug-*` aliases log what a subsystem does
([debugging](analysis/debugging.md)).

### Statistics

`--dump=stats` runs the program and prints statistics when it ends normally.
It takes the JIT's finished compiles, then drops its queued compiles and the
one in progress, so the exit waits for no compile. An eager run can queue
hundreds of compiles, and one large function can hold Cranelift for seconds.
It then prints its compiled and rejected functions, the call count behind each
rejection, the compiles with their wall and CPU time, and how many compiles it
dropped, as `dropped: N` ([jit](impl/jit.md)). It also prints the
page-claim histogram and the regions the teardown left alive. The other `--dump=` keywords print a compiler
artifact and exit without running the program; `stats` is the one that runs
it. `(vm/config :stats)` reads whether the process started with it.

`--dump=bytecode` prints what the emitter wrote for the file: every code object
the file builds, nested lambdas included, with its instructions, constants,
source locations, region tables, and yield, call and charge sites. Two runs
over one file print one text, so a recorded dump is a golden.
`make bytecode-golden` records it for the corpus and the standard library, and
`make bytecode-golden-check` compares the current binary against the record.

### WASM policy

A `wasm` build takes `--wasm=`; no other build accepts the flag.

```bash
elle --wasm=off script.lisp         # disable WASM (default)
elle --wasm=full script.lisp        # compile everything upfront
elle --wasm=3 script.lisp           # compile each closure on its third call
elle --wasm=lazy script.lisp        # the same, on the eleventh call
```

| Policy | CLI | Behavior |
|--------|-----|----------|
| Off | `--wasm=off`, `--wasm=0` | WASM disabled (default) |
| Full | `--wasm=full` | Full-module compilation |
| Lazy | `--wasm=N`, `--wasm=lazy` | Each closure compiled from its Nth call; `lazy` is N = 11 |

### Module paths

```bash
elle --path=/opt/elle-lib:vendor script.lisp   # look here for a bare import spec
elle --home=/opt/elle script.lisp              # the project root
```

`--path` (or `ELLE_PATH`) lists the directories `import/resolve` searches,
after the working directory, for a spec that has no prefix and does not start
with `./`. `--home` (or `ELLE_HOME`) names the root that `std/` and `plugin/`
resolve under. [resolution.md](modules/resolution.md) owns the rules.

### Boot image

```bash
elle --boot-image=on script.lisp     # the embedded blob, then the warm cache under --cache= (default)
elle --boot-image=DIR script.lisp    # the warm cache in DIR alone
elle --boot-image=off script.lisp    # compile core, prelude and stdlib
```

A boot image is core.lisp, prelude.lisp and stdlib.lisp already compiled, as
page bytes an instance maps instead of running the front end. A release binary
that `make elle` built carries one, the embedded blob, and the default
hydrates it. A binary without a blob reads the warm cache instead. On a hit,
boot hydrates the stored image; on a miss, boot compiles from source and
stores one for the next start. `elle image dump-boot FILE` writes one
explicitly.

The default keeps the warm cache in `boot` beneath the `--cache=` directory, so
`--cache=` with no directory turns the warm cache off and leaves the blob. An
image boot runs user code as a source boot does, on every tier.
[boot.md](impl/image/boot.md) owns the policy, the key that ties an image to
the binary that stored it, and the paths the default does not reach.

### The cache directory

```bash
elle --cache=DIR script.lisp         # cache in DIR
elle --cache= script.lisp            # read and write no cache
```

The cache directory holds what a later start can reuse instead of compiling
again: a boot image ([boot.md](impl/image/boot.md)) and compiled WebAssembly
modules ([wasm.md](impl/wasm.md)). With no flag, elle takes the first of these
that names a directory:

| Source | Directory |
|--------|-----------|
| `ELLE_CACHE` | `$ELLE_CACHE` |
| `XDG_CACHE_HOME` | `$XDG_CACHE_HOME/elle` |
| `HOME` | `$HOME/.cache/elle` |
| none of them | `elle-cache` in the platform temp directory |

Caching is off only when you ask for that: `--cache=` with no directory, or an
empty `ELLE_CACHE`. Any other empty variable counts as unset. Neither `TMPDIR`
nor `TMP` chooses the directory. Many Linux sessions set neither, and a cache
that depends on them is off there, so every start of a binary with no blob
compiles its boot sources.

## Elle API

### Reading configuration

```lisp
(vm/config)                    # returns the full config struct
(vm/config :trace)             # returns the current trace keyword set
(vm/config :jit)               # returns the JIT threshold, or nil
(vm/config :mlir)              # returns the MLIR threshold, or nil
(vm/config :max-depth)         # returns the non-tail call depth cap
(vm/config :jit-verify)        # returns whether Cranelift verifies JIT code
(vm/config :home)              # returns --home, or nil
(vm/config :path)              # returns the --path entries as an array
(vm/config :plugin-suffix)     # returns "so", "dylib" or "dll"
```

`import/resolve` reads all three ([resolution.md](modules/resolution.md)).

```lisp
(assert (array? (vm/config :path)) "no --path is an empty array")
(assert (contains? |"so" "dylib" "dll"| (vm/config :plugin-suffix)))
```

`:jit` answers the number of calls after which the JIT compiles a function,
or nil when this build carries no JIT or the JIT is off. `:mlir` answers the
same for the MLIR tier. A `wasm` build answers `:wasm` with its policy
keyword, and no other build knows the key.

```lisp
(let [jit (vm/config :jit)]
  (assert (or (nil? jit) (> jit 0)) "a JIT threshold is a positive count"))
```

### The depth cap

`:max-depth` caps how many non-tail closure calls may be in progress on one
fiber. The default is 10,000,000. A call past the cap halts the program with
`:stack-overflow`, and no signal mask catches the halt. A tail call does not
count toward the cap.

```lisp
(assert (= (vm/config :max-depth) 10000000) "the default depth cap")
```

A non-tail call costs memory, not native stack, so the cap is what stops a
runaway recursion before it takes the machine's memory. Each call in progress
holds a few hundred bytes. [impl/vm.md](impl/vm.md) owns the mechanism.

### Setting configuration

`vm/config-set` is the setter. `(vm/config)` answers an immutable struct, so
`put` on it builds a new value and changes nothing on the VM.

```lisp
# Enable trace keywords (takes effect immediately)
(vm/config-set :trace |:call :signal|)
(vm/config-set :trace ||)

# Change the depth cap (a positive integer)
(vm/config-set :max-depth 1000000)
(vm/config-set :max-depth 10000000)
```

`(vm/config-set :jit N)` sets the JIT threshold to the positive integer `N`,
and `(vm/config-set :mlir N)` sets the MLIR tier's. Each refuses a value that
is not a positive integer, and a threshold for a tier this run has off. Inside
`elle test` alone, each also takes `:off` and `:eager` ([test-runner](test-runner.md)).
There a threshold needs only a build that carries the tier, so the runner can
put back the setting it read. Any other process refuses `:off` and `:eager`
with an `:argument-error`.

```lisp
(let [[ok? err] (protect (vm/config-set :jit :off))]
  (assert (not ok?) "a program cannot turn the JIT off")
  (assert (= (get err :error) :argument-error)))
(let [[ok? err] (protect (vm/config-set :jit :later))]
  (assert (not ok?) "a keyword other than :off or :eager is no policy")
  (assert (= (get err :error) :type-error)))
```

## Implementation

`RuntimeConfig` is stored on the VM struct (not in a global static), so each
VM — each test worker, each embedded instance — has its own.

`vm/config` reads that struct through SIG_QUERY, and `vm/config-set` writes it.

For hot paths (VM dispatch loop), trace keywords are mirrored in a shared
atomic bitfield, the instance's `TraceCell`, to avoid set lookups on every
instruction.

An embedding host builds a `Config` itself, so the policies a user build does
not expose are reachable from Rust: `JitPolicy::Off` and `JitPolicy::Eager`
are what the rig sets for a sidecar that asks for them. `GraftPolicy::Off` is
another, and keeps every call a call in the units an instance compiles after
its boot ([grafting a call](impl/graft.md)).
