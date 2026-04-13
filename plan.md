# GPU Backend Plan: LIR → SPIR-V via MLIR

## Context

Elle has three compilation backends: bytecode VM (interpreter), Cranelift
JIT (fast tier-1), and WASM/Wasmtime (tier-2, sandboxing). This branch
added a Vulkan compute plugin with a prototype SPIR-V emitter in Elle
(`lib/spirv.lisp`) and async GPU dispatch via fence fd polling.

The question: how do we make GPU compute a first-class compilation target
rather than a plugin that marshals data? And should we unify the backend
story while we're at it?

## Current State (this branch)

- `plugins/vulkan/` — Vulkan compute dispatch (ash + gpu-allocator)
- Three-phase async: dispatch/wait/collect, fence fd via io_uring
- `lib/spirv.lisp` — runtime SPIR-V emission from Elle builder calls
- `lib/gpu.lisp` — gpu:compile + gpu:run convenience layer
- SignalBits widened to u64 (room for GPU signals)
- IoRequest/IoOp made pub for plugin async I/O

## The Problem

Four separate codegen paths with no shared optimization:

```
LIR ─┬→ Bytecode (VM interpreter)
     ├→ Cranelift IR → x86 (JIT, ~1MB)
     ├→ WASM bytecode → Wasmtime (~15MB)
     └→ SPIR-V (GPU, proposed)
```

Each has its own instruction selector, its own bugs, its own maintenance.
Optimizations done for one don't help the others. Adding GPU as a fourth
standalone backend makes this worse.

## Proposed Architecture: MLIR

Replace Wasmtime (~15MB) with an MLIR pipeline that handles both CPU
tier-2 and GPU codegen through a single optimization + lowering stack.

### Compilation Tiers

| Tier | Backend | When | Binary Cost |
|------|---------|------|-------------|
| 0 | Bytecode VM | All code, always | 0 (built-in) |
| 1 | Cranelift JIT | Hot + non-polymorphic | ~1MB |
| 2 | MLIR → LLVM | Hot + numeric (CPU optimized) | ~40MB |
| 2g | MLIR → SPIR-V/AMDGPU | Explicit gpu/map or data-parallel | (shared with tier 2) |

### MLIR Pipeline

```
LIR → Elle MLIR Dialect → Standard Dialects → Backend
```

**Elle MLIR Dialect** (custom):
- `elle.call` — function call with signal propagation
- `elle.signal` — signal emission (error, yield, io, gpu)
- `elle.fiber_yield` — cooperative suspension point
- `elle.closure_env` — closure environment access
- `elle.scope_region` — scope-based memory lifetime

**Signal Lowering Pass**: Converts Elle dialect to standard MLIR:
- `elle.signal` → conditional branch
- `elle.fiber_yield` → coroutine intrinsics
- `elle.scope_region` → alloca + lifetime markers

**Standard MLIR Dialects** (after signal lowering):
- `arith` — arithmetic (add, mul, cmp) for int + float
- `scf` — structured control flow (for, while, if)
- `memref` — buffer alloc/dealloc, load/store, subview
- `func` — function definitions, call/return

**CPU Path** (tier 2):
```
Standard dialects → llvm dialect → LLVM IR → x86/ARM/WASM
```
LLVM optimization passes: loop vectorization, LICM, GVN, SROA, inlining.
Targets x86-64 (hot code), aarch64 (cross-compile), wasm32 (sandbox).

**GPU Path** (tier 2g):
```
Standard dialects → linalg dialect → gpu dialect → spirv / rocdl
```
- `linalg.generic` / `linalg.map` — data-parallel operations
- `gpu.launch_func` — kernel dispatch
- `gpu.alloc` / `gpu.memcpy` — device memory management
- `spirv.module` — portable Vulkan SPIR-V output
- `rocdl` — AMD-native GPU ISA (bypasses SPIR-V for perf)

### Dispatch Model

**Opt-in to start.** GPU dispatch is expensive (buffer transfer, kernel
launch overhead). Users control when it happens:

```lisp
(gpu/map f data)           ;; explicit GPU dispatch
(gpu/reduce f init data)   ;; explicit GPU reduction
```

The compiler emits SPIR-V for `f` via the MLIR GPU path. `f` must be
silent + numeric (enforced by signal analysis). The runtime handles
buffer allocation, H2D/D2H transfer, and async dispatch.

