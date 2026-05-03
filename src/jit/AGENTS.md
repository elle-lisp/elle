# jit

JIT compilation for Elle using Cranelift.

## Responsibility

Compile `LirFunction` to native x86_64 code. Non-polymorphic functions
are JIT candidates (see `signals/AGENTS.md` for signal definitions).
Yielding functions use side-exit: JIT code calls a runtime helper that
builds a `SuspendedFrame` and returns `YIELD_SENTINEL` to the interpreter.

## Architecture

```
LirFunction -> JitCompiler -> Cranelift IR -> Native code -> JitCode
```

### Background compilation

JIT compilation runs on a dedicated background thread (`elle-jit`).
When a function becomes hot (called `jit_hotness_threshold` times,
default 10), its LIR is cloned, stripped of non-Send fields (`syntax`,
`doc`), and sent to the worker via `crossbeam_channel`. The interpreter
continues running the function while Cranelift compiles it.

The worker thread has a persistent `FiberHeap` (installed once, never
freed) so `translate_const` can allocate String/Keyword/Symbol Values
for constants embedded in native code.

On every call to `try_jit_call`, the VM polls for completed
compilations via non-blocking `try_recv()`. Compiled code is inserted
into `jit_cache`; rejections are recorded in `jit_rejections`.

Diagnostics (`jit/rejections`, `--stats`) call `drain_jit_pending()`
to block until all pending compilations finish before reporting.

## Interface

| Type | Purpose |
|------|---------|
| `JitCompiler` | Translates LIR to native code via Cranelift |
| `JitCode` | Wrapper for native function pointer + module lifetime + yield metadata |
| `JitError` | Compilation errors |
| `BatchMember` | A member of an SCC compilation group (SymbolId + LirFunction) |
| `YieldPointMeta` | Metadata for a yield point: resume IP and spilled register count |
| `YIELD_SENTINEL` | Sentinel value indicating JIT function yielded (side-exited) |
| `discover_compilation_group` | Discover call peers for batch JIT compilation |
| `JitWorker` | Background compilation thread with persistent `FiberHeap` |
| `JitTask` | Compilation request (cloned LIR + cache key) |
| `JitResult` | Compilation result (JitCode or JitError) |

## Calling Convention

JIT-compiled functions use this calling convention:

```rust
type JitFn = unsafe extern "C" fn(
    env: *const Value,      // closure environment (captures array)
    args: *const Value,     // arguments array
    nargs: u32,             // number of arguments
    vm: *mut VM,            // pointer to VM (for function calls, fiber access)
    self_bits: u64,         // tag+payload bits of the closure (for self-tail-call detection)
) -> Value;
```

Values are 16-byte tagged unions (see `value/repr/AGENTS.md`).

The 5th parameter `self_bits` enables self-tail-call optimization: when a
function tail-calls itself, the JIT compares the callee against `self_bits`.
If equal, it updates the arg variables and jumps to the loop header instead
of calling `elle_jit_tail_call`. This turns self-recursive tail calls into
native loops.

**Return values:**
- Normal return: tagged-union `Value`
- Tail call: `TAIL_CALL_SENTINEL` (0xDEAD_BEEF_DEAD_BEEFu64)
- Yield: `YIELD_SENTINEL` (0xDEAD_CAFE_DEAD_CAFEu64) — `fiber.signal` and `fiber.suspended` are set by the yield helper

## JIT Phases

The JIT was built incrementally:

| Phase | Scope |
|-------|-------|
| Phase 1 | Constants, arithmetic, comparison, variables, terminators. Capture-free functions only. |
| Phase 2 | Closures with captures: `LoadCapture`, `LoadCaptureRaw`, `StoreCapture`. |
| Phase 3 | Data structures (`Cons`, `Car`, `Cdr`, `MakeVector`, `IsPair`), lboxes (`MakeCaptureCell`, `LoadCaptureCell`, `StoreCaptureCell`), function calls (`Call`, `TailCall`). VM pointer parameter added for call dispatch. |
| Phase 4 | Self-tail-call optimization, JIT-to-JIT calling, batch compilation, `ValueConst`. |

## Phase 4 Scope (Current)

