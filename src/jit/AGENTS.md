# jit

JIT compilation for Elle using Cranelift.

## Responsibility

Compile non-suspending `LirFunction` to native x86_64 code. Only functions
where `!effect.may_suspend()` are JIT candidates (no yield/debug/polymorphic
complexity — Cranelift native frames can't be snapshot/restored mid-execution).

## Architecture

```
LirFunction -> JitCompiler -> Cranelift IR -> Native code -> JitCode
```

## Interface

| Type | Purpose |
|------|---------|
| `JitCompiler` | Translates LIR to native code via Cranelift |
| `JitCode` | Wrapper for native function pointer + module lifetime |
| `JitError` | Compilation errors |
| `BatchMember` | A member of an SCC compilation group (SymbolId + LirFunction) |
| `discover_compilation_group` | Discover call peers for batch JIT compilation |

## Calling Convention

JIT-compiled functions use this calling convention:

```rust
type JitFn = unsafe extern "C" fn(
    env: *const Value,      // closure environment (captures array)
    args: *const Value,     // arguments array
    nargs: u32,             // number of arguments
    vm: *mut VM,            // pointer to VM (for globals, function calls)
    self_bits: u64,         // NaN-boxed bits of the closure (for self-tail-call detection)
) -> Value;
```

Values are 8 bytes (`u64` underneath the NaN-boxing).

The 5th parameter `self_bits` enables self-tail-call optimization: when a
function tail-calls itself, the JIT compares the callee against `self_bits`.
If equal, it updates the arg variables and jumps to the loop header instead
of calling `elle_jit_tail_call`. This turns self-recursive tail calls into
native loops.

## JIT Phases

The JIT was built incrementally:

| Phase | Scope |
|-------|-------|
| Phase 1 | Constants, arithmetic, comparison, variables, terminators. Capture-free functions only. |
| Phase 2 | Closures with captures: `LoadCapture`, `LoadCaptureRaw`, `StoreCapture`. |
| Phase 3 | Data structures (`Cons`, `Car`, `Cdr`, `MakeVector`, `IsPair`), cells (`MakeCell`, `LoadCell`, `StoreCell`), globals (`LoadGlobal`, `StoreGlobal`), function calls (`Call`, `TailCall`). VM pointer parameter replaced `globals` pointer. |
| Phase 4 | Self-tail-call optimization, JIT-to-JIT calling, batch compilation, `ValueConst`. |

## Phase 4 Scope (Current)

Supported instructions:
- **Constants**: `Const` (Int, Float, Bool, Nil, EmptyList, Symbol, Keyword), `ValueConst`
- **Arithmetic**: `BinOp` (inline integer fast path, extern fallback), `UnaryOp` (Not fully inlined, Neg/BitNot inline integer fast path)
- **Comparison**: `Compare` (inline integer fast path, extern fallback)
- **Variables**: `Move`, `Dup`, `LoadLocal`, `StoreLocal`, `LoadCapture`, `LoadCaptureRaw`
- **Data structures**: `Cons`, `Car`, `Cdr`, `MakeVector`, `IsPair`
- **Cells**: `MakeCell`, `LoadCell`, `StoreCell`, `StoreCapture`
- **Globals**: `LoadGlobal`, `StoreGlobal`
- **Function calls**: `Call`, `TailCall` (self-calls become native loops; non-self calls use `elle_jit_tail_call` trampoline)
- **Terminators**: `Return`, `Jump`, `Branch`

Unsupported (returns JitError::UnsupportedInstruction):
- `MakeClosure` — rare in hot loops, deferred
- Fiber/yield: LoadResumeValue, Yield

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | ~70 | Public API, `JitError` type |
| `compiler.rs` | ~910 | `JitCompiler`, `RuntimeHelpers`, compilation entry point |
| `translate.rs` | ~940 | `FunctionTranslator`, LIR instruction translation |
| `runtime.rs` | ~460 | Arithmetic, comparison, type-checking helpers |
| `dispatch.rs` | ~640 | Data structure, cell, global, function call helpers (incl. JIT-to-JIT) |
| `code.rs` | ~100 | `JitCode` wrapper type |
| `fastpath.rs` | ~250 | Inline integer fast paths for arithmetic/comparison |
| `group.rs` | ~590 | Compilation group discovery for batch JIT (no Cranelift dependency) |

## Runtime Helpers

All operations go through `extern "C"` runtime helpers for safety.
These handle type checking and NaN-boxing.

### runtime.rs (pure arithmetic on NaN-boxed values)

- **Arithmetic**: `elle_jit_add`, `_sub`, `_mul`, `_div`, `_rem`
- **Bitwise**: `elle_jit_bit_and`, `_or`, `_xor`, `_shl`, `_shr`
- **Unary**: `elle_jit_neg`, `elle_jit_not`, `elle_jit_bit_not`
- **Comparison**: `elle_jit_eq`, `_ne`, `_lt`, `_le`, `_gt`, `_ge`
- **Type checking**: `elle_jit_is_nil`, `elle_jit_is_truthy`, `elle_jit_is_int`

### dispatch.rs (heap/VM interaction)

- **Data structures**: `elle_jit_cons`, `elle_jit_car`, `elle_jit_cdr`, `elle_jit_make_vector`, `elle_jit_is_pair`
- **Cells**: `elle_jit_make_cell`, `elle_jit_load_cell`, `elle_jit_store_cell`, `elle_jit_store_capture`
- **Globals**: `elle_jit_load_global`, `elle_jit_store_global` (require VM pointer)
- **Function calls**: `elle_jit_call` (dispatches to native functions, JIT-cached closures, or interpreter fallback)

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
- **Eq/Ne**: Use bit equality on the full NaN-boxed value (both have the same
  TAG_INT prefix, so bit equality is correct for integers).
- **Ordered comparisons** (Lt/Le/Gt/Ge) and **shifts** (Shl/Shr): Sign-extend
  the 48-bit payload to 64 bits before the native operation.
- **Not** (unary): Fully inlined with no slow path. The truthiness check
  (`ushr 48` then compare against `0x7FF9`) works for all types — only nil and
  false have the falsy tag. Returns `TAG_TRUE` or `TAG_FALSE` directly.
- **Neg/BitNot** (unary): Same diamond pattern as binary ops but with a
  single-operand tag check (`band` with `TAG_INT_MASK`, `icmp eq` against
  `TAG_INT`). Neg sign-extends the payload, negates, truncates, re-tags.
  BitNot XORs the payload with `PAYLOAD_MASK` (flips all 48 payload bits),
  re-tags.

## Direct Self-Calls

Solo-compiled functions with a known SymbolId (i.e., bound to a global) get a
one-entry `scc_peers` map pointing to themselves. This means self-recursive
calls emit direct Cranelift calls instead of going through `elle_jit_call`.

Benefits:
- Eliminates hash lookup in `jit_cache` per self-call
- Eliminates arity checking (known at compile time)
- Eliminates dispatch overhead (direct call vs. indirect)
- Passes correct `self_bits` so the callee's self-tail-call optimization works

When `self_sym` is `None` (anonymous closures), behavior is unchanged — calls
go through `elle_jit_call` as before.

## Fiber Integration

The effect system ensures fibers and JIT coexist safely:

- **JIT-safe fiber primitives**: `fiber/new`, `fiber/status`, `fiber/value`,
  `fiber/bits`, `fiber/mask` have `Effect::raises()` — `may_suspend()` is
  false, so closures calling them can be JIT-compiled. `fiber?` has
  `Effect::none()`. These all return `SIG_OK` or `SIG_ERROR`, which
  `jit_handle_primitive_signal` handles.

- **JIT-excluded fiber primitives**: `fiber/resume` and `fiber/signal` have
  `Effect::yields_raises()` — `may_suspend()` is true. Any closure calling
  them transitively inherits this effect, so the JIT gate rejects them.

- **SIG_QUERY handling**: `jit_handle_primitive_signal` dispatches `SIG_QUERY`
  to `vm.dispatch_query()` and returns the result. This supports primitives
  like `list-primitives` and `primitive-meta` that read VM state but don't
  suspend execution.

- **Catch-all panic**: `jit_handle_primitive_signal` panics on unexpected
  signal bits (not `SIG_OK`, `SIG_ERROR`, `SIG_HALT`, or `SIG_QUERY`).
  Reaching this means the effect system has a bug — a suspending primitive
  was called from JIT code.

## JIT-to-JIT Calling

When `elle_jit_call` dispatches to a closure, it checks `vm.jit_cache` for
the callee's bytecode pointer. If found, it calls the JIT code directly
without building an interpreter environment — zero heap allocations on the
fast path. This is critical for recursive functions like `fib`.

Key details:
- **Zero-copy env**: `closure.env.as_ptr() as *const u64` — safe because
  `Value` is `#[repr(transparent)]` over `u64`.
- **Zero-copy args**: `args_ptr` passes through from the JIT caller directly.
- **Zero-copy native args**: Native function calls use `args_ptr as *const Value`
  to create a slice without Vec allocation.
- **Call depth tracking**: Increments/decrements `call_depth`, checks > 1000.
- **Tail call handling**: If the callee returns `TAIL_CALL_SENTINEL`, the
  pending tail call is executed via `execute_closure_bytecode`.
- **Exception propagation**: Checks `fiber.signal` for `SIG_ERROR` after call.

## Error Handling in Dispatch

All dispatch helpers (`elle_jit_call`, `elle_jit_tail_call`,
`elle_jit_load_global`) set `vm.fiber.signal` to `(SIG_ERROR, condition)` on
error and return `TAG_NIL`. The JIT checks for pending errors after each call
via `elle_jit_has_exception` (which checks `fiber.signal` for `SIG_ERROR`).
No errors are silently swallowed.

## Invariants

1. **Only non-suspending functions.** `JitCompiler::compile` returns
   `JitError::NotPure` for functions where `effect.may_suspend()` is true
   (yields, debug, or polymorphic). Errors (SIG_ERROR) and FFI (SIG_FFI)
   are fine — they don't require frame snapshot/restore.

2. **NaN-boxing correctness.** The JIT uses the exact same bit patterns as
   `Value::int()`, `Value::float()`, etc. Constants are encoded at compile time.

3. **Module lifetime.** `JitCode` keeps the `JITModule` alive via `Arc` so the
   native code isn't freed while still in use.

4. **Always enabled.** JIT is a required dependency (Cranelift). No feature gate.

5. **VM pointer for runtime calls.** The 4th parameter changed from `globals`
   to `vm` in Phase 3 to support function calls and global variable access.

6. **Self-tail-call identity.** The 5th parameter `self_bits` is the NaN-boxed
   closure pointer. Self-tail-calls are detected by comparing the callee's bits
   against `self_bits`.

7. **No silent error swallowing.** Every error path in dispatch helpers sets
   `vm.fiber.signal` to `(SIG_ERROR, condition)` before returning `TAG_NIL`.

8. **Value is repr(transparent) over u64.** JIT-to-JIT calling and native
   function dispatch cast `*const u64` to `*const Value` (and vice versa)
   without copying. If `Value`'s representation changes, these casts break.

## Cell Optimization for Locally-Defined Variables

The JIT uses `LirFunction.cell_locals_mask` to avoid unnecessary `LocalCell`
heap allocations. In the VM interpreter, every locally-defined variable inside
a lambda gets a `LocalCell(NIL)` at function entry (because `StoreUpvalue`
requires cell indirection to write through `Rc<Vec<Value>>`). In JIT code,
locally-defined variables are Cranelift variables (CPU registers/stack), so
cell wrapping is only needed when `binding.needs_cell()` is true (captured by
nested closure or mutated via `set!`).

The optimization applies to three code paths in `translate.rs`:

1. **`init_locally_defined_vars`**: Only calls `elle_jit_make_cell` when the
   bit is set in `cell_locals_mask`; others get NIL directly.
2. **`LoadCapture` for locals**: Skips `load_cell` unwrapping when bit not set.
3. **`StoreCapture` for locals**: Skips `store_cell` when bit not set, uses
   `def_var` directly.

Impact: 3.2x speedup on N-Queens N=12 (4.4s → 1.38s), 30x reduction in
kernel time (2.4s → 80ms) from eliminated allocation pressure.

## Future Phases

- Phase 5:
  - Inline type checks for arithmetic fast paths
  - JIT-native exception handling (setjmp/longjmp or Cranelift exception tables)
  - Benchmarks and profiling
