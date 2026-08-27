use super::*;

/// Legacy scope-mark helpers (no-ops — replaced by `DecrefRegion`).
#[no_mangle]
pub extern "C" fn elle_jit_region_enter() -> JitValue {
    JitValue::nil()
}
#[no_mangle]
pub extern "C" fn elle_jit_region_exit() -> JitValue {
    JitValue::nil()
}
#[no_mangle]
pub extern "C" fn elle_jit_region_exit_call() -> JitValue {
    JitValue::nil()
}
#[no_mangle]
pub extern "C" fn elle_jit_region_rotate() -> JitValue {
    JitValue::nil()
}

/// Push a fresh per-activation region-remap frame on JIT function entry.
///
/// The JIT analog of the interpreter's `execute_bytecode_saving_stack` pushing
/// an `activation_region_map` (src/vm/execute.rs). Emitted in every compiled
/// function's Cranelift prologue so the body's per-execution alloc regions
/// (`elle_jit_resolve_alloc_region`) and slot-resolved `DecrefRegion`s
/// (`elle_jit_decref_region`) resolve against THIS activation's slot→phys map,
/// not the caller's. Covers every entry path (top-level, JIT-to-JIT, SCC direct)
/// because it is part of the compiled function. docs/impl/region/rules.md Rule 4
/// ("per activation").
#[no_mangle]
pub extern "C" fn elle_jit_push_region_map(vm: *mut ()) {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    vm.push_activation_region_map();
}

/// Pop the current region-remap frame. Emitted before every `return` in a
/// compiled function (normal return, tail-call-sentinel return, yield/Emit
/// side-exit AFTER the suspend captured the map, error return). Like the
/// interpreter's `pop_activation_region_map`, this only drops the lookup table —
/// it never decrefs entries (freeing happens via `DecrefRegion`/owned-param
/// releases; a tail-moved arg's ownership transfers to the callee).
#[no_mangle]
pub extern "C" fn elle_jit_pop_region_map(vm: *mut ()) {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    vm.pop_activation_region_map();
}

/// Resolve (mint + record in the current activation's slot→phys map) this
/// allocation's per-slot physical region and return its raw id
/// (docs/impl/region/ctx.md). The emitter passes the returned id straight to
/// the alloc helper (`elle_jit_pair`, `_make_array`, …) as its explicit region
/// argument. Mirrors the interpreter's `runtime_region_for_alloc_slot` feeding
/// the handler an explicit `region_id`.
/// `runtime_region_for_alloc_slot` records slot→phys so the matching
/// `DecrefRegion(slot)` and any cross-yield resume still resolve the region.
#[no_mangle]
pub extern "C" fn elle_jit_resolve_alloc_region(vm: *mut (), slot: u32) -> u32 {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let static_id = crate::hir::region::StaticRegion::new(slot)
        .expect("JIT alloc region slot is nonzero — emitter invariant");
    vm.runtime_region_for_alloc_slot(static_id).get()
}

/// Resolve a **merged** slot's per-execution physical region with mint-or-reuse —
/// the builder-idiom merge runtime (docs/impl/region/merging.md § Merging). The
/// emitter calls this instead of `elle_jit_resolve_alloc_region` for a slot it
/// found in `LirFunction.merged_slots` at compile time, so the first member (the
/// child) mints `R` and a later member (the parent) reuses it: both land in one
/// region freed by the single `DecrefRegion`. Without this the JIT would mint fresh
/// for every member and diverge from the interpreter's region count (the merge tree
/// stays leak-free either way — the parent's cascade frees the orphaned child — but
/// the tiers must agree on physical-region identity).
#[no_mangle]
pub extern "C" fn elle_jit_resolve_alloc_region_merged(vm: *mut (), slot: u32) -> u32 {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let static_id = crate::hir::region::StaticRegion::new(slot)
        .expect("JIT alloc region slot is nonzero — emitter invariant");
    vm.runtime_region_for_merged_alloc_slot(static_id).get()
}

