//! Region reference-counting and ownership-forest instruction handlers.
//!
//! These arms of the dispatch loop manage the per-region RC baseline and the
//! ownership forest's adopt / subtree-drop / co-owned-group emit
//! (docs/impl/region/ownership.md). They are factored out of the dispatch table so
//! it stays a flat one-line-per-opcode jump table; the heavy tracing/freelog
//! bookkeeping lives here.

use crate::vm::core::VM;

/// Set the freelog "reason" for a region demise — the shared body of the three
/// `handle_decref_*` arms. When freelog is enabled it records `"<subject> @ <loc>"`,
/// appending a captured backtrace when the region is about to free (rc ≤ 1) under
/// `--trace=freebt`. `subject` builds the op-specific prefix lazily, so a disabled
/// freelog (the hot path) pays nothing. Kept as one helper so a region-demise op
/// reuses it rather than adding another copy of the loc/backtrace shape.
fn freelog_decref_reason(
    heap: &crate::value::fiberheap::FiberHeap,
    region_id: crate::hir::region::RuntimeRegion,
    locations: crate::value::closure::LocationTable<'_>,
    instr_ip: usize,
    subject: impl FnOnce() -> String,
) {
    if !crate::value::fiberheap::freelog::enabled() {
        return;
    }
    let loc = locations
        .get(instr_ip)
        .map(|l| format!("{l}"))
        .unwrap_or_else(|| "?".to_string());
    let bt = if heap.region_rc(region_id) <= 1 && crate::config::get().has_trace("freebt") {
        format!(
            "\n    backtrace:\n{}",
            std::backtrace::Backtrace::force_capture()
        )
    } else {
        String::new()
    };
    crate::value::fiberheap::freelog::set_reason_owned(format!("{} @ {loc}{bt}", subject()));
}

pub(crate) fn handle_incref_region(vm: &mut VM, bytecode: &[u8], ip: &mut usize) {
    // Cross-region increfs are value-based (auto-incref in
    // `alloc_obj` scans an object's contents; runtime push/put
    // for mutable collections). This arm is defensive: resolve
    // the slot in the current frame and incref the physical
    // region if it exists, otherwise skip (never mint).
    let region = vm.read_static_region(bytecode, ip);
    let phys = vm
        .fiber
        .activation_region_maps
        .last()
        .and_then(|f| f.get(&region.get()).map(|m| m.region));
    if let Some(phys) = phys {
        crate::value::arena::incref_for_escape(
            unsafe { &mut *vm.heap_ptr },
            Some(phys),
            crate::value::arena::EscapeSite::ImmutableContents,
        );
    }
}

pub(crate) fn handle_decref_region(
    vm: &mut VM,
    bytecode: &[u8],
    ip: &mut usize,
    locations: crate::value::closure::LocationTable<'_>,
    instr_ip: usize,
) {
    let raw_region = vm.read_static_region(bytecode, ip);
    if let Some(region_id) = vm.take_runtime_region_for_drop_slot(raw_region) {
        if crate::config::get().has_trace("rc") {
            let rc = vm.heap().region_rc(region_id);
            eprintln!(
                "[trace:rc] DecrefRegion({region_id}) [slot {raw_region}] rc={rc} alloc_count={}",
                vm.heap().len()
            );
        }
        freelog_decref_reason(vm.heap(), region_id, locations, instr_ip, || {
            format!("DecrefRegion(bytecode slot {raw_region})")
        });
        vm.heap().decref_region(region_id);
    }
}

pub(crate) fn handle_decref_value_region(
    vm: &mut VM,
    locations: crate::value::closure::LocationTable<'_>,
    instr_ip: usize,
) {
    let value = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: stack underflow on DecrefValueRegion");
    let region_id = crate::value::arena::result_region_of(unsafe { &mut *vm.heap_ptr }, value);
    // The result's *runtime* region is released unconditionally
    // (immediates — region `None` — excepted). The caller half of the
    // prediction-free calling convention: the callee handed
    // back one owning reference via `IncrefValueRegion`, and
    // this consumes it at the result binding's decref_point.
    if let Some(region_id) = region_id {
        if crate::config::get().has_trace("rc") {
            let rc = vm.heap().region_rc(region_id);
            let loc = locations
                .get(instr_ip)
                .map(|l| format!("{l}"))
                .unwrap_or_else(|| "?".to_string());
            eprintln!(
                "[trace:rc] DecrefValueRegion({region_id}) rc={rc} of {} @ {loc} ip={instr_ip}",
                value.type_name()
            );
        }
        freelog_decref_reason(vm.heap(), region_id, locations, instr_ip, || {
            format!(
                "DecrefValueRegion of {} (runtime region {})",
                value.type_name(),
                region_id,
            )
        });
        vm.heap().decref_region(region_id);
    } else if crate::config::get().has_trace("rc") {
        let loc = locations
            .get(instr_ip)
            .map(|l| format!("{l}"))
            .unwrap_or_else(|| "?".to_string());
        eprintln!(
            "[trace:rc] DecrefValueRegion: skip (no region) of {} @ {loc} ip={instr_ip}",
            value.type_name()
        );
    }
}

