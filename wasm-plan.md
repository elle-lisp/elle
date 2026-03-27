# WASM Backend Plan

## What exists

Branch `wasm-backend`, worktree `~/git/tmp/elle-wasm`. 10 commits, 40 tests.

```
e4315e15 Add WASM backend AGENTS.md for future context
23d5e173 WASM backend: loop+br_table CFG, stdlib compiles and runs
05ec2d84 WASM backend: stdlib compilation, cond codegen bug identified
3a2825c5 WASM backend: file-mode compilation, recursion
b44d132c WASM backend: strings, signal propagation
558a4217 WASM backend: MakeClosure + closure calls
b4a50087 WASM backend: LoadCapture/StoreCapture/LoadCaptureRaw
af162201 WASM backend: data operations via rt_data_op
0e6f3fbc WASM backend: primitive function calls
2f9e6521 WASM backend: Phase 0 + control flow
```

### What works

- **Emitter** (`src/wasm/emit.rs`): LIR → WASM via `wasm-encoder`. Constants,
  arithmetic, comparisons, unary ops, type checks, control flow (loop+br_table),
  closures (funcref table), captures, data ops, strings. Handles any CFG.

- **Host** (`src/wasm/store.rs`, `host.rs`): Wasmtime Engine/Store/Linker.
  Five host functions: `rt_call` (dynamic dispatch), `rt_load_const` (constant
  pool), `rt_data_op` (20 opcodes for cons/car/cdr/arrays/lbox/structs),
  `rt_make_closure`, `call_primitive`.

- **Handle table** (`src/wasm/handle.rs`): u64 → Value mapping for heap objects.

- **Env stack** (`host.rs:env_stack_ptr`): Stack allocator for closure envs in
  linear memory. Each `call_wasm_closure` bumps forward, restores on return.
  Handles nested/recursive closure calls without overwriting.

- **stdlib.lisp**: 1553 lines compile to 638KB WASM (2139 constants, 88+
  closures). Compiles, validates, runs on Wasmtime. Stdlib exports callable
  via `eval_wasm_with_stdlib`.

- **Pipeline**: `compile_to_lir` (single expression) and `compile_file_to_lir`
  (letrec mode) in `src/pipeline/compile.rs`. `eval_wasm` and
  `eval_wasm_with_stdlib` in `src/wasm/mod.rs`.

### Architecture (settled)

| Decision | Choice | Why |
|----------|--------|-----|
| Heap objects | Host-side behind opaque u64 handles | Avoids serialization on every FFI/primitive boundary |
| Control flow | loop + br_table dispatch | Handles any CFG; structured-if broke on cond patterns |
| Closures | funcref table + host-side env building | Host builds env (captures+params+locals) in linear memory |
| Env allocation | Stack allocator in linear memory | Prevents nested calls from overwriting each other |
| Value encoding | Two i64 (tag, payload) | Identical to existing Value repr |

Read `src/wasm/AGENTS.md` for full technical reference.

---

## Remaining work

### Phase 1 completion: `ELLE_WASM=1 make smoke`

#### 1. Wire `ELLE_WASM=1` into main.rs

