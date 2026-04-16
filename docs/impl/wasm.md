# WASM Backend

> **Feature-gated:** The WASM backend requires `--features wasm` at build
> time. It is disabled by default to reduce binary size. Build with
> `cargo build --features wasm` to enable it.

The WASM backend compiles Elle programs to WebAssembly and executes them
via Wasmtime. It is an alternative to the bytecode VM, sharing the same
front end (reader → expander → analyzer → HIR → LIR).

## Quick start

```bash
# Full-module WASM backend
elle --wasm=full script.lisp

# With disk cache (amortizes Wasmtime compilation)
elle --wasm=full --cache=/tmp/elle-wasm script.lisp

# Debug output (host call tracing)
elle --wasm=full --debug-wasm script.lisp

# Dump the generated WASM module
elle --wasm=full --wasm-dump script.lisp
# => writes /tmp/elle-wasm-dump.wasm (inspect with wasm-tools)

# Without stdlib (for testing the emitter in isolation)
elle --wasm=full --wasm-no-stdlib script.lisp

# Tiered mode: JIT individual hot closures to WASM during VM execution
elle --wasm=11 script.lisp
```

## Architecture

```text
LIR → WasmEmitter → WASM module bytes → Wasmtime → execution
```

Two execution modes:

- **Full-module** (`--wasm=full`): compiles stdlib + user code as a
  single WASM module. Replaces the bytecode VM entirely. Supports
  closures, fibers, yield, tail calls, I/O, and the async scheduler.
  Missing: `eval` (dynamic compilation).

- **Tiered** (`--wasm=N`): compiles individual hot closures
  to WASM on demand during bytecode VM execution. Complements the VM
  rather than replacing it. Currently limited to leaf functions
  (no closures, tail calls, or yield).

### Pipeline (full-module)

```text
1. Concatenate stdlib + user source (wrapped in ev/run)
2. Parse → expand → analyze → lower → LIR (compile_file_to_lir)
3. Collect nested closures from MakeClosure instructions
4. Emit each closure as a WASM function (emit_closure_function)
5. Emit entry function (emit_function)
6. Package into a WASM module with imports, table, memory
7. Compile via Wasmtime (cranelift) → native code
8. Instantiate and call __elle_entry
```

### Value representation

Elle values are 16 bytes: `(tag: u64, payload: u64)`. In WASM, each
value occupies two `i64` locals (tag + payload). Immediate values
(int, float, nil, bool, symbol, keyword) are constructed directly in
WASM. Heap values (strings, arrays, closures, etc.) live on the host
behind opaque `u64` handles — the payload is a handle index into the
host's `HandleTable`.

### Host function interface

WASM code calls the host for operations that need Rust heap access:

| Import | Purpose |
|--------|---------|
| `call_primitive` | Dispatch to one of 330+ Elle primitives |
| `rt_call` | Call a closure, NativeFn, or parameter |
| `rt_load_const` | Load a heap constant from the pool |
| `rt_data_op` | Cons, car, cdr, array ops, lbox, struct ops |
| `rt_make_closure` | Build a closure value from captures + metadata |
| `rt_push_param` / `rt_pop_param` | Dynamic parameter binding |
| `rt_prepare_tail_call` | Resolve tail call target, build callee env |
| `rt_yield` | Save suspension frame for yield/yield-through |
| `rt_get_resume_value` | Load the resume value after suspension |
| `rt_load_saved_reg` | Restore a saved register during resume |

### Closure calling convention

Closure WASM type: `(env_ptr: i32, args_ptr: i32, nargs: i32, ctx: i32) -> (tag: i64, payload: i64, status: i32)`

- `env_ptr`: byte offset into linear memory where the closure's
  environment is laid out as `[captures][params][locals]`, each slot
  16 bytes (tag + payload).
- `ctx`: resume state (0 = initial call, >0 = resuming after yield).
- `status`: 0 = normal return, >0 = suspended (resume state ID).

Tail calls use `return_call_indirect` (WASM tail-call proposal) via
`rt_prepare_tail_call`, which resolves the target and builds the
callee's env at the caller's env position.

### Suspension and resume

Yielding closures use a CPS-like scheme:

1. At each yield point and yield-through call site, the emitter
   assigns a resume state ID.
2. On yield: live registers are spilled to linear memory, then
   `rt_yield` saves them as a `WasmSuspensionFrame` on the host.
