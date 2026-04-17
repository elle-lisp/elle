# MLIR Backend

> **Feature-gated:** The MLIR backend requires `--features mlir` at build
> time and a working LLVM 22 + MLIR install (the `melior` crate links to
> them). It is disabled by default.

The MLIR backend is a tier-2 path that takes a hot, **GPU-eligible**
`LirFunction`, lowers it through the MLIR `arith` / `func` / `cf` /
`memref` dialects, converts to the LLVM dialect, and JIT-compiles via
the MLIR `ExecutionEngine`. The result is a native function pointer
called from the VM with C calling convention.

It runs alongside the bytecode VM and the Cranelift JIT — not as a
replacement. The same eligibility predicate also drives SPIR-V emission
for GPU dispatch (see [impl/spirv.md](spirv.md) and
[impl/gpu.md](gpu.md)).

## Pipeline

```text
LirFunction → lower_to_module → MLIR (arith/func/cf/memref)
            → PassManager(create_to_llvm) → LLVM dialect
            → ExecutionEngine::new           → native code
            → invoke_packed                  → i64 result
```

The eligibility check (`LirFunction::is_gpu_eligible`) is layered:

1. **Signal** — only `errors`-or-silent functions; no yield, I/O, FFI,
   or polymorphic.
2. **Structural** — `Arity::Exact(N)`, no captures, no mutable cells.
3. **Instruction whitelist** — every `LirInstr` and `Terminator` must
   be GPU-safe (constants, arithmetic, comparison, local slots,
   parameter loads, `Jump` / `Branch` / `Return`).

A second, stricter predicate `is_mlir_cpu_eligible` requires that the
returned register be reachable from integer operations only — nil,
bool, and compare results all become `i64` (`0` / `1`) and would lose
their tag if reboxed as a `Value`. CPU dispatch from the VM uses the
strict predicate; GPU dispatch (where the caller reads i64s out of a
buffer) uses the looser one.

## Value model

MLIR sees a flat scalar world: every Elle value enters as `i64`.
Float parameters are bitcast i64→f64 at function entry; float
returns are bitcast f64→i64 before `func.return`. A `ScalarType`
tag (`Int` or `Float`) tracks each SSA value's type for dispatch
between integer and float MLIR ops.

| Elle constant | MLIR encoding |
|---------------|---------------|
| `Int(n)`      | `arith.constant n : i64` |
| `Bool(b)`     | `0` or `1` |
| `Nil`         | `0` |
| `Float(f)`    | `f.to_bits() as i64` |

Local slots use `memref.alloca` of `memref<i64>` allocated in the
entry block — that handles cross-block phi-style patterns
(`StoreLocal` in one block, `LoadLocal` in another) without needing
to lower SSA φ nodes by hand. Within a block, LIR `Reg`s map directly
to MLIR `Value`s.

Comparisons emit `arith.cmpi` (returns `i1`) immediately followed by
`arith.extui` to `i64`. Branches compare the cond reg to `0` with
`cmpi ne` rather than truncating to `i1` — `trunci` would take the
low bit and read e.g. `2` as false.

## VM integration

`VM::try_mlir_call` (in `src/vm/mlir_entry.rs`) is consulted on every
closure call before the Cranelift JIT path. It:

1. Skips non-`is_gpu_candidate` closures (cheap field check).
2. Returns the cached engine result if available.
3. Returns early if the closure is in the rejection set.
4. Reads the closure call counter — only proceeds past
   `jit_hotness_threshold`. The counter is owned by the JIT path,
   which runs after MLIR; MLIR only reads.
5. Runs `is_mlir_cpu_eligible` (full instruction walk).
6. Compiles via `MlirCache::compile`, caches by bytecode pointer,
   and invokes.

Arguments are unboxed to i64: integers pass through directly; floats
are bitcast f64→i64 by the caller. A `param_types: u64` bitmask
(bit i = 1 means param i is float) is passed to `lower_to_module`,
which inserts `arith.bitcast(i64→f64)` at function entry for float
params. The same bitmask is part of the cache key, so the same
closure called with `(f 1)` vs `(f 1.0)` gets separate compiled code.
Non-numeric args (nil, string, etc.) fall through to bytecode.

The result is reboxed based on the compiled function's return type
(`ScalarType::Int` → `Value::int`, `ScalarType::Float` →
`Value::float(f64::from_bits(...))`). Failures are reported as a
structured error (`error_val("mlir-error", ...)`) carried via
`SIG_ERROR` — the rejection is also recorded so future calls don't
retry.

## MlirCache

`MlirCache` owns:

- A single `melior::Context` with all dialects registered (~4ms to
  create — done once).
- `engines: HashMap<(*const u8, u64), (ExecutionEngine, String, ScalarType)>` —
  keyed by (bytecode pointer, param_types bitmask).
- `spirv_cache: HashMap<*const u8, Vec<u8>>` — SPIR-V bytes from
  `compile_spirv` (see [impl/spirv.md](spirv.md)).
- `rejections: HashSet<(*const u8, u64)>` — (pointer, param_types)
  pairs known to fail conversion or verification.

The cache lives on the VM and is `unsafe impl Send + Sync` because
the VM is single-threaded; the engine and context are never accessed
concurrently.

## Files

```text
src/mlir/mod.rs       Module entry, tests
src/mlir/lower.rs     LIR → MLIR (arith/func/cf/memref)
src/mlir/execute.rs   One-shot compile + invoke (mlir_call)
src/mlir/cache.rs     MlirCache: shared context + engine cache
src/mlir/spirv.rs     LIR → SPIR-V (see impl/spirv.md)
src/vm/mlir_entry.rs  VM::try_mlir_call dispatch
src/lir/types.rs      is_gpu_eligible / is_mlir_cpu_eligible / is_gpu_instruction
```

## Primitives

| Name | Signal | Purpose |
|------|--------|---------|
| `fn/gpu-eligible?` | errors | True if the closure passes `is_gpu_eligible` |
| `mlir/compile-spirv` | query+errors | Compile a closure to SPIR-V bytes (see [impl/spirv.md](spirv.md)) |
| `git` / `fn/git?` / `disgit` | query+errors | Cache SPIR-V bytes on the closure template (see [impl/gpu.md](gpu.md)) |

## See also

- [impl/lir.md](lir.md) — the IR being lowered
- [impl/jit.md](jit.md) — the Cranelift tier that runs after MLIR rejection
- [impl/spirv.md](spirv.md) — the GPU lowering path that shares the eligibility check
- [impl/gpu.md](gpu.md) — end-to-end GPU compute via MLIR + SPIR-V + Vulkan
- [impl/differential.md](differential.md) — cross-tier agreement harness using `compile/run-on`
