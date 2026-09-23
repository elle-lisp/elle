# GPU Compute

<!-- audited: 2026-09-23 -->

How a plain Elle closure becomes a dispatched compute kernel, across the
MLIR backend and the Vulkan plugin.

> **Feature-gated:** compiler-generated kernels need an Elle built with
> `--features mlir`, the `mlir-translate` binary ([impl/spirv.md](spirv.md)),
> and the `vulkan` plugin from the `plugins` submodule. The plugin needs a
> driver with the `VK_KHR_external_fence_fd` extension. No CI job builds this
> combination, so nothing below runs on every build.

Three layers cooperate:

```text
┌──────────────────────────────────────────────────────────────┐
│  Elle: gpu:map over a (numeric!) closure                     │  lib/gpu.lisp
├──────────────────────────────────────────────────────────────┤
│  MLIR backend:  closure → LIR → SPIR-V bytes                 │  src/mlir/spirv.rs
├──────────────────────────────────────────────────────────────┤
│  Vulkan plugin: SPIR-V → pipeline → buffers → dispatch       │  plugins/vulkan/
└──────────────────────────────────────────────────────────────┘
```

The compiler (LIR → SPIR-V) and the runtime (Vulkan dispatch) are
independent — you can also write SPIR-V by hand with
[lib/spirv.lisp](../../lib/spirv.lisp), or load a pre-compiled `.spv` file.
The `gpu:map` convenience layer wires them together.

`std/gpu` takes the plugin as its `:vulkan` argument. Every example here is a
function this document defines and never calls, because the build that runs
it carries neither MLIR nor the plugin.

## Current state

The GPU path has defects that keep it from working end to end:

- **Only the first workgroup is right.** The generated kernel indexes its
  buffers with `gpu.thread_id x`, the index inside one workgroup, and has no
  bounds check. `gpu:map` dispatches `ceil(n / wg-size)` workgroups, so every
  workgroup computes the first `wg-size` elements again.
- **Only `:i64` matches the kernel.** The generated kernel reads and writes
  `i64` elements. `gpu:map` accepts `:dtype :i32`, `:u32` and `:f32`, and
  uploads 4-byte elements the kernel reads as 8-byte ones.
- **The workgroup size is not part of the SPIR-V cache key**
  ([impl/spirv.md](spirv.md)), so a second compile of one closure at a new
  size returns the first kernel.
- **Teardown crashes.** `VulkanState::drop` destroys the device, and the
  allocator field drops after it and frees its memory through the destroyed
  device.
- **The kernels declare the `Int64` capability,** and `init_vulkan` enables no
  device features.

## End-to-end: `gpu:map`

`(gpu:map f & input-arrays)` takes the named options `:ctx`, `:dtype` and
`:wg-size` after the input arrays. It returns an array of the results.

The closure must be GPU-eligible, and the stdlib arithmetic wrappers are not:
`*` is a call. Write the kernel with `%` intrinsics and prove their operands
with `(numeric!)`:

