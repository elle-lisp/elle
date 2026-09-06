# WASM Backend

<!-- audited: 2026-09-06 -->

The WASM backend compiles Elle programs to WebAssembly and runs them under
Wasmtime, over the same front end the bytecode VM uses.

> **Feature-gated:** The WASM backend requires `--features wasm` at build
> time. It is disabled by default to reduce binary size. Build with
> `cargo build --features wasm` to enable it.

It is an alternative to the bytecode VM, sharing that front end
(reader → expander → analyzer → HIR → LIR) and replacing everything below it.

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

A compiled function reports on **two channels, and both must be read**.
`status` says whether the function suspended. The `SignalBits` it raised
go to `SIGNAL_SLOT`, the 8 bytes at the base of linear memory. Neither
word implies the other: a function can raise `:error` and return
(`status = 0`, signal set), suspend carrying `:io` (`status > 0`, signal
on the frame), or return a value (both clear).

Tail calls use `return_call_indirect` (WASM tail-call proposal) via
`rt_prepare_tail_call`, which resolves the target and builds the
callee's env at the caller's env position.

### `rt_call` and the suspended flag

`rt_call` returns four words: `(tag: i64, payload: i64, signal: i64,
suspended: i64)`. `suspended` is the host's answer to "must my caller
park?", and the emitted code branches on it alone.

Emitted code must not derive that answer from the signal word. The
callee's signal is its own vocabulary — `:io`, `:wait`, a user bit — and
which of those suspend is the interpreter's rule, not the emitter's.
`signals::dispatch::classify` owns it, and `rt_call` applies it once so
the tiers cannot disagree.

Testing a bit instead costs correctness twice over. A suspending signal
need not carry any particular bit — `fiber/emit` of a bare `|:io|` does
not — so a bit test misses the park, captures no continuation frame, and
drops the code after the suspend; the fiber then returns its resume value.
And a host that answers "suspended" by OR-ing a bit back onto the signal
puts that bit where programs can see it: `fiber/bits` reports
`|:io :yield|` where the VM reports `|:io|`. Pinned by
[wasm-suspend-not-by-bit.lisp](../../tests/elle/wasm-suspend-not-by-bit.lisp).

A non-zero `status` likewise does not mean "parked". `(yield v)` and
`(error …)` both compile to an `Emit` terminator and both route through
`rt_yield`, so an error also returns `status > 0`. `handle_wasm_result`
reads the signal off the frame `rt_yield` pushed and classifies it, which
is what makes an uncaught error unwind rather than park.

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
[wasm-bytecode-closure-call.lisp](../../tests/elle/wasm-bytecode-closure-call.lisp)
and [wasm-collection-call.lisp](../../tests/elle/wasm-collection-call.lisp).

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
   `drive_resume_chain` ([resume.rs](../../src/wasm/resume.rs)) walks the chain.

### Cross-thread spawn (dual-compiled bytecode)

`sys/spawn`/`sys/spawn-vm` deep-copy a closure to a fresh OS-thread **bytecode**
VM and run it there — WASM functions are not callable off the main store. So the
full-module emitter *dual-compiles*: alongside the WASM body it emits ordinary
bytecode for every closure (`emit_module_closures`) and turns each into a
blueprint, stored on the host as `closure_bytecodes`. When `rt_make_closure`
builds a WASM closure value it builds the code object from that blueprint, so a
spawned worker can run `template.code()` on the VM.

