# SPIR-V Backend

<!-- audited: 2026-09-23 -->

Two paths turn Elle into SPIR-V compute kernels for Vulkan: the MLIR compiler path, and a hand-written emitter in pure Elle.

The bytes either path produces are fed to the vulkan plugin's `shader`
primitive (see [impl/gpu.md](gpu.md)):

- **`src/mlir/spirv.rs`** — automatic, compiler-generated. Wraps a
  GPU-eligible `LirFunction` in a `gpu.module`, runs MLIR's standard SPIR-V
  conversion passes, and serializes the result with `mlir-translate`. It needs
  `--features mlir`. Used by `mlir/compile-spirv`, `git` and `gpu:map`.
- **[lib/spirv.lisp](../../lib/spirv.lisp)** — hand-written DSL. A pure-Elle
  SPIR-V bytecode emitter for crafting compute shaders directly, with no MLIR.
  Used by `gpu:compile`.

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
`gpu.container_module` attribute. The kernel `@main` takes one
`memref<?xi64>` per input parameter plus one output buffer. Its body loads
`gpu.thread_id x`, which is the invocation's index inside its workgroup, and
indexes each input with it. It then runs the lowered LIR and stores the result
into the output buffer at the same index. A float result is bitcast to `i64`
for the store.

Single-block functions are emitted directly; multi-block functions
go through an `scf` → `spirv` pass to handle structured control flow
(branches turn into `scf.if`).

`mlir-translate` is a separate binary because the C API doesn't expose
SPIR-V serialization. The backend looks for it in `$MLIR_TRANSLATE`, then in
`$MLIR_SYS_220_PREFIX/bin`, then on `PATH`, and runs it with stdin and stdout
pipes. A failure surfaces as a structured `mlir-error`.

## Hand-written path (`lib/spirv.lisp`)

For shaders the compiler can't yet generate (multi-buffer fused
kernels, custom decorations, compute primitives outside the LIR
whitelist), Elle ships a SPIR-V bytecode emitter as a normal
library. `(spv:compute local-size-x num-buffers body-fn f32-bits)` returns
the shader as `bytes`. It declares `num-buffers` storage buffers of `f32`,
and `body-fn` receives a builder that emits the kernel's instructions:

```lisp
(def spv ((import "std/spirv")))

(def add-shader
  (spv:compute 64 3
    (fn [s]
      (let [id (s:global-id)
            a (s:load 0 id)
            b (s:load 1 id)]
        (s:store 2 id (s:fadd a b))))
    (fn [x] (error :this-kernel-has-no-f32-constants))))

(assert (= :bytes (type-of add-shader)))
(assert (= (slice add-shader 0 4) (bytes 3 2 35 7))
        "the SPIR-V magic word 0x07230203, little-endian")
```

The library defines opcode constants, a builder closure, and helpers
for common patterns:

- Storage classes, memory model, capabilities, decorations
- Type, constant, and pointer construction
- Arithmetic / comparison / bitwise opcodes
- Structured control flow (`OpLoopMerge`, `OpSelectionMerge`,
  `OpBranchConditional`)
- Buffer load/store via `OpAccessChain`

`f32-bits` converts an `f32` constant to its bit pattern. Elle has no native
`f32` value, so `gpu:compile` passes the vulkan plugin's `f32-bits`. A kernel
with no `f32` constant never calls it.

## Wire format

Both paths produce a SPIR-V binary that begins with the magic word
`0x07230203` (little-endian: `03 02 23 07`). Subsequent words declare
capabilities, the memory model, the entry point (`main`), and the
function body. Workgroup size is set via the `LocalSize` execution
mode, taken from `local-size-x` on the hand-written path and from the
workgroup size argument on the compiler path.

## Caching

The MLIR `MlirCache` carries a `spirv_cache: HashMap<*const u8, Vec<u8>>`
keyed by the closure's bytecode pointer. The key does not include the
workgroup size:

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

| File | Content |
|------|---------|
| [src/mlir/spirv.rs](../../src/mlir/spirv.rs) | Compiler path: LIR → MLIR `gpu.module` → SPIR-V bytes |
| [src/mlir/cache.rs](../../src/mlir/cache.rs) | `compile_spirv` and `get_spirv` on `MlirCache` |
| [src/vm/signal/query.rs](../../src/vm/signal/query.rs) | The handlers for `mlir/compile-spirv` and `git` |
| [src/primitives/introspection.rs](../../src/primitives/introspection.rs) | Primitive definitions: `mlir/compile-spirv`, `fn/gpu-eligible?` |
| [src/primitives/meta.rs](../../src/primitives/meta.rs) | Primitive definitions: `git`, `fn/git?`, `disgit` |
| [lib/spirv.lisp](../../lib/spirv.lisp) | Hand-written SPIR-V DSL |

The vulkan plugin turns SPIR-V into a compute pipeline in its `shader.rs`, in
the `plugins` submodule.

## Primitives

| Name | Signal | Returns |
|------|--------|---------|
| `mlir/compile-spirv` | query+errors | the SPIR-V `bytes` of a GPU-eligible closure; only in an MLIR build |
| `git` | query+errors+gpu | the closure, with its SPIR-V cached on the template; only in an MLIR build |
| `fn/git?` | silent | whether the closure's template holds SPIR-V; false for a non-closure |
| `disgit` | errors | the cached SPIR-V `bytes`; an error if the closure was never GIT'd |

```lisp
(def plain (fn [x] x))
(assert (not (fn/git? plain)))
(assert (not (fn/git? 1)))
(def [dis-ok? dis-err] (protect (disgit plain)))
(assert (not dis-ok?))
(assert (= (get dis-err :error) :mlir-error))
```

## See also

- [impl/mlir.md](mlir.md) — the LIR → MLIR lowering shared with the CPU path
- [impl/gpu.md](gpu.md) — Vulkan dispatch consuming SPIR-V bytes
- [impl/lir.md](lir.md) — the eligibility predicate and instruction whitelist
- [lib/spirv.lisp](../../lib/spirv.lisp) — the DSL's source
