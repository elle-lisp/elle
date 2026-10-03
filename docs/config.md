# Runtime Configuration (`vm/config`)

<!-- audited: 2026-09-29 -->

What a build decides, what the `elle` command line sets, and what a running
program reads and changes through `vm/config`.

A build fixes the runtime a program runs on: which optimizing tier it carries,
and which I/O backend. The command line sets what a user may reasonably
change per run — tracing, dumps, paths and caches — and no flag chooses a
tier. A running program reads the rest through `vm/config` and changes a few
settings through `vm/config-set`.

## Builds

A build carries at most one optimizing tier, and the cargo features pick it:

| Feature | Default | What it adds |
|---------|---------|--------------|
| `jit` | on | The Cranelift JIT, the tier of the default build |
| `mlir` | off | The MLIR tier. It replaces the JIT: an `mlir` build runs no JIT |
| `wasm` | off | The WebAssembly backend. It replaces both, and adds `--wasm=` |
| `ffi` | on | C interop through libffi |
| `uring` | on | io_uring on Linux. Without it, and on every other platform, I/O goes through the thread pool |

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
[argv_cli.rs](../tests/integration/argv_cli.rs).

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

### Trace flags

The `--trace` flag replaces all `--debug-*` flags with a unified,
composable interface:

```bash
elle --trace=call script.lisp        # trace function calls
elle --trace=call,signal script.lisp # trace calls and signals
elle --trace=all script.lisp         # trace everything
```

Available trace keywords:

| Keyword | Description |
|---------|-------------|
| `:call` | Function calls: name, arg count, dispatch decision |
| `:signal` | Signal dispatch: bits received, squelch, capability denial |
| `:compile` | Compilation: phase entry/exit with timing |
| `:fiber` | Fiber operations: resume, swap, status transitions |
| `:hir` | HIR analysis: binding resolution, signal inference |
| `:lir` | LIR lowering: slot allocation, capture cells |
| `:emit` | Bytecode emission |
| `:jit` | JIT compilation: decisions, rejections, batch compilation |
| `:io` | I/O operations |
| `:import` | Module import resolution |
| `:macro` | Macro expansion |
| `:wasm` | WASM backend: host calls, compilation |
| `:capture` | Capture analysis decisions |
| `:arena` | Heap allocation, region enter/exit |
| `:escape` | Escape analysis decisions |
| `:bytecode` | Bytecode dump before execution |
| `:posix` | POSIX-signal subsystem (os/sig-* primitives, signalfd/kqueue) |
| `:chan` | Channel wake protocol (register/deregister, send wake) |
| `:rc` | Reference-count operations on regions |
| `:regions` | Region inference and lifetime decisions |
| `:anf` | A-normal form lift pass |
| `:pages` | Region page allocation |
| `:boot` | Boot-sequence timing: primitive registration, core, prelude, stdlib compile/execute (string-traced, no bit) |
| `:census` | Post-boot heap census: per-tag object counts and bytes, capture cells, pointer-slot density, unsealed variants (string-traced, no bit; see [sealing.md](impl/image/sealing.md)) |
| `:free` | Region free diagnostics (string-traced, no bit) |
| `:guardfree` | Guarded-free diagnostics (string-traced, no bit) |
| `:freebt` | Free backtrace diagnostics (string-traced, no bit) |
| `:scrub` | Zero a freed page's body so a stale deref lands on a tag no live value carries (string-traced, no bit) |
| `:residue` | Teardown leak dump: the surviving regions and their cross-region edges (string-traced, no bit) |
| `:park` | Park and resume diagnostics: every suspended-frame park and every frame replay, with the frame's shape (string-traced, no bit) |
| `:syncjit` | Compile with Cranelift on the VM thread instead of the background worker (string-traced, no bit) |

The CLI rejects unknown keywords and lists valid names in the error. Elle code
can still set unknown trace keywords for forward compatibility.

Trace output format: `[trace:KEYWORD] message` on stderr, for easy
grep filtering.

### Old flags (aliases)

Old `--debug-*` flags are kept as aliases for backward compatibility:

| Old flag | Equivalent |
|----------|-----------|
| `--debug` | `--trace=bytecode` |
| `--debug-jit` | `--trace=jit` |
| `--debug-resume` | `--trace=fiber` |
| `--debug-stack` | `--trace=call` |
| `--debug-wasm` | `--trace=wasm` |

### Statistics

`--dump=stats` runs the program and prints statistics when it ends normally:
the JIT's compiled and rejected functions, the page-claim histogram, and the
regions the teardown left alive. The other `--dump=` keywords print a compiler
artifact and exit without running the program; `stats` is the one that runs
it.

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

### Boot image

```bash
elle --boot-image=off script.lisp    # compile core, prelude and stdlib (default)
elle --boot-image=on script.lisp     # warm-cache a boot image under --cache=
elle --boot-image=DIR script.lisp    # warm-cache it in DIR
```

A boot image is core.lisp, prelude.lisp and stdlib.lisp already compiled, as
page bytes an instance maps instead of running the front end. On a hit, boot
hydrates it; on a miss, boot compiles from source and stores one for the next
start. `elle image dump-boot FILE` writes one explicitly.

The default is off. A hydrated stdlib reaches neither the JIT tier nor
cross-unit inlining yet, so turning it on trades steady-state throughput for
startup; [boot.md](impl/image/boot.md) owns the policy and names the two
milestones the default waits on.

## Elle API

### Reading configuration

```lisp
(vm/config)                    # returns the full config struct
(vm/config :trace)             # returns the current trace keyword set
(vm/config :jit)               # returns the JIT threshold, or nil
(vm/config :mlir)              # returns the MLIR threshold, or nil
(vm/config :max-depth)         # returns the non-tail call depth cap
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

### Future feature flags

The following keywords are accepted in trace sets without error, for
forward compatibility:

- `:spirv` — SPIR-V shader compilation
- `:mlir` — MLIR compilation tier
- `:gpu` — GPU offloading

## Implementation

`RuntimeConfig` is stored on the VM struct (not in a global static), so each
VM — each test worker, each embedded instance — has its own.

`vm/config` reads that struct through SIG_QUERY and `vm/config-set` writes it.
A write takes effect immediately — no restart needed.

For hot paths (VM dispatch loop), trace keywords are mirrored in a shared
atomic bitfield, the instance's `TraceCell`, to avoid set lookups on every
instruction.

An embedding host builds a `Config` itself, so the policies a user build does
not expose are reachable from Rust: `JitPolicy::Off` and `JitPolicy::Eager`
are what the rig sets for a sidecar that asks for them.
