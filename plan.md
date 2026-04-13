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

### Vulkan Plugin Architecture

```
plugins/vulkan/src/
  lib.rs       — 8 primitives: init, shader, dispatch, wait, collect, submit, decode, f32-bits
  context.rs   — VulkanState: Entry, Instance, Device, PhysicalDevice, Queue, Allocator, fence_fd_fn
  shader.rs    — GpuShader: pipeline, pipeline_layout, descriptor_set_layout, shader_module
  dispatch.rs  — GpuHandle: ctx, fence, fence_fd, command_pool, descriptor_pool, buffers
  decode.rs    — Result bytes → Elle array conversion (f32 only)
```

**Async dispatch flow** (from `gpu:run` in `lib/gpu.lisp`):
```
plugin:dispatch(shader, wg_x, wg_y, wg_z, buffers)
  ├─ Lock VulkanState
  ├─ Create command pool (TRANSIENT) + command buffer
  ├─ Allocate buffers via gpu-allocator (host-mapped)
  ├─ Upload input data via mapped pointer memcpy
  ├─ Create descriptor set, bind buffers as STORAGE_BUFFER
  ├─ Record vkCmdDispatch(wg_x, wg_y, wg_z)
  ├─ Memory barrier: SHADER_WRITE → HOST_READ
  ├─ Create fence with VK_EXTERNAL_FENCE_HANDLE_TYPE_SYNC_FD
  ├─ vkQueueSubmit + vkGetFenceFdKHR → export fence fd
  └─ Drop lock (GPU works independently)

plugin:wait(handle)
  └─ (SIG_YIELD | SIG_IO, IoRequest::poll_fd(fence_fd, POLLIN))
      └─ Fiber suspends → io_uring POLL_ADD or thread-pool poll(2)
         └─ GPU fence signals → fiber resumes

plugin:collect(handle)
  ├─ Read output buffers via mapped pointers
  └─ Encode: [u32 count, per-buffer: u32 elem_count + N×4 bytes f32 data]

plugin:decode(bytes, :f32)
  └─ → array of f32 arrays
```

### SPIR-V Builder (`lib/spirv.lisp`, 325 lines)

8 functions total. MCP signal analysis:
- **Silent (4)**: `string-word-count`, `encode-word`, `emit-inst`, `make-module`
- **Yielding (4)**: `compute`, `serialize`, `emit-entry-point`, `string-to-words`

The yield comes from polymorphic `body-fn` callback and `serialize`'s `map`
calls, not from I/O.

**Supported builder ops** (on shader context `s`):
| Category | Ops |
|----------|-----|
| Memory | `global-id`, `load buf idx`, `store buf idx val` |
| Float arith | `fadd`, `fsub`, `fmul`, `fdiv`, `fmod` |
| Int arith | `iadd`, `isub`, `imul`, `idiv`, `umod` |
| Float cmp | `flt`, `fgt`, `fle`, `feq` |
| Int cmp | `slt` |
| Conversion | `u2f`, `f2u` |
| Constants | `const-f`, `const-u` |
| Selection | `select` (f32), `select-u` (u32) |

### GPU Library (`lib/gpu.lisp`, 77 lines)

7 functions. MCP signal analysis:
- **Silent (3)**: `gpu-input`, `gpu-output`, `gpu-inout`
- **Yielding (4)**: `gpu-load-shader`, `gpu-compile`, `gpu-init`, `gpu-run`

## The Problem

Four separate codegen paths with no shared optimization:

```
LIR ─┬→ Bytecode (VM interpreter)             src/lir/emit/
     ├→ Cranelift IR → x86 (JIT, ~1MB)        src/jit/
     ├→ WASM bytecode → Wasmtime (~15MB)       src/wasm/
     └→ SPIR-V (GPU, proposed)                 lib/spirv.lisp (runtime)
```

Each has its own instruction selector, its own bugs, its own maintenance.
Optimizations done for one don't help the others. Adding GPU as a fourth
standalone backend makes this worse.

### Backend Comparison

| Aspect | Bytecode | Cranelift JIT | WASM/Wasmtime |
|--------|----------|---------------|---------------|
| Form | Stack-based, ~80 opcodes | Register-based Cranelift IR | State machine + CPS |
| Yield | `Emit` opcode | Side-exit sentinel (0xDEAD_CAFE...) | CPS resume via br_table |
| Fast path | None | Diamond CFG for int arith | None |
| Self-tail-call | Interpreter loop | Native loop (3.2× speedup) | State variable loop |
| Signal gate | All code | Rejects polymorphic, MakeClosure | Rejects MakeClosure, TailCall, Yield |