Supported instructions:
- **Constants**: `Const` (Int, Float, Bool, Nil, EmptyList, Symbol, Keyword), `ValueConst`
- **Arithmetic**: `BinOp` (inline integer fast path, extern fallback), `UnaryOp` (Not fully inlined, Neg/BitNot inline integer fast path)
- **Comparison**: `Compare` (inline integer fast path, extern fallback)
- **Variables**: `Move`, `Dup`, `LoadLocal`, `StoreLocal` (via `local_slot_to_var`), `LoadCapture`, `LoadCaptureRaw`
- **Data structures**: `Cons`, `Car`, `Cdr`, `MakeVector`, `IsPair`
- **LBoxes**: `MakeCaptureCell`, `LoadCaptureCell`, `StoreCaptureCell`, `StoreCapture`
- **Globals**: Accessed as depth-0 upvalues via `LoadCapture`/`LoadCaptureRaw`; `LoadGlobal`/`StoreGlobal` are dead instructions (unreachable in VM dispatch)
- **Function calls**: `Call`, `TailCall` (self-calls become native loops; non-self calls use `elle_jit_tail_call` trampoline)
- **Terminators**: `Return`, `Jump`, `Branch`

Unsupported (returns JitError::UnsupportedInstruction):
- `MakeClosure` — rare in hot loops, deferred
- Variadic functions with `Struct`/`StrictStruct` varargs — need fiber for keyword error reporting

Supported in yielding functions (via side-exit):
- `LoadResumeValue` — emitted as dead code (unreachable in JIT, resume goes through interpreter)
- `Yield` — emitted as side-exit: spill registers, call `elle_jit_yield`, return `YIELD_SENTINEL`

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | ~90 | Public API, `JitError` type |
| `compiler.rs` | ~1046 | `JitCompiler`, compilation entry point (`new`, `compile`, `compile_batch`, `translate_function`) |
| `vtable.rs` | ~412 | `RuntimeHelpers` struct + `register_symbols` + `declare_helpers` — the JIT vtable |
| `translate.rs` | ~1155 | `FunctionTranslator`, LIR instruction translation |
| `runtime.rs` | ~460 | Arithmetic, comparison, type-checking helpers |
| `dispatch.rs` | ~285 | Re-exports from `calls`, `data`, `suspend`; array push/extend, param frame, struct helpers, signal bound check |
| `calls.rs` | ~758 | Sentinels, `YieldPointMeta`/`CallSiteMeta`, `elle_jit_call`, `elle_jit_tail_call`, call-related helpers, env building |
| `data.rs` | ~422 | Data structure ops: cons/car/cdr, array, lbox, captures |
| `suspend.rs` | ~511 | Yield side-exit helpers: `elle_jit_yield`, `elle_jit_yield_through_call`, `elle_jit_has_signal` |
| `code.rs` | ~163 | `JitCode` wrapper type |
| `fastpath.rs` | ~250 | Inline integer fast paths for arithmetic/comparison |
| `group.rs` | ~590 | Compilation group discovery for batch JIT (no Cranelift dependency) |
| `helpers.rs` | ~368 | Helper methods on `FunctionTranslator` (call emission, exception checks) |
| `worker.rs` | ~130 | Background JIT worker thread: `JitWorker`, `JitTask`, `JitResult` |

## Runtime Helpers

All operations go through `extern "C"` runtime helpers for safety.
These handle type checking and tagged-union encoding.

### runtime.rs (pure arithmetic on tagged-union values)

- **Arithmetic**: `elle_jit_add`, `_sub`, `_mul`, `_div`, `_rem`
- **Bitwise**: `elle_jit_bit_and`, `_or`, `_xor`, `_shl`, `_shr`
- **Unary**: `elle_jit_neg`, `elle_jit_not`, `elle_jit_bit_not`
- **Comparison**: `elle_jit_eq`, `_ne`, `_lt`, `_le`, `_gt`, `_ge`
- **Type checking**: `elle_jit_is_nil`, `elle_jit_is_truthy`, `elle_jit_is_int`

### dispatch.rs (thin re-export layer + non-call helpers)

`dispatch.rs` re-exports everything from `calls.rs`, `data.rs`, and `suspend.rs` so that
`vtable.rs` can reference all helpers as `dispatch::elle_jit_*`. It also houses:

- **Array mutation**: `elle_jit_array_push`, `elle_jit_array_extend`
- **Parameter frames**: `elle_jit_push_param_frame`
- **Struct access**: `elle_jit_struct_get_or_nil`, `elle_jit_struct_get_destructure`, `elle_jit_struct_rest`
- **Signal bound checking**: `elle_jit_check_signal_bound`

