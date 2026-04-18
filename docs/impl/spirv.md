# SPIR-V Backend

> **Feature-gated:** SPIR-V emission lives inside the MLIR backend
> (`--features mlir`). It additionally depends on `mlir-translate`
> being on `PATH` for the final binary serialization step.

The SPIR-V backend takes a GPU-eligible `LirFunction` and emits a
SPIR-V compute kernel suitable for Vulkan dispatch. It is the bridge
between Elle's LIR and an actual GPU: the bytes it produces are fed
to `vulkan/shader` (see [impl/gpu.md](gpu.md)).

There are two ways to produce SPIR-V in this codebase:

- **`src/mlir/spirv.rs`** — automatic, compiler-generated. Wraps the
  LIR function in a `gpu.module`, runs MLIR's standard SPIR-V
  conversion passes, and serializes via `mlir-translate`. Used by
  `mlir/compile-spirv` and `gpu:map`.
- **`lib/spirv.lisp`** — hand-written DSL. A pure-Elle SPIR-V
  bytecode emitter for crafting compute shaders directly (no MLIR
  required). Used by `gpu:compile`.

Both paths produce the same wire format and feed the same Vulkan
plugin.

## Compiler-generated path

```text
LirFunction → generate_gpu_module      (textual MLIR)
            → Module::parse            (typed MLIR)
            → PassManager
                gpu.module:
                  arith-to-spirv
                  control-flow-to-spirv
                  scf-to-spirv
                  mem-ref-to-spirv
                gpu-to-spirv
                spirv.module:
                  spirv-lower-abi-attributes
                  spirv-update-vce
            → extract spirv.module text
            → mlir-translate --serialize-spirv → bytes
```

Closures with captures are rejected at the top of
`generate_gpu_module` — SPIR-V kernels would need extra uniform
buffers to pass captured values, which is separate work. The
`is_gpu_eligible` predicate allows captures (for the MLIR-CPU tier),
so the SPIR-V path has its own guard.

The module is wrapped with the SPIR-V target environment
(`v1.0`, `[Shader, Int64, Float64]`,
`[SPV_KHR_storage_buffer_storage_class]`) and the
`gpu.container_module` attribute. The kernel signature has one
`memref<?xi64>` per input parameter plus one output buffer; the
function body loads `gpu.thread_id x` as the global ID, indexes each
input with it, runs the lowered LIR, and stores the result into the
output buffer at the same index.

Single-block functions are emitted directly; multi-block functions
go through an `scf` → `spirv` pass to handle structured control flow
(branches turn into `scf.if`).

`mlir-translate` is a separate binary because the C API doesn't expose
SPIR-V serialization. It is invoked via `Command::new("mlir-translate")`
with stdin/stdout pipes; failures surface as a structured
`mlir-error`.

## Hand-written path (`lib/spirv.lisp`)

For shaders the compiler can't yet generate (multi-buffer fused
kernels, custom decorations, compute primitives outside the LIR
whitelist), Elle ships a SPIR-V bytecode emitter as a normal
library:

```text
(spv:compute local-size-x num-buffers body-fn f32-bits)
  → bytes
```

The library defines opcode constants, a builder closure, and helpers
for common patterns:

- Storage classes, memory model, capabilities, decorations
- Type, constant, and pointer construction
- Arithmetic / comparison / bitwise opcodes
- Structured control flow (`OpLoopMerge`, `OpSelectionMerge`,
  `OpBranchConditional`)
- Buffer load/store via `OpAccessChain`

`f32-bits` is plumbed through from the host side because Elle has no
native `f32` literal — the vulkan plugin exports a helper to convert
from the host's representation.

## Wire format

Both paths produce a SPIR-V binary that begins with the magic word
`0x07230203` (little-endian: `03 02 23 07`). Subsequent words declare
capabilities, the memory model, the entry point (`main`), and the
function body. Workgroup size is set via the `LocalSize` execution
mode, captured from the `local-size-x` (compiler path) or
`workgroup_size` (mlir path) parameter.

## Caching

The MLIR `MlirCache` carries a `spirv_cache: HashMap<*const u8, Vec<u8>>`
keyed by the closure's bytecode pointer:

- `mlir/compile-spirv` always re-uses the cache (and the shared MLIR
  context) — repeated calls for the same closure are O(1).
- `(git f)` additionally stores the bytes inside the closure's
  `template.spirv: OnceCell<Vec<u8>>`, so subsequent calls skip the
  cache lookup entirely. `(fn/git? f)` predicates on this cell;
  `(disgit f)` returns the cached bytes.

`gpu:map` consults `(fn/git? f)` first and falls back to
`mlir/compile-spirv` — letting users pre-compile hot kernels with
`(git f)` and amortize the SPIR-V build.

## Files

```text
src/mlir/spirv.rs              Compiler path: LIR → MLIR gpu.module → SPIR-V bytes
src/mlir/cache.rs              compile_spirv / get_spirv on MlirCache
src/vm/signal.rs               Dispatch for mlir/compile-spirv and git
src/primitives/introspection.rs  Primitive defs (mlir/compile-spirv, fn/gpu-eligible?)
src/primitives/meta.rs         Primitive defs (git, fn/git?, disgit)
lib/spirv.lisp                 Hand-written SPIR-V DSL
plugins/vulkan/src/shader.rs   SPIR-V → Vulkan compute pipeline
```

## Primitives

| Name | Signal | Purpose |
|------|--------|---------|
| `mlir/compile-spirv` | query+errors | Lower a GPU-eligible closure to SPIR-V bytes |
| `git` | query+errors | Compile and cache SPIR-V on the closure template |
| `fn/git?` | errors | True if the closure has cached SPIR-V |
| `disgit` | errors | Return the cached SPIR-V bytes |

All four return / accept the bytes as an Elle `bytes` value (the same
type produced by `read-bytes`); they are passed straight to
`vulkan/shader`.

## See also

- [impl/mlir.md](mlir.md) — the LIR → MLIR lowering shared with the CPU path
- [impl/gpu.md](gpu.md) — Vulkan dispatch consuming SPIR-V bytes
- [impl/lir.md](lir.md) — the eligibility predicate and instruction whitelist
- `plugins/vulkan/AGENTS.md` — Vulkan plugin internals
- `lib/AGENTS.md` (lib/spirv) — Elle DSL reference
