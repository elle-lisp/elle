// audited: 2026-10-22
//! Collection-mutation and access intrinsics (`%get`/`%put`/`%del`/`%has?`/the
//! `*-push` family/`%pop`/`%freeze`/`%thaw`).
//!
//! docs/impl/region/ctx.md
//!
//! Unlike the pure predicates, these can allocate a fresh container. So they run
//! their bodies under a per-call result-region discipline (`run_alloc_intrinsic`,
//! `run_in_slot`), and delegate to the shared `prim_*` funnel natives so the
//! interpreter and JIT paths stay in lockstep.

use crate::hir::region::RuntimeRegion;
use crate::value::Value;
use crate::vm::core::VM;

pub(crate) fn handle_intr_get(vm: &mut VM) {
    use crate::value::TableKey;
    let key = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    let obj = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    let result = if let Some(elems) = obj.as_array() {
        let i = key.as_int().expect("%get: array index must be int") as usize;
        elems[i]
    } else if let Some(a) = obj.array_mut_read() {
        let i = key.as_int().expect("%get: @array index must be int") as usize;
        a[i]
    } else if let Some(pairs) = obj.as_struct() {
        // An unhashable key (a float, a mutable container) is a program bug:
        // it can never have been stored, so a lookup with one is nonsense
        // rather than an absent key. Panic — the interpreter contract is that
        // `%get`'s callers only reach the struct arm with a hashable key.
        let tk = TableKey::from_value(&key).expect("%get: unhashable key");
        crate::value::sorted_struct_get(pairs, &tk)
            .copied()
            .unwrap_or(Value::NIL)
    } else if let Some(t) = obj.struct_mut_read() {
        let tk = TableKey::from_value(&key).expect("%get: unhashable key");
        t.get(&tk).unwrap_or(Value::NIL)
    } else if let Some(r) = obj.with_string(|s| {
        let i = key.as_int().expect("%get: string index must be int") as usize;
        // The grapheme is born in the SOURCE string's own region, a pass-through
        // result like `CollectionCallResult`. `%get` carries no region operand,
        // because it is not modelled as allocating and the JIT path allocates
        // nothing. So the result co-locates with the indexed string, whose
        // lifetime covers the result's use. A heap string always has a region.
        match crate::segment::graphemes(s, vm.unicode_generation).nth(i) {
            Some(g) => {
                let region = crate::value::arena::region_of(unsafe { &mut *vm.heap_ptr }, obj)
                    .expect("%get: indexed string must have a region");
                crate::value::build::string(unsafe { &mut *vm.heap_ptr }, g, region)
            }
            None => Value::NIL,
        }
    }) {
        r
    } else {
        panic!("%get: unsupported type {}", obj.type_name())
    };
    vm.fiber.stack.push(result);
}

/// Run a conditionally-allocating intrinsic body (`%put`/`%del`/`%string-push`)
/// with the same per-call result-region discipline as `VM::dispatch_native_call`:
/// these opcodes run the very primitive bodies the funnel natives use. Mint a
/// fresh region, run the body into it, then pass-through-retain. The caller's
/// `DecrefValueRegion`, emitted because the region walk marks these ops as
/// `call_result_regions`, then frees the right *runtime* region. That is the
/// minted region for an immutable fresh copy, or arg 0's region for an in-place
/// mutation (balanced by the retain).
///
/// Both the driving `vm` and the `heap` are explicit: the interpreter handlers
/// pass their own `vm`/`vm.heap_ptr`; the JIT helpers resolve the `vm` from the
/// threaded `JitCtx` and pass `(*vm).heap_ptr` for the heap. The VM is named at
/// every call (docs/impl/region/ctx.md).
pub(crate) fn run_alloc_intrinsic(
    vm: *mut crate::vm::VM,
    heap: *mut crate::value::fiberheap::FiberHeap,
    body: impl FnOnce(&mut crate::primitives::ctx::NativeCtx) -> (crate::value::SignalBits, Value),
) -> (crate::value::SignalBits, Value) {
    let region = unsafe { (*heap).new_runtime_region() };
    let (bits, value) = {
        let mut ctx =
            crate::primitives::ctx::NativeCtx::with_region_vm(region, unsafe { &mut *heap }, vm);
        body(&mut ctx)
    };
    crate::value::arena::pass_through_retain(unsafe { &mut *heap }, value, region);
    (bits, value)
}

