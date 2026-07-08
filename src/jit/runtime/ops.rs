use super::*;

/// Check if value is nil
#[no_mangle]
pub extern "C" fn elle_jit_is_nil(tag: u64, _payload: u64) -> JitValue {
    JitValue::bool_val(tag == TAG_NIL)
}

/// Check if value is truthy (not nil and not false)
#[no_mangle]
pub extern "C" fn elle_jit_is_truthy(tag: u64, payload: u64) -> JitValue {
    let v = Value { tag, payload };
    JitValue::bool_val(v.is_truthy())
}

/// Check if value is an integer
#[no_mangle]
pub extern "C" fn elle_jit_is_int(tag: u64, _payload: u64) -> JitValue {
    JitValue::bool_val(tag == TAG_INT)
}

/// Check if value is the empty list
#[no_mangle]
pub extern "C" fn elle_jit_is_empty(tag: u64, _payload: u64) -> JitValue {
    use crate::value::repr::TAG_EMPTY_LIST;
    JitValue::bool_val(tag == TAG_EMPTY_LIST)
}

/// Check if value is a boolean
#[no_mangle]
pub extern "C" fn elle_jit_is_bool(tag: u64, _payload: u64) -> JitValue {
    use crate::value::repr::{TAG_FALSE, TAG_TRUE};
    JitValue::bool_val(tag == TAG_TRUE || tag == TAG_FALSE)
}

/// Check if value is a float
#[no_mangle]
pub extern "C" fn elle_jit_is_float(tag: u64, _payload: u64) -> JitValue {
    use crate::value::repr::TAG_FLOAT;
    JitValue::bool_val(tag == TAG_FLOAT)
}

/// Check if value is a string (immutable or mutable)
#[no_mangle]
pub extern "C" fn elle_jit_is_string(tag: u64, _payload: u64) -> JitValue {
    use crate::value::repr::{TAG_STRING, TAG_STRING_MUT};
    JitValue::bool_val(tag == TAG_STRING || tag == TAG_STRING_MUT)
}

/// Check if value is a keyword
#[no_mangle]
pub extern "C" fn elle_jit_is_keyword(tag: u64, _payload: u64) -> JitValue {
    use crate::value::repr::TAG_KEYWORD;
    JitValue::bool_val(tag == TAG_KEYWORD)
}

/// Check if value is a symbol
#[no_mangle]
pub extern "C" fn elle_jit_is_symbol_check(tag: u64, _payload: u64) -> JitValue {
    use crate::value::repr::TAG_SYMBOL;
    JitValue::bool_val(tag == TAG_SYMBOL)
}

/// Check if value is bytes (immutable or mutable)
#[no_mangle]
pub extern "C" fn elle_jit_is_bytes(tag: u64, _payload: u64) -> JitValue {
    use crate::value::repr::{TAG_BYTES, TAG_BYTES_MUT};
    JitValue::bool_val(tag == TAG_BYTES || tag == TAG_BYTES_MUT)
}

/// Check if value is a box (lbox)
#[no_mangle]
pub extern "C" fn elle_jit_is_box(tag: u64, _payload: u64) -> JitValue {
    use crate::value::repr::TAG_LBOX;
    JitValue::bool_val(tag == TAG_LBOX)
}

/// Check if value is a closure
#[no_mangle]
pub extern "C" fn elle_jit_is_closure(tag: u64, _payload: u64) -> JitValue {
    use crate::value::repr::TAG_CLOSURE;
    JitValue::bool_val(tag == TAG_CLOSURE)
}

/// Check if value is a fiber
#[no_mangle]
pub extern "C" fn elle_jit_is_fiber(tag: u64, _payload: u64) -> JitValue {
    use crate::value::repr::TAG_FIBER;
    JitValue::bool_val(tag == TAG_FIBER)
}

/// Get type keyword for a value
#[no_mangle]
pub extern "C" fn elle_jit_type_of(tag: u64, payload: u64) -> JitValue {
    let v = Value { tag, payload };
    JitValue::from_value(Value::keyword(v.type_name()))
}

