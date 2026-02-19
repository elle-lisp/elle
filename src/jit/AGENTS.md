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
    globals: *mut (),       // pointer to VM globals
) -> Value;
```

Values are 8 bytes (`u64` underneath the NaN-boxing).

## Phase 1 Scope

Supported instructions:
- `Const` (Int, Float, Bool, Nil, EmptyList)
- `BinOp` (all arithmetic/bitwise via runtime helpers)
- `UnaryOp` (Neg, Not, BitNot via runtime helpers)
- `Compare` (all via runtime helpers)
- `Move`, `Dup`
- `LoadLocal`, `StoreLocal`
- `LoadCapture`, `LoadCaptureRaw`
- Terminators: `Return`, `Jump`, `Branch`

Unsupported (returns `JitError::UnsupportedInstruction`):
- `Call`, `TailCall`, `MakeClosure`
- `Cons`, `MakeVector`, `Car`, `Cdr`
- `MakeCell`, `LoadCell`, `StoreCell`
- `LoadGlobal`, `StoreGlobal`
- Exception handling instructions
- `Yield` (returns `JitError::NotPure`)

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | ~60 | Public API, `JitError` type |
| `compiler.rs` | ~600 | LIR -> Cranelift IR translation |
| `runtime.rs` | ~350 | Runtime helpers callable from JIT code |
| `code.rs` | ~70 | `JitCode` wrapper type |

## Runtime Helpers

All arithmetic operations go through `extern "C"` runtime helpers for safety.
These handle type checking and NaN-boxing:

- `elle_jit_add`, `elle_jit_sub`, `elle_jit_mul`, `elle_jit_div`, `elle_jit_rem`
- `elle_jit_bit_and`, `elle_jit_bit_or`, `elle_jit_bit_xor`, `elle_jit_shl`, `elle_jit_shr`
- `elle_jit_neg`, `elle_jit_not`, `elle_jit_bit_not`
- `elle_jit_eq`, `elle_jit_ne`, `elle_jit_lt`, `elle_jit_le`, `elle_jit_gt`, `elle_jit_ge`
- `elle_jit_cons`, `elle_jit_is_nil`, `elle_jit_is_truthy`

## Invariants

1. **Only pure functions.** `JitCompiler::compile` returns `JitError::NotPure`
   for functions with `Effect::Yields` or `Effect::Polymorphic`.

2. **NaN-boxing correctness.** The JIT uses the exact same bit patterns as
   `Value::int()`, `Value::float()`, etc. Constants are encoded at compile time.

3. **Module lifetime.** `JitCode` keeps the `JITModule` alive via `Arc` so the
   native code isn't freed while still in use.

4. **Feature-gated.** All JIT code is behind `#[cfg(feature = "jit")]`. The
   project compiles without the JIT.

## Future Phases

- Phase 2: Function calls (intra-JIT and JIT-to-interpreter)
- Phase 3: Data structures (cons, car, cdr, vectors)
- Phase 4: Exception handling
- Phase 5: Inline type checks for fast paths
