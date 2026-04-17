# GPU Compute

> **Feature-gated:** End-to-end GPU compute requires `--features mlir`
> (for compiler-generated SPIR-V) and the `vulkan` plugin built and
> loadable. Tested on AMD RADV; any Vulkan 1.0 + `Int64` driver should
> work.

The GPU pipeline turns a plain Elle closure into a dispatched compute
kernel. Three layers cooperate:

```text
┌──────────────────────────────────────────────────────────────┐
│  Elle: (gpu:map (fn [x] (* x x)) [1 2 3 4])                  │  lib/gpu.lisp
├──────────────────────────────────────────────────────────────┤
│  MLIR backend:  closure → LIR → SPIR-V bytes                 │  src/mlir/spirv.rs
├──────────────────────────────────────────────────────────────┤
│  Vulkan plugin: SPIR-V → pipeline → buffers → dispatch       │  plugins/vulkan/
└──────────────────────────────────────────────────────────────┘
```

The compiler (LIR → SPIR-V) and the runtime (Vulkan dispatch) are
independent — you can also write SPIR-V by hand with `lib/spirv.lisp`,
or load a pre-compiled `.spv` file. The `gpu:map` convenience layer
wires them together.

## End-to-end: `gpu:map`

```text
(gpu:map f & input-arrays &named ctx dtype wg-size)
```

What happens:

1. Verify `(fn/arity f)` matches the number of input arrays.
2. Verify all input arrays have the same length `n`.
3. SPIR-V: if `(fn/git? f)` use cached bytes via `(disgit f)`,
   otherwise compile fresh via `(mlir/compile-spirv f wg-size)`.
4. Build the compute pipeline: `(plugin:shader ctx spirv num-bufs)`.
5. Build buffer specs: each input gets `{:data ... :usage :input
   :dtype dtype}`; one output buffer of size `n * elem-size`.
6. Dispatch: `(plugin:dispatch shader wg-count 1 1 bufs)` returns a
   handle.
7. Suspend the fiber: `(plugin:wait handle)` blocks on the GPU fence
   fd via `IoOp::Task` (no thread pool thread is held).
8. Decode: `(plugin:decode (plugin:collect handle) dtype)` produces
   an Elle array.

Workgroup count is `ceil(n / wg-size)` with `wg-size` defaulting to
256. `dtype` defaults to `:i64`; `:i32`, `:u32`, and `:f32` are also
recognized.

The closure must be GPU-eligible — see
[impl/mlir.md](mlir.md) for the predicate. In short: pure arithmetic,
fixed arity, no mutable cells, no calls, no signals other than
`:error`. Immutable numeric captures are allowed for the MLIR-CPU
tier but **not** for SPIR-V — the SPIR-V path rejects closures with
captures (they would need extra uniform buffers, which is separate
work).

## End-to-end: `gpu:compile` + `gpu:run`

For shaders the compiler can't generate (multi-buffer fused kernels,
custom layouts), use the hand-written DSL:

```text
(gpu:compile ctx local-size-x num-buffers body-fn)
  → shader

(gpu:run shader [x y z] [(gpu:input ...) (gpu:output n) ...])
  → array of decoded f32 results
```

`body-fn` is a closure that receives the SPIR-V builder context `s`
and emits opcodes via `s:load`, `s:store`, `s:fadd`, `s:global-id`,
etc. See `lib/spirv.lisp` for the full opcode surface and
[impl/spirv.md](spirv.md) for the wire format.

## The vulkan plugin

`vulkan/init` creates `VkInstance` + `VkDevice` + queue (one of each;
no multi-device support). The state is wrapped in
`Arc<Mutex<VulkanState>>` so it can be cloned into Send closures for
the thread pool.

`vulkan/shader` accepts SPIR-V either as bytes (compiler-generated or
loaded into Elle) or as a string path to a `.spv` file, then builds
a `VkComputePipeline` with one storage buffer per binding.

`vulkan/submit` is the only async primitive. It:

1. Takes an array of buffer specs.
2. Builds a Send closure that allocates GPU buffers, uploads input
   data, records the dispatch, submits to the queue, waits on a
   fence FD.
