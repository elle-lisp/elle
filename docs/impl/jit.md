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

Rejected functions are marked so the VM doesn't retry them.

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
- [impl/differential.md](differential.md) — cross-tier agreement harness