### calls.rs (function call dispatch)

- **Sentinels**: `TAIL_CALL_SENTINEL`, `YIELD_SENTINEL`
- **Metadata types**: `YieldPointMeta`, `CallSiteMeta`
- **Exception check**: `elle_jit_has_exception`
- **Function calls**: `elle_jit_call`, `elle_jit_tail_call`, `elle_jit_call_array`, `elle_jit_tail_call_array`
- **Call depth**: `elle_jit_call_depth_enter`, `elle_jit_call_depth_exit`
- **Misc call helpers**: `elle_jit_resolve_tail_call`, `elle_jit_pop_param_frame`, `elle_jit_make_closure`
- **Env building**: `build_closure_env_for_jit` (interpreter fallback env construction)

### data.rs (heap/VM interaction)

- **Data structures**: `elle_jit_cons`, `elle_jit_car`, `elle_jit_cdr`, `elle_jit_make_array`, `elle_jit_is_pair`, and array/slice ops
- **LBoxes**: `elle_jit_make_capture`, `elle_jit_load_capture_cell`, `elle_jit_store_capture_cell`, `elle_jit_load_capture`, `elle_jit_store_capture`
- **Type checks**: `elle_jit_is_array`, `elle_jit_is_struct`, `elle_jit_is_set`, etc.

## Self-Tail-Call Optimization

Self-recursive tail calls (a function calling itself in tail position) are
optimized to native loops. The JIT generates this block structure:

```
entry_block:
    // Extract function params (env, args, nargs, vm, self_bits)
    // Load initial args into arg variables
    // Jump to loop_header

loop_header:
    // Merge point for self-tail-calls
    // Jump to first LIR block

lir_blocks:
    // ... instructions ...
    // TailCall: if func == self_bits, update arg vars, jump to loop_header
    //           if func != self_bits, call elle_jit_tail_call, return
```

Key implementation details:
- **Arg variables**: Parameters are stored in Cranelift variables (not read
  from the args pointer). This allows self-tail-calls to update them.
- **Loop header**: A merge block that self-tail-calls jump to. Sealed after
  all LIR blocks are translated (to allow back-edges).
- **Arity check**: Self-tail-call optimization only applies when the call
  has the same number of arguments as the function's arity.
- **Arg evaluation order**: New arg values are read before any are updated,
  handling cases like `(f b a)` where args are swapped.

## Inline Integer Fast Paths

For each arithmetic (`BinOp`) and comparison (`Compare`) operation, the JIT
emits a diamond-shaped CFG that checks if both operands are integers and
performs the operation inline, falling back to the extern runtime helper for
non-integer operands:

```
current_block:
    tag check: both operands have TAG_INT?
    brif -> fast_block / slow_block

fast_block:
    extract payloads, native op, re-tag result
    jump -> merge_block(fast_result)

slow_block:
    call extern helper (e.g., elle_jit_add)
    jump -> merge_block(slow_result)

merge_block(phi):
    result = phi
```

Special cases:
- **Div/Rem**: An extra `int_check_block` checks for zero divisor after the
  tag check. If divisor is zero, falls to `slow_block` (two predecessors).
- **Eq/Ne**: Use tag+payload equality (both have the same
  TAG_INT tag, so direct comparison is correct for integers).
- **Ordered comparisons** (Lt/Le/Gt/Ge) and **shifts** (Shl/Shr): Use the
  full i64 payload directly for the native operation.
- **Not** (unary): Fully inlined with no slow path. The truthiness check
  (compare tag against the falsy tags) works for all types — only nil and
  false have falsy tags. Returns `TAG_TRUE` or `TAG_FALSE` directly.
- **Neg/BitNot** (unary): Same diamond pattern as binary ops but with a
  single-operand tag check (`icmp eq` against `TAG_INT`). Neg negates the
  i64 payload, re-tags. BitNot inverts the payload bits, re-tags.

## Direct Self-Calls

Solo-compiled functions with a known SymbolId (i.e., bound to a top-level def) get a
one-entry `scc_peers` map pointing to themselves. This means self-recursive
calls emit direct Cranelift calls instead of going through `elle_jit_call`.