```lisp
(assert (fn/gpu-eligible? (fn [x] (numeric!) (%mul x x))))
(assert (not (fn/gpu-eligible? (fn [x] (* x x))))
        "a call to the stdlib * is not GPU-eligible")

(defn gpu-squares [vulkan xs]
  "Square each integer of xs on the GPU."
  (let [gpu ((import "std/gpu") :vulkan vulkan)]
    (gpu:map (fn [x] (numeric!) (%mul x x)) xs)))
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
7. Suspend the fiber: `(plugin:wait handle)` polls the GPU fence fd
   through the scheduler (no thread pool thread is held).
8. Decode: `(plugin:decode (plugin:collect handle) dtype)` produces
   an Elle array.

Workgroup count is `ceil(n / wg-size)` with `wg-size` defaulting to
256. `dtype` defaults to `:i64`.

GPU eligibility is the predicate in [impl/mlir.md](mlir.md). In short: numeric
constants, arithmetic, comparisons and local access; fixed arity; no mutable
cells, no calls, and no signal other than `:error`. Immutable numeric captures
are allowed for the MLIR-CPU tier but **not** for SPIR-V — the SPIR-V path
rejects closures with captures (they would need extra uniform buffers, which
is separate work).

## End-to-end: `gpu:compile` + `gpu:run`

For shaders the compiler can't generate (multi-buffer fused kernels,
custom layouts), use the hand-written DSL.
`(gpu:compile ctx local-size-x num-buffers body-fn)` returns a shader, and
`(gpu:run shader [x y z] buffers)` dispatches `x × y × z` workgroups and
returns the decoded `f32` results of the output buffers:

```lisp
(defn gpu-add [vulkan a b]
  "Add two f32 arrays element by element on the GPU."
  (let [gpu ((import "std/gpu") :vulkan vulkan)
        ctx (gpu:init)
        shader (gpu:compile ctx 64 3
                 (fn [s]
                   (let [id (s:global-id)]
                     (s:store 2 id (s:fadd (s:load 0 id) (s:load 1 id))))))]
    (gpu:run shader [(/ (+ (length a) 63) 64) 1 1]
             [(gpu:input a) (gpu:input b) (gpu:output (length a))])))
```

`body-fn` receives the SPIR-V builder context `s` and emits opcodes via
`s:load`, `s:store`, `s:fadd`, `s:global-id`, and the rest. The DSL's kernel
reads `s:global-id`, so each workgroup handles its own slice. See
[lib/spirv.lisp](../../lib/spirv.lisp) for the full opcode surface and
[impl/spirv.md](spirv.md) for the wire format.

## The vulkan plugin

`vulkan/init` creates `VkInstance` + `VkDevice` + queue (one of each;
no multi-device support), on the first physical device with a compute queue.
The state is wrapped in `Arc<Mutex<VulkanState>>`, which is what lets a shader
hold the context it was built against and lock it again at dispatch.

`vulkan/shader` accepts SPIR-V either as bytes (compiler-generated or
loaded into Elle) or as a string path to a `.spv` file, then builds
a `VkComputePipeline` with one storage buffer per binding.

`vulkan/dispatch` allocates the GPU buffers, uploads the input data,
records the dispatch, and submits it to the queue. It returns a handle
carrying the fence FD, and it does not wait.

`vulkan/wait` is the only async primitive. It returns
`SIG_YIELD | SIG_IO` with a poll request on the handle's fence FD, so the
fiber suspends until the GPU signals the fence and no thread-pool thread
is held.

`vulkan/collect` reads the output and in-out buffers back once the fence has
signalled.

`vulkan/submit` does all three in one call. It blocks the calling thread
on `wait_for_fences` rather than suspending the fiber, because the stable
ABI exposes no `IoRequest::task` for a plugin to return.

`vulkan/decode` parses the envelope `collect` returns. The element count is
always the buffer's size in bytes divided by four, whatever the element type;
the `:i64` decode reads pairs of those words:

```text
4 bytes        buffer count (u32 LE)
per buffer:
  4 bytes      byte size / 4 (u32 LE)
  4 × count    bytes of data