/// Polymorphic length — panics on unsupported types (intrinsic contract).
#[no_mangle]
pub extern "C" fn elle_jit_length(tag: u64, payload: u64) -> JitValue {
    let v = Value { tag, payload };
    use unicode_segmentation::UnicodeSegmentation;
    let len = if v.is_empty_list() || v.is_nil() {
        0
    } else if v.is_pair() {
        v.list_to_vec().expect("%length: improper list").len()
    } else if let Some(a) = v.as_array() {
        a.len()
    } else if let Some(a) = v.as_array_mut() {
        a.borrow().len()
    } else if let Some(s) = v.as_struct() {
        s.len()
    } else if let Some(s) = v.as_struct_mut() {
        s.borrow().len()
    } else if let Some(s) = v.as_set() {
        s.len()
    } else if let Some(s) = v.as_set_mut() {
        s.borrow().len()
    } else if let Some(b) = v.as_bytes() {
        b.len()
    } else if let Some(b) = v.as_bytes_mut() {
        b.borrow().len()
    } else if let Some(r) = v.with_string(|s| s.graphemes(true).count()) {
        r
    } else if let Some(buf) = v.as_string_mut() {
        let b = buf.borrow();
        std::str::from_utf8(&b)
            .expect("%length: @string invalid UTF-8")
            .graphemes(true)
            .count()
    } else {
        panic!("%length: unsupported type {}", v.type_name())
    };
    JitValue::from_value(Value::int(len as i64))
}

/// Polymorphic get — panics on unsupported types (intrinsic contract).
#[no_mangle]
pub extern "C" fn elle_jit_get(obj_tag: u64, obj_pay: u64, key_tag: u64, key_pay: u64) -> JitValue {
    let obj = Value {
        tag: obj_tag,
        payload: obj_pay,
    };
    let key = Value {
        tag: key_tag,
        payload: key_pay,
    };
    use crate::value::TableKey;
    let result = if let Some(elems) = obj.as_array() {
        elems[key.as_int().expect("%get: index must be int") as usize]
    } else if let Some(a) = obj.as_array_mut() {
        a.borrow()[key.as_int().expect("%get: index must be int") as usize]
    } else if let Some(pairs) = obj.as_struct() {
        let tk = TableKey::from_value(&key).expect("%get: unhashable key");
        crate::value::sorted_struct_get(pairs, &tk)
            .copied()
            .unwrap_or(Value::NIL)
    } else if let Some(t) = obj.as_struct_mut() {
        let tk = TableKey::from_value(&key).expect("%get: unhashable key");
        t.borrow().get(&tk).copied().unwrap_or(Value::NIL)
    } else {
        panic!("%get: unsupported type {}", obj.type_name())
    };
    JitValue::from_value(result)
}

/// Polymorphic put — panics on type error (intrinsic contract).
#[no_mangle]
pub extern "C" fn elle_jit_put(
    obj_tag: u64,
    obj_pay: u64,
    key_tag: u64,
    key_pay: u64,
    val_tag: u64,
    val_pay: u64,
    jit_ctx: *mut crate::jit::JitCtx,
) -> JitValue {
    let obj = Value {
        tag: obj_tag,
        payload: obj_pay,
    };
    let key = Value {
        tag: key_tag,
        payload: key_pay,
    };
    let val = Value {
        tag: val_tag,
        payload: val_pay,
    };
    // Same per-call result-region discipline as the interp handler
    // (`handle_intr_put`) and `dispatch_native_call`: mint a fresh region, run
    // the prim into it, pass-through-retain. The compiler emits the matching
    // value-based `DecrefValueRegion` (the op is a `call_result_region`). The VM
    // comes from the threaded `JitCtx`.
    let vm = unsafe { (*jit_ctx).vm() };
    let (bits, result) =
        crate::vm::types::run_alloc_intrinsic(vm, unsafe { (*vm).heap_ptr }, |ctx| {
            crate::primitives::access::prim_put(ctx, &[obj, key, val])
        });
    assert!(
        !bits.contains(crate::value::SIG_ERROR),
        "%put: intrinsic contract violated"
    );
    JitValue::from_value(result)
}