Benefits:
- Eliminates hash lookup in `jit_cache` per self-call
- Eliminates arity checking (known at compile time)
- Eliminates dispatch overhead (direct call vs. indirect)
- Passes correct `self_bits` so the callee's self-tail-call optimization works

When `self_sym` is `None` (anonymous closures), behavior is unchanged — calls
go through `elle_jit_call` as before.

## Fiber Integration and Yield Side-Exit

The signal system and JIT side-exit mechanism enable fibers and JIT to coexist:

- **JIT-safe fiber primitives**: `fiber/new`, `fiber/status`, `fiber/value`,
  `fiber/bits`, `fiber/mask` have `Signal::errors()` — `may_suspend()` is
  false, so closures calling them can be JIT-compiled. `fiber?` has
  `Signal::silent()`. These all return `SIG_OK` or `SIG_ERROR`, which
  `jit_handle_primitive_signal` handles.

- **JIT-excluded fiber primitives**: `fiber/resume` and `emit` have
  `Signal::yields_errors()` — `may_suspend()` is true. Any closure calling
  them transitively inherits this signal, so the JIT gate rejects them.

- **Yield side-exit**: When a JIT-compiled function reaches a `Yield` terminator,
  it calls `elle_jit_yield` (a runtime helper) which:
  1. Reads yield point metadata from `JitCode.yield_points`
  2. Spills live registers to a temporary buffer
  3. Builds a `SuspendedFrame` with the bytecode resume IP and spilled stack
  4. Sets `fiber.signal = (SIG_YIELD, yielded_value)` and `fiber.suspended`
  5. Returns `YIELD_SENTINEL` to the JIT caller
  
  The JIT caller detects `YIELD_SENTINEL` and returns it to the interpreter,
  which resumes via `execute_bytecode_from_ip`.

- **Yield-through-call**: When a JIT-compiled function calls another function
  that yields, the JIT detects the yield via post-call signal check and calls
  `elle_jit_yield_through_call` to build the caller's `SuspendedFrame` and
  append it to the suspended frame chain.

- **SIG_YIELD handling**: `jit_handle_primitive_signal` now handles `SIG_YIELD`
  from primitives (e.g., `fiber/resume`) by returning `YIELD_SENTINEL`.

- **SIG_QUERY handling**: `jit_handle_primitive_signal` dispatches `SIG_QUERY`
  to `vm.dispatch_query()` and returns the result. This supports primitives
  like `list-primitives` and `primitive-meta` that read VM state but don't
  suspend execution.

- **Catch-all panic**: `jit_handle_primitive_signal` panics on unexpected
  signal bits (not `SIG_OK`, `SIG_ERROR`, `SIG_HALT`, `SIG_YIELD`, or `SIG_QUERY`).
  Reaching this means the signal system has a bug — a polymorphic primitive
  was called from JIT code.

## JIT-to-JIT Calling

When `elle_jit_call` dispatches to a closure, it checks `vm.jit_cache` for
the callee's bytecode pointer. If found, it calls the JIT code directly
without building an interpreter environment — zero heap allocations on the
fast path. This is critical for recursive functions like `fib`.

Key details:
- **Zero-copy env**: `closure.env.as_ptr() as *const Value` — safe because
  `Value` layout is stable.
- **Zero-copy args**: `args_ptr` passes through from the JIT caller directly.
- **Zero-copy native args**: Native function calls use `args_ptr as *const Value`
  to create a slice without Vec allocation.
- **Call depth tracking**: Increments/decrements `call_depth` for stack traces.
- **Tail call handling**: If the callee returns `TAIL_CALL_SENTINEL`, the
  pending tail call is executed via `execute_closure_bytecode`.
- **Exception propagation**: Checks `fiber.signal` for `SIG_ERROR` after call.

## Error Handling in Dispatch

All dispatch helpers (`elle_jit_call`, `elle_jit_tail_call`) set `vm.fiber.signal` to `(SIG_ERROR, error_value)` on
error and return `TAG_NIL`. The JIT checks for pending error signals after each
call via `elle_jit_has_exception` (which checks `fiber.signal` for `SIG_ERROR`).
No errors are silently swallowed.

## Invariants

1. **Only non-polymorphic functions.** `JitCompiler::compile` returns
   `JitError::Polymorphic` for functions where `signal.propagates != 0` (polymorphic).
   Functions with `Signal::silent()` or `Signal::yields()` are accepted.
   Errors (SIG_ERROR) and FFI (SIG_FFI) are fine — they don't require frame
   snapshot/restore.