pub(crate) fn handle_intr_put(vm: &mut VM) {
    let val = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    let key = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    let obj = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    // Delegate to prim_put — it handles all the polymorphic cases. Runtime type
    // errors (an unhashable key, say) propagate via fiber.signal so `protect` can
    // observe them, matching the `prim_put` funnel native that compiled
    // call-position `%put` lowers to. The immutable-copy result is born in this
    // call's own minted region (run_alloc_intrinsic).
    let (bits, result) = run_alloc_intrinsic(vm, vm.heap_ptr, |ctx| {
        crate::primitives::access::prim_put(ctx, &[obj, key, val])
    });
    if bits.intersects(crate::value::SIG_ERROR) {
        vm.fiber.signal = Some((bits, result));
        vm.fiber.stack.push(Value::NIL);
        return;
    }
    vm.fiber.stack.push(result);
}

pub(crate) fn handle_intr_del(vm: &mut VM) {
    let key = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    let obj = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    let (bits, result) = run_alloc_intrinsic(vm, vm.heap_ptr, |ctx| {
        crate::primitives::lstruct::prim_del(ctx, &[obj, key])
    });
    if bits.intersects(crate::value::SIG_ERROR) {
        vm.fiber.signal = Some((bits, result));
        vm.fiber.stack.push(Value::NIL);
        return;
    }
    vm.fiber.stack.push(result);
}

pub(crate) fn handle_intr_has(vm: &mut VM) {
    let key = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    let obj = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    // `%has?` is Immediate (returns a bool, allocates nothing), but `prim_has_key`
    // is a PrimFn requiring a NativeCtx; a `boundary` ctx over the VM mints its
    // own (unused) region.
    let (bits, result) = crate::primitives::lstruct::prim_has_key(
        &mut crate::primitives::ctx::NativeCtx::boundary_vm(vm),
        &[obj, key],
    );
    if bits.intersects(crate::value::SIG_ERROR) {
        vm.fiber.signal = Some((bits, result));
        vm.fiber.stack.push(Value::NIL);
        return;
    }
    vm.fiber.stack.push(result);
}

pub(crate) fn handle_intr_push(vm: &mut VM) {
    let value = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    let collection = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    // Delegate to prim_push: @array mutates in place (returns arg 0,
    // pass-through), immutable array yields a fresh copy. The immutable copy is
    // born in this call's own minted region (run_alloc_intrinsic) and
    // pass-through-retained, matching the call-result-region model
    // (`produces_call_result_region`) the lowerer uses for %array-push, which
    // frees the result via a value-based DecrefValueRegion.
    let (bits, result) = run_alloc_intrinsic(vm, vm.heap_ptr, |ctx| {
        crate::primitives::intrinsics::prim_push(ctx, &[collection, value])
    });
    if bits.intersects(crate::value::SIG_ERROR) {
        // A type mismatch reaching this opcode is a compiler bug (emitted
        // without the operand proof it requires) — panic like the sibling
        // intrinsics, not signal. The catchable path is the registered
        // NativeFn, which validates dynamic value-position calls.
        panic!("%array-push: unsupported type {}", collection.type_name());
    }
    vm.fiber.stack.push(result);
}