Route file execution through `eval_wasm_with_stdlib` when the env var is set.
The stdlib is compiled together with user source as a single letrec unit
(because stdlib closures are WASM closures, not bytecode — can't mix backends).

**Files**: `src/main.rs`

#### 2. Stub remaining LIR instructions

These are no-ops or panics today. Need at minimum stubs that don't break
programs that don't use them:

| Instruction | What it needs | Urgency |
|-------------|---------------|---------|
| `PushParamFrame`/`PopParamFrame` | Host function for dynamic parameter bindings | Medium — `parameterize` used in stdlib |
| `StructRest` | Host data_op for `{:a a & rest}` patterns | Low — rare in examples |
| `CheckSignalBound` | Host check + error signal | Low — only `silence` |
| `CallArrayMut` nargs=-1 | Unpack array in rt_call | Medium — splice uses this |

#### 3. Tail calls

Currently emitted as regular `Call` + `Return`. Deep recursion will stack
overflow. Two options:

- **WASM tail call proposal** (`return_call_indirect`): standardized, already
  enabled in our Engine config. Requires knowing the callee's type at emit time
  — only works for self-calls or calls to known-type functions.

- **Host-side trampoline**: return a "tail call me" sentinel from the WASM
  function, host loops. Simpler but slower.

For Phase 1, the trampoline is easier. Self-tail-call optimization (same
closure, reset env) handles the common case.

#### 4. Test against examples

Run each `examples/*.lisp` and `tests/elle/*.lisp` that doesn't use
yield/IO/fibers. Catalog failures, fix what's fixable. The goal isn't 100%
pass rate — it's enough to validate the architecture.

---

### Phase 2: Yield via Wasmtime stack switching

#### 1. Enable stack switching

Wasmtime's `cont.new`, `suspend`, `resume` (experimental, Phase 3 in W3C).
Need to check the current Wasmtime 43 API for stack switching support.

If not yet in Wasmtime 43: fall back to the state machine transform. The
LIR already carries `yield_points` and `call_sites` metadata for this.

#### 2. Emit `suspend` for Yield terminators

```wasm
;; Yield value to parent
(suspend $elle_yield)
;; Resume value is on the stack
```

`LoadResumeValue` becomes a no-op — the resume value is already on the
WASM stack after `resume`.

#### 3. WasmFiber + scheduler

```rust
struct WasmFiber {
    continuation: Option<Continuation>,
    handle_table: HandleTable,  // or shared with parent
    signal: Option<(SignalBits, Value)>,
    status: FiberStatus,
    mask: SignalBits,
    param_frames: Vec<Vec<(u32, Value)>>,
}
```

Replace the VM's `execute_scheduled` / `ev/run` loop with a host-side
scheduler that resumes continuations.

#### 4. Signal routing

When a primitive returns SIG_IO / SIG_YIELD, the host intercepts before
returning to WASM. The fiber suspends, scheduler dispatches the I/O
request, resumes on completion.

#### 5. Fiber primitives

`fiber/new`, `fiber/resume`, `fiber/abort`, `fiber/cancel` — host
functions that create/manage WasmFibers.

**Milestone**: yielding generators, coroutines, I/O scheduling work.

---

### Phase 3: Full runtime

- I/O backend integration (reuse existing `IoBackend` trait)
- `eval` via dynamic module instantiation in same Wasmtime store
- Module import/loading
- REPL support

**Milestone**: `ELLE_WASM=1 make test` passes.

---

### Phase 4: Plugins + cleanup

- Define WIT interfaces for plugin API
- Port `elle-base64` as proof-of-concept WASM component
- Component linking in Wasmtime

Delete old backend:

| Path | Lines | What |
|------|-------|------|
| `src/vm/` | ~7,000 | Dispatch loop, fiber swap, call, signal, env |
| `src/jit/` | ~8,000 | Cranelift JIT compiler |
| `src/lir/emit/` | ~1,300 | Bytecode emitter |
| `src/compiler/bytecode.rs` | ~630 | Bytecode instruction set |

Replace `ClosureTemplate.bytecode` with `ClosureTemplate.wasm_func_idx: u32`.
Remove `jit_code`, `lir_function` fields.

**Milestone**: `make test` passes, ~17k lines deleted.

---

## Key files

| File | What | Size |
|------|------|------|
| `src/wasm/emit.rs` | LIR → WASM emitter | Core of the backend |
| `src/wasm/store.rs` | Host functions, call_wasm_closure, dispatch_data_op | Largest file |
| `src/wasm/host.rs` | ElleHost state, handle table, primitive table | Config |
| `src/wasm/handle.rs` | HandleTable: u64 → Value | Small |
| `src/wasm/mod.rs` | eval_wasm entry points | Small |
| `src/pipeline/compile.rs` | compile_to_lir, compile_file_to_lir | Integration |
| `src/value/closure.rs` | ClosureTemplate.wasm_func_idx | One field added |
| `tests/wasm_smoke.rs` | 40 tests | Regression suite |
| `tests/wasm_stdlib.rs` | stdlib compilation + execution tests | Integration |