2. **Yield metadata is populated during emission.** `Emitter::emit()` returns
   `(Bytecode, Vec<YieldPointInfo>, Vec<CallSiteInfo>)`. The caller attaches
   these to `LirFunction.yield_points` and `LirFunction.call_sites` before
   storing on a `Closure`. The JIT reads this metadata to generate side-exit code.

3. **YieldPointMeta is derived from YieldPointInfo.** During JIT compilation,
   `YieldPointInfo.stack_regs.len()` is converted to `YieldPointMeta.num_spilled`
   (the count of spilled values). The JIT stores this in `JitCode.yield_points`
   for runtime lookup.

4. **YIELD_SENTINEL is distinct from TAIL_CALL_SENTINEL.** Both are sentinel
   values returned by JIT code, but they trigger different handlers:
   - `TAIL_CALL_SENTINEL` (0xDEAD_BEEF_DEAD_BEEFu64) → resolve pending tail call
   - `YIELD_SENTINEL` (0xDEAD_CAFE_DEAD_CAFEu64) → side-exit to interpreter

5. **Value encoding correctness.** The JIT uses the exact same tag+payload
   patterns as `Value::int()`, `Value::float()`, etc. Constants are encoded at compile time.

6. **Module lifetime.** `JitCode` keeps the `JITModule` alive via `Arc` so the
   native code isn't freed while still in use.

7. **Enabled by default via `jit` Cargo feature.** Disable with `--no-default-features`.

8. **VM pointer for runtime calls.** The 4th parameter is `vm` to support
   function calls, fiber access, and yield side-exit helpers.

9. **Self-tail-call identity.** The 5th parameter `self_bits` is the
   tagged-union closure pointer. Self-tail-calls are detected by comparing the callee's bits
   against `self_bits`.

10. **No silent error swallowing.** Every error path in dispatch helpers sets
    `vm.fiber.signal` to `(SIG_ERROR, condition)` before returning `TAG_NIL`.

11. **Value layout is assumed stable.** JIT-to-JIT calling and native function
    dispatch pass `*const Value` pointers directly. If Value's representation
    changes (see `value/repr/AGENTS.md`), these casts break.

12. **Yield helpers set fiber.signal and fiber.suspended.** `elle_jit_yield`
     and `elle_jit_yield_through_call` are responsible for building the
     `SuspendedFrame` and setting `fiber.signal = (SIG_YIELD, value)` and
     `fiber.suspended`. The JIT caller must not modify these fields.

13. **Variadic functions with `VarargKind::List` are JIT-supported.** The JIT
     entry block emits a Cranelift cons-building loop that iterates over
     `args[fixed..nargs]` in reverse, calling `elle_jit_cons` to build the
     rest-arg list. `capture_params_mask` is checked for the rest param slot.
     Functions with `VarargKind::Struct` or `VarargKind::StrictStruct` are
     still rejected (they require fiber access for keyword error reporting)
     and fall back to the interpreter.

## Dual Address Space for Variables

`LoadLocal`/`StoreLocal` and `LoadCapture`/`StoreCapture` use different address
spaces:

- **Stack-relative (LoadLocal/StoreLocal):** The lowerer assigns slots starting
  at `num_params` (it initializes `num_locals = num_params`). The JIT maps these
  via `local_slot_to_var()` in `helpers.rs`: slots >= `num_params` are offset
  into the `local_var_base` region; slots < `num_params` defensively map to
  `arg_var_base`.

- **Env-relative (LoadCapture/StoreCapture):** Indices address the closure
  environment array directly. Captures with index < `num_captures` load from
  the env pointer; indices >= `num_captures` address locally-defined variables
  (params and locals that were hoisted into the env for lbox reasons).

This separation means LoadLocal/StoreLocal never touch the env pointer and
LoadCapture/StoreCapture never use stack-relative slots.

## LBox Optimization for Locally-Defined Variables

The JIT uses `LirFunction.capture_locals_mask` to avoid unnecessary `CaptureCell`
heap allocations. In the VM interpreter, every locally-defined variable inside
a lambda gets a `CaptureCell(NIL)` at function entry (because `StoreUpvalue`
requires lbox indirection to write through `Rc<Vec<Value>>`). In JIT code,
locally-defined variables are Cranelift variables (CPU registers/stack), so
lbox wrapping is only needed when `binding.needs_capture()` is true (captured by
nested closure or mutated via `set!`).