```

It answers one array for a single buffer, and an array of arrays for several.
`:raw` answers each buffer as `bytes`.

## Buffer specs

`gpu:input`, `gpu:output`, `gpu:inout` are convenience wrappers; the
underlying spec is a struct. A persistent buffer from `vulkan/persist` can
stand in the list in place of a spec.

| Key | Value | Effect |
|-----|-------|--------|
| `:data` | array | Upload to GPU |
| `:size` | int (bytes) | Allocate an output buffer |
| `:usage` | `:input` / `:output` / `:inout` | Direction |
| `:dtype` | `:f32` (default) / `:i64` / `:i32` / `:u32` | Element type of the upload |

## Eligibility, errors, and skipping

GPU eligibility (`fn/gpu-eligible?`) is a compile-time property of
the closure's LIR — it does not depend on runtime arguments. If a
closure isn't eligible, `mlir/compile-spirv` and `git` both signal a
`:mlir-error` whose message says the closure is not GPU-eligible, and the call
site has to fall back to the CPU.

`gpu:map` does not fall back on its own: if the closure is ineligible or the
GPU is missing, the error propagates. A test gates itself out with a
`:gated` error when a prerequisite is missing — see
[gpu-map.lisp](../../tests/elle/gpu-map.lisp).

## Files

```text
lib/gpu.lisp                     gpu:map, gpu:compile, gpu:run, buffer specs
lib/spirv.lisp                   Hand-written SPIR-V DSL
src/mlir/spirv.rs                Compiler-generated SPIR-V
src/primitives/meta.rs           git / fn/git? / disgit
src/primitives/introspection.rs  fn/gpu-eligible? / mlir/compile-spirv
src/lir/types/func.rs            is_gpu_eligible / is_mlir_cpu_eligible
src/lir/types/mod.rs             is_gpu_instruction
plugins/vulkan/src/lib.rs        Plugin entry, primitive table, buffer specs
plugins/vulkan/src/context.rs    VulkanState init + Drop
plugins/vulkan/src/shader.rs     SPIR-V → VkComputePipeline
plugins/vulkan/src/dispatch.rs   Buffer setup, command recording, fence export, readback
plugins/vulkan/src/decode.rs     Result bytes → Elle array
```

## Primitives

| Name | Signal | Purpose |
|------|--------|---------|
| `vulkan/init` | errors | Create Vulkan context |
| `vulkan/shader` | errors | Build a compute pipeline from SPIR-V |
| `vulkan/dispatch` | errors | Submit a compute dispatch (returns handle) |
| `vulkan/wait` | yields+io+errors | Suspend on the GPU fence fd |
| `vulkan/collect` | errors | Read result bytes |
| `vulkan/decode` | errors | Bytes → Elle array (`:f32`, `:u32`, `:i32`, `:i64`, or `:raw`) |
| `vulkan/submit` | errors | One-shot dispatch + wait, blocking the thread |
| `vulkan/f32-bits` | errors | IEEE 754 f32 bit pattern of a number |
| `vulkan/persist` | errors | Create a persistent GPU buffer |
| `vulkan/update` | errors | Re-upload data to a persistent GPU buffer |

No primitive here declares `:gpu`, so denying that bit withholds none of
them. The one primitive that declares `:gpu` is `git`, which compiles a
closure to SPIR-V and dispatches nothing. What a capability for the GPU
would have to ask instead is in
[signals/authority.md](../signals/authority.md).

## Configuration

The runtime config (`vm/config`) accepts the GPU trace keywords `:mlir`,
`:spirv` and `:gpu` from Elle, and none has a trace bit yet, so asking for
them turns nothing on:

```lisp
(def trace-before (get (vm/config) :trace))
(put (vm/config) :trace |:gpu :spirv|)
(assert (empty? (get (vm/config) :trace)) "the GPU keywords carry no bit")
(put (vm/config) :trace trace-before)
```

`--trace=gpu,spirv` on the command line is accepted too, and traces nothing.

## See also

- [impl/mlir.md](mlir.md) — LIR → MLIR lowering and CPU tier
- [impl/spirv.md](spirv.md) — SPIR-V emission paths and caching
- [impl/lir.md](lir.md) — eligibility predicate
- [plugins.md](../plugins.md) — the plugin system, and the submodule the Vulkan
  plugin's own reference lives in
- [lib/AGENTS.md](../../lib/AGENTS.md) — Elle library reference (lib/gpu, lib/spirv)
- [gpu-map.lisp](../../tests/elle/gpu-map.lisp),
  [gpu-select.lisp](../../tests/elle/gpu-select.lisp),
  [spirv.lisp](../../tests/elle/spirv.lisp) — runnable examples