## Vulkan Plugin Gap Analysis

Current `dispatch.rs` allocates every Vulkan resource per-dispatch. No reuse.

| Resource | Current Code | Phase 1 Target |
|----------|-------------|----------------|
| Command pool | `create_command_pool` per dispatch (line 85, TRANSIENT flag) | Per-thread pool, `vkResetCommandPool` between dispatches |
| Descriptor pool | `create_descriptor_pool` per dispatch (line 123) | Recycling pool or `vkResetDescriptorPool` |
| Buffers | `create_buffers()` allocates fresh via gpu-allocator (line 259) | Pool keyed by `(size_bucket, MemoryLocation)` |
| Fences | `create_fence` with `ExportFenceCreateInfo` per dispatch (line 199) | Verify reuse after reset with SYNC_FD handle type |
| Data types | f32 only (decode.rs hardcodes f32) | f32, i32, u32 |
| Persistent bufs | None; `GpuHandle::drop` frees everything | `gpu-buffer` value type with generation counter |

### Buffer Pool Design

```rust
struct BufferPool {
    free: HashMap<(SizeBucket, MemoryLocation), Vec<(vk::Buffer, Allocation)>>,
}

impl BufferPool {
    fn acquire(&mut self, size: usize, loc: MemoryLocation) -> Option<(vk::Buffer, Allocation)>;
    fn release(&mut self, buf: vk::Buffer, alloc: Allocation, size: usize, loc: MemoryLocation);
    fn trim(&mut self, max_per_bucket: usize);  // memory pressure
}
```

Size bucketing: round up to next power of 2 (256, 512, 1K, ..., 256M).
Memory locations from `dispatch.rs` lines 275-279:
- `BufferUsage::Input` → `MemoryLocation::CpuToGpu`
- `BufferUsage::Output` → `MemoryLocation::GpuToCpu`
- `BufferUsage::InOut` → `MemoryLocation::CpuToGpu`

## SPIR-V Builder Gap Analysis

### Missing for Mandelbrot

The inner loop `while |z|² < 4 && iter < max_iter` requires structured
control flow that the builder doesn't support.

**Required SPIR-V opcodes** (not yet emitted):
| Opcode | Number | Purpose |
|--------|--------|---------|
| `OpBranch` | 249 | Unconditional branch |
| `OpBranchConditional` | 250 | Conditional branch |
| `OpLoopMerge` | 246 | Structured loop header annotation |
| `OpSelectionMerge` | 247 | Structured if header annotation |
| `OpVariable` | 59 | Function-scoped local variable |
| `OpPhi` | 245 | Loop-carried dependency (alt: use local vars) |
| `OpLogicalAnd` | 167 | Combine loop conditions |
| `OpLogicalNot` | 168 | Negate condition |
| `OpSGreaterThan` | 187 | Signed integer comparison |

### New Builder API

```lisp
;; ── Control flow ──────────────────────────────────────────
(s:block)                          ;; allocate a label ID (no emission)
(s:begin-block lbl)                ;; emit OpLabel, start new basic block
(s:branch lbl)                     ;; unconditional branch (terminates block)
(s:branch-cond cond then else)     ;; conditional branch (terminates block)
(s:loop-merge merge continue)      ;; must immediately precede branch/branch-cond
(s:selection-merge merge)          ;; must immediately precede branch-cond

;; ── Local variables ───────────────────────────────────────
(s:var-f)                          ;; OpVariable Function storage, f32 ptr
(s:var-u)                          ;; OpVariable Function storage, u32 ptr
;;   both return {:id <spir-v-id> :type <type-id>}
(s:load-var var)                   ;; OpLoad from function-scoped variable
(s:store-var var val)              ;; OpStore to function-scoped variable

;; ── Comparison / logic ────────────────────────────────────
(s:sgt a b)                        ;; signed greater-than (u32 operands)
(s:logical-and a b)                ;; boolean AND
(s:logical-not a)                  ;; boolean NOT

;; ── Integer bitwise ──────────────────────────────────────
(s:ior a b)                        ;; OpBitwiseOr (197)
(s:iand a b)                       ;; OpBitwiseAnd (199)
(s:ishl a b)                       ;; OpShiftLeftLogical (196)
(s:ishr a b)                       ;; OpShiftRightLogical (194)
(s:umin a b)                       ;; min(a,b) via OpSelect + OpSLessThan

;; ── Type reinterpretation ────────────────────────────────
(s:bitcast-u2f val)                ;; OpBitcast u32→f32 (preserves bits)
(s:bitcast-f2u val)                ;; OpBitcast f32→u32 (preserves bits)
```

