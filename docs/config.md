# Runtime Configuration (`vm/config`)

<!-- audited: 2026-09-22 -->

Elle exposes a runtime configuration system reachable from both CLI flags and
Elle code. All debug/trace flags, JIT policies, and WASM policies are
controlled through a single mutable config struct on the VM.

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

### Where elle's flags stop

`--` ends them. Elle reads every flag below wherever it appears before that
separator, and hands the program every argument after it — the separator
included — through `sys/args`.

```bash
elle --jit=off script.lisp -- --jit=off   # elle takes the first, the script the second
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

### JIT policy

```bash
elle --jit=off script.lisp          # disable JIT
elle --jit=eager script.lisp        # compile on first call
elle --jit=adaptive script.lisp     # compile after threshold (default)
```

Named policies replace opaque integers:

| Policy | CLI | Old CLI | Behavior |
|--------|-----|---------|----------|
| Off | `--jit=off` | `--jit=0` | JIT disabled |
| Eager | `--jit=eager` | `--jit=1` | Compile on first call |
| Adaptive | `--jit=adaptive` | `--jit=11` | Compile after 10 calls (default) |

Old integer syntax still works as aliases.

The binary and the embedding library start from the same JIT policy. A host
that wants the interpreter alone asks for it, the way the CLI does.

### MLIR policy

```bash
elle --mlir=off script.lisp         # disable MLIR (default)
elle --mlir=eager script.lisp       # compile on first eligible call
elle --mlir=adaptive script.lisp    # compile after threshold
```

| Policy | CLI | Behavior |
|--------|-----|----------|
| Off | `--mlir=off` | MLIR disabled (default) |
| Eager | `--mlir=eager` | Compile on first eligible call |
| Adaptive | `--mlir=adaptive` | Compile after 10 calls |

Integer syntax works too: `--mlir=N` sets threshold to N-1.

The MLIR policy is independent of the JIT policy. When compiled with
`--features mlir`, GPU-eligible functions are compiled through
MLIR → LLVM for optimized native execution. The policy controls when
this compilation happens. Functions not eligible for MLIR fall through
to the Cranelift JIT regardless of the MLIR policy.

`mlir` is not a default feature, so a stock build has no tier to start. The
CLI therefore starts this one off, and `--mlir=` opts in.

### WASM policy

```bash
elle --wasm=off script.lisp         # disable WASM (default)
elle --wasm=full script.lisp        # compile everything upfront
elle --wasm=lazy script.lisp        # per-function lazy compilation
```

| Policy | CLI | Old CLI | Behavior |
|--------|-----|---------|----------|
| Off | `--wasm=off` | `--wasm=0` | WASM disabled (default) |
| Full | `--wasm=full` | `--wasm=full` | Full-module compilation |
| Lazy | `--wasm=lazy` | `--wasm=N` | Per-function lazy compilation |

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
(vm/config :jit)               # returns the JIT policy keyword
(vm/config :wasm)              # returns the WASM policy keyword
(vm/config :mlir)              # returns the MLIR policy keyword
(vm/config :max-depth)         # returns the non-tail call depth cap
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

# Change JIT policy
(vm/config-set :jit :eager)
(vm/config-set :jit :off)
(vm/config-set :jit :adaptive)

# Custom JIT policy via closure
(vm/config-set :jit
  (fn [info]
    (if (and (get info :silent) (> (get info :calls) 5))
      :jit
      :skip)))

# Change WASM policy
(vm/config-set :wasm :full)
(vm/config-set :wasm :off)

# Change MLIR policy
(vm/config-set :mlir :eager)
(vm/config-set :mlir :off)

# Change the depth cap (a positive integer)
(vm/config-set :max-depth 1000000)
```

### Custom JIT policy

When a closure is provided as the JIT policy, the VM calls it before
compiling each hot function. The closure receives a struct:

```lisp
{:name "map"
 :calls 15
 :silent true
 :captures 0
 :bytecode-size 48
 :arity 2}
```

It must return one of:
- `:jit` — compile with Cranelift
- `:wasm` — compile with WASM backend
- `:skip` — keep in interpreter

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
