// audited: 2026-09-29
//! The JIT's region helpers: each does what the interpreter does for one region instruction or activation boundary.
//!
//! docs/impl/region/mechanism.md
//! docs/impl/region/ownership.md
//! docs/impl/region/unwind.md

use super::*;

/// No-op helpers. The vtable declares and registers them, and the translator
/// emits no call to any of them.
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
/// The JIT twin of the interpreter's `VM::open_activation` (src/vm/execute.rs).
/// Every compiled function's prologue emits it, so the body's alloc regions
/// (`elle_jit_resolve_alloc_region`) and slot-resolved `DecrefRegion`s
/// (`elle_jit_decref_region`) resolve against THIS activation's slot→phys map,
/// not the caller's. It covers every entry path, interpreter→JIT and JIT-to-JIT,
/// because it is part of the compiled function. The map is per activation
/// (docs/impl/region/rules.md).
#[no_mangle]
pub extern "C" fn elle_jit_push_region_map(vm: *mut ()) {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    vm.push_activation_region_map();
}

/// Pop the current region-remap frame.
///
/// A compiled function emits it before every `return`: the normal return, the
/// tail-call-sentinel return, the error return, and the yield/Emit side-exit
/// after the suspend captured the map. It calls `VM::pop_activation_region_map`,
/// which drops the lookup table and never decrefs an entry. `DecrefRegion` and
/// the owned-param releases do the freeing, and a tail-moved argument's
/// ownership moves to the callee.
#[no_mangle]
pub extern "C" fn elle_jit_pop_region_map(vm: *mut ()) {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    vm.pop_activation_region_map();
}

/// Resolve this allocation's per-slot physical region and return its raw id
/// (docs/impl/region/ctx.md).
///
/// `VM::runtime_region_for_alloc_slot` mints a fresh region or takes a pending
/// join, and records slot→phys in the current activation's map. The matching
/// `DecrefRegion(slot)` and any cross-yield resume read that record. The
/// emitter passes the returned id straight to the alloc helper (`elle_jit_pair`,
/// `_make_array`, …) as its explicit region argument, as the interpreter hands
/// its handler an explicit `region_id`.
#[no_mangle]
pub extern "C" fn elle_jit_resolve_alloc_region(vm: *mut (), slot: u32) -> u32 {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let static_id = crate::hir::region::StaticRegion::new(slot)
        .expect("JIT alloc region slot is nonzero — emitter invariant");
    vm.runtime_region_for_alloc_slot(static_id).get()
}

/// Resolve a **merged** slot's per-execution physical region with mint-or-reuse,
/// the builder-idiom merge runtime (docs/impl/region/merging.md).
///
/// The emitter calls this instead of `elle_jit_resolve_alloc_region` for a slot
/// it finds in `LirFunction.merged_slots` at compile time. The first member (the
/// child) mints `R` and a later member (the parent) reuses it, so both land in
/// one region that the single `DecrefRegion` frees. The two tiers must agree on
/// which physical region each member lives in.
#[no_mangle]
pub extern "C" fn elle_jit_resolve_alloc_region_merged(vm: *mut (), slot: u32) -> u32 {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let static_id = crate::hir::region::StaticRegion::new(slot)
        .expect("JIT alloc region slot is nonzero — emitter invariant");
    vm.runtime_region_for_merged_alloc_slot(static_id).get()
}

