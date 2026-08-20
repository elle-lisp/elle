# JIT

The JIT compiles hot functions from LIR to native code using Cranelift.

## Architecture

```text
LIR → FunctionTranslator → Cranelift IR → Native code → JitCode
```

## Key types

- **`JitCompiler`** — manages the Cranelift `JITModule`, declares
  runtime helper symbols, tracks compilation stats
- **`FunctionTranslator`** — walks LIR basic blocks and instructions,
  emitting Cranelift IR
- **`JitCode`** — wraps the native function pointer; keeps the module
  alive for the code's lifetime
- **`RuntimeHelpers`** — extern symbols the JIT calls back into the
  VM (allocation, GC barriers, signal checks)

## Function selection

Functions become JIT candidates based on a hotness threshold
(default 10, controlled by `--jit=N`). The VM increments a counter on
each call; when it crosses the threshold, the function is compiled.

## Rejection tracking

Not all functions can be JIT-compiled. The JIT rejects functions that:

- Use features not yet implemented in the translator
- Fail Cranelift verification

**Negative-cache invariant.** A function whose compilation is rejected is
recorded in `jit_rejections` and **never re-submitted**: every subsequent
call falls through to the interpreter directly. The rejection is keyed by the
function's bytecode pointer, which uniquely identifies its (immutable) LIR, so
a re-submission could only ever reproduce the identical rejection — it is pure
wasted work.

This invariant is load-bearing under eager JIT. With `--jit=eager` the hotness
threshold is 0, so *every* call is "hot"; absent the negative cache, each call
to an un-jit'able function re-submits it to the background worker. A single
un-jit'able function called in a hot loop (e.g. stdlib `-`/`/`, which build a
rest-arg closure → `MakeClosure` rejection) then saturates the JIT worker
thread, re-compiling the same function thousands of times and burning CPU that
dwarfs the program's real work. The `jit/rejections` report exposes a per-
function `:attempts` count; the negative cache holds `attempts == 1` no matter
how many times the function is called.

## The code-address registry

Native samplers (`/usr/bin/sample`, `eu-stack`) cannot name JIT frames: the
code lives in anonymous Cranelift mappings, so a wedged thread's stack shows
`??? (in <unknown binary>)` exactly where the answer is. The registry closes
that gap. Every successful compile — solo and batch, on every thread — records
`(entry address, label)` in one process-global table (`src/jit/registry.rs`).
The label is the function's declared name when one exists, else its
smallest-offset source location (`ClosureTemplate::display_label`) — lowering
names almost nothing, so the location is what actually identifies a function
to a reader. The table only grows; entries are never removed,
because a stack captured at any time may reference code whose `JitCode` has
since been dropped.

`(vm/query "jit/map" nil)` renders the table as one `0x<addr> <name>` line per
entry, sorted by address. The test runner prints it after the thread
photograph when a form misses its deadline (`src/test.lisp`,
`note-timeout-stacks`), so a sampled JIT frame resolves to the nearest
preceding entry — the registry records entry addresses, not sizes, and
Cranelift lays functions out contiguously enough for nearest-preceding to
name the frame.

## Yield-through-call

For functions that call other functions which might yield, the JIT
collects yield-site metadata during LIR emission. This enables proper
save/restore sequences so a yielded fiber can resume into JIT code.

## CLI flags

```text
--jit=0       Disable JIT entirely
--jit=N       Compile after N-1 calls (default: --jit=11, threshold 10)
--jit=1       Compile on first call
--stats       Print compilation stats on exit
```

## Files

```text
src/jit/compiler.rs    JitCompiler, module management
src/jit/translate.rs   FunctionTranslator, LIR → Cranelift IR
src/jit/code.rs        JitCode wrapper
src/jit/vtable.rs      Runtime helper dispatch table
src/jit/dispatch.rs    JIT dispatch integration with VM
```

---

## See also

- [impl/lir.md](lir.md) — LIR that the JIT translates
- [impl/vm.md](vm.md) — VM fallback and dispatch
- [impl/bytecode.md](bytecode.md) — bytecode alternative
- [impl/mlir.md](mlir.md) — MLIR tier-2 path consulted before Cranelift
- [impl/wasm.md](wasm.md) — WebAssembly backend
- [impl/gpu.md](gpu.md) — GPU compute via SPIR-V + Vulkan
- [impl/differential.md](differential.md) — cross-tier agreement testing