**Automatic promotion** is a future goal: the compiler detects data-parallel
patterns in hot loops and offers to dispatch to GPU. This requires cost
modeling (is the data large enough to amortize transfer overhead?).

### Signal-Driven Eligibility

Signal analysis (already computed in HIR) determines what can compile where:

| Signal Profile | VM | Cranelift | MLIR CPU | MLIR GPU |
|---------------|-----|-----------|----------|----------|
| Polymorphic | yes | no | no | no |
| Silent | yes | yes | yes | candidate |
| Silent + numeric | yes | yes | yes | yes |
| Yields | yes | yes (side-exit) | yes | no |
| I/O | yes | no | no | no |

"Numeric" means: no heap allocation, no closures, no strings — only
int/float arithmetic, comparisons, and array element access.

### Memory Management

**Buffer Pool** (per GPU context):
- Cache allocations by (size, memory_type)
- Reuse on dispatch, free to pool on collect
- Actually free on context destroy or under memory pressure

**Persistent Device Buffers**:
- `gpu-buffer` type that lives on device across dispatches
- Avoids redundant H2D transfers for read-only data
- Invalidation protocol when host data changes

**Pinned Host Memory**:
- AMD RADV with ReBAR: HOST_VISIBLE | DEVICE_LOCAL (zero-copy)
- gpu-allocator already selects optimal memory type

**Command Pool Recycling**:
- One command pool per thread (not per dispatch)
- Reset between dispatches instead of create/destroy

### User-Supplied Kernels

Three levels of escape hatch:

1. **Pre-compiled SPIR-V** — `(gpu:load-shader ctx "kernel.spv" 3)`
2. **Elle SPIR-V builder** — `(spv:compute 256 3 (fn [s] ...))`
3. **Compiler-generated** — `(gpu/map f data)` compiles `f` to SPIR-V

All three produce the same thing: a Vulkan compute pipeline. The runtime
doesn't care how the SPIR-V was generated.

### Integration with Async Scheduler

GPU operations integrate with the existing io_uring scheduler:

1. `gpu/dispatch` — submit command buffer + create exportable fence
2. Fence fd registered with io_uring via `IoOp::PollFd`
3. Fiber suspends (zero threads consumed)
4. io_uring signals fence completion
5. Fiber resumes, does readback via `gpu/collect`

Cross-thread dispatch: `Arc<Mutex<VulkanState>>` supports submission from
any thread. Per-thread command pools avoid contention.

## Implementation Phases

### Phase 1: Stabilize Current Plugin (weeks)
- Buffer pool in plugins/vulkan/
- Command pool recycling
- Persistent device buffers (gpu-buffer type)
- Extend lib/spirv.lisp: loops, local variables (for mandelbrot)
- Mandelbrot GPU demo

### Phase 2: MLIR Integration (months)
- Add melior (Rust MLIR bindings) or llvm-sys dependency
- Define Elle MLIR dialect ops
- LIR → Elle dialect lowering
- Signal lowering pass
- LLVM backend (replace Wasmtime for tier-2 CPU)

### Phase 3: GPU Codegen via MLIR (months)
- gpu/map primitive with compiler support
- Numeric function detection in HIR
- MLIR GPU path: linalg → gpu → spirv
- Connect to existing Vulkan runtime

### Phase 4: Optimization (ongoing)
- Kernel fusion (multiple gpu/map calls → single dispatch)
- Cost modeling for automatic GPU promotion
- Shared memory / workgroup optimizations
- AMD-native path via rocdl dialect

## Open Questions

1. **melior vs llvm-sys**: melior wraps MLIR C API but is less mature.
   llvm-sys is battle-tested but doesn't expose MLIR. May need both,
   or direct C API bindings.

2. **Cranelift coexistence**: Keep Cranelift for tier-1 (fast compile)
   even with LLVM for tier-2? Or replace entirely? Cranelift's 1MB
   footprint and fast compilation are hard to beat for JIT.

3. **WASM story**: LLVM has a wasm32 target. Does this replace Wasmtime
   entirely, or do we keep Wasmtime for its sandbox guarantees?

4. **Build complexity**: LLVM/MLIR are C++ libraries. Building from
   source adds significant compile time. System packages (Gentoo
   `llvm`, `mlir`) reduce this but create version coupling.

5. **Fiber ↔ GPU workgroup mapping**: Can we lower `fiber/new` on a
   silent+numeric function to a GPU workgroup dispatch? This would
   make fibers the uniform parallelism primitive across CPU and GPU.