/// Increment the reference count of a region named by a static slot.
///
/// The JIT analog of the interpreter's `IncrefRegion` arm
/// (`handle_incref_region`): resolve the slot through the current activation's
/// region map and incref the physical region; skip an unmapped slot (never
/// mint). The lowerer emits `IncrefRegion` for a coalesced mint and a store
/// edge it names by static slot.
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
/// The JIT twin of the interpreter's `handle_decref_region`. It resolves the
/// slot through the current activation map with
/// `take_runtime_region_for_drop_slot`, which also CLEARS the slot so the next
/// loop iteration re-mints. Then it strict-decrefs the resolved physical region.
/// An unmapped slot (a conditional alloc that never ran this activation) is a
/// no-op. The slot is never read as a physical region id: a live runtime region
/// sharing the slot's small id would be the one released.
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
/// Mirrors the interpreter's `handle_decref_value_region` EXACTLY. It uses
/// `result_region_of` (NOT `region_of`), so a value bound through a compiled
/// `MakeCaptureCell` is unwrapped one level: the release targets the inner
/// call-result's region, and the cell's own compiled `DecrefRegion` frees the
/// cell's region. With `region_of` here, the cell's region takes a second
/// decref, a double free. The release consumes the one owning reference the
/// callee handed back through `IncrefValueRegion`.
#[no_mangle]
pub extern "C" fn elle_jit_decref_value_region(tag: u64, payload: u64, vm: *mut ()) {
    let value = Value { tag, payload };
    // The heap is the driving VM's own — threaded explicitly through the helper
    // ABI so two embedded instances each reach their own heap, never a per-thread
    // slot (docs/impl/region/ctx.md).
    let heap = unsafe { &mut *(*(vm as *mut crate::vm::VM)).heap_ptr };
    if let Some(region) = crate::value::arena::result_region_of(heap, value) {
        heap.decref_region(region);
    }
}

/// Release a capture cell's OWN runtime region (the `DecrefCellRegion`
/// instruction). It uses `region_of` (NOT `result_region_of`): it frees the
/// per-value env cell `populate_env` minted, and never unwraps to the inner
/// value's caller-owned region. Mirrors the interpreter's
/// `handle_decref_cell_region`.
#[no_mangle]
pub extern "C" fn elle_jit_decref_cell_region(tag: u64, payload: u64, vm: *mut ()) {
    let value = Value { tag, payload };
    let heap = unsafe { &mut *(*(vm as *mut crate::vm::VM)).heap_ptr };
    if let Some(region) = crate::value::arena::region_of(heap, value) {
        heap.decref_region(region);
    }
}

/// Increment the reference count of a value's region (the `IncrefValueRegion`
/// instruction). Mirrors the interpreter's `handle_incref_value_region`: it
/// resolves with `result_region_of`, which unwraps a capture cell. The caller's
/// `DecrefValueRegion` consumes this return-value handoff.
#[no_mangle]
pub extern "C" fn elle_jit_incref_value_region(tag: u64, payload: u64, vm: *mut ()) {
    let value = Value { tag, payload };
    let heap = unsafe { &mut *(*(vm as *mut crate::vm::VM)).heap_ptr };
    let r = crate::value::arena::result_region_of(heap, value);
    crate::value::arena::incref_for_escape(heap, r, crate::value::arena::EscapeSite::ReturnValue);
}

/// Record the pending join the next mint of `slot` consumes (the `JoinRegion`
/// instruction, docs/impl/region/colocation.md). Mirrors the interpreter's
/// `handle_join_region`.
#[no_mangle]
pub extern "C" fn elle_jit_join_region(tag: u64, payload: u64, vm: *mut (), slot: u32) {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let Some(static_id) = crate::hir::region::StaticRegion::new(slot) else {
        return;
    };
    vm.set_pending_join(static_id, Value { tag, payload });
}

/// No-op helpers, like the four at the top of this file: the vtable declares
/// and registers them, and the translator emits no call to either.
#[no_mangle]
pub extern "C" fn elle_jit_incref(tag: u64, payload: u64) -> JitValue {
    let _val = crate::value::Value { tag, payload };
    JitValue::nil()
}

#[no_mangle]
pub extern "C" fn elle_jit_decref(tag: u64, payload: u64) -> JitValue {
    let _val = crate::value::Value { tag, payload };
    JitValue::nil()
}