/// Increment the reference count of a region named by a static slot.
///
/// Resolves the slot through the current activation's region map (mirror of the
/// interpreter's defensive `IncrefRegion` arm, src/vm/dispatch.rs); skips if the
/// slot is unmapped (never mints). The lowerer no longer emits `IncrefRegion`
/// (cross-region increfs are value-based via `alloc_obj`), so this is
/// defensive-correctness for tier parity.
#[no_mangle]
pub extern "C" fn elle_jit_incref_region(vm: *mut (), slot: u32) {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let Some(static_id) = crate::hir::region::StaticRegion::new(slot) else {
        return;
    };
    let phys = vm
        .fiber
        .activation_region_maps
        .last()
        .and_then(|f| f.get(&static_id.get()).map(|m| m.region));
    if let Some(phys) = phys {
        crate::value::arena::incref_for_escape(
            unsafe { &mut *vm.heap_ptr },
            Some(phys),
            crate::value::arena::EscapeSite::ImmutableContents,
        );
    }
}

/// Decrement (drop the initial reference of) the region named by a static slot.
///
/// The JIT analog of the interpreter's `DecrefRegion` arm (src/vm/dispatch.rs):
/// resolve the slot through the current activation map
/// (`take_runtime_region_for_drop_slot` — which also CLEARS the slot so the next
/// loop iteration re-mints), then strict-decref the resolved physical region.
/// `None` (a conditional alloc that never executed this activation) is a benign
/// no-op. This replaces the old static-slot-as-physical-region bug that froze a
/// live runtime region sharing the slot's small id.
#[no_mangle]
pub extern "C" fn elle_jit_decref_region(vm: *mut (), slot: u32) {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let Some(static_id) = crate::hir::region::StaticRegion::new(slot) else {
        return;
    };
    if let Some(region) = vm.take_runtime_region_for_drop_slot(static_id) {
        unsafe { (*vm.heap_ptr).decref_region(region) };
    }
}

/// Release a value's runtime region (the `DecrefValueRegion` instruction).
///
/// Mirrors the interpreter's `DecrefValueRegion` arm EXACTLY: uses
/// `result_region_of` (NOT `region_of`) so a value bound through a compiled
/// `MakeCaptureCell` is unwrapped one level — the release targets the inner
/// call-result's region, while the cell's own region is freed by its compiled
/// `DecrefRegion`. Using `region_of` here decrefs the cell's region a second
/// time (phantom/double-free — the redis eager-JIT crash). Consumes the one
/// owning reference the callee handed back via `IncrefValueRegion`.
#[no_mangle]
pub extern "C" fn elle_jit_decref_value_region(tag: u64, payload: u64, vm: *mut ()) {
    let value = Value { tag, payload };
    // The heap is the driving VM's own — threaded explicitly through the helper
    // ABI so two embedded instances each reach their own heap, never a per-thread
    // slot (docs/impl/region/ctx.md "JIT intrinsic helpers reach the VM").
    let heap = unsafe { &mut *(*(vm as *mut crate::vm::VM)).heap_ptr };
    if let Some(region) = crate::value::arena::result_region_of(heap, value) {
        heap.decref_region(region);
    }
}

/// Release a capture cell's OWN runtime region (the `DecrefCellRegion`
/// instruction). Uses `region_of` (NOT `result_region_of`): frees the per-value
/// env cell `populate_env` minted, never unwrapping to the inner value's
/// caller-owned region. Mirrors the interpreter's `DecrefCellRegion` arm.
#[no_mangle]
pub extern "C" fn elle_jit_decref_cell_region(tag: u64, payload: u64, vm: *mut ()) {
    let value = Value { tag, payload };
    let heap = unsafe { &mut *(*(vm as *mut crate::vm::VM)).heap_ptr };
    if let Some(region) = crate::value::arena::region_of(heap, value) {
        heap.decref_region(region);
    }
}

/// Increment the reference count of a value's region (the `IncrefValueRegion`
/// instruction). Mirrors the interpreter's arm: `result_region_of` (unwrap a
/// capture cell), the return-value handoff the caller's `DecrefValueRegion`
/// consumes.
#[no_mangle]
pub extern "C" fn elle_jit_incref_value_region(tag: u64, payload: u64, vm: *mut ()) {
    let value = Value { tag, payload };
    let heap = unsafe { &mut *(*(vm as *mut crate::vm::VM)).heap_ptr };
    let r = crate::value::arena::result_region_of(heap, value);
    crate::value::arena::incref_for_escape(heap, r, crate::value::arena::EscapeSite::ReturnValue);
}

/// Increment the durable reference count for a heap value.
/// Called by JIT `StoreLocal` to track binding references.
#[no_mangle]
pub extern "C" fn elle_jit_incref(tag: u64, payload: u64) -> JitValue {
    let _val = crate::value::Value { tag, payload };
    JitValue::nil()
}