### Mandelbrot Kernel (actual, from `demos/mandelbrot/mandelbrot.lisp`)

The production shader computes coordinates from `global-id` + viewport
params and outputs ARGB32 pixels directly via `bitcast`. No CPU-side
coordinate generation or color mapping.

```
Buffer 0: params [x-min y-min dx dy width max-iter] (input, 6 f32)
Buffer 1: pixels (output, W×H f32 holding u32 ARGB32 bit patterns)
```

Key techniques:
- **Coord from global-id**: `px = id % width`, `py = id / width` (integer
  division via f32 truncation), `cx = x-min + px * dx`, `cy = y-min + py * dy`
- **Max-iter via params buffer**: shader compiled once, max-iter passed
  per-dispatch. No recompilation when user changes iteration count.
- **ARGB32 color in shader**: Bernstein polynomial palette computed per-pixel.
  `ior`/`ishl` pack RGB channels, `bitcast-u2f` stores raw u32 bits.
- **Raw bytes blit**: `plugin:collect` returns raw bytes, written directly
  to Cairo pixel buffer via `ffi/write` (skip 8-byte collect header).
  No decode, no per-pixel iteration on CPU.

Performance (RX 7900 XTX, 1600×1200 @ 256 iterations):
- First frame: ~23ms (includes SPIR-V compilation + pipeline creation)
- Subsequent frames: ~10ms dispatch + ~4ms blit = **~14ms total**
```

## Proposed Architecture: MLIR

Replace Wasmtime (~15MB) with an MLIR pipeline that handles both CPU
tier-2 and GPU codegen through a single optimization + lowering stack.
Net binary cost change: ~40MB MLIR replaces ~15MB Wasmtime = +25MB.

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

### Elle MLIR Dialect (custom)

**Types**:
- `!elle.value` — 16-byte tagged union (`tag: i64, payload: i64`),
  matching the JIT calling convention in `src/jit/compiler.rs` lines 87-97

**Operations**:

```mlir
// Function with signal metadata (from LirFunction.signal)
elle.func @add(%arg0: !elle.value, %arg1: !elle.value) -> !elle.value
    attributes { signal_bits = 0 : i64, propagates = 0 : i32 } {
  // ...
}

// Call with signal routing (Call vs SuspendingCall from LIR)
%result = elle.call @f(%x, %y) : (!elle.value, !elle.value) -> !elle.value
    { callee_signal_bits = 0 : i64 }

// Signal emission (from Emit terminator, src/lir/types.rs:556-560)
elle.signal %value { bits = 2 : i64 }, ^resume_block

// Fiber yield (special case of signal: bits = SIG_YIELD)
elle.fiber_yield %value, ^resume_block

// Closure environment access (from LoadCapture/StoreCapture)
%val = elle.load_capture %env[3] : !elle.value
elle.store_capture %env[3], %val : !elle.value

