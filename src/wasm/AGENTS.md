# WASM Backend

LIR → WASM emission via `wasm-encoder`, execution via Wasmtime.

## Architecture

```
LIR → WasmEmitter (emit.rs) → .wasm bytes + const_pool
                                    ↓
                              Wasmtime Engine/Store/Linker (store.rs)
                                    ↓
                              Host functions (store.rs, host.rs)
                                    ↓
                              HandleTable (handle.rs) ← heap objects live here
```

### Key design: host-side handles

WASM code sees Values as `(tag: i64, payload: i64)`. Immediates (int, float,
nil, bool, symbol, keyword) are constructed in WASM. Heap values (strings,
arrays, closures, etc.) have payload = opaque handle index into `HandleTable`
on the host. All heap operations cross the WASM-host boundary via host
function calls.

### Control flow: loop + br_table

Multi-block functions use a state machine: `loop { block*N { br_table } }`.
Each LIR basic block is a case. Jump/Branch set state and `br` to loop.
Return uses WASM `return`. Single-block functions skip the loop.

This was chosen over recursive structured-if emission because `cond`
generates deeply nested if/else patterns where merge-point analysis breaks.
The loop+br_table approach handles any CFG topology.

### Closures

Each `MakeClosure` in LIR produces a separate WASM function in a `funcref`
table. `rt_make_closure` creates a `Closure` value with `wasm_func_idx`.
`rt_call` dispatches closure calls by building env in linear memory
(captures + params + local slots) and invoking via table lookup.

Closure functions have type `(env_ptr: i32, args_ptr: i32, nargs: i32,
ctx: i32) -> (tag: i64, payload: i64)`. The env pointer points to linear
memory where all variable access happens via `LoadCapture`/`StoreCapture`.

## Files

| File | Purpose |
|------|---------|
| `emit.rs` | LIR → WASM emission. `emit_module()` is the entry point. |
| `handle.rs` | `HandleTable`: maps u64 handles to `Rc<HeapObject>`. |
| `host.rs` | `ElleHost` state (handle table + primitive dispatch + const pool). |
| `store.rs` | Wasmtime setup, host function registration, `call_wasm_closure`, `dispatch_data_op`. |
| `mod.rs` | `eval_wasm()` entry point. |

## Host functions (WASM imports)

| Import | Purpose |
|--------|---------|
| `call_primitive` | Dispatch by prim_id (unused currently, rt_call covers this) |
| `rt_call` | Dynamic function call: NativeFn or WASM closure dispatch |
| `rt_load_const` | Load heap constant from const_pool by index |
| `rt_data_op` | Data operations (cons, car, cdr, arrays, lbox, etc.) by opcode |
| `rt_make_closure` | Create Closure value with wasm_func_idx + captures |

## Linear memory layout

| Region | Offset | Purpose |
|--------|--------|---------|
| Args buffer | 256 (ARGS_BASE) | Call args + data op args |
| Env buffer | 4096 (ENV_BASE) | Closure env for `call_wasm_closure` |

## WASM local layout

Entry function: `[tags: i64 * N] [payloads: i64 * N] [env_ptr: i32] [signal/state: i32]`

Closure function (params 0-3): `[tags: i64 * N] [payloads: i64 * N] [signal/state: i32]`
- Param 0 = env_ptr, Param 1 = args_ptr (unused), Param 2 = nargs (unused), Param 3 = ctx

Register mapping: `tag_local(Reg(i)) = offset + i`, `pay_local(Reg(i)) = offset + N + i`
where offset = 0 for entry, 4 for closures.

## What works (37 tests)

- All LIR instructions except Eval, PushParamFrame, PopParamFrame, CheckSignalBound, StructRest
- Constants (int, float, bool, nil, empty_list, symbol, keyword, string)
- Arithmetic, comparisons, bitwise, unary
- Control flow: if/else (nested, cond), let*, defn, letrec
- All 331 primitives via rt_call
- Closures: creation, calling, capture, higher-order, recursion, mutual recursion
- Data: cons, car, cdr, arrays, structs, destructuring, lbox
- Strings via constant pool
- Signal propagation (error early-return after host calls)
- stdlib.lisp compiles to 638KB WASM and runs

## Known issues

### Stdlib export initialization (the one blocker)

stdlib.lisp compiles and runs, but calling the returned exports closure
fails with "rt_call: cannot call integer". The issue: letrec forward
references use LBox cells. When `call_wasm_closure` builds the env in
linear memory, it copies the Closure's `env` (captured values) directly.
But for letrec, these captures include LBox cells that were initialized
*after* the closure was created — the LBox cell has been updated on the
host side, but `call_wasm_closure` copies the env from `closure.env`
which still holds the original LBox handle.

The fix: `call_wasm_closure` should write the LBox *handle* to linear
memory (not the dereferenced value), and `LoadCapture` in WASM already
auto-unwraps LBox via `rt_data_op(OP_LOAD_LBOX)`. The issue is that
`call_wasm_closure` currently writes captures using `value_to_wasm()`
which gives the handle for the LBox itself — but the WASM LoadCapture
code only unwraps if the tag is `TAG_LBOX`. Need to verify the tag is
correctly written.

**To debug:** dump the env contents in `call_wasm_closure` for the
stdlib exports closure call. Check whether LBox cells are being written
with their TAG_LBOX tag or if they're being dereferenced prematurely.

### Missing features (not blockers for Phase 1)

- `Eval` — needs dynamic module compilation (Phase 3)
- `PushParamFrame`/`PopParamFrame` — dynamic parameters (needed by some stdlib)
- `CheckSignalBound` — compile-time signal validation
- `StructRest` — `{:a a & rest}` patterns
- Tail calls — emitted as regular calls (stack overflow on deep recursion)
- `CallArrayMut` nargs=-1 protocol — not implemented in rt_call yet