/// Decrement refcount only (no drop). Called by JIT `StoreLocalRefcounted`
/// to release the old binding's reference. The old value may still be
/// reachable through collections or other bindings — actual freeing is
/// deferred to scope exit.
#[no_mangle]
pub extern "C" fn elle_jit_decref(tag: u64, payload: u64) -> JitValue {
    let _val = crate::value::Value { tag, payload };
    JitValue::nil()
}

/// Link the child value's region as an Owned member of the parent value's
/// region — the `AdoptRegion` instruction. Mirrors the interpreter's
/// `handle_adopt_region` arm (src/vm/dispatch/region.rs): resolve both values to
/// their runtime regions (`result_region_of`, which unwraps a capture cell to the
/// inner value), and adopt — freezing the child's RC so it is reclaimed only by
/// the parent's subtree drop. An immediate operand (no region) or a self-edge
/// (same region) is a no-op. Unlike the interpreter arm, the parent and child
/// arrive as explicit Value pairs (the compiled code loaded them into SSA
/// registers purely to drive this adopt), not popped off an operand stack.
#[no_mangle]
pub extern "C" fn elle_jit_adopt_region(
    parent_tag: u64,
    parent_payload: u64,
    child_tag: u64,
    child_payload: u64,
    vm: *mut (),
) {
    let parent = Value {
        tag: parent_tag,
        payload: parent_payload,
    };
    let child = Value {
        tag: child_tag,
        payload: child_payload,
    };
    let heap = unsafe { &mut *(*(vm as *mut crate::vm::VM)).heap_ptr };
    let parent_region = crate::value::arena::result_region_of(heap, parent);
    let child_region = crate::value::arena::result_region_of(heap, child);
    if let (Some(p), Some(c)) = (parent_region, child_region) {
        if p != c {
            heap.adopt_region(p, c);
        }
    }
}

/// Link the child value's region as an Owned member of the parent value's
/// region, resolving BOTH operands with `region_of` — NOT `result_region_of` —
/// the `AdoptCellRegion` instruction. Mirrors the interpreter's
/// `handle_adopt_cell_region` arm: a `CaptureCell` operand's OWN region is
/// adopted (never unwrapped to its content), which is what lets the forest own a
/// capture cell's arena and reclaim a local recursive/letrec closure clique as a
/// unit (docs/impl/region/adopt.md § "The capture adopt"). This is the
/// `region_of`-adopt counterpart of `elle_jit_adopt_region`, exactly as
/// `elle_jit_decref_cell_region` is the `region_of` counterpart of
/// `elle_jit_decref_value_region`. An immediate operand (no region) or a self-edge
/// (same region) is a no-op.
#[no_mangle]
pub extern "C" fn elle_jit_adopt_cell_region(
    parent_tag: u64,
    parent_payload: u64,
    child_tag: u64,
    child_payload: u64,
    vm: *mut (),
) {
    let parent = Value {
        tag: parent_tag,
        payload: parent_payload,
    };
    let child = Value {
        tag: child_tag,
        payload: child_payload,
    };
    let heap = unsafe { &mut *(*(vm as *mut crate::vm::VM)).heap_ptr };
    let parent_region = crate::value::arena::region_of(heap, parent);
    let child_region = crate::value::arena::region_of(heap, child);
    if let (Some(p), Some(c)) = (parent_region, child_region) {
        if p != c {
            heap.adopt_region(p, c);
        }
    }
}

/// Adopt the child value's region into the CURRENT activation's owner node —
/// the `AdoptIntoActivation` instruction. Mirrors the interpreter's
/// `handle_adopt_into_activation` arm (src/vm/dispatch/region.rs): resolve the
/// child to its runtime region (`result_region_of`, which unwraps a capture
/// cell), lazily mint the activation's pages-less owner node, and adopt —
/// freezing the child's RC so the node's subtree drop at the activation's
/// normal completion is its sole demise (docs/impl/region/owner.md § "Owner
/// nodes — an activation as a forest root"). An immediate child (no region)
/// adopts nothing and mints no node. The child arrives as an explicit Value
/// pair (compiled code loads it purely to drive the adopt), not popped off an
/// operand stack.
#[no_mangle]
pub extern "C" fn elle_jit_adopt_into_activation(child_tag: u64, child_payload: u64, vm: *mut ()) {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let child = Value {
        tag: child_tag,
        payload: child_payload,
    };
    let child_region = crate::value::arena::result_region_of(unsafe { &mut *vm.heap_ptr }, child);
    if let Some(c) = child_region {
        // Idempotent on an already-Owned child, mirroring the interpreter arm:
        // a re-delivered region keeps its first owner instead of tripping the
        // one-owner adopt assert (docs/impl/region/owner.md § "Owner nodes").
        if vm.heap().region_is_owned(c) {
            return;
        }
        let node = vm.activation_owner_node();
        if node != c {
            vm.heap().adopt_region(node, c);
        }
    }
}

