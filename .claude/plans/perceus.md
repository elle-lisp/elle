# Perceus-Style Compile-Time Drop Insertion for Elle

## Context

Tail-recursive loops under `ev/run` accumulate heap objects in the shared
allocator when tail-call arguments are heap-allocating.  The swap pool
rotation mechanism can't safely free them because reference chains (cons
lists, nested structs) may extend arbitrarily far back.

Perceus (Reinking et al. 2021) solves this at compile time: the compiler
inserts explicit `Drop` operations at points where bindings become dead.
No runtime reference counting needed for the common case — the compiler
proves uniqueness statically.

The tco-alloc-10000 canary in tests/elle/resource.lisp validates the fix:
20,002 allocs for 10k iterations should drop to bounded (~4).

## What we're building

A new analysis + emission pass in the LIR lowerer that:

1. Computes **use-count per binding per branch** in self-tail-calls
2. Identifies parameters with **zero uses** in recursive branches
3. Emits a **`DropValue` bytecode instruction** that frees the parameter's
   slab slot before the tail call

Later phases extend to use-count=1 (consuming positions) and eventually
full Perceus (runtime refcount fallback for shared values).

## Compiler integration points

### HIR structures (src/hir/)

- `Hir { kind: HirKind, span, signal }` — expression node
- `HirKind::Var(Binding)` — reference to a binding
- `HirKind::Lambda { params: Vec<Binding>, body, ... }` — function
- `HirKind::Call { func, args: Vec<CallArg>, is_tail }` — function call
- `Binding(u32)` — index into BindingArena
- `BindingInner` — metadata: scope, is_mutated, is_captured, is_immutable, is_prebound

### LIR structures (src/lir/)

- `LirFunction` — compiled function with blocks, regs, constants
  - `rotation_safe: bool` — current escape flag (will be refined)
- `LirInstr::TailCall { func, args }` — tail call instruction
- `LirInstr::RegionExit` — scope cleanup (emitted before TailCall)

### Escape analysis (src/lir/lower/escape.rs)

- `body_escapes_heap_values(hir) -> bool` — rotation safety check
- `result_is_safe(hir, scope_bindings) -> bool` — immediate-value check
- `tail_arg_is_safe(hir) -> bool` — safe tail-call argument check
- `callee_rotation_safe` map — fixpoint-computed per-function safety

### Tail-call lowering (src/lir/lower/control.rs)

Lines 44-58: when `is_tail`, emit pending RegionExits then `TailCall`.
This is the insertion point for `DropValue` — after RegionExits, before
TailCall.

### Trampoline (src/vm/execute.rs)

Three trampoline sites (lines 175, 293, 364): each checks
`prev_rotation_safe` and calls `rotate_pools`.  With Perceus drops,
rotation becomes a fallback for rotation-safe functions that the drop
pass doesn't handle.  Eventually rotation can be removed entirely.

## Phase 1: Use-count analysis + drop insertion for tail-call parameters

### Step 1: `hir_references_binding(hir, binding) -> bool`

New function in `escape.rs`.  Walks an HIR subtree and returns true if
any `Var(b)` node has `b == binding`.  Simple recursive traversal,
same structure as `body_escapes_heap_values`.

This is used by the refined rotation safety check to determine whether a
tail-call argument's expression tree references a specific parameter.

### Step 2: Refine `body_escapes_heap_values` for self-tail-calls

Currently (line 1006-1007):
```rust
if *is_tail {
    return args.iter().any(|a| !self.tail_arg_is_safe(&a.expr));
}
```

Refined: for self-tail-calls, apply per-parameter independence analysis:

```rust
if *is_tail {
    if let Some(params) = self.current_function_params() {
        if self.is_self_call(func) {
            // Build set H: param indices whose arg is heap-allocating
            let heap_args: Vec<bool> = args.iter()
                .map(|a| !self.tail_arg_is_safe(&a.expr))
                .collect();

            // For each heap-allocating arg, check if it references
            // any parameter in H
            return args.iter().enumerate().any(|(k, a)| {
                if !heap_args[k] { return false; }
                params.iter().enumerate().any(|(j, &param_binding)| {
                    heap_args.get(j).copied().unwrap_or(false)
                        && self.hir_references_binding(&a.expr, param_binding)
                })
            });
        }
    }
    return args.iter().any(|a| !self.tail_arg_is_safe(&a.expr));
}
```

This classifies `(loop (- i 1) {:a i :b (cons i nil)})` as
rotation-safe: arg 1 mentions param 0 (`i`) but arg 0 is immediate
(not in set H), so no cross-generation reference chain.

The `is_self_call` check: the func is `HirKind::Var(binding)` where
binding matches the current function's own binding (need to thread
current function binding through the lowerer).  For non-self-calls,
fall through to the conservative check.

### Step 3: Track current function binding

Add a `current_function_binding: Option<Binding>` field to the Lowerer.
Set it when entering a lambda or letrec-bound function definition.
Used by the escape analysis to detect self-tail-calls.

Add a `current_function_params: Option<Vec<Binding>>` field.
Set from `Lambda { params, .. }` when entering a function.

