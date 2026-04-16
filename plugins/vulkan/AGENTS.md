# plugins/vulkan/ — Vulkan compute plugin

GPU compute dispatch via Vulkan. Uses `ash` (thin Vulkan bindings) and
`gpu-allocator` for buffer management. AMD RADV tested.

## Architecture

The plugin wraps Vulkan compute pipelines. `vulkan/submit` is the only
async primitive — it builds an `IoOp::Task` closure and returns
`(SIG_YIELD | SIG_IO, IoRequest::task(closure))`. The fiber suspends;
the thread pool runs the Vulkan dispatch + fence wait; the fiber resumes
with result bytes.

Thread safety: Vulkan state lives in `Arc<Mutex<VulkanState>>`. The Arc
is cloned into the Send closure. Numeric data is extracted from Elle
arrays to `Vec<f32>` before closure creation.

## Files

| File | Purpose |
|------|---------|
| `src/lib.rs` | Entry point, primitive table, primitive functions |
| `src/context.rs` | VulkanState, device init, Drop cleanup |
| `src/shader.rs` | SPIR-V loading, compute pipeline creation |
| `src/dispatch.rs` | Buffer setup, command recording, fence wait |
| `src/decode.rs` | Result bytes → Elle array conversion |
| `shaders/vecadd.comp` | GLSL vector addition (reference shader) |

## Primitives

| Name | Signal | Purpose |
|------|--------|---------|
| `vulkan/init` | errors | Create Vulkan instance + device + queue |
| `vulkan/shader` | errors | Load SPIR-V, create compute pipeline |
| `vulkan/submit` | yields+errors+io | Dispatch compute, suspend fiber |
| `vulkan/decode` | errors | Convert result bytes to Elle arrays |

## External types

- `"vulkan-ctx"` — wraps `GpuCtx { inner: Arc<Mutex<VulkanState>> }`
- `"vulkan-shader"` — wraps `GpuShader { ctx, pipeline, layout, ... }`

## Buffer spec format

`vulkan/submit` takes an array of buffer specs:
- `{:data [1.0 2.0 ...] :usage :input}` — upload only
- `{:size 16 :usage :output}` — readback only (size in bytes)
- `{:data [1.0 2.0 ...] :usage :inout}` — upload + readback

## Output bytes format

- 4 bytes: buffer count (u32 LE)
- Per output buffer: 4 bytes element count (u32 LE) + N*4 bytes f32 data