/// Free the current activation's owner node at the compiled function's normal
/// completion — the JIT twin of the interpreter trampoline's clean-break
/// release (`VM::release_activation_owner_node`). Emitted on the `Return` path
/// (before the region-map pop) of a function whose LIR carries
/// `AdoptIntoActivation`; a function that cannot mint a node never pays the
/// call.
#[no_mangle]
pub extern "C" fn elle_jit_release_activation_owner_node(vm: *mut ()) {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    vm.release_activation_owner_node();
}

/// Run the releases a COMPILED activation abandoned by an **error** still owed —
/// the compiled entry to the same walk the interpreter reaches through
/// `VM::release_abandoned_frame` (docs/impl/region/mechanism.md § "An abandoned
/// frame runs the releases it still owes").
///
/// `slots` and `regions` are the function's two release tables, materialized once
/// by the compiled prologue: the value routes' local slots and the slot routes'
/// static region ids. `locals` is the frame's local slots spilled in slot order,
/// so `locals[s]` is what `LoadLocal s` reads. The slot route needs no spill —
/// its receipt is the activation region map, which the prologue pushed and this
/// call reads ahead of the matching pop.
///
/// The payload is read off `fiber.signal`, which the raise has already installed:
/// the callee's error at the post-call exit, this frame's own emitted value at
/// the `Emit` exit. Only an **error** abandons the frame, so a signal without
/// `SIG_ERROR` walks nothing — the post-call exception check also fires on a
/// halt, which the interpreter's trampoline likewise declines to walk.
///
/// # Safety
/// `slots` must point at `num_slots` contiguous `u16`s, `regions` at
/// `num_regions` contiguous `u32`s, and `locals` at `num_locals` contiguous
/// `Value`s. Any of the three may be null when its count is 0.
#[no_mangle]
pub extern "C" fn elle_jit_release_abandoned_frame(
    vm: *mut (),
    slots: *const u16,
    num_slots: u64,
    regions: *const u32,
    num_regions: u64,
    locals: *const Value,
    num_locals: u64,
) {
    /// A null pointer with a zero count is the empty table, not a slice to build.
    unsafe fn table<'a, T>(ptr: *const T, count: u64) -> &'a [T] {
        if count == 0 || ptr.is_null() {
            &[]
        } else {
            std::slice::from_raw_parts(ptr, count as usize)
        }
    }
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let Some((bits, payload)) = vm.fiber.signal else {
        return;
    };
    if !bits.intersects(crate::value::SIG_ERROR) {
        return;
    }
    unsafe {
        vm.release_abandoned(
            table(slots, num_slots),
            table(regions, num_regions),
            payload,
            crate::vm::core::FrameLocals::Spilled(table(locals, num_locals)),
        )
    };
}

/// Free a co-owned region group as one unit — the `FreeRegionGroup` instruction.
/// Mirrors the interpreter's `handle_free_region_group` arm
/// (src/vm/dispatch/region.rs): `members_ptr` points at `count` member Values the
/// compiled code spilled to a stack slot (exactly as `elle_jit_push_param_frame`
/// takes its pairs); each is resolved to its runtime region (`result_region_of`)
/// and the whole set is freed together, so interior member↔member references
/// reclaim with the group and only genuinely-Shared frontier references cascade.
/// An immediate member (no region) is skipped; an empty/null set is a no-op.
#[no_mangle]
pub extern "C" fn elle_jit_free_region_group(members_ptr: *const Value, count: u64, vm: *mut ()) {
    let heap = unsafe { &mut *(*(vm as *mut crate::vm::VM)).heap_ptr };
    let count = count as usize;
    let mut members: Vec<crate::hir::region::RuntimeRegion> = Vec::with_capacity(count);
    if !members_ptr.is_null() {
        let values = unsafe { std::slice::from_raw_parts(members_ptr, count) };
        for &value in values {
            if let Some(r) = crate::value::arena::result_region_of(heap, value) {
                members.push(r);
            }
        }
    }
    if !members.is_empty() {
        heap.free_region_group(&members);
    }
}