// Scope allocation region (from RegionEnter/RegionExit)
elle.scope_region {
  // allocations freed on exit
}
```

### Signal Lowering Pass

Converts Elle dialect to standard MLIR (runs before backend selection):
- `elle.signal` → conditional branch to signal handler + resume continuation
- `elle.fiber_yield` → coroutine intrinsics (`llvm.coro.suspend` on CPU path)
- `elle.scope_region` → `memref.alloca` + lifetime markers
- `elle.call` with `callee_signal_bits != 0` → call + signal check branch
- `elle.load_capture`/`elle.store_capture` → `memref.load`/`memref.store` on env pointer

### Numeric Specialization Pass

For GPU-eligible functions (see detection criteria below), replaces tagged
values with unboxed scalars:
- `!elle.value` → `i64` or `f64` (inferred from usage)
- `elle.call` → `func.call` with unboxed signature
- `BinOp` on `!elle.value` → `arith.addi`/`arith.addf` directly

### Standard MLIR Dialects (after signal lowering)

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
- `rocdl` — AMD-native GPU ISA (long-term, requires ROCm dependency)

## LIR → MLIR Instruction Mapping

Every `LirInstr` variant from `src/lir/types.rs:315-503` classified by
target eligibility. The LIR has ~60 instruction variants in SSA form.

### GPU-Eligible Instructions

These lower to standard MLIR dialects and can target both CPU and GPU:

| LIR Instruction | MLIR Op | Notes |
|----------------|---------|-------|
| `Const { Int(i64) }` | `arith.constant : i64` | |
| `Const { Float(f64) }` | `arith.constant : f64` | |
| `Const { Bool(b) }` | `arith.constant : i1` | |
| `Const { Nil }` | `arith.constant 0 : i64` | nil as zero for numeric contexts |
| `BinOp { Add }` | `arith.addi` / `arith.addf` | type-dispatched |
| `BinOp { Sub }` | `arith.subi` / `arith.subf` | |
| `BinOp { Mul }` | `arith.muli` / `arith.mulf` | |
| `BinOp { Div }` | `arith.divsi` / `arith.divf` | |
| `BinOp { Rem }` | `arith.remsi` / `arith.remf` | |
| `BinOp { BitAnd }` | `arith.andi` | integer only |
| `BinOp { BitOr }` | `arith.ori` | |
| `BinOp { BitXor }` | `arith.xori` | |
| `BinOp { Shl }` | `arith.shli` | |
| `BinOp { Shr }` | `arith.shrsi` | arithmetic shift |
| `UnaryOp { Neg }` | `arith.negf` / `arith.subi(0, x)` | |
| `UnaryOp { Not }` | `arith.xori(x, true)` | boolean not |
| `UnaryOp { BitNot }` | `arith.xori(x, -1)` | bitwise complement |
| `Compare { Eq..Ge }` | `arith.cmpi` / `arith.cmpf` | predicate variants |
| `LoadLocal { slot }` | `memref.load %locals[slot]` | stack locals as memref |
| `StoreLocal { slot, src }` | `memref.store %src, %locals[slot]` | |
| `Return(reg)` | `func.return` / `spirv.Return` | |
| `Jump(label)` | `cf.br ^label` | |
| `Branch { cond, then, else }` | `cf.cond_br %cond, ^then, ^else` | |

### CPU-Only Instructions

| LIR Instruction | Reason | MLIR (CPU only) |
|----------------|--------|-----------------|
| `Call` / `SuspendingCall` / `TailCall` | GPU kernels cannot call functions | `func.call` / `elle.call` |
| `MakeClosure` | Closures are heap objects | `elle.make_closure` → runtime call |
| `LoadCapture` / `StoreCapture` / `LoadCaptureRaw` | Closure environment | `elle.load_capture` / `elle.store_capture` |
| `MakeCaptureCell` / `LoadCaptureCell` / `StoreCaptureCell` | Mutable capture indirection | runtime calls |
| `Cons` / `Car` / `Cdr` | Linked-list heap allocation | runtime calls |
| `MakeArrayMut` | Heap array allocation | runtime calls |
| `ArrayMutLen` / `ArrayMutExtend` / `ArrayMutPush` | Heap array mutation | runtime calls |
| `CallArrayMut` / `TailCallArrayMut` | Splice-based calls | runtime calls |
| `IsNil` / `IsPair` / `IsArray` / `IsArrayMut` | Tagged-union type dispatch | `arith.cmpi` on tag field |
| `IsStruct` / `IsStructMut` / `IsSet` / `IsSetMut` | Tagged-union type dispatch | `arith.cmpi` on tag field |
| `CarDestructure` / `CdrDestructure` | Error-signaling destructuring | runtime calls |
| `ArrayMutRefDestructure` / `ArrayMutSliceFrom` | Bounds-checked access | runtime calls |
| `StructGetOrNil` / `StructGetDestructure` / `StructRest` | Struct field access | runtime calls |
| `CarOrNil` / `CdrOrNil` / `ArrayMutRefOrNil` | Silent destructuring | runtime calls |
| `Eval` | Runtime compilation | not lowerable |
| `PushParamFrame` / `PopParamFrame` | Dynamic parameters | runtime calls |
| `CheckSignalBound` | Runtime signal checking | runtime calls |
| `RegionEnter` / `RegionExit` / `RegionExitCall` | Scope allocation | `elle.scope_region` |
| `LoadResumeValue` | Coroutine resume | `elle.fiber_yield` resume |
| `ValueConst` | Runtime heap values | `elle.load_const` |
| `Emit` terminator | Signal emission | `elle.signal` (CPU only) |

## Signal-Driven Eligibility

### Signal System Reference

`Signal` from `src/signals/mod.rs`:
```rust
pub struct Signal {
    pub bits: SignalBits,      // u64 bitmask: which signals this function may emit
    pub propagates: u32,       // bitmask: which parameter indices propagate signals
}
```

Built-in signal bits (`src/signals/mod.rs`):
| Bit | Constant | Meaning |
|-----|----------|---------|
| 0 | `SIG_ERROR` | Exception/panic |
| 1 | `SIG_YIELD` | Cooperative suspension |
| 2 | `SIG_DEBUG` | Breakpoint/trace |
| 3 | `SIG_RESUME` | Fiber resumption (VM-internal) |
| 4 | `SIG_FFI` | Foreign function call |
| 8 | `SIG_HALT` | VM termination |
| 9 | `SIG_IO` | I/O request to scheduler |
| 11 | `SIG_EXEC` | Subprocess capability |
| 12 | `SIG_FUEL` | Instruction budget exhaustion |
| 13 | `SIG_SWITCH` | Fiber switch (VM-internal) |
| 14 | `SIG_WAIT` | Structured concurrency wait |
| 15 | (reserved) | GPU signal (not yet defined) |
| 16-31 | user | Up to 16 user-defined signals per compilation unit |

Key predicates (`src/signals/mod.rs`):
- `may_suspend()`: `bits & (SIG_YIELD | SIG_DEBUG) != 0 || propagates != 0`
- `may_yield()`: `bits & SIG_YIELD != 0`
- `is_polymorphic()`: `propagates != 0`

### Compilation Gate Table

Signal analysis (computed in HIR, stored on `LirFunction.signal`) determines
what can compile where:

| Signal Profile | VM | Cranelift JIT | MLIR CPU | MLIR GPU |
|---------------|-----|---------------|----------|----------|
| Polymorphic (`propagates != 0`) | yes | no | no | no |
| Silent (`bits == 0, propagates == 0`) | yes | yes | yes | candidate |
| Silent + numeric (see below) | yes | yes | yes | **yes** |
| Yields (`SIG_YIELD`) | yes | yes (side-exit) | yes (coroutine) | no |
| I/O (`SIG_YIELD \| SIG_IO`) | yes | yes (side-exit) | yes (coroutine) | no |
| Errors only (`SIG_ERROR`) | yes | yes | yes | no |

Strict subset relationship: **GPU-eligible ⊂ JIT-eligible ⊂ VM-eligible**.

### Existing JIT Compilation Gate (`src/jit/compiler.rs`)

Single-function JIT rejects:
1. `signal.propagates != 0` — polymorphic (line ~108)
2. `MakeClosure` in body (lines ~130-136)
3. Struct/named varargs with ≥1 params (lines ~117-123)

Batch JIT (`src/jit/group.rs`) additionally rejects:
4. `signal.may_suspend()` (line ~71)
5. `num_captures > 0` (line ~76)
6. `Eval` instruction (line ~144)

### Numeric Function Detection (new analysis)

A function is **numeric** (GPU-eligible) when it satisfies all of:

```rust
fn is_gpu_eligible(f: &LirFunction) -> bool {
    // ── Signal checks (cheapest, first) ─────────────────
    f.signal.bits == SignalBits::EMPTY       // completely silent
    && f.signal.propagates == 0              // not polymorphic
    // ── Structural checks ───────────────────────────────
    && f.num_captures == 0                   // no closure environment
    && matches!(f.arity, Arity::Exact(_))    // fixed arity
    && f.capture_params_mask == 0            // no mutable param cells
    && f.capture_locals_mask == 0            // no mutable local cells
    // ── Instruction whitelist (most expensive, last) ────
    && f.blocks.iter().all(|b| {
        b.instructions.iter().all(|si| is_gpu_instruction(&si.instr))
        && is_gpu_terminator(&b.terminator.terminator)
    })
}