/// Link the child value's region as an Owned member of the parent value's
/// region, the `AdoptRegion` instruction.
///
/// Mirrors the interpreter's `handle_adopt_region` (src/vm/dispatch/region.rs).
/// It resolves both values with `result_region_of`, which unwraps a capture cell
/// to the inner value. The adopt freezes the child's RC, so only the parent's
/// subtree drop reclaims it; `adopt_region` leaves a joined region `Counted`
/// (docs/impl/region/colocation.md). An immediate operand (no region) or a
/// self-edge (same region) is a no-op. The parent and child arrive as explicit
/// Value pairs that the compiled code loaded only to drive this adopt.
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
/// region, resolving BOTH operands with `region_of`, NOT `result_region_of`:
/// the `AdoptCellRegion` instruction.
///
/// Mirrors the interpreter's `handle_adopt_cell_region`. It adopts a
/// `CaptureCell` operand's OWN region and never unwraps it to its content. That
/// lets the forest own a capture cell's arena and reclaim a local
/// recursive/letrec closure clique as a unit (docs/impl/region/adopt.md). It is
/// the `region_of` counterpart of `elle_jit_adopt_region`, as
/// `elle_jit_decref_cell_region` is of `elle_jit_decref_value_region`. An
/// immediate operand (no region) or a self-edge (same region) is a no-op.
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

/// Adopt the child value's region into the CURRENT activation's owner node,
/// the `AdoptIntoActivation` instruction.
///
/// Mirrors the interpreter's `handle_adopt_into_activation`
/// (src/vm/dispatch/region.rs). It resolves the child with `result_region_of`,
/// which unwraps a capture cell, lazily mints the activation's pages-less owner
/// node, and adopts. The adopt freezes the child's RC, so the node's subtree
/// drop at the activation's normal completion is its sole demise
/// (docs/impl/region/owner.md). An immediate child (no region) adopts nothing
/// and mints no node. The child arrives as an explicit Value pair that the
/// compiled code loads only to drive the adopt.
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
        // one-owner adopt assert.
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
/// completion: the JIT twin of the interpreter trampoline's clean-break
/// release (`VM::release_activation_dues`).
///
/// The `Return` path emits it before the region-map pop, in a function whose
/// LIR carries `AdoptIntoActivation`. A function that cannot mint a node never
/// pays the call.
#[no_mangle]
pub extern "C" fn elle_jit_release_activation_dues(vm: *mut ()) {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    vm.release_activation_dues();
}

/// Run the releases still owed by a COMPILED activation that an **error**
/// abandoned. This is the compiled entry to the walk the interpreter reaches
/// through `VM::release_abandoned_frame` (docs/impl/region/unwind.md).
///
/// `slots` and `regions` are the function's two release tables, which the
/// compiled prologue materializes once: the value routes' local slots and the
/// slot routes' static region ids. `locals` is the frame's local slots spilled
/// in slot order, so `locals[s]` is what `LoadLocal s` reads. The slot route
/// needs no spill. Its receipt is the activation region map, which the prologue
/// pushed and this call reads ahead of the matching pop.
///
/// The helper reads the payload off `fiber.signal`, which the raise has already
/// installed: the callee's error at the post-call exit, or this frame's own
/// emitted value at the `Emit` exit. Only an **error** abandons the frame, so a
/// signal without `SIG_ERROR` walks nothing. The post-call exception check also
/// fires on a halt, which the interpreter's trampoline does not walk either.
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

/// Free a co-owned region group as one unit, the `FreeRegionGroup` instruction.
///
/// Mirrors the interpreter's `handle_free_region_group`
/// (src/vm/dispatch/region.rs). `members_ptr` points at `count` member Values
/// that the compiled code spilled to a stack slot, as it does for
/// `elle_jit_push_param_frame`'s pairs. The helper resolves each member with
/// `result_region_of` and frees the whole set together. Interior member↔member
/// references reclaim with the group, and only Shared frontier references
/// cascade. An immediate member (no region) is skipped; an empty or null set is
/// a no-op.
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