/// Polymorphic del — panics on type error (intrinsic contract).
#[no_mangle]
pub extern "C" fn elle_jit_del(
    obj_tag: u64,
    obj_pay: u64,
    key_tag: u64,
    key_pay: u64,
    jit_ctx: *mut crate::jit::JitCtx,
) -> JitValue {
    let obj = Value {
        tag: obj_tag,
        payload: obj_pay,
    };
    let key = Value {
        tag: key_tag,
        payload: key_pay,
    };
    let vm = unsafe { (*jit_ctx).vm() };
    let (bits, result) =
        crate::vm::types::run_alloc_intrinsic(vm, unsafe { (*vm).heap_ptr }, |ctx| {
            crate::primitives::lstruct::prim_del(ctx, &[obj, key])
        });
    assert!(
        !bits.contains(crate::value::SIG_ERROR),
        "%del: intrinsic contract violated"
    );
    JitValue::from_value(result)
}

/// Polymorphic has? — panics on type error (intrinsic contract).
#[no_mangle]
pub extern "C" fn elle_jit_has(
    obj_tag: u64,
    obj_pay: u64,
    key_tag: u64,
    key_pay: u64,
    jit_ctx: *mut crate::jit::JitCtx,
) -> JitValue {
    let obj = Value {
        tag: obj_tag,
        payload: obj_pay,
    };
    let key = Value {
        tag: key_tag,
        payload: key_pay,
    };
    // `%has?` is Immediate (no allocation), but `prim_has_key` is a PrimFn, so it
    // needs a `NativeCtx`; a `boundary` ctx over the threaded VM mints its own
    // (unused) region.
    let vm = unsafe { &mut *(*jit_ctx).vm() };
    let (bits, result) = crate::primitives::lstruct::prim_has_key(
        &mut crate::primitives::ctx::NativeCtx::boundary_vm(vm),
        &[obj, key],
    );
    assert!(
        !bits.contains(crate::value::SIG_ERROR),
        "%has?: intrinsic contract violated"
    );
    JitValue::from_value(result)
}

/// Push — panics on type error (intrinsic contract).
///
/// Mirrors `handle_intr_push` in `src/vm/types.rs`. Routes through the shared
/// `prim_push` body via `run_alloc_intrinsic`: an @array mutates in place
/// (through `arena::push_with_incref`, so cross-region values inserted into the
/// @array keep their source region alive) and is returned as a pass-through; an
/// immutable array yields a fresh copy born in this call's own minted region.
/// Both cases pass-through-retain, and the compiler emits the matching value-based
/// `DecrefValueRegion` (%array-push is now a `produces_call_result_region` op),
/// exactly like `elle_jit_put`/`del`/`string_push`.
#[no_mangle]
pub extern "C" fn elle_jit_push(
    arr_tag: u64,
    arr_pay: u64,
    val_tag: u64,
    val_pay: u64,
    jit_ctx: *mut crate::jit::JitCtx,
) -> JitValue {
    let arr = Value {
        tag: arr_tag,
        payload: arr_pay,
    };
    let val = Value {
        tag: val_tag,
        payload: val_pay,
    };
    let vm = unsafe { (*jit_ctx).vm() };
    let (bits, result) =
        crate::vm::types::run_alloc_intrinsic(vm, unsafe { (*vm).heap_ptr }, |ctx| {
            crate::primitives::intrinsics::prim_push(ctx, &[arr, val])
        });
    assert!(
        !bits.contains(crate::value::SIG_ERROR),
        "%array-push: intrinsic contract violated"
    );
    JitValue::from_value(result)
}

