use super::core::VM;
use crate::hir::region::RuntimeRegion;
use crate::value::Value;

pub(crate) fn handle_is_nil(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsNil");
    vm.fiber.stack.push(Value::bool(val.is_nil()));
}

pub(crate) fn handle_is_pair(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsPair");
    vm.fiber.stack.push(Value::bool(val.is_pair()));
}

pub(crate) fn handle_is_number(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsNumber");
    vm.fiber.stack.push(Value::bool(val.is_number()));
}

pub(crate) fn handle_is_symbol(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsSymbol");
    vm.fiber.stack.push(Value::bool(val.is_symbol()));
}

pub(crate) fn handle_not(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on Not");
    vm.fiber.stack.push(Value::bool(!val.is_truthy()));
}

pub(crate) fn handle_is_array(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsArray");
    vm.fiber.stack.push(Value::bool(val.is_array()));
}

pub(crate) fn handle_is_array_mut(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsArrayMut");
    vm.fiber.stack.push(Value::bool(val.is_array_mut()));
}

pub(crate) fn handle_is_struct(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsStruct");
    vm.fiber.stack.push(Value::bool(val.is_struct()));
}

pub(crate) fn handle_array_len(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on ArrayMutLen");
    let len = if let Some(a) = val.as_array_mut() {
        a.borrow().len() as i64
    } else if let Some(t) = val.as_array() {
        t.len() as i64
    } else {
        0
    };
    vm.fiber.stack.push(Value::int(len));
}

pub(crate) fn handle_is_struct_mut(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsStructMut");
    vm.fiber.stack.push(Value::bool(val.is_struct_mut()));
}

pub(crate) fn handle_is_empty_list(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsEmptyList");
    vm.fiber.stack.push(Value::bool(val.is_empty_list()));
}

pub(crate) fn handle_is_set(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsSet");
    vm.fiber.stack.push(Value::bool(val.is_set()));
}

pub(crate) fn handle_is_set_mut(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsSetMut");
    vm.fiber.stack.push(Value::bool(val.is_set_mut()));
}

pub(crate) fn handle_is_bool(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsBool");
    vm.fiber.stack.push(Value::bool(val.is_bool()));
}

pub(crate) fn handle_is_int(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsInt");
    vm.fiber.stack.push(Value::bool(val.is_int()));
}

pub(crate) fn handle_is_float(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsFloat");
    vm.fiber.stack.push(Value::bool(val.is_float()));
}

pub(crate) fn handle_is_string(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsString");
    vm.fiber
        .stack
        .push(Value::bool(val.is_string() || val.is_string_mut()));
}

pub(crate) fn handle_is_keyword(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsKeyword");
    vm.fiber.stack.push(Value::bool(val.is_keyword()));
}

pub(crate) fn handle_is_bytes(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsBytes");
    vm.fiber
        .stack
        .push(Value::bool(val.is_bytes() || val.is_bytes_mut()));
}

pub(crate) fn handle_is_box(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsBox");
    vm.fiber.stack.push(Value::bool(val.is_lbox()));
}

pub(crate) fn handle_is_closure(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsClosure");
    vm.fiber.stack.push(Value::bool(val.is_closure()));
}

pub(crate) fn handle_is_fiber(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsFiber");
    vm.fiber.stack.push(Value::bool(val.is_fiber()));
}

pub(crate) fn handle_type_of(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on TypeOf");
    vm.fiber.stack.push(Value::keyword(val.type_name()));
}

pub(crate) fn handle_length(vm: &mut VM) {
    let val = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    use unicode_segmentation::UnicodeSegmentation;
    let len = if val.is_empty_list() || val.is_nil() {
        0
    } else if val.is_pair() {
        val.list_to_vec().expect("%length: improper list").len()
    } else if let Some(a) = val.as_array() {
        a.len()
    } else if let Some(a) = val.as_array_mut() {
        a.borrow().len()
    } else if let Some(s) = val.as_struct() {
        s.len()
    } else if let Some(s) = val.as_struct_mut() {
        s.borrow().len()
    } else if let Some(s) = val.as_set() {
        s.len()
    } else if let Some(s) = val.as_set_mut() {
        s.borrow().len()
    } else if let Some(b) = val.as_bytes() {
        b.len()
    } else if let Some(b) = val.as_bytes_mut() {
        b.borrow().len()
    } else if let Some(r) = val.with_string(|s| s.graphemes(true).count()) {
        r
    } else if let Some(buf) = val.as_string_mut() {
        let b = buf.borrow();
        std::str::from_utf8(&b)
            .expect("%length: @string invalid UTF-8")
            .graphemes(true)
            .count()
    } else {
        panic!("%length: unsupported type {}", val.type_name())
    };
    vm.fiber.stack.push(Value::int(len as i64));
}

