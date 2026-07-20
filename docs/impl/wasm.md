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
elle --wasm=full --cache=target/elle-wasm script.lisp

# Debug output (host call tracing)
elle --wasm=full --debug-wasm script.lisp

# Dump the generated WASM module
elle --wasm=full --wasm-dump script.lisp
# => writes /dev/shm/elle-wasm-dump.wasm (inspect with wasm-tools)

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
1. Concatenate stdlib + user source (build_full_source)
2. Parse → expand → analyze → lower → LIR (compile_file_to_lir)
3. Collect nested closures from MakeClosure instructions
4. Emit each closure as a WASM function (emit_closure_function)
5. Emit entry function (emit_function)
6. Package into a WASM module with imports, table, memory
7. Compile via Wasmtime (cranelift) → native code
8. Instantiate and call __elle_entry
```

In step 1 the user code is not wrapped whole in a single `(ev/run (fn [] …))`.
That naive wrap makes every user top-level `def` a *fn-body* binding, where
redefinition (`(def a 10) (def a (+ a 1))`) is a duplicate-binding error — yet a
file's top level uses sequential shadowing, so the VM accepts it. To match the
VM, `build_scheduled_toplevel` keeps user **definitions** at the file-letrec top
level and wraps only runs of consecutive **expressions** in `(ev/run (fn [] …))`,
preserving order so a spawn and its join stay in one scheduler session. This is
the source-level analogue of the VM's `execute_scheduled`, which wraps the
scheduler around already-top-level-analyzed bytecode.

Splicing stdlib as *source* (step 1) makes stdlib callable from WASM at
**runtime** — but it does nothing for **compile time**. Macro expansion in
step 2 runs on the compile context's macro VM, and a prelude or user macro's
transformer body may call a stdlib function while expanding (`assert`'s
transformer calls `pair?` to detect a comparison form). So the full-module
path also loads stdlib into the compile context (`init_stdlib` in
`eval_wasm_raw`) on a heap shared with the macro VM, exactly as the bytecode
runtime does. Without it, expansion fails with `undefined variable: pair?`
before any WASM is emitted. The two mechanisms are independent: the
source-splice serves runtime calls, the `init_stdlib` load serves macro
expansion. User references still bind to the spliced letrec definitions
(lexical scope shadows the registered exports), so the emitted WASM calls the
compiled stdlib.

A quoted **symbol** inside a *compound* literal (`(quote (= a 2))`, which
`assert`'s comparison branch emits) is baked into the const pool at emit time,
which must intern each symbol into the driving instance's table. The emitter
threads that table (`WasmEmitter::symbols`) on every full-module path; the
standalone/tiered path has no instance table and refuses such closures via
`standalone_emittable` instead.

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

### Callee dispatch spectrum

`rt_call` and `rt_prepare_tail_call` resolve the target value and dispatch
on its runtime type, mirroring the interpreter's `call_inner` /
`tail_call_inner` so every callee shape behaves identically across tiers:

- **Compiled closure** (`wasm_func_idx` set) — the common case; run in the
  module's function table (or a pre-compiled per-closure `Module`).
- **NativeFn** / **parameter** — dispatched host-side directly.
- **Bytecode closure** (`wasm_func_idx == None`) — `core.lisp`, the prelude,
  and any closure the module never compiled run via the host VM
  (`run_bytecode_closure`); only stdlib is compiled into the full module.
- **Callable collection** — a struct/array/set/string/bytes applied as a
  function (`(struct :k)`, `(arr i)`, `(set x)`) indexes the collection via
  the shared `call_collection` path (`run_collection_call`). The async
  scheduler's request dispatch relies on this: `handle-wait` reads
  `(request :op)` / `(request :fiber)` off a struct request.

The last two are the host-VM fallbacks: without them a call reaching a
bytecode closure or a collection-as-function raises a `cannot call …` type
error that terminates the compiled entry. Pinned by
`tests/elle/wasm-bytecode-closure-call.lisp` and
`tests/elle/wasm-collection-call.lisp`.

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

### Cross-thread spawn (dual-compiled bytecode)

`sys/spawn`/`sys/spawn-vm` deep-copy a closure to a fresh OS-thread **bytecode**
VM and run it there — WASM functions are not callable off the main store. So the
full-module emitter *dual-compiles*: alongside the WASM body it emits ordinary
bytecode for every closure (`emit_module_closures`), stored on the host as
`closure_bytecodes` (instructions + constants + **child prototypes**). When
`rt_make_closure` builds a WASM closure value it stitches this bytecode into the
`ClosureTemplate`, so a spawned worker can run `template.code()` on the VM.

The child prototypes are essential: a closure's bytecode `MakeClosure`
instructions index the template's `child_protos` (the nested-lambda blueprints).
Reconstructing the template without them leaves that list empty and the worker
panics on its first `MakeClosure` (`src/vm/closure.rs`). Pinned by
`wasm::tests::wasm_full_spawn_*`.

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

### Debug builds optimize dependencies

The `830ms cold` figure is a release build. A **debug** build applies

```toml
[profile.dev.package."*"]
opt-level = 3
```

so every dependency — `cranelift-codegen`/`cranelift-frontend`/`regalloc2`
included — compiles optimized while `elle` itself stays at `opt-level = 0` with
full debug assertions. Without the override those crates build at `opt-level =
0`, where Cranelift's hot SSA-construction and bitset ops are un-inlined
call-per-op and a single stdlib module takes **~10s** to Wasmtime-compile
(profiled: `cranelift_bitset` / `SSABuilder` self-time dominates); with it the
cold compile drops to **~0.016s**. (`--cache=<dir>` amortizes either way.)

`cranelift-codegen` is shared between the WASM path (via `wasmtime`) and the
**Cranelift JIT** (`cranelift-jit`), so optimizing it also speeds up JIT
compilation. The generated JIT *code* is unchanged: the JIT pins its own output
at `opt_level = "speed"` (`src/jit/compiler.rs`) independent of how
`cranelift-codegen` was itself built, so the tier is behaviour-identical across
the override — only compile *latency* moves. A faster background compile does
shift *when* a hot function crosses from the interpreter to JIT mid-execution,
so two crossover invariants are each pinned by a test that fails if the shift
mishandles the boundary: a fuel-suspended callee's frame survives the JIT tier
(`tests/elle/fuel-jit-preempt.lisp`), and a value emitted from JIT-compiled code
is retained as it escapes into `fiber.signal`, where the resumer reads it
(`tests/elle/region-jit-emit-escape-uaf.lisp`).

## Full-module coverage and its two teardown/lowering invariants

The full-module tier runs the whole corpus under `make smoke-wasm` except
`eval.lisp`/`eval-env.lisp` (dynamic compilation is not a WASM backend feature —
`WASM_SKIP` in the Makefile). Two invariants that this tier — and only this tier —
must uphold are worth calling out, because each is invisible on the VM/JIT path
and each is pinned by a specific corpus file run under `--wasm=full`.

- **io-backend externals are quiesced before the heap's teardown free-sweep.**
  Every region instruction is a structural no-op on this tier (§ "The
  backend-realization frontier" in memory.md), so a scheduler I/O backend — a
  heap `ExternalObject` (`Value::external("io-backend", …)`) whose `pending` map
  holds `Port`/`ProcessHandle` values for ops submitted-but-unreaped at exit (a
  POSIX signal waiter, a spawned-process waiter) — is never reclaimed during
  execution. It strands to `RegionStore::teardown_all`, which frees regions in id
  order, not lifetime order. If the backend's `Drop` ran the cancel-and-drain
  there, `drain_cqes` → `complete_port_op` would dereference a `Port` an earlier
  region in the same sweep already freed. So `eval_wasm_raw` drains every
  io-backend (`FiberHeap::collect_external_data("io-backend")` →
  `IoBackend::quiesce`) after execution returns and *before* the heap drops, when
  every value is still live; the backend's own `Drop` then finds nothing pending.
  The VM never hits this — its live region reclamation drops the backend while its
  `Port`s are still valid. Canonical reference: `tests/elle/posix.lisp`.

- **a fn-local reassigned mutable binding's slot is never value-route decref'd +
  nil-stamped.** `allocate_slot` gives such a binding its own never-reused stack
  slot, holding a live value for the binding's whole scope. The region analysis
  can still keep a spurious assign-value region for an immediate-valued counter
  (`(assign ii (%add ii 1))`) whose `decref_point` lands inside the loop; the
  lowerer's value-route release would nil-stamp that slot before the increment
  reads it, and the emitter's inline `BinOp Add` would read `Nil` as 0, sticking
  the counter so the loop never terminates. `emit_decrefs_for` refuses the value
  route for any slot in `reassigned_local_slots` (fed from
  `RegionInfo::reassigned_local_bindings`), an over-keep, never a mid-scope
  nil-stamp. This is a lowering the VM/JIT (bytecode-derived LIR) never take. The
  branch-result-loop nil-stamp guard is untouched — the suppression keys on the
  slot's binding, not the branch-union region
  (`tests/elle/region-branch-result-loop-uaf.lisp` stays green). Canonical
  reference: `tests/elle/region-capture-cell-loop-uaf.lisp`.

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
| `--wasm-dump` | Write WASM bytes to `/dev/shm/elle-wasm-dump.wasm` |
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
