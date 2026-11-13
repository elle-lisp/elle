// audited: 2026-11-13
//! The tail-call JIT entry point: a native, parameter or collection callee answers it, and a closure leaves for the trampoline.
//!
//! docs/impl/jit.md
//! docs/impl/region/relocate.md
//! docs/impl/region/handoff.md

use super::*;

/// Handle a non-self tail call from JIT code.
///
/// A native, parameter or collection callee answers here. A CLOSURE callee always
/// leaves through `TAIL_CALL_SENTINEL`, so the trampoline that owns the activation
/// runs it, compiled or as bytecode, and mutual tail recursion costs no native
/// stack.
///
/// `defer_callee` and `arena_slot` are the `TailCall`'s own deferral channels,
/// flattened to `u32` for the C ABI (`TailDeferrals`). `handoff` is the call's
/// hand-off word, holding its late mints, or null where it has none.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub extern "C" fn elle_jit_tail_call(
    func_tag: u64,
    func_payload: u64,
    args_ptr: *const Value,
    nargs: u32,
    vm: *mut (),
    region_id: u32,
    defer_callee: u32,
    arena_slot: u32,
    handoff: *mut u64,
) -> JitValue {
    jit_tail_call_inner(
        func_tag,
        func_payload,
        args_ptr,
        nargs,
        vm,
        region_id,
        false,
        TailDeferrals {
            callee: defer_callee,
            arena_slot,
        },
        TailHandoff::read(handoff),
    )
}

/// The body of [`elle_jit_tail_call`], shared with the spliced entry point
/// [`elle_jit_tail_call_array`].
///
/// `spliced_args` is the same fact `tail_call_inner` reads on the interpreter
/// tier. An ordinary tail call MOVES its arguments: the caller's reference
/// transfers. A spliced one holds them in an args array the convention owns
/// and reclaims, so the callee mints one reference per parameter instead.
#[allow(clippy::too_many_arguments)]
pub(super) fn jit_tail_call_inner(
    func_tag: u64,
    func_payload: u64,
    args_ptr: *const Value,
    nargs: u32,
    vm: *mut (),
    region_id: u32,
    spliced_args: bool,
    defer: TailDeferrals,
    handoff: TailHandoff,
) -> JitValue {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let func = Value {
        tag: func_tag,
        payload: func_payload,
    };

    // Handle native functions, run as a call in call position runs one. A
    // suspend leaves every stash standing for the replay, so it makes every
    // late mint, as the interpreter's tail call does.
    if let Some(def) = func.as_native_def() {
        let args = args_ptr_to_value_slice(args_ptr, nargs);
        return jit_native_call(vm, def, args, region_id, |vm| {
            // Each stash holds its minted reference from here on.
            let _ = handoff.late.mint(unsafe { &mut *vm.heap_ptr }, args);
            handoff.made(handoff.late);
        });
    }

    if let Some((id, default)) = func.as_parameter() {
        let value = vm.call_parameter(id, default, nargs as usize);
        return JitValue::from_value(value.unwrap_or(Value::NIL));
    }

    // Handle closures — always use TAIL_CALL_SENTINEL so the trampoline
    // handles the call without growing the native stack.  This ensures
    // mutual tail recursion (A→B→A) doesn't overflow.
    if let Some(closure) = func.as_closure() {
        if !vm.check_arity(&closure.template.arity(), nargs as usize) {
            return JitValue::nil();
        }

        let args: Vec<Value> = (0..nargs as usize)
            .map(|i| unsafe { *args_ptr.add(i) })
            .collect();

        // The two releases this frame replacement strands, resolved while THIS
        // activation's region map is still the current one — the compiled caller's,
        // which its `Return` path has not popped yet. Read exactly as the
        // interpreter's `tail_call_inner` reads them, and before the pending call
        // copies the closure's env uncounted.
        let arena = crate::hir::region::StaticRegion::new(defer.arena_slot)
            .and_then(|slot| vm.runtime_region_for_release_slot(slot));
        let callee = (defer.callee != 0)
            .then(|| vm.tail_callee_release_region(func))
            .flatten();

        // Counted as `elle_jit_call` counts a non-tail call, and built as the
        // interpreter's `tail_call_inner` builds it: a pure MOVE, or a SPLICED
        // call whose callee mints its own (docs/impl/jit.md). The late mints
        // come ahead of it, as there.
        let late = handoff.late.mint(unsafe { &mut *vm.heap_ptr }, &args);
        let Some(tail) = vm.pending_tail_call_to(func, closure, args, spliced_args) else {
            // Bad keyword args — the error is on the fiber, and no callee runs.
            late.unmint(unsafe { &mut *vm.heap_ptr });
            return JitValue::nil();
        };
        handoff.made(handoff.late);
        vm.pending_tail_call = Some(tail);
        // Hand both stranded releases to the activation that runs the callee. This
        // activation is the compiled caller's, and it pops its own dues slot on the
        // way out with the sentinel, so recording there would drop them
        // (docs/impl/region/relocate.md). Written after the pending call is set, so
        // a path that fails earlier leaves nothing for another activation's push to
        // take.
        vm.pending_tail_deferrals
            .extend(arena.into_iter().chain(callee));

        return TAIL_CALL_SENTINEL;
    }

    // Callable collections: struct, array, set, string, bytes — in TAIL
    // position. Routed through the shared `dispatch_collection_call` for the
    // region routing and Rule-5 pass-through retain. Then returned exactly
    // like the native-tail Ok path above (`jit_handle_primitive_signal` →
    // `JitValue::from_value`): the value is handed back in the return register
    // with its one owning reference. The JIT-compiled caller's post-`TailCall`
    // block runs the owned-arg releases + result handling.
    // Handing the value over through `fiber.signal` instead reaches neither, and
    // a returned element co-located in its collection's region is then freed
    // under the caller's borrow.
    if let Some(result) = {
        let region = crate::hir::region::StaticRegion::new(region_id)
            .expect("JIT region slot is nonzero — emitter invariant");
        vm.dispatch_collection_call(&func, args_ptr_to_value_slice(args_ptr, nargs), region)
    } {
        match result {
            Ok(value) => {
                return JitValue::from_value(value);
            }
            Err((kind, msg)) => {
                vm.set_error(kind, msg);
                return JitValue::nil();
            }
        }
    }

    vm.set_error("type-error", format!("Cannot call {}", vm.show_value(func)));
    JitValue::nil()
}
