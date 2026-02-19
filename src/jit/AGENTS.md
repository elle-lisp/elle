# jit

JIT compilation for Elle using Cranelift.

## Responsibility

Compile pure `LirFunction` to native x86_64 code. Only `Effect::Pure` functions
are JIT candidates (no yield/coroutine complexity).

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

## Calling Convention

JIT-compiled functions use this calling convention:

```rust
type JitFn = unsafe extern "C" fn(
    env: *const Value,      // closure environment (captures array)
    args: *const Value,     // arguments array
    nargs: u32,             // number of arguments
    vm: *mut VM,            // pointer to VM (for globals, function calls)
) -> Value;
```

Values are 8 bytes (`u64` underneath the NaN-boxing).

## Phase 3 Scope (Current)

Supported instructions:
- **Constants**: `Const` (Int, Float, Bool, Nil, EmptyList, Symbol, Keyword), `ValueConst`
- **Arithmetic**: `BinOp` (all via runtime helpers), `UnaryOp` (Neg, Not, BitNot)
- **Comparison**: `Compare` (all via runtime helpers)
- **Variables**: `Move`, `Dup`, `LoadLocal`, `StoreLocal`, `LoadCapture`, `LoadCaptureRaw`
- **Data structures**: `Cons`, `Car`, `Cdr`, `MakeVector`, `IsPair`
- **Cells**: `MakeCell`, `LoadCell`, `StoreCell`, `StoreCapture`
- **Globals**: `LoadGlobal`, `StoreGlobal`
- **Function calls**: `Call`, `TailCall`
- **Terminators**: `Return`, `Jump`, `Branch`

Unsupported (returns `JitError::UnsupportedInstruction`):
- `MakeClosure` (complex, rare in hot loops)
- Exception handling: `PushHandler`, `PopHandler`, `CheckException`, `MatchException`,
  `BindException`, `LoadException`, `ClearException`, `ReraiseException`, `Throw`
- Coroutines: `LoadResumeValue`, `Yield` (returns `JitError::NotPure`)
- Emitter-only: `JumpIfFalseInline`, `JumpInline`

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | ~70 | Public API, `JitError` type |
| `compiler.rs` | ~500 | `JitCompiler`, `RuntimeHelpers`, compilation entry point |
| `translate.rs` | ~630 | `FunctionTranslator`, LIR instruction translation |
| `runtime.rs` | ~420 | Arithmetic, comparison, type-checking helpers |
| `dispatch.rs` | ~320 | Data structure, cell, global, function call helpers |
| `code.rs` | ~80 | `JitCode` wrapper type |

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
- **Function calls**: `elle_jit_call` (dispatches to native, VM-aware, or closures)

## Invariants

1. **Only pure functions.** `JitCompiler::compile` returns `JitError::NotPure`
   for functions with `Effect::Yields` or `Effect::Polymorphic`.

2. **NaN-boxing correctness.** The JIT uses the exact same bit patterns as
   `Value::int()`, `Value::float()`, etc. Constants are encoded at compile time.

3. **Module lifetime.** `JitCode` keeps the `JITModule` alive via `Arc` so the
   native code isn't freed while still in use.

4. **Feature-gated.** All JIT code is behind `#[cfg(feature = "jit")]`. The
   project compiles without the JIT.

5. **VM pointer for runtime calls.** The 4th parameter changed from `globals`
   to `vm` in Phase 3 to support function calls and global variable access.

## Future Phases

- Phase 4: Exception handling
- Phase 5: MakeClosure support
- Phase 6: Inline type checks for fast paths
