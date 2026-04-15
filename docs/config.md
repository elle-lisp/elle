# Runtime Configuration (`vm/config`)

Elle exposes a unified runtime configuration system accessible from both
CLI flags and Elle code. All debug/trace flags, JIT policies, and WASM
policies are controlled through a single mutable config struct on the VM.

## CLI flags

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
| `:gc` | Garbage collection / arena operations |
| `:import` | Module import resolution |
| `:macro` | Macro expansion |
| `:wasm` | WASM backend: host calls, compilation |
| `:capture` | Capture analysis decisions |
| `:arena` | Heap allocation, region enter/exit |
| `:escape` | Escape analysis decisions |
| `:bytecode` | Bytecode dump before execution |

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

## Elle API

### Reading configuration

```lisp
(vm/config)                    # returns the full config struct
(vm/config :trace)             # returns the current trace keyword set
(vm/config :jit)               # returns the JIT policy keyword
(vm/config :wasm)              # returns the WASM policy keyword
```

### Setting configuration

```lisp
# Enable trace keywords (takes effect immediately)
(put (vm/config) :trace |:call :signal|)

# Change JIT policy
(put (vm/config) :jit :eager)
(put (vm/config) :jit :off)
(put (vm/config) :jit :adaptive)

# Custom JIT policy via closure
(put (vm/config) :jit
  (fn [info]
    (if (and (get info :silent) (> (get info :calls) 5))
      :jit
      :skip)))

# Change WASM policy
(put (vm/config) :wasm :full)
(put (vm/config) :wasm :off)
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
- `:mlir` — MLIR compilation
- `:gpu` — GPU offloading

## Implementation

`RuntimeConfig` is stored on the VM struct (not in a global static).
This allows per-fiber or per-test configuration without global state.

The `vm/config` primitive uses SIG_QUERY to read/write the VM's
RuntimeConfig. Changes take effect immediately — no restart needed.

For hot paths (VM dispatch loop), trace keywords are mirrored in a
`trace_bits: u32` bitfield to avoid HashSet lookups on every instruction.