pub(crate) fn handle_intr_string_push(vm: &mut VM) {
    let value = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    let collection = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    // Delegate to prim_string_push for the push itself. A runtime type
    // mismatch here means the compiler emitted the intrinsic opcode
    // without the operand proof it requires — a compiler bug, so panic
    // loudly like the sibling intrinsics (%array-push, %bytes-push, %get,
    // %length). The catchable-error path is the registered NativeFn that
    // validates dynamic value-position calls; signaling from here instead
    // would let code the compiler stamped signal-free raise SIG_ERROR at
    // runtime.
    let (bits, result) = run_alloc_intrinsic(vm, vm.heap_ptr, |ctx| {
        crate::primitives::intrinsics::prim_string_push(ctx, &[collection, value])
    });
    if bits.intersects(crate::value::SIG_ERROR) {
        panic!(
            "%string-push: expected string or @string, got {} (pushed value: {})",
            collection.type_name(),
            value.type_name()
        );
    }
    vm.fiber.stack.push(result);
}

pub(crate) fn handle_intr_bytes_push(vm: &mut VM) {
    let value = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    let collection = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    // Delegate to prim_bytes_push. @bytes appends in place and returns arg 0, a
    // pass-through. Immutable bytes yields a fresh copy born in this call's own
    // minted region (run_alloc_intrinsic) and pass-through-retained, matching the
    // call-result-region model (`produces_call_result_region`) the lowerer uses
    // for %bytes-push.
    let (bits, result) = run_alloc_intrinsic(vm, vm.heap_ptr, |ctx| {
        crate::primitives::intrinsics::prim_bytes_push(ctx, &[collection, value])
    });
    if bits.intersects(crate::value::SIG_ERROR) {
        panic!(
            "%bytes-push: expected bytes or @bytes (value an integer byte or a bytes value), \
             got {} (value {})",
            collection.type_name(),
            value.type_name()
        );
    }
    vm.fiber.stack.push(result);
}

pub(crate) fn handle_intr_pop(vm: &mut VM) {
    let val = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    let empty = val
        .array_mut_read()
        .expect("intr_pop: expected @array")
        .is_empty();
    if empty {
        vm.set_error("argument-error", "pop: empty @array");
        vm.fiber.stack.push(Value::NIL);
    } else {
        // `pop_with_decref` MOVES the element out, holding the caller's owning
        // reference before releasing the container's. This opcode path takes
        // no pass-through retain of its own — the body's retain IS the
        // caller's reference (the funnel-native path likewise skips its retain
        // for the `moves_out` `%pop`; `dispatch_native_call`).
        let popped = crate::value::arena::pop_with_decref(unsafe { &mut *vm.heap_ptr }, val);
        vm.fiber.stack.push(popped);
    }
}

pub(crate) fn handle_intr_freeze(vm: &mut VM, region: RuntimeRegion) {
    let val = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    let result = run_in_slot(
        vm,
        region,
        "%freeze",
        crate::primitives::intrinsics::prim_freeze,
        val,
    );
    vm.fiber.stack.push(result);
}

pub(crate) fn handle_intr_thaw(vm: &mut VM, region: RuntimeRegion) {
    let val = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    let result = run_in_slot(
        vm,
        region,
        "%thaw",
        crate::primitives::intrinsics::prim_thaw,
        val,
    );
    vm.fiber.stack.push(result);
}

/// Run the `%freeze` or `%thaw` body over `val`, born in `region`: the slot the
/// lowerer assigned the allocating op, which its matching `DecrefRegion` frees.
/// The JIT runs the same body (`elle_jit_freeze`, `elle_jit_thaw`), so the two
/// tiers cannot drift. A signal is a broken intrinsic contract.
fn run_in_slot(
    vm: &mut VM,
    region: RuntimeRegion,
    name: &str,
    body: crate::value::heap::PrimFn,
    val: Value,
) -> Value {
    let vm_ptr: *mut VM = vm;
    let heap = unsafe { &mut *(*vm_ptr).heap_ptr };
    let mut ctx = crate::primitives::ctx::NativeCtx::with_region_vm(region, heap, vm_ptr);
    let (bits, result) = body(&mut ctx, &[val]);
    assert!(
        !bits.intersects(crate::value::SIG_ERROR),
        "{name}: intrinsic contract violated"
    );
    result
}