/// String push — panics on type error (intrinsic contract).
///
/// If `coll` is `@string` (mutable), appends `val`'s UTF-8 bytes in place and
/// returns the same collection (pass-through). If `coll` is an immutable
/// string, a new concatenated string is born in this call's own minted region
/// (run_alloc_intrinsic). Routes through the shared `prim_string_push` exactly
/// like the interp handler (`handle_intr_string_push`) — one body, no drift.
#[no_mangle]
pub extern "C" fn elle_jit_string_push(
    coll_tag: u64,
    coll_pay: u64,
    val_tag: u64,
    val_pay: u64,
    jit_ctx: *mut crate::jit::JitCtx,
) -> JitValue {
    let coll = Value {
        tag: coll_tag,
        payload: coll_pay,
    };
    let val = Value {
        tag: val_tag,
        payload: val_pay,
    };
    let vm = unsafe { (*jit_ctx).vm() };
    let (bits, result) =
        crate::vm::types::run_alloc_intrinsic(vm, unsafe { (*vm).heap_ptr }, |ctx| {
            crate::primitives::intrinsics::prim_string_push(ctx, &[coll, val])
        });
    assert!(
        !bits.contains(crate::value::SIG_ERROR),
        "%string-push: intrinsic contract violated"
    );
    JitValue::from_value(result)
}

/// Bytes push — panics on type error (intrinsic contract).
///
/// Mirrors `handle_intr_bytes_push` in `src/vm/types.rs`. Routes through the
/// shared `prim_bytes_push` body via `run_alloc_intrinsic`: a `@bytes` appends
/// the truncated `u8` in place and is returned as a pass-through; an immutable
/// `bytes` yields a fresh copy born in this call's own minted region and
/// pass-through-retained, with the compiler's matching value-based
/// `DecrefValueRegion` (%bytes-push is now a `produces_call_result_region` op). No
/// cross-region RC tracking of the pushed value is needed because `@bytes` stores
/// raw bytes, not Value references (per `docs/regions.md`).
#[no_mangle]
pub extern "C" fn elle_jit_bytes_push(
    coll_tag: u64,
    coll_pay: u64,
    val_tag: u64,
    val_pay: u64,
    jit_ctx: *mut crate::jit::JitCtx,
) -> JitValue {
    let coll = Value {
        tag: coll_tag,
        payload: coll_pay,
    };
    let val = Value {
        tag: val_tag,
        payload: val_pay,
    };
    let vm = unsafe { (*jit_ctx).vm() };
    let (bits, result) =
        crate::vm::types::run_alloc_intrinsic(vm, unsafe { (*vm).heap_ptr }, |ctx| {
            crate::primitives::intrinsics::prim_bytes_push(ctx, &[coll, val])
        });
    assert!(
        !bits.contains(crate::value::SIG_ERROR),
        "%bytes-push: intrinsic contract violated"
    );
    JitValue::from_value(result)
}

/// Pop — panics on type error or empty (intrinsic contract).
///
/// Routes through `arena::pop_with_decref`, the single move-out funnel every tier
/// shares: it hands the popped element back to the caller, holding the caller's
/// owning reference before releasing the container's so a sole-owned element's
/// region is never freed under the returned Value. Like the interpreter opcode
/// (`handle_intr_pop`), this unchecked JIT path takes no pass-through retain of its
/// own — the body's retain IS the caller's reference. (The checked native-call
/// path routes through `dispatch_native_call`, which skips its own
/// `pass_through_retain` for the `moves_out` `%pop`/`pop`.)
#[no_mangle]
pub extern "C" fn elle_jit_pop(
    tag: u64,
    payload: u64,
    jit_ctx: *mut crate::jit::JitCtx,
) -> JitValue {
    let v = Value { tag, payload };
    assert!(v.is_array_mut(), "%pop: expected @array");
    // The heap comes from the threaded `JitCtx`'s VM — this instance's own heap,
    // not a per-thread slot (docs/impl/region/ctx.md "JIT intrinsic helpers reach
    // the VM through a JitCtx").
    let heap = unsafe { &mut *(*(*jit_ctx).vm()).heap_ptr };
    JitValue::from_value(crate::value::arena::pop_with_decref(heap, v))
}