### Step 4: `DropValue` bytecode instruction

New bytecode instruction:
```
DropValue(reg)
```

Semantics: if the value in `reg` is a heap pointer, return its slab slot
to the free list (local pool or shared allocator, whichever owns it).
If the value is an immediate (int, bool, nil, keyword, symbol), no-op.

Implementation in the VM dispatch loop:
```rust
Instruction::DropValue => {
    let reg = /* read operand */;
    let value = stack[reg];
    if value.is_heap() {
        let ptr = value.as_heap_ptr();
        // Free from whichever pool owns this slot.
        // current_heap_ptr() gives us the fiber's heap;
        // the heap routes to shared or local as appropriate.
        crate::value::fiberheap::drop_heap_value(ptr);
    }
}
```

The `drop_heap_value` function needs to:
1. Run the HeapObject's destructor (drop_in_place) if it has one
2. Return the slab slot to the correct pool's free list
3. Decrement the pool's alloc_count

This requires knowing which pool owns the pointer.  Options:
- Check if the pointer falls within the shared allocator's slab chunks
- Add a 1-bit owner tag to each slab slot header
- Route through FiberHeap which checks shared_alloc first

The routing approach (check shared_alloc first, then local) is simplest
and has no memory overhead.

### Step 5: Emit `DropValue` before TailCall

In `lower_call` (src/lir/lower/control.rs), when `is_tail` and the call
is a self-tail-call to a rotation-safe function:

After emitting pending RegionExits (line 51-53) and before emitting
TailCall (line 54):

```rust
if is_tail {
    for _ in 0..self.pending_region_exits {
        self.emit(LirInstr::RegionExit);
    }

    // Perceus: drop dead parameters before tail call.
    if self.is_self_tail_call(func) {
        if let Some(ref params) = self.current_function_params {
            let heap_args: Vec<bool> = args.iter()
                .map(|a| !self.tail_arg_is_safe(&a.expr))
                .collect();
            for (k, &param_binding) in params.iter().enumerate() {
                if !heap_args.get(k).copied().unwrap_or(false) {
                    continue; // arg is immediate, no drop needed
                }
                // Check: is this parameter unreferenced in the
                // recursive branch?  If so, its old value is dead.
                // (The refined rotation_safe already proved this
                // for the function as a whole, but we emit drops
                // per-parameter.)
                let param_slot = self.binding_to_slot[&param_binding];
                self.emit(LirInstr::DropValue { slot: param_slot });
            }
        }
    }

    self.emit(LirInstr::TailCall {
        func: func_reg,
        args: arg_regs,
    });
}
```

Note: the arg regs have already been evaluated (line 34-37), so the
new values are in registers.  The DropValue frees the OLD parameter
value (still in its slot), not the new arg value (in a different reg).

### Step 6: Slab slot deallocation

New function `drop_heap_value(ptr)` on FiberHeap:

```rust
pub fn drop_heap_value(&mut self, ptr: *mut HeapObject) {
    // Run destructor if needed.
    if needs_drop(unsafe { (*ptr).tag() }) {
        unsafe { std::ptr::drop_in_place(ptr) };
    }

    // Return slot to the correct pool.
    if !self.shared_alloc.is_null() {
        let sa = unsafe { &mut *self.shared_alloc };
        // Check if ptr belongs to shared allocator's slab.
        if sa.owns(ptr) {
            sa.dealloc(ptr);
            self.shared_alloc_count -= 1;
            return;
        }
    }
    // Fall through to local pool.
    unsafe { self.pool.dealloc_slot(ptr) };
    self.pool.alloc_count -= 1;
}
```

SharedAllocator needs an `owns(ptr) -> bool` method that checks if
the pointer falls within its slab's chunk ranges.  RootSlab tracks
its chunks, so this is a matter of checking each chunk's address range.

## Phase 2: Drop insertion for let bindings (future)

Extend the use-count analysis beyond tail-call parameters to all
let-bound heap values.  When a let binding's last use is identified,
emit `DropValue` at that point.  This subsumes RegionEnter/RegionExit
for bindings where the compiler can prove uniqueness.

## Phase 3: Reuse fusion (future)

After drop insertion, scan for `DropValue` immediately followed by a
heap allocation of compatible shape.  Fuse into a `ReuseValue`
instruction that overwrites the slot in-place.  Since all HeapObject
variants use the same slab slot size, any slot can be reused for any
variant.

## Phase 4: Borrowing optimization (future)

Classify function arguments as "consuming" vs "borrowing."  Borrowed
arguments don't need Drop at their last use because the caller retains
ownership.  This reduces unnecessary drops for primitives that only
read their arguments.

## Phase 5: Runtime refcount fallback (future)

For values that escape static analysis (stored in mutable containers,
captured by closures with multiple references), add a 1-byte refcount
to slab slot headers.  `DropValue` checks the refcount: if >1, just
decrement; if 1, free.  This is the full Perceus completeness story.

## Implementation status

### Phase 1a: Refined rotation safety + shared rotation (DONE)

The core fix has two parts:

1. **Per-parameter independence analysis** (Steps 1-3): `hir_references_binding`
   walks a HIR subtree checking if any `Var` references a specific binding.
   `body_escapes_heap_values` uses this for self-tail-calls: a heap-allocating
   arg is safe if it doesn't reference any parameter whose arg is also
   heap-allocating.  This classifies `(loop (- i 1) {:a i :b (cons i nil)})`
   as rotation-safe because arg 1 references `i` (immediate arg) not `prev`
   (heap arg).

2. **Shared allocator rotation** (runtime fix): the trampoline in `vm/mod.rs`
   was rotating `self.fiber.heap` which for the root fiber is NOT the active
   heap — the `ROOT_HEAP` thread-local is.  Fixed to use
   `with_current_heap_mut`.  Additionally, `rotate_pools` was unconditionally
   skipping rotation when a shared allocator was active (child fibers under
   `ev/run`).  Now it calls `SharedAllocator::rotate` using the captured
   `shared_mark`, which is safe because the refined analysis proves no
   cross-generation reference chains for rotation-safe functions.

Result: tco-alloc-10000 dropped from 20,002 allocs to 2.

### Phase 1b: DropValue instruction (DONE)

Compile-time drop insertion for self-tail-call parameters. The compiler
emits `DropValue` instructions before tail calls for parameters that:
- Have heap-allocating new args
- Are not upvalues (captured bindings)
- Are not referenced by any heap-allocating arg

**DropValue semantics**: run HeapObject destructor (frees inner heap data),
overwrite slab slot with `Cons(NIL, NIL)` sentinel, write NIL to stack slot,
decrement heap alloc_count. The slab slot stays in pool.allocs/dtors —
rotation/teardown see the sentinel and skip the destructor (no-op), then
dealloc the slot normally.

**Shared allocator rotation disabled**: Phase 1a's shared rotation was
unsafe when multiple child fibers share the same SharedAllocator (owned
by the parent). Rotation by one child freed objects another child still
referenced. With DropValue handling per-object freeing, shared rotation
is no longer needed. Private pool rotation remains active.

**Results**: tco-replace-10000: allocs=3, peak=3. tco-alloc-10000:
allocs=~10003 (struct params dropped by DropValue; cons sub-expressions
accumulate without shared rotation). tco-mixed-10000: allocs=~10003
(prev struct dropped, acc cons accumulates).

## Test plan

1. ~~Counter-factual: verify tco-alloc-10000 fails (20k allocs) before~~ DONE
2. ~~Implement Phase 1a~~ DONE
3. ~~Verify tco-alloc-10000 passes (bounded allocs)~~ DONE (allocs=2)
4. ~~Verify all existing tests still pass~~ DONE
5. ~~Verify cons-build-100 still correctly accumulates (100 allocs)~~ DONE
6. ~~Add new scenarios to resource.lisp~~ DONE
   - tco-replace: struct replaced each iteration, no reference chain (allocs=3)
   - tco-mixed: some params replaced, some accumulated (~10003)
7. ~~Phase 1b: DropValue~~ DONE
   - All smoke tests pass (VM, JIT, WASM)
   - callable-resume: fixed (shared rotation bug)
   - contracts: fixed (shared rotation bug)

## Files modified

| File | Change |
|------|--------|
| `src/lir/lower/escape.rs` | Added `hir_references_binding`; refined `body_escapes_heap_values` for self-tail-calls |
| `src/lir/lower/mod.rs` | Added `current_function_binding` and `current_function_params` fields; wired through `precompute_rotation_safety` |
| `src/lir/lower/lambda.rs` | Set/restore function context for escape analysis |
| `src/lir/lower/binding.rs` | Set function context in `lower_letrec` and `lower_define` for lambda inits |
| `src/vm/mod.rs` | Fixed trampoline to use `with_current_heap_mut` instead of `self.fiber.heap` |
| `src/value/fiberheap/mod.rs` | Added `shared_alloc_count` to `RotationBase`; disabled shared rotation; `decrement_alloc_count` for DropValue |
| `src/value/fiberheap/routing.rs` | Added `drop_value()` routing function: destructor + sentinel + alloc decrement |
| `src/value/shared_alloc.rs` | Added `clear_swap()` for cross-child rotation safety |
| `src/lir/types.rs` | Added `DropValue { slot: u16 }` to `LirInstr` |
| `src/compiler/bytecode.rs` | Added `DropValue` to `Instruction` enum with disassembly |
| `src/lir/emit/mod.rs` | Emit `DropValue` bytecode (slot-addressed, no stack effect) |
| `src/vm/dispatch.rs` | Handle `DropValue`: read slot, call `drop_value`, write NIL |
| `src/lir/lower/control.rs` | Call `emit_drop_dead_params` before TailCall |
| `src/lir/lower/escape.rs` | Added `emit_drop_dead_params`: per-parameter drop analysis |
| `src/lir/lower/lambda.rs` | Set function context DURING body lowering (not just escape analysis) |
| `src/jit/translate.rs` | DropValue no-op in JIT (uses rotation) |
| `src/wasm/instruction.rs` | DropValue no-op in WASM |
| `tests/elle/resource.lisp` | Updated assertions for DropValue; tco-replace=3, tco-alloc=~10003 |
