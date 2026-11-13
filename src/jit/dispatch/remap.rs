// audited: 2026-11-11
//! The JIT's region-remap frame helpers: the push at entry, the pop before each return, and the clear at a self tail call.
//!
//! docs/impl/region/mechanism.md
//! docs/impl/region/rules.md
//! docs/impl/region/merging.md

/// Push a fresh per-activation region-remap frame on JIT function entry.
///
/// The JIT analog of the interpreter's `execute_bytecode_saving_stack` pushing
/// an `activation_region_map` (src/vm/execute.rs). Every compiled function's
/// Cranelift prologue emits it. So the body's per-execution alloc regions
/// (`elle_jit_resolve_alloc_region`) and slot-resolved `DecrefRegion`s
/// (`elle_jit_decref_region`) resolve against THIS activation's slot→phys map,
/// not the caller's. It covers every entry path, interpreter→JIT and
/// JIT-to-JIT, because it is part of the compiled function
/// (docs/impl/region/rules.md Rule 4, "per activation").
#[no_mangle]
pub extern "C" fn elle_jit_push_region_map(vm: *mut ()) {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    vm.push_activation_region_map();
}

/// Pop the current region-remap frame. Emitted before every `return` in a
/// compiled function: a normal return, the tail-call sentinel, the error
/// return, and the yield/Emit side-exit AFTER the suspend captured the map.
///
/// Like the interpreter's `pop_activation_region_map`, this only drops the
/// lookup table, and never decrefs an entry. `DecrefRegion` and the
/// owned-param releases free the regions, and a tail-moved argument's ownership
/// transfers to the callee.
#[no_mangle]
pub extern "C" fn elle_jit_pop_region_map(vm: *mut ()) {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    vm.pop_activation_region_map();
}

/// Empty the current region-remap frame at a compiled self tail call, before
/// the jump to the loop header. The loop keeps its activation, so the frame
/// still maps every slot the last trip resolved, and each release is dead past
/// the call. The trampoline empties the frame the same way for a
/// frame-replacing tail callee (`VM::clear_region_map_for_tail_callee`,
/// docs/impl/region/merging.md).
#[no_mangle]
pub extern "C" fn elle_jit_clear_region_map(vm: *mut ()) {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    vm.clear_region_map_for_tail_callee();
}