/// Freeze — mutable → immutable copy (pass-through for already-immutable types).
///
/// `%freeze` is an `IntrinsicOp::allocates` op: the lowerer's `emit_alloc`
/// assigns it a static region SLOT with a matching `DecrefRegion(slot)`, so the
/// fresh immutable copy must be born in THAT slot's resolved physical region —
/// not a fresh mint. The emitter resolves the slot to
/// its physical region id with `elle_jit_resolve_alloc_region` (the same
/// region-id ABI `List`/`MakeArrayMut`/`MaterializeConst` use) and threads it
/// here; we rebuild the region and run the shared `prim_freeze` body through a
/// `with_region` ctx — one body shared with the interpreter
/// (`handle_intr_freeze`), no drift, and the `DecrefRegion(slot)` frees exactly
/// this region.
#[no_mangle]
pub extern "C" fn elle_jit_freeze(
    tag: u64,
    payload: u64,
    region: u32,
    jit_ctx: *mut crate::jit::JitCtx,
) -> JitValue {
    let v = Value { tag, payload };
    let region = crate::hir::region::RuntimeRegion::new(region)
        .expect("%freeze: JIT alloc region slot resolves nonzero — emitter invariant");
    let vm_ptr = unsafe { (*jit_ctx).vm() };
    let heap = unsafe { &mut *(*vm_ptr).heap_ptr };
    let (bits, result) = crate::primitives::intrinsics::prim_freeze(
        &mut crate::primitives::ctx::NativeCtx::with_region_vm(region, heap, vm_ptr),
        &[v],
    );
    assert!(
        !bits.contains(crate::value::SIG_ERROR),
        "%freeze: intrinsic contract violated"
    );
    JitValue::from_value(result)
}

/// Thaw — immutable → mutable copy (pass-through for already-mutable types).
///
/// Same region accounting as [`elle_jit_freeze`]: `%thaw` is an
/// `IntrinsicOp::allocates` op, so the fresh mutable copy is born in the
/// emitter-resolved SLOT region (threaded as `region`), freed by the matching
/// `DecrefRegion(slot)`. Runs the shared `prim_thaw` body through a `with_region`
/// ctx — one body shared with the interpreter (`handle_intr_thaw`).
#[no_mangle]
pub extern "C" fn elle_jit_thaw(
    tag: u64,
    payload: u64,
    region: u32,
    jit_ctx: *mut crate::jit::JitCtx,
) -> JitValue {
    let v = Value { tag, payload };
    let region = crate::hir::region::RuntimeRegion::new(region)
        .expect("%thaw: JIT alloc region slot resolves nonzero — emitter invariant");
    let vm_ptr = unsafe { (*jit_ctx).vm() };
    let heap = unsafe { &mut *(*vm_ptr).heap_ptr };
    let (bits, result) = crate::primitives::intrinsics::prim_thaw(
        &mut crate::primitives::ctx::NativeCtx::with_region_vm(region, heap, vm_ptr),
        &[v],
    );
    assert!(
        !bits.contains(crate::value::SIG_ERROR),
        "%thaw: intrinsic contract violated"
    );
    JitValue::from_value(result)
}

/// Bitwise identity comparison (pointer identity for heap values)
#[no_mangle]
pub extern "C" fn elle_jit_identical(a_tag: u64, a_pay: u64, b_tag: u64, b_pay: u64) -> JitValue {
    JitValue::bool_val(a_tag == b_tag && a_pay == b_pay)
}

// =============================================================================
// Error Handling
// =============================================================================

/// Type error (called from JIT code when type check fails)
#[no_mangle]
pub extern "C" fn elle_jit_type_error(expected: *const u8, expected_len: usize) -> JitValue {
    let msg = unsafe {
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(expected, expected_len))
    };
    eprintln!("JIT type error: expected {}", msg);
    JitValue::nil()
}