The optimization applies to three code paths in `translate.rs`:

1. **`init_locally_defined_vars`**: Only calls `elle_jit_make_capture` when the
   bit is set in `capture_locals_mask`; others get NIL directly.
2. **`LoadCapture` for locals**: Skips `load_capture_cell` unwrapping when bit not set.
3. **`StoreCapture` for locals**: Skips `store_capture_cell` when bit not set, uses
   `def_var` directly.

Impact: 3.2x speedup on N-Queens N=12 (4.4s → 1.38s), 30x reduction in
kernel time (2.4s → 80ms) from eliminated allocation pressure.

## Yield Side-Exit Implementation Details

### YieldPointMeta

Stored in `JitCode.yield_points`, indexed by yield point index:
- `resume_ip: usize` — Bytecode offset to resume at (matches `SuspendedFrame.ip`)
- `num_spilled: u16` — Number of values on the operand stack at yield time

The JIT yield helper reads `num_spilled` to know how many u64 values to read
from the spilled buffer and convert back to `Value`s.

### Yield Point Recording

During bytecode emission, when a `Terminator::Yield` is encountered:
1. The emitter records the bytecode position after the Yield opcode as `resume_ip`
2. The emitter captures the current operand stack state as `stack_regs`
3. A `YieldPointInfo` is pushed to `Emitter.yield_points`

After emission, the caller attaches `yield_points` to `LirFunction.yield_points`.

During JIT compilation, `YieldPointInfo` is converted to `YieldPointMeta`:
```rust
YieldPointMeta {
    resume_ip: yp.resume_ip,
    num_spilled: yp.stack_regs.len() as u16,
}
```

### Call Site Recording

During bytecode emission, when a `LirInstr::Call` is encountered in a function
where `signal.may_suspend()`:
1. The emitter records the bytecode position after the Call opcode as `resume_ip`
2. The emitter captures the operand stack state (after popping func/args, before pushing result) as `stack_regs`
3. A `CallSiteInfo` is pushed to `Emitter.call_sites`

This metadata is used by the JIT to generate yield-through-call code: when a
callee yields, the JIT builds the caller's `SuspendedFrame` using the recorded
resume IP and stack state.

### Environment Reconstruction on Side-Exit

When building a `SuspendedFrame` for interpreter resumption, the JIT yield
helpers (`elle_jit_yield`, `elle_jit_yield_through_call`) reconstruct the full
interpreter environment from the closure's captures and the spilled locals:

```
env = [closure.env[0], ..., closure.env[n-1], local_0, ..., local_{m-1}]
stack = [operand_0, ..., operand_k]
```

The interpreter's `LoadUpvalue` accesses `env[idx]` for ALL variables —
captures, params, and locally-defined vars. The JIT stores captures in
`closure.env` and params/locals in Cranelift variables. The spill buffer
layout is `[locals..., operands...]`, where `num_locals` (from yield/call-site
metadata) gives the split point. The first `num_locals` spilled values are
appended to `closure.env` to form the full env; the remaining values form
the operand stack.

## Future Phases

- Phase 5:
   - Inline type checks for arithmetic fast paths
   - JIT-native signal handling (setjmp/longjmp or Cranelift exception tables)
   - Benchmarks and profiling

### Open task: Flip* bytecodes

The JIT currently skips all Flip* bytecodes (`FlipEnter`, `FlipSwap`,
`FlipExit`). These are emitted for `while`/`loop` forms where the body
passes `can_flip_while_loop()` (no dangerous outward set, all break
values safe). In the interpreter, they implement rotation-based
reclamation. In JIT code, they are no-ops — the JIT side-exit path
doesn't need rotation because yielding functions fall back to the
interpreter.

To enable rotation in JIT-compiled silent loops, `translate.rs` needs
implementations for:
- `FlipEnter` — push flip frame
- `FlipSwap` — rotate pools (call `rotate_pools` helper)
- `FlipExit` — pop flip frame and release

The helpers exist in `fiberheap/routing.rs` and `fiberheap/mod.rs`.
The JIT just needs to emit calls to them at the right points, mirroring
`RegionEnter`/`RegionExit` handling in `dispatch.rs`. This is blocked on
Phase 2A (rotation slot deallocation) being enabled first.