pub(crate) fn handle_decref_cell_region(
    vm: &mut VM,
    locations: crate::value::closure::LocationTable<'_>,
    instr_ip: usize,
) {
    let value = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: stack underflow on DecrefCellRegion");
    // `region_of`, NOT `result_region_of`: free the CELL's own
    // region (the per-value env cell `populate_env` minted),
    // never unwrap to the inner value's caller-owned region.
    // This is the captured-binding half of the owned-binding
    // release. Immediates excepted (region None).
    let region_id = crate::value::arena::region_of(unsafe { &mut *vm.heap_ptr }, value);
    if let Some(region_id) = region_id {
        if crate::config::get().has_trace("rc") {
            let rc = vm.heap().region_rc(region_id);
            eprintln!("[trace:rc] DecrefCellRegion({region_id}) rc={rc}");
        }
        freelog_decref_reason(vm.heap(), region_id, locations, instr_ip, || {
            format!(
                "DecrefCellRegion of {} (runtime region {})",
                value.type_name(),
                region_id,
            )
        });
        vm.heap().decref_region(region_id);
    } else if crate::config::get().has_trace("rc") {
        eprintln!("[trace:rc] DecrefCellRegion: skip (no region)");
    }
}

pub(crate) fn handle_incref_value_region(
    vm: &mut VM,
    locations: crate::value::closure::LocationTable<'_>,
    instr_ip: usize,
) {
    // Peek, don't pop: the value is the function result and
    // must remain on the stack for the caller.
    let value = *vm
        .fiber
        .stack
        .last()
        .expect("VM bug: stack underflow on IncrefValueRegion");
    if crate::config::get().has_trace("rc") {
        let loc = locations
            .get(instr_ip)
            .map(|l| format!("{l}"))
            .unwrap_or_else(|| "?".to_string());
        eprintln!(
            "[trace:rc] IncrefValueRegion of {} @ {loc} ip={instr_ip}",
            value.type_name()
        );
    }
    let region_id = crate::value::arena::result_region_of(unsafe { &mut *vm.heap_ptr }, value);
    // Mirror of DecrefValueRegion, but unconditional: a
    // function hands its caller one owning reference to the
    // result's runtime region. Skip immediates / unregioned
    // values (no region) — they never participate in RC.
    crate::value::arena::incref_for_escape(
        unsafe { &mut *vm.heap_ptr },
        region_id,
        crate::value::arena::EscapeSite::ReturnValue,
    );
}

pub(crate) fn handle_adopt_region(vm: &mut VM) {
    // The ownership forest: link the child value's region as Owned
    // by the parent value's region (docs/impl/region/ownership.md
    // § "Adoption and subtree drop"). Pop both (the lowerer loaded
    // them solely to drive this adopt), resolve each to its runtime
    // region, and adopt — freezing the child's RC so it frees only
    // with the parent's subtree drop. An immediate operand (no
    // region) or a self-edge (same region) is a no-op.
    let child = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: stack underflow on AdoptRegion (child)");
    let parent = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: stack underflow on AdoptRegion (parent)");
    let child_region = crate::value::arena::result_region_of(unsafe { &mut *vm.heap_ptr }, child);
    let parent_region = crate::value::arena::result_region_of(unsafe { &mut *vm.heap_ptr }, parent);
    if let (Some(p), Some(c)) = (parent_region, child_region) {
        if p != c {
            if crate::config::get().has_trace("rc") {
                eprintln!("[trace:rc] AdoptRegion parent={p} child={c}");
            }
            vm.heap().adopt_region(p, c);
        }
    }
}

pub(crate) fn handle_adopt_cell_region(vm: &mut VM) {
    // The cell-aware adopt of the ownership forest: like `handle_adopt_region`,
    // but resolves BOTH operands with `region_of`, NOT `result_region_of`, so a
    // `CaptureCell` operand's OWN region is used (never unwrapped to its content).
    // This is what lets the forest own a capture cell's arena and reclaim a local
    // recursive/letrec closure clique — cell↔closure — as one subtree
    // (docs/impl/region/adopt.md § "The capture adopt"). An immediate operand (no
    // region) or a self-edge (same region) is a no-op, exactly as `AdoptRegion`.
    let child = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: stack underflow on AdoptCellRegion (child)");
    let parent = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: stack underflow on AdoptCellRegion (parent)");
    let child_region = crate::value::arena::region_of(unsafe { &mut *vm.heap_ptr }, child);
    let parent_region = crate::value::arena::region_of(unsafe { &mut *vm.heap_ptr }, parent);
    if let (Some(p), Some(c)) = (parent_region, child_region) {
        if p != c {
            if crate::config::get().has_trace("rc") {
                eprintln!("[trace:rc] AdoptCellRegion parent={p} child={c}");
            }
            vm.heap().adopt_region(p, c);
        }
    }
}