fn is_gpu_instruction(i: &LirInstr) -> bool {
    matches!(i,
        LirInstr::Const { value: LirConst::Int(_) | LirConst::Float(_)
                                | LirConst::Bool(_) | LirConst::Nil, .. }
        | LirInstr::BinOp { .. }
        | LirInstr::UnaryOp { .. }
        | LirInstr::Compare { .. }
        | LirInstr::LoadLocal { .. }
        | LirInstr::StoreLocal { .. }
    )
}

fn is_gpu_terminator(t: &Terminator) -> bool {
    matches!(t,
        Terminator::Return(_) | Terminator::Jump(_) | Terminator::Branch { .. }
    )
}
```

**Composition**: GPU-eligible functions are a strict subset of
escape-analysis-safe functions (`src/lir/lower/escape.rs`): silent implies
not suspending (condition 2), no captures implies condition 1 satisfied.

"Numeric" means: no heap allocation, no closures, no strings, no function
calls, no signal emission — only int/float arithmetic, comparisons, local
variable access, and control flow.

## Dispatch Model

**Opt-in to start.** GPU dispatch is expensive (buffer transfer, kernel
launch overhead). Users control when it happens:

```lisp
(gpu/map f data)           ;; explicit GPU dispatch
(gpu/reduce f init data)   ;; explicit GPU reduction
```

The compiler emits SPIR-V for `f` via the MLIR GPU path. `f` must be
silent + numeric (enforced by `is_gpu_eligible`). The runtime handles
buffer allocation, H2D/D2H transfer, and async dispatch.

**Automatic promotion** is a future goal: the compiler detects data-parallel
patterns in hot loops and offers to dispatch to GPU. This requires cost
modeling (is the data large enough to amortize transfer overhead?).

## Memory Management

### Host-Device Transfer Protocol

Current implementation in `dispatch.rs`:

1. **Allocate**: `gpu-allocator` with `AllocationScheme::GpuAllocatorManaged`
   selects optimal memory type per `MemoryLocation`:
   - `CpuToGpu` (input/inout): prefers `HOST_VISIBLE | DEVICE_LOCAL` on
     AMD RADV with ReBAR (zero-copy); falls back to `HOST_VISIBLE` staging
   - `GpuToCpu` (output): `HOST_VISIBLE | HOST_CACHED` for readback

2. **Upload**: `memcpy` via `allocation.mapped_ptr()` (lines 104-117).
   Host-mapped, no staging buffer needed.

3. **Barrier**: `SHADER_WRITE → HOST_READ` pipeline barrier in command
   buffer (lines 179-191). Ensures GPU writes visible to host.

4. **Readback**: `collect_ref()` reads via mapped pointer (lines 234-257).
   Synchronous — GPU already done by this point (fence signaled).

### Buffer Pool (Phase 1)

Cache allocations by `(SizeBucket, MemoryLocation)`:
- Size bucketing: next power of 2 (256, 512, 1K, ..., 256M)
- Pool lives on `VulkanState` (protected by existing `Arc<Mutex<>>`)
- `acquire()` on dispatch, `release()` on collect
- `trim(max_per_bucket)` under memory pressure or context destroy

### Persistent Device Buffers (Phase 1)

`gpu-buffer` value type:
- Wraps `(Arc<Mutex<VulkanState>>, vk::Buffer, Allocation, usize, generation)`
- Created by `gpu:persist` from buffer spec
- Host mutation increments generation; dispatch checks generation to skip
  H2D transfer if unchanged
- Invalidation: caller must explicitly `gpu:update` when host data changes

### Command Pool Recycling (Phase 1)

Current: `create_command_pool` per dispatch with `TRANSIENT` flag (line 85).
Target: per-thread pool stored on `VulkanState`, `vkResetCommandPool`
between dispatches. Avoids ~100μs create/destroy overhead per dispatch.

## User-Supplied Kernels

Three levels of escape hatch:

1. **Pre-compiled SPIR-V** — `(gpu:load-shader ctx "kernel.spv" 3)`
2. **Elle SPIR-V builder** — `(spv:compute 256 3 (fn [s] ...))`
3. **Compiler-generated** — `(gpu/map f data)` compiles `f` to SPIR-V

All three produce the same thing: a Vulkan compute pipeline. The runtime
doesn't care how the SPIR-V was generated.

## Integration with Async Scheduler

GPU operations integrate with the existing io_uring scheduler:

1. `gpu/dispatch` — submit command buffer + create exportable fence
2. Fence fd registered with io_uring via `IoOp::PollFd`
3. Fiber suspends (zero threads consumed)
4. io_uring signals fence completion
5. Fiber resumes, does readback via `gpu/collect`

Cross-thread dispatch: `Arc<Mutex<VulkanState>>` supports submission from
any thread. Per-thread command pools avoid contention.

### Cooperative GTK Event Loop

GTK4 apps traditionally use `g_application_run` which blocks the thread
in GTK's event loop. This prevents Elle's fiber scheduler from processing
yields (GPU fence waits, I/O, etc.).

Solution (`lib/gtk4/bind.lisp:run-app`): replace `g_application_run` with
a cooperative loop:

```lisp
(defn run-app [app &named quit]
  (default quit (fn [] false))
  (g-application-register app nil nil)
  (g-application-activate app)
  (def ctx (g-main-context-default))
  (while (not (quit))
    (g-main-context-iteration ctx 0)   # non-blocking GTK event pump
    (ev/sleep 0.001)))                  # yield to Elle's fiber scheduler