The blueprint travels whole, through `TemplateProto::wasm_closure`
([region/template.md](region/template.md) § "The WASM backend is handed a
blueprint instead"). Every field of it earns the trip. The nested-lambda
blueprints are the loudest: a closure's bytecode `MakeClosure` instructions
index `child_protos`, so a code object built without them leaves that list empty
and the worker panics on its first `MakeClosure`
([closure.rs](../../src/vm/closure.rs)). The two
release tables are the quietest, and are what an abandoned frame on that worker
walks. Pinned by `wasm::tests::closure`.

### Register allocation

LIR uses SSA-style virtual registers (unlimited). The register
allocator ([regalloc.rs](../../src/wasm/regalloc.rs)) compacts them:

- Cross-block registers get dedicated WASM locals.
- Within-block registers share locals from a pool (greedy linear scan).

A WASM function declares every local up front, so the compacted count is what
the module pays. `--debug-wasm` prints each closure's `virtual regs → slots`;
over a stdlib module the widest functions come out around a third of their
virtual-register count.

## Performance

Every `--wasm=full` run prints its own `[wasm]` line — the LIR, emission,
wasmtime-compile and execute times of that program, and the size of the module.
Read the run rather than a number written down here: the tier is not on any
production workload, so a figure in this document ages with nothing to fail.

### What's fast

- Integer and float arithmetic (inline WASM i64/f64 ops)
- Comparisons and boolean logic (inline tag checks)
- Local variable access (WASM locals, no memory traffic)
- Tail calls (WASM `return_call_indirect`, no stack growth)
- Repeated runs with `--cache`, which skips the wasmtime compile

### What's slow

- Every closure call crosses the host boundary twice, through `rt_call` and a
  wasmtime trampoline, where the bytecode VM dispatches directly
- Every heap data operation (cons, car, cdr, array-ref, lbox) is a
  host call via `rt_data_op`
- A cold wasmtime compile of the stdlib module, which is megabytes of WASM

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

A **debug** build applies

```toml
[profile.dev.package."*"]
opt-level = 3
```

so every dependency — `cranelift-codegen`/`cranelift-frontend`/`regalloc2`
included — compiles optimized while `elle` itself stays at `opt-level = 0` with
full debug assertions. Without the override those crates build at `opt-level =
0`, where Cranelift's hot SSA-construction and bitset ops are un-inlined
call-per-op (profiled: `cranelift_bitset` / `SSABuilder` self-time dominates)
and one stdlib module costs seconds of wasmtime compile where the override
costs a fraction of one. (`--cache=<dir>` amortizes either way.)

`cranelift-codegen` is shared between the WASM path (via `wasmtime`) and the
**Cranelift JIT** (`cranelift-jit`), so optimizing it also speeds up JIT
compilation. The generated JIT *code* is unchanged: the JIT pins its own output
at `opt_level = "speed"` ([compiler.rs](../../src/jit/compiler.rs)) independent
of how `cranelift-codegen` was itself built, so the tier is behaviour-identical
across the override — only compile *latency* moves. A faster background compile
does shift *when* a hot function crosses from the interpreter to JIT
mid-execution, so two crossover invariants are each pinned by a test that fails
if the shift mishandles the boundary: a fuel-suspended callee's frame survives
the JIT tier ([fuel-jit-preempt.lisp](../../tests/elle/fuel-jit-preempt.lisp)),
and a value emitted from JIT-compiled code is retained as it escapes into
`fiber.signal`, where the resumer reads it
([region-jit-emit-escape-uaf.lisp](../../tests/elle/region-jit-emit-escape-uaf.lisp)).

### One Cranelift in the dependency graph

That sharing holds only while `wasmtime` and the JIT's `cranelift-*` crates
resolve to the same `cranelift-codegen`. Cargo picks one version per
semver-incompatible requirement, so a JIT held a minor line behind the
Cranelift `wasmtime` carries puts two complete copies of the code generator
into a `--features wasm` build. The copies do not stay independent either:
they share the one `regalloc2` the union of their requirements allows, so the
JIT every default build runs gets its register allocator chosen by the WASM
tier's pin.

The two pins therefore move together. `wasmtime 46` carries Cranelift 0.133,
and `Cargo.toml` pins `cranelift-codegen`, `-frontend`, `-module`, `-jit`, and
`-native` at 0.133 to match. Raising `wasmtime` means raising the JIT's
Cranelift in the same change, which is a code change and not only a manifest
one — 0.133 interns memory-operation flags per function (see
[impl/jit.md](jit.md) § "Memory flags on emitted loads").
`integration::deps` ([deps.rs](../../tests/integration/deps.rs)) reads
`Cargo.lock` and fails if the graph ever holds two versions of
`cranelift-codegen` or `regalloc2`.

## Full-module coverage and its two teardown/lowering invariants

The full-module tier runs the whole corpus under `make smoke-wasm` except
`eval.lisp`/`eval-env.lisp` (dynamic compilation is not a WASM backend feature —
`WASM_SKIP` in the Makefile). Two invariants that this tier — and only this tier —
must uphold are worth calling out, because each is invisible on the VM/JIT path
and each is pinned by a specific corpus file run under `--wasm=full`.

- **every io-backend strands to the heap's teardown.** Every region instruction
  is a structural no-op on this tier (its emitter lowers each to nothing —
  [dispatch.rs](../../src/wasm/instruction/dispatch.rs)), so a scheduler I/O
  backend — a heap `ExternalObject` (`Value::external("io-backend", …)`) whose
  `pending` map holds `Port`/`ProcessHandle` values for ops
  submitted-but-unreaped at exit (a POSIX signal waiter, a spawned-process
  waiter) — is never reclaimed during execution. Where the VM strands one
  backend, this tier strands every one.

  What handles them is not this tier's: `FiberHeap::quiesce_io_backends` drains
  each before the region sweep, on every tier, because the VM reaches the same
  state whenever a program ends without dropping its backend
  ([src/io/AGENTS.md](../../src/io/AGENTS.md) § "A hold is let go while its
  store is still there"). This tier is where it shows up on the widest range of
  programs, so it is the coverage that pins it. Canonical reference:
  [posix.lisp](../../tests/elle/posix.lisp).

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
  ([region-branch-result-loop-uaf.lisp](../../tests/elle/region-branch-result-loop-uaf.lisp)
  stays green). Canonical reference:
  [region-capture-cell-loop-uaf.lisp](../../tests/elle/region-capture-cell-loop-uaf.lisp).

## Testing

CI gates on `make check-wasm` only: the feature compiles, and the full-module
tier boots one module (the `[wasm]` marker proves the tier engaged — a
non-wasm binary accepts `--wasm=full` and silently runs the VM). The corpus
passes below do not gate CI while the tier carries no production workloads.

```bash
# Build gate: feature compiles, tier boots (the CI gate)
make check-wasm

# WASM smoke tests (all elle scripts except eval)
make smoke-wasm

# Individual test
elle --wasm=full tests/elle/arithmetic.lisp

# Tiered mode test
elle --wasm=11 tests/elle/wasm-tier.lisp

# Rust-side WASM tests — the feature is off by default, so name it
cargo test -p elle --lib --features wasm wasm::
```

## The module cache is a cache

`--cache=path` stores each compiled module as a serialized wasmtime artifact
named for a hash of the WASM bytes it was compiled from. Those bytes are the
only thing the name captures, and they are not the whole of what the artifact
depends on: `Module::deserialize` accepts an artifact only from the wasmtime
that wrote it, and refuses one written by any other version.

So a read that yields no usable module is a MISS, not an error. The compiler
falls back to compiling the WASM fresh and overwrites the entry, which repairs
the cache in place. The same fallback covers every other reason the bytes on
disk may be unusable — a truncated write, a file another tool put there, a
host whose CPU features no longer match.

The alternative is to fail the run, and it fails it for good: nothing in the
program deletes the entry, so every later run reads the same unusable bytes
and stops the same way. An upgrade would strand every user holding a warm
cache until they cleared it by hand, and the error naming a wasmtime version
gives no hint that a directory is what needs deleting. A cache that cannot
miss is not a cache.
`wasm::tests::cache::cache_entry_that_cannot_be_deserialized_recompiles` pins
the fallback on both cached paths.

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
| `--wasm-no-sparse-spill` | Spill every register at a suspend point, not the live ones |
| `--jit=0` | Disable cranelift optimization in Wasmtime |

---

## See also

- [src/wasm/AGENTS.md](../../src/wasm/AGENTS.md) — which file each part of the
  backend lives in
- [impl/lir.md](lir.md) — LIR that the WASM emitter consumes
- [impl/vm.md](vm.md) — bytecode VM (full-module WASM replaces it; tiered complements it)
- [impl/jit.md](jit.md) — Cranelift JIT alternative
- [impl/mlir.md](mlir.md) — MLIR/LLVM tier-2 backend
- [impl/gpu.md](gpu.md) — GPU compute via SPIR-V + Vulkan