pub(crate) fn handle_intr_get(vm: &mut VM) {
    use crate::value::TableKey;
    let key = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    let obj = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    let result = if let Some(elems) = obj.as_array() {
        let i = key.as_int().expect("%get: array index must be int") as usize;
        elems[i]
    } else if let Some(a) = obj.as_array_mut() {
        let i = key.as_int().expect("%get: @array index must be int") as usize;
        a.borrow()[i]
    } else if let Some(pairs) = obj.as_struct() {
        // An unhashable key (a float, a mutable container) is a program bug:
        // it can never have been stored, so a lookup with one is nonsense
        // rather than an absent key. Panic — the interpreter contract is that
        // `%get`'s callers only reach the struct arm with a hashable key.
        let tk = TableKey::from_value(&key).expect("%get: unhashable key");
        crate::value::sorted_struct_get(pairs, &tk)
            .copied()
            .unwrap_or(Value::NIL)
    } else if let Some(t) = obj.as_struct_mut() {
        let tk = TableKey::from_value(&key).expect("%get: unhashable key");
        t.borrow().get(&tk).copied().unwrap_or(Value::NIL)
    } else if let Some(r) = obj.with_string(|s| {
        use unicode_segmentation::UnicodeSegmentation;
        let i = key.as_int().expect("%get: string index must be int") as usize;
        // The grapheme is born in the SOURCE string's own region (a pass-through
        // result, like `CollectionCallResult`): `%get` carries no region operand
        // (it is not modelled as allocating; the JIT path does not allocate at
        // all), so the result co-locates with the indexed string whose lifetime
        // covers the result's use. A heap string always has a region.
        match s.graphemes(true).nth(i) {
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
/// with the same per-call result-region discipline as `VM::dispatch_native_call`
/// — these opcodes run the very primitive bodies the funnel natives use. Mint a
/// fresh region, run the body into it, then pass-through-retain so the caller's
/// `DecrefValueRegion` (emitted because the region walk now marks these ops as
/// `call_result_regions`) frees the right *runtime* region: the minted region
/// for an immutable fresh copy, or arg 0's region for an in-place mutation
/// (balanced by the retain).
///
/// Both the driving `vm` and the `heap` are explicit: the interpreter handlers
/// pass their own `vm`/`vm.heap_ptr`; the JIT helpers resolve the `vm` from the
/// threaded `JitCtx` and pass `(*vm).heap_ptr` for the heap. The VM is named at
/// every call (docs/impl/region/ctx.md "JIT intrinsic helpers reach the VM through
/// a JitCtx").
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
    // errors (e.g. unhashable key) propagate via fiber.signal so `protect` can
    // observe them, matching the `prim_put` funnel native that compiled
    // call-position `%put` lowers to. The immutable-copy result is born in this
    // call's own minted region (run_alloc_intrinsic).
    let (bits, result) = run_alloc_intrinsic(vm, vm.heap_ptr, |ctx| {
        crate::primitives::access::prim_put(ctx, &[obj, key, val])
    });
    if bits.contains(crate::value::SIG_ERROR) {
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
    if bits.contains(crate::value::SIG_ERROR) {
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
    if bits.contains(crate::value::SIG_ERROR) {
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
    // pass-through-retained — matching the call-result-region model
    // (`produces_call_result_region`) the lowerer now uses for %array-push, which
    // frees the result via a value-based DecrefValueRegion.
    let (bits, result) = run_alloc_intrinsic(vm, vm.heap_ptr, |ctx| {
        crate::primitives::intrinsics::prim_push(ctx, &[collection, value])
    });
    if bits.contains(crate::value::SIG_ERROR) {
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
    if bits.contains(crate::value::SIG_ERROR) {
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
    // Delegate to prim_bytes_push: @bytes appends in place (returns arg 0,
    // pass-through), immutable bytes yields a fresh copy born in this call's own
    // minted region (run_alloc_intrinsic) and pass-through-retained — matching the
    // call-result-region model (`produces_call_result_region`) the lowerer now uses
    // for %bytes-push.
    let (bits, result) = run_alloc_intrinsic(vm, vm.heap_ptr, |ctx| {
        crate::primitives::intrinsics::prim_bytes_push(ctx, &[collection, value])
    });
    if bits.contains(crate::value::SIG_ERROR) {
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
        .array_mut_ref()
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
    let heap = unsafe { &mut *vm.heap_ptr };
    let result = if let Some(a) = val.as_array_mut() {
        crate::value::build::array(heap, a.borrow().clone(), region)
    } else if let Some(t) = val.as_struct_mut() {
        let entries: Vec<_> = t.borrow().iter().map(|(k, v)| (k.clone(), *v)).collect();
        crate::value::build::struct_from_sorted(heap, entries, region)
    } else if let Some(s) = val.as_set_mut() {
        crate::value::build::set(heap, s.borrow().clone(), region)
    } else if let Some(buf) = val.as_string_mut() {
        let b = buf.borrow();
        let s = std::str::from_utf8(&b).expect("%freeze: @string invalid UTF-8");
        crate::value::build::string(heap, s, region)
    } else if let Some(b) = val.as_bytes_mut() {
        crate::value::build::bytes(heap, b.borrow().clone(), region)
    } else {
        // Already immutable — pass through
        val
    };
    vm.fiber.stack.push(result);
}

pub(crate) fn handle_intr_thaw(vm: &mut VM, region: RuntimeRegion) {
    let val = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    let heap = unsafe { &mut *vm.heap_ptr };
    let result = if let Some(a) = val.as_array() {
        crate::value::build::array_mut(heap, a.to_vec(), region)
    } else if let Some(s) = val.as_struct() {
        let entries: std::collections::BTreeMap<_, _> =
            s.iter().map(|(k, v)| (k.clone(), *v)).collect();
        crate::value::build::struct_mut_from(heap, entries, region)
    } else if let Some(s) = val.as_set() {
        crate::value::build::set_mut(heap, s.iter().cloned().collect(), region)
    } else if let Some(r) =
        val.with_string(|s| crate::value::build::string_mut(heap, s.as_bytes().to_vec(), region))
    {
        r
    } else if let Some(b) = val.as_bytes() {
        crate::value::build::bytes_mut(heap, b.to_vec(), region)
    } else {
        // Already mutable — pass through
        val
    };
    vm.fiber.stack.push(result);
}

pub(crate) fn handle_identical(vm: &mut VM) {
    let b = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on Identical");
    let a = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on Identical");
    // Bitwise tag+payload equality (pointer identity for heap values)
    vm.fiber
        .stack
        .push(Value::bool(a.tag == b.tag && a.payload == b.payload));
}

pub(crate) fn handle_ne(vm: &mut VM) {
    let b = vm.fiber.stack.pop().expect("VM bug: Stack underflow on Ne");
    let a = vm.fiber.stack.pop().expect("VM bug: Stack underflow on Ne");
    // Fast path: bitwise identical → not equal is false
    if a == b {
        vm.fiber.stack.push(Value::FALSE);
        return;
    }
    // Numeric coercion: int-int stays exact, mixed promotes to f64
    if a.is_number() && b.is_number() {
        if let (Some(x), Some(y)) = (a.as_int(), b.as_int()) {
            vm.fiber
                .stack
                .push(if x != y { Value::TRUE } else { Value::FALSE });
            return;
        }
        if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
            vm.fiber
                .stack
                .push(if x != y { Value::TRUE } else { Value::FALSE });
            return;
        }
    }
    vm.fiber.stack.push(Value::TRUE);
}

pub(crate) fn handle_bit_not_intr(vm: &mut VM) {
    let val = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    let n = val.as_int().expect("%bit-not: expected integer");
    vm.fiber.stack.push(Value::int(!n));
}

#[cfg(test)]
mod tests;