3. Returns `(SIG_YIELD | SIG_IO, IoRequest::task(closure))` — the
   fiber suspends, the thread pool runs the closure, the fiber
   resumes with the result bytes.

Numeric data is extracted from Elle arrays into `Vec<f32>` (or
`Vec<i64>`, etc., per `dtype`) **before** the closure is built — the
closure carries plain `Vec<T>` so it satisfies `Send`.

`vulkan/decode` parses the result-bytes envelope:

```text
4 bytes      buffer count (u32 LE)
per buffer:
  4 bytes    element count (u32 LE)
  N*size bytes  data
```

## Buffer specs

`gpu:input`, `gpu:output`, `gpu:inout` are convenience wrappers; the
underlying spec is a struct:

| Key | Value | Effect |
|-----|-------|--------|
| `:data` | array | Upload to GPU |
| `:size` | int (bytes) | Allocate output |
| `:usage` | `:input` / `:output` / `:inout` | Direction |
| `:dtype` | `:i64` / `:i32` / `:u32` / `:f32` | Element type for upload + decode |

## Eligibility, errors, and skipping

GPU eligibility (`fn/gpu-eligible?`) is a compile-time property of
the closure's LIR — it does not depend on runtime arguments. If a
closure isn't eligible, `mlir/compile-spirv` and `git` both fail with
`mlir-error :reason :not-gpu-eligible` and the call site has to fall
back to CPU.

`gpu:map` does not auto-fallback: if the closure is ineligible or the
GPU is missing, the error propagates. Tests use `(protect ...)` to
detect missing prerequisites and `(exit 0)` with a SKIP message —
see `tests/elle/gpu-map.lisp` for the canonical pattern.

## Files

```text
lib/gpu.lisp                     gpu:map, gpu:compile, gpu:run, buffer specs
lib/spirv.lisp                   Hand-written SPIR-V DSL
src/mlir/spirv.rs                Compiler-generated SPIR-V
src/primitives/meta.rs           git / fn/git? / disgit
src/primitives/introspection.rs  fn/gpu-eligible? / mlir/compile-spirv
src/lir/types.rs                 is_gpu_eligible / is_gpu_instruction
plugins/vulkan/src/lib.rs        Plugin entry, primitive table
plugins/vulkan/src/context.rs    VulkanState init + Drop
plugins/vulkan/src/shader.rs     SPIR-V → VkComputePipeline
plugins/vulkan/src/dispatch.rs   Buffer setup, command recording, fence wait
plugins/vulkan/src/decode.rs     Result bytes → Elle array
```

## Primitives

| Name | Signal | Purpose |
|------|--------|---------|
| `vulkan/init` | errors | Create Vulkan context |
| `vulkan/shader` | errors | Build a compute pipeline from SPIR-V |
| `vulkan/dispatch` | errors | Submit a compute dispatch (returns handle) |
| `vulkan/wait` | yields+io+errors | Block on the GPU fence |
| `vulkan/collect` | errors | Read result bytes |
| `vulkan/decode` | errors | Bytes → Elle array (per dtype) |
| `vulkan/submit` | yields+io+errors | One-shot dispatch + wait |

## Configuration

The runtime config (`vm/config`) recognizes the following GPU-related
trace keywords (currently silent forward-compat — they have no
dedicated bit yet):

- `:mlir` — MLIR compilation events
- `:spirv` — SPIR-V emission events
- `:gpu` — GPU dispatch events

Set with `--trace=gpu,spirv` on the CLI or
`(put (vm/config) :trace |:gpu :spirv|)` from Elle.

## See also

- [impl/mlir.md](mlir.md) — LIR → MLIR lowering and CPU tier
- [impl/spirv.md](spirv.md) — SPIR-V emission paths and caching
- [impl/lir.md](lir.md) — eligibility predicate
- `plugins/vulkan/AGENTS.md` — plugin internals
- `lib/AGENTS.md` (lib/gpu, lib/spirv) — Elle library reference
- `tests/elle/gpu-map.lisp`, `tests/elle/gpu-select.lisp`,
  `tests/elle/spirv.lisp` — runnable examples