3. On resume: `ctx` parameter is non-zero. The resume prologue
   dispatches on `ctx` via `br_table`, restores saved registers,
   and jumps to the continuation block.
4. For yield-through-call (callee yields through a non-yielding
   caller), the caller's frame is saved too, forming a chain.
   `drive_resume_chain` in `resume.rs` walks the chain.

### Register allocation

LIR uses SSA-style virtual registers (unlimited). The register
allocator (`regalloc.rs`) compacts them:

- Cross-block registers get dedicated WASM locals.
- Within-block registers share locals from a pool (greedy linear scan).

This reduces WASM local counts from ~1700 to ~200 for a typical
stdlib compilation.

## Source layout

| File | Lines | Purpose |
|------|-------|---------|
| `emit.rs` | 680 | Module structure, orchestration |
| `instruction.rs` | 867 | LIR → WASM instruction translation |
| `controlflow.rs` | 280 | CFG dispatch (loop + br_table) |
| `suspend.rs` | 341 | CPS spill/restore, block splitting |
| `linker.rs` | 784 | Host function registration, data op dispatch |
| `store.rs` | 520 | Engine/Store, env preparation, module execution |
| `host.rs` | 382 | ElleHost state, handle wrappers, I/O |
| `lazy.rs` | 637 | Tiered compilation (per-closure) |
| `regalloc.rs` | 463 | Register allocation |
| `resume.rs` | 204 | Fiber resume chain |
| `mod.rs` | 189 | Entry points |
| `handle.rs` | 106 | Handle table + shared arg reading |

## Performance

Current state (fib(30), release build, cached):

```text
Bytecode VM:     54ms
WASM backend:  1092ms  (execute only, wasmtime compile cached)
```

The gap is the WASM→host→WASM boundary crossing on every closure call
(~400ns per call via `rt_call` + wasmtime trampolines, vs ~20ns for
the bytecode VM's direct dispatch).

Wasmtime compilation: ~830ms cold, ~3ms with `--cache`.
Arithmetic and comparisons are already inline WASM (no host calls).

### What's fast

- Integer and float arithmetic (inline WASM i64/f64 ops)
- Comparisons and boolean logic (inline tag checks)
- Local variable access (WASM locals, no memory traffic)
- Tail calls (WASM `return_call_indirect`, no stack growth)
- Repeated runs with disk cache (3ms compile)

### What's slow

- Every closure call crosses the host boundary twice
- Every heap data operation (cons, car, cdr, array-ref, lbox) is a
  host call via `rt_data_op`
- Module compilation is 830ms cold (2.2MB of WASM for stdlib + hello)

### Improvement path

1. **Inline closure calls**: emit `call_indirect` for calls to
   closures in the same module, preparing the env in WASM instead of
   crossing to the host. Requires tracking known closure targets at
   the LIR or emitter level.

2. **Inline data operations**: tag checks are already inline. Next:
   LBox load/store via a linear-memory side table, then cons cell
   caching for list traversal.

3. **Separate stdlib compilation**: compile stdlib as a separate WASM
   module, cached independently. Link user code against it.

## Testing

```bash
# WASM smoke tests (all elle scripts except eval)
make smoke-wasm

# Individual test
elle --wasm=full tests/elle/arithmetic.lisp

# Tiered mode test
elle --wasm=11 tests/elle/wasm-tier.lisp

# Rust-side WASM tests
cargo test wasm
```

## CLI flags

| Flag | Effect |
|------|--------|
| `--wasm=full` | Full-module WASM backend |
| `--wasm=N` | Tiered WASM compilation (threshold N-1) |
| `--cache=path` | Disk cache for compiled WASM modules |
| `--debug-wasm` | Print host call traces to stderr |
| `--wasm-dump` | Write WASM bytes to `/tmp/elle-wasm-dump.wasm` |
| `--wasm-lir` | Print LIR before WASM emission |
| `--wasm-no-stdlib` | Skip stdlib (for emitter testing) |
| `--jit=0` | Disable cranelift optimization in Wasmtime |

---

## See also

- [impl/lir.md](lir.md) — LIR that the WASM emitter consumes
- [impl/vm.md](vm.md) — bytecode VM (full-module WASM replaces it; tiered complements it)
- [impl/jit.md](jit.md) — Cranelift JIT alternative
- [impl/mlir.md](mlir.md) — MLIR/LLVM tier-2 backend
- [impl/gpu.md](gpu.md) — GPU compute via SPIR-V + Vulkan