```

FFI callbacks (draw, click, key) cannot yield. Operations that yield
(GPU dispatch, I/O) must run in `ev/spawn`'d fibers, not directly from
callbacks. The spawned fibers execute during `ev/sleep` in the cooperative
loop.

## Implementation Phases

### Phase 1: Stabilize Current Plugin (weeks)

Ordered steps:

**1a. SPIR-V builder: loops + local variables** (`lib/spirv.lisp`) **DONE**
- Added `OpVariable`, `OpLoad`, `OpStore` for function-scoped locals
- Added `OpBranch`, `OpBranchConditional`, `OpLoopMerge`, `OpSelectionMerge`
- Added `OpLogicalAnd`, `OpLogicalNot`, `OpSGreaterThan`
- Validated with `spirv-val` for add, localvar, loop, mandelbrot shaders
- 10 tests in `tests/elle/spirv.lisp`
- Libraries wrapped in closure convention (`lib/spirv.lisp`, `lib/gpu.lisp`)

**1b. Mandelbrot kernel** (`demos/gpu/mandelbrot.lisp`) **DONE**
- Standalone compute-only demo + interactive GTK4 explorer with GPU/CPU fallback
- Shader computes coords from global-id + viewport params, outputs ARGB32 via bitcast
- No CPU-side coordinate generation or color mapping — raw bytes blit to Cairo pixel buffer
- 1600×1200 @ 256 iterations in ~10ms dispatch + ~4ms blit (RX 7900 XTX)
- First frame ~23ms (includes shader compile), subsequent frames ~10ms
- Test case in `tests/elle/plugins/vulkan.lisp`: 4 known-point validation
- Also added: `OpBitwiseOr`(197), `OpBitwiseAnd`(199), `OpShiftLeftLogical`(196),
  `OpShiftRightLogical`(194), `OpBitcast`(124), `umin` (via `OpSelect`)

**1c. Buffer pool + command pool recycling** (`plugins/vulkan/src/`) **DONE**
- `BufferPool` on `VulkanState`: keyed by `(size_bucket, MemoryLocation)`
- Power-of-2 size bucketing (min 256 bytes), returned to pool on GpuHandle::drop
- Single command pool on `VulkanState`, reset via `vkResetCommandPool`

**1d. Integer data type support** **DONE**
- `BufferSpec.data` is `Vec<u8>` (raw bytes, not f32-specific)
- Buffer specs accept `:dtype` keyword (`:f32` default, `:u32`, `:i32`)
- `vulkan/decode` supports `:f32`, `:u32`, `:i32`, `:raw`

**1e. Persistent device buffers** **DONE**
- `GpuBuffer` type: wraps buffer + allocation, survives across dispatches
- `vulkan/persist ctx spec` creates, `vulkan/update buf spec` re-uploads
- `DispatchBuffer` enum: dispatch accepts both specs and persistent refs
- Returned to buffer pool on GC

**1f. End-to-end Mandelbrot demo** **DONE**
- `demos/mandelbrot/mandelbrot.lisp`: GTK4 explorer with GPU acceleration + CPU fallback
- `demos/gpu/mandelbrot.lisp`: standalone compute-only benchmark
- Cooperative GTK event loop via `b:run-app` (`g_main_context_iteration` + `ev/sleep`)
- `ev/spawn` for render to avoid yielding through FFI callbacks
- Fullscreen with GPU (2× resolution), windowed with CPU

### Phase 2: MLIR Integration (months)

**Dependency research** (completed):

The melior crate (0.27.0) provides safe Rust bindings to the MLIR C API.
Requires LLVM/MLIR system installation. Current state:

| Crate | Version | LLVM Required | Status |
|-------|---------|---------------|--------|
| `melior` | 0.27.0 | LLVM 22 | Alpha, API unstable |
| `mlir-sys` | 220.0.1 | LLVM 22 | Low-level C bindings |
| `inkwell` | (latest) | LLVM 11-21 | Mature, LLVM IR only (no MLIR) |

**Local environment**: Gentoo has LLVM 19/20/21 but no MLIR libraries.
Gentoo's `llvm-core/llvm` ebuilds don't expose an MLIR USE flag. MLIR
must be built from source (~30 min, ~500MB disk).

**Practical path**:
- **Option A**: Build LLVM 22 + MLIR from source, use `melior 0.27.0`.
  Pro: full MLIR ecosystem (arith, scf, memref, gpu, spirv dialects).
  Con: heavyweight build dependency, API churn.
- **Option B**: Use `inkwell` for direct LLVM IR emission (skip MLIR).
  Pro: mature crate, works with system LLVM 19-21. Con: no MLIR
  dialects, no gpu→spirv lowering, manual optimization passes.
- **Option C**: Use `pliron` (pure Rust MLIR-like framework, no C deps).
  Pro: no system dependency. Con: immature, no LLVM codegen backend.

**SPIR-V path**: MLIR has native gpu→spirv conversion passes. With
melior, the path is: Elle dialect → standard dialects → gpu dialect →
spirv dialect → SPIR-V bytes. This replaces our hand-written
`lib/spirv.lisp` for compiler-generated shaders.

**Recommended**: Option A (melior) once LLVM 22 is packaged. Meanwhile,
the existing SPIR-V builder + Vulkan plugin handles user-authored GPU
compute. The compiler-generated GPU path (Phase 3) can wait.

**Steps when ready**:
1. Build LLVM 22 + MLIR from source (or emerge when available)
2. Add melior dependency with `MLIR_SYS_*_PREFIX` pointing to install
3. Define Elle MLIR dialect ops (see dialect section above)
4. LIR → Elle dialect lowering (instruction mapping table above)
5. Signal lowering pass
6. Numeric specialization pass
7. LLVM backend (replace Wasmtime for tier-2 CPU)

### Phase 3: GPU Codegen via MLIR (months)

- `gpu/map` primitive with compiler support
- `is_gpu_eligible()` analysis (see detection criteria above)
- MLIR GPU path: linalg → gpu → spirv
- Connect to existing Vulkan runtime

### Phase 4: Optimization (ongoing)

- Kernel fusion (multiple `gpu/map` calls → single dispatch)
- Cost modeling for automatic GPU promotion
- Shared memory / workgroup optimizations
- AMD-native path via rocdl dialect (requires ROCm; long-term/optional)

## Validation Strategy

| Target | Method | When |
|--------|--------|------|
| SPIR-V output | `spirv-val` on generated bytecode | Phase 1a, every builder change |
| Numeric detection | Property tests: random LIR → verify classifier | Phase 3 |
| Buffer pool | Stress test: concurrent dispatches, verify no leaks | Phase 1c |
| Mandelbrot | Visual regression: CPU output == GPU output (pixel-exact) | Phase 1f |
| MLIR lowering | Round-trip: LIR → MLIR → LLVM → execute, compare with bytecode VM | Phase 2 |

## Open Questions

1. **melior vs inkwell** (RESEARCHED): melior 0.27.0 wraps MLIR C API,
   requires LLVM 22 (not yet in Gentoo). inkwell targets LLVM 11-21
   (available now) but has no MLIR dialects. Recommendation: wait for
   LLVM 22 packaging, use melior for full dialect ecosystem. Meanwhile,
   the SPIR-V builder handles GPU compute and Cranelift handles CPU JIT.

2. **Cranelift coexistence**: Keep Cranelift for tier-1 (fast compile)
   even with LLVM for tier-2. Cranelift's 1MB footprint and <1ms
   compile time are unbeatable for JIT. LLVM tier-2 is for hot numeric
   code that benefits from vectorization/LICM/GVN.

3. **WASM story**: LLVM has a wasm32 target. Does this replace Wasmtime
   entirely, or do we keep Wasmtime for its sandbox guarantees?

4. **Build complexity** (RESEARCHED): Gentoo's llvm-core/llvm ebuilds
   have no MLIR USE flag. MLIR must be built from source (~30 min,
   ~500MB). `MLIR_SYS_*_PREFIX` env var points melior to the install.
   This is the primary practical blocker for Phase 2.

5. **Fiber ↔ GPU workgroup mapping**: Can we lower `fiber/new` on a
   silent+numeric function to a GPU workgroup dispatch? This would
   make fibers the uniform parallelism primitive across CPU and GPU.

6. **SIG_GPU definition**: Bit 15 is reserved in comments but
   `SIG_GPU` is not yet defined as a constant in `src/signals/mod.rs`
   and `:gpu` is not registered in the signal registry. Deferred to
   Phase 3 — not needed while GPU is plugin-based.