pub(crate) fn handle_adopt_into_activation(vm: &mut VM) {
    // The ownership forest's activation owner: adopt the child value's region
    // into the CURRENT activation's owner node (docs/impl/region/owner.md
    // § "Owner nodes — an activation as a forest root"). Pop the child (the
    // lowerer loads it solely to drive this adopt), resolve its runtime region
    // (`result_region_of` — unwraps a capture cell), lazily mint the node, and
    // adopt — freezing the child's RC so the node's subtree drop at the
    // activation's normal completion is its sole demise. An immediate operand
    // (no region) adopts nothing and mints no node.
    let child = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: stack underflow on AdoptIntoActivation");
    let child_region = crate::value::arena::result_region_of(unsafe { &mut *vm.heap_ptr }, child);
    if let Some(c) = child_region {
        // Idempotent on an already-Owned child: a region delivered to this
        // channel a second time (a masked-`:error` fiber restarted after
        // handing out the same payload) keeps its FIRST owner, whose release
        // post-dominates the later hand-off's discard-gated use — instead of
        // tripping `adopt_region`'s one-owner assert. The compiler-paired
        // `AdoptRegion` sites keep the strict assert; only this consumer-facing
        // channel absorbs re-delivery (docs/impl/region/owner.md § "Owner
        // nodes").
        if vm.heap().region_is_owned(c) {
            return;
        }
        let node = vm.activation_owner_node();
        if node != c {
            if crate::config::get().has_trace("rc") {
                eprintln!("[trace:rc] AdoptIntoActivation node={node} child={c}");
            }
            vm.heap().adopt_region(node, c);
        }
    }
}

pub(crate) fn handle_free_region_group(vm: &mut VM, bytecode: &[u8], ip: &mut usize) {
    // The ownership forest: free a co-owned region group as one unit.
    // Pop the `count` member
    // values the lowerer loaded to drive this free, resolve each to its
    // runtime region, and free the whole set together — interior
    // member↔member references reclaim with the group, only Shared
    // frontier references cascade. The drop is wholesale (count-independent),
    // so members carry their unchanged reference counts harmlessly.
    let count = bytecode[*ip] as usize;
    *ip += 1;
    let mut members: Vec<crate::hir::region::RuntimeRegion> = Vec::with_capacity(count);
    for _ in 0..count {
        let value = vm
            .fiber
            .stack
            .pop()
            .expect("VM bug: stack underflow on FreeRegionGroup");
        if let Some(r) = crate::value::arena::result_region_of(unsafe { &mut *vm.heap_ptr }, value)
        {
            members.push(r);
        }
    }
    if crate::config::get().has_trace("rc") {
        eprintln!("[trace:rc] FreeRegionGroup members={members:?}");
    }
    if !members.is_empty() {
        vm.heap().free_region_group(&members);
    }
}

pub(crate) fn handle_assert_region_matches(vm: &mut VM, bytecode: &[u8], ip: &mut usize) {
    // Debug-only equivalence oracle for compile-time region
    // coalescing. Read the slot operand unconditionally so `ip`
    // stays aligned in every build (release no-ops the check).
    let region = vm.read_static_region(bytecode, ip);
    // Prove the coalesced slot resolves — through THIS
    // activation's map — to the same physical region the value
    // on top of the stack actually lives in. The value is the
    // return value (peek, never pop: the following
    // `IncrefRegion`/`Return` reads it). A mismatch means a slot
    // was made to name the wrong region; its free-time cascade
    // would reclaim a live region (a UAF). Detonate here, at the
    // exact instruction, under the trustworthy guardfree oracle,
    // rather than corrupt the heap later (docs/impl/region/mechanism.md
    // § "the equivalence oracle").
    #[cfg(debug_assertions)]
    {
        let value = *vm
            .fiber
            .stack
            .last()
            .expect("VM bug: stack underflow on AssertRegionMatches");
        let resolved = vm
            .fiber
            .activation_region_maps
            .last()
            .and_then(|f| f.get(&region.get()).map(|m| m.region));
        let actual = crate::value::arena::region_of(unsafe { &mut *vm.heap_ptr }, value);
        assert!(
            resolved == actual,
            "AssertRegionMatches: coalesced slot {region} resolved to \
             {resolved:?} but the value ({}) lives in {actual:?} — a \
             mis-coalesce (docs/impl/region/mechanism.md § \"the equivalence \
             oracle\")",
            value.type_name(),
        );
    }
    #[cfg(not(debug_assertions))]
    let _ = region;
}
