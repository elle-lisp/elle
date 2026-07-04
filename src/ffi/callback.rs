//! FFI callback trampolines.
//!
//! Allows passing Elle closures as C function pointers to C APIs
//! (e.g., qsort comparators, signal handlers, iteration callbacks).
//!
//! # Architecture
//!
//! `create_callback` wraps an Elle closure in a libffi closure. When C
//! code calls the resulting function pointer, the trampoline:
//! 1. Reads C arguments using the signature's type descriptors
//! 2. Calls the Elle closure on the VM captured at creation, via
//!    `execute_bytecode_saving_stack`
//! 3. Writes the return value back to the result buffer
//!
//! # Limitations
//!
//! - Callbacks can only be invoked under the VM that created them.
//!   Single-threaded use only.
//! - If the Elle closure signals an error, the callback writes a
//!   zero return value and stashes the error on that VM's FFI subsystem;
//!   `ffi/call` drains it (`take_callback_error`) after the C function
//!   returns.

use crate::ffi::call::prepare_cif;
use crate::ffi::marshal::{read_value_from_buffer, write_value_to_buffer};
use crate::ffi::types::{Signature, TypeDesc};
use crate::value::{Closure, Value};
use std::collections::HashMap;
use std::ffi::c_void;
use std::rc::Rc;

// ── Callback data ───────────────────────────────────────────────────

/// Data captured by an FFI callback trampoline.
///
/// Leaked onto the heap (via `Box::leak`) so the libffi closure can
/// reference it with `'static` lifetime. Recovered and dropped by
/// `free_callback`.
struct CallbackData {
    /// The Elle closure to invoke.
    closure: Rc<Closure>,
    /// The closure VALUE `closure` was cloned out of, installed as the body's
    /// executing-closure register on each invocation (a self-recursive callback
    /// resolves its self-reference to it). Live for the callback's lifetime:
    /// `ffi/callback` declares a store of its closure argument, so the value's
    /// region outlives the callback registration.
    closure_value: Value,
    /// The signature describing C argument and return types.
    signature: Signature,
    /// The VM that created this callback and will invoke it — captured at
    /// `create_callback` so the C-invoked trampoline (which has no other handle)
    /// reaches it. Sound by the single-VM callback limitation: the callback is
    /// owned by this VM's FFI subsystem, so it never outlives the VM, and the VM
    /// drives the C call that re-enters it here.
    vm: *mut crate::vm::VM,
}

/// An active callback that keeps the libffi closure alive.
///
/// Stored in `FFISubsystem::callbacks` keyed by code pointer address.
pub(crate) struct ActiveCallback {
    /// The libffi closure (owns the trampoline code page).
    _closure: libffi::middle::Closure<'static>,
    /// The leaked userdata box (recovered on free).
    userdata_ptr: *mut CallbackData,
    /// The callable C function pointer address.
    pub code_ptr: usize,
}

// ── Trampoline ──────────────────────────────────────────────────────

/// The generic callback function invoked by libffi.
///
/// # Safety
///
/// Called by libffi when C code invokes the closure's code pointer.
/// `args` points to an array of pointers to argument values.
/// `result` points to a buffer where the return value must be written.
///
/// # Coupling: the driving VM
///
/// The VM is the one captured in `userdata.vm` at `create_callback`. It is only
/// safe to invoke the callback while that VM is the active driver (the C call that
/// re-enters here runs under it), which the single-VM limitation guarantees.
unsafe extern "C" fn trampoline_callback(
    _cif: &libffi::low::ffi_cif,
    result: &mut c_void,
    args: *const *const c_void,
    userdata: &CallbackData,
) {
    let sig = &userdata.signature;
    let closure = &userdata.closure;

    // 1. The VM captured at creation; its heap mints each argument's own
    //    per-execution region (step 2).
    let vm = &mut *userdata.vm;

    // 2. Read C arguments into Elle Values. Each heap-typed arg (:struct/array/
    //    bytes) is born in its OWN per-execution region (docs/impl/region-rules.md Rule 6,
    //    no commingling) — see `convert_callback_arg`. The callee owns each arg
    //    (own_params=false move; see `VM::build_callback_env`) and releases it
    //    value-based at the param's last use, freeing that region.
    let mut elle_args = Vec::with_capacity(sig.args.len());
    for (i, arg_desc) in sig.args.iter().enumerate() {
        // libffi passes a pointer to each argument value.
        let arg_ptr = *args.add(i);
        match convert_callback_arg(vm.heap(), arg_ptr as *const u8, arg_desc) {
            Ok(v) => elle_args.push(v),
            Err(e) => {
                // The callee never runs, so it never releases the args converted
                // so far — each owns its own region; release them here.
                release_callback_arg_regions(vm.heap(), &elle_args);
                let err_region = vm.heap().new_runtime_region();
                let err = crate::value::error_val_in(
                    vm.heap(),
                    "ffi-error",
                    format!("callback: failed to read arg {}: {}", i, e),
                    err_region,
                );
                vm.ffi_mut().set_callback_error(err);
                zero_result(result, &sig.ret);
                return;
            }
        }
    }

    // 3. Build closure environment and execute
    let new_env_rc = match vm.build_callback_env(closure, &elle_args) {
        Some(env) => env,
        None => {
            // populate_env rejected the args (bad &keys/&named keywords); the
            // error is already set on the fiber. The callee never runs, so
            // release the args it would have owned. Surface it as a closure error.
            release_callback_arg_regions(vm.heap(), &elle_args);
            let err = vm.fiber.signal.take().map(|(_, v)| v).unwrap_or(Value::NIL);
            vm.ffi_mut().set_callback_error(err);
            zero_result(result, &sig.ret);
            return;
        }
    };

    vm.fiber.call_depth += 1;
    // Hand the callee its executing-closure register via the one-shot — the
    // C-invoked trampoline is an entry into a closure body like any other.
    vm.pending_entry_closure = userdata.closure_value;
    let exec = vm.execute_bytecode_saving_stack(&closure.template.code(), &new_env_rc);
    vm.fiber.call_depth -= 1;

    // 4. Handle result
    use crate::value::fiber::{SIG_ERROR, SIG_OK};
    match exec.bits {
        SIG_OK => {
            let (_, value) = vm.fiber.signal.take().unwrap_or((SIG_OK, Value::NIL));
            if let Err(e) = write_return_value(result, &value, &sig.ret, vm.heap()) {
                vm.ffi_mut().set_callback_error(e);
            }
        }
        SIG_ERROR => {
            let (_, err_value) = vm.fiber.signal.take().unwrap_or((SIG_ERROR, Value::NIL));
            vm.ffi_mut().set_callback_error(err_value);
            zero_result(result, &sig.ret);
        }
        _ => {
            // Yield or other signal inside a callback is not supported.
            let err_region = vm.heap().new_runtime_region();
            let err = crate::value::error_val_in(
                vm.heap(),
                "ffi-error",
                format!("callback: unexpected signal {} from closure", exec.bits),
                err_region,
            );
            vm.ffi_mut().set_callback_error(err);
            zero_result(result, &sig.ret);
        }
    }
}

/// Convert one C callback argument into an Elle Value, minting a fresh
/// per-execution region so a heap-typed arg (`:struct`/array/bytes) is born in
/// its OWN region — never commingled with sibling args (Rule 6,
/// docs/impl/region-rules.md). The callee owns the arg (own_params=false move; see
/// `VM::build_callback_env`) and releases it value-based at the param's last use,
/// freeing this region. A scalar/pointer arg is an immediate (no region) — it
/// allocates nothing, so the minted region is unused and recycled here; likewise
/// on a read error.
unsafe fn convert_callback_arg(
    heap: &mut crate::value::fiberheap::FiberHeap,
    ptr: *const u8,
    desc: &TypeDesc,
) -> crate::error::LResult<Value> {
    let region = heap.new_runtime_region();
    // Build the call's allocation capability over the freshly minted region so the
    // arg value is born there (docs/impl/region-ctx.md) — same region the unused-id
    // recycle below keys off.
    let value = {
        let mut ctx = crate::primitives::ctx::Alloc::with_region(region, heap);
        read_value_from_buffer(ptr, desc, &mut ctx)
    };
    // Keep the region only if a heap value was actually born in it (region_of is
    // Some); otherwise recycle the unused id (a tolerant no-op + recycle).
    let born_heap = matches!(&value, Ok(v) if crate::value::arena::region_of(heap, *v).is_some());
    if !born_heap {
        heap.decref_region_if_present(region);
    }
    value
}

/// Release the per-execution regions of converted callback args on a path where
/// the callee never runs (a read error, or `build_callback_env` rejecting the
/// args) — each heap arg owns its region (rc=1) and would otherwise leak.
/// Immediate args have no region (a no-op).
unsafe fn release_callback_arg_regions(
    heap: &mut crate::value::fiberheap::FiberHeap,
    args: &[Value],
) {
    for v in args {
        // Release each heap arg's per-execution region; immediates have none.
        if let Some(region) = crate::value::arena::region_of(heap, *v) {
            heap.decref_region_if_present(region);
        }
    }
}

/// Write an Elle return value into the libffi result buffer.
///
/// For primitive types, writes directly to avoid going through
/// `write_value_to_buffer` which may have alignment concerns. Returns `Err(error
/// value)` if a struct/array return fails to serialize (the trampoline stashes it
/// on the VM); the buffer is zeroed in that case.
unsafe fn write_return_value(
    result: &mut c_void,
    value: &Value,
    ret: &TypeDesc,
    heap: &mut crate::value::fiberheap::FiberHeap,
) -> Result<(), Value> {
    let ptr = result as *mut c_void as *mut u8;
    match ret {
        TypeDesc::Void => {}
        TypeDesc::I32 | TypeDesc::Int => {
            let n = value.as_int().unwrap_or(0) as i32;
            *(ptr as *mut i32) = n;
        }
        TypeDesc::U32 | TypeDesc::UInt => {
            let n = value.as_int().unwrap_or(0) as u32;
            *(ptr as *mut u32) = n;
        }
        TypeDesc::I64 | TypeDesc::Long | TypeDesc::SSize => {
            let n = value.as_int().unwrap_or(0);
            *(ptr as *mut i64) = n;
        }
        TypeDesc::U64 | TypeDesc::ULong | TypeDesc::Size => {
            // Bit-reinterpret back to u64: completes the lossless round-trip
            // with from_c.rs. See from_c.rs module-level doc for convention.
            let n = value.as_int().unwrap_or(0) as u64;
            *(ptr as *mut u64) = n;
        }
        TypeDesc::I8 | TypeDesc::Char => {
            let n = value.as_int().unwrap_or(0) as i8;
            *(ptr as *mut i8) = n;
        }
        TypeDesc::U8 | TypeDesc::UChar => {
            let n = value.as_int().unwrap_or(0) as u8;
            *ptr = n;
        }
        TypeDesc::I16 | TypeDesc::Short => {
            let n = value.as_int().unwrap_or(0) as i16;
            *(ptr as *mut i16) = n;
        }
        TypeDesc::U16 | TypeDesc::UShort => {
            let n = value.as_int().unwrap_or(0) as u16;
            *(ptr as *mut u16) = n;
        }
        TypeDesc::Float => {
            let f = value
                .as_float()
                .or_else(|| value.as_int().map(|i| i as f64))
                .unwrap_or(0.0);
            *(ptr as *mut f32) = f as f32;
        }
        TypeDesc::Double => {
            let f = value
                .as_float()
                .or_else(|| value.as_int().map(|i| i as f64))
                .unwrap_or(0.0);
            *(ptr as *mut f64) = f;
        }
        TypeDesc::Bool => {
            let v: std::ffi::c_int = if value.is_truthy() { 1 } else { 0 };
            *(ptr as *mut std::ffi::c_int) = v;
        }
        TypeDesc::Ptr | TypeDesc::Str => {
            let p = if value.is_nil() {
                0usize
            } else if let Some(addr) = value.as_pointer() {
                addr
            } else if let Some(cell) = value.as_managed_pointer() {
                cell.get().unwrap_or(0)
            } else {
                0usize
            };
            *(ptr as *mut usize) = p;
        }
        TypeDesc::Struct(_) | TypeDesc::Array(_, _) => {
            if let Err(e) = write_value_to_buffer(ptr, value, ret) {
                zero_result(result, ret);
                let err_region = heap.new_runtime_region();
                return Err(crate::value::error_val_in(
                    heap,
                    "ffi-error",
                    format!("callback: failed to write return value: {}", e),
                    err_region,
                ));
            }
        }
    }
    Ok(())
}

/// Write zeros into the result buffer for the given return type.
///
/// Used when the callback encounters an error and must still provide
/// a valid return value to C.
unsafe fn zero_result(result: &mut c_void, ret: &TypeDesc) {
    if let Some(size) = ret.size() {
        let ptr = result as *mut c_void as *mut u8;
        std::ptr::write_bytes(ptr, 0, size);
    }
}

// ── Public API ──────────────────────────────────────────────────────

/// Create an FFI callback from an Elle closure and a C signature.
///
/// Returns an `ActiveCallback` whose `code_ptr` can be passed to C
/// functions expecting a function pointer.
pub(crate) fn create_callback(
    closure: Rc<Closure>,
    closure_value: Value,
    signature: Signature,
    vm: *mut crate::vm::VM,
) -> Result<ActiveCallback, String> {
    // Validate: signature must not be variadic (callbacks can't be variadic)
    if signature.fixed_args.is_some() {
        return Err("ffi/callback: variadic signatures are not supported for callbacks".into());
    }

    // Build the libffi CIF
    let cif = prepare_cif(&signature);

    // Leak the userdata so the closure has 'static lifetime
    let userdata = Box::new(CallbackData {
        closure,
        closure_value,
        signature,
        vm,
    });
    let userdata_ptr = Box::into_raw(userdata);
    let userdata_ref: &'static CallbackData = unsafe { &*userdata_ptr };

    // Create the libffi closure.
    // We use c_void as the return type R because we write the actual
    // result manually in the trampoline via write_value_to_buffer.
    let ffi_closure = libffi::middle::Closure::new(cif, trampoline_callback, userdata_ref);

    // code_ptr() returns &unsafe extern "C" fn() — dereference to get
    // the actual function pointer, then cast to usize.
    let code_ptr = *ffi_closure.code_ptr() as usize;

    Ok(ActiveCallback {
        _closure: ffi_closure,
        userdata_ptr,
        code_ptr,
    })
}

/// Free an active callback, recovering the leaked userdata.
///
/// # Safety
///
/// The caller must ensure that no C code still holds or will call
/// the function pointer after this returns.
pub(crate) fn free_callback(callback: ActiveCallback) {
    // Recover the leaked Box and drop it
    unsafe {
        drop(Box::from_raw(callback.userdata_ptr));
    }
    // The libffi closure (_closure) is dropped automatically
}

// ── Callback storage ────────────────────────────────────────────────

/// Storage for active callbacks, keyed by code pointer address.
#[derive(Default)]
pub(crate) struct CallbackStore {
    callbacks: HashMap<usize, ActiveCallback>,
}

impl CallbackStore {
    pub fn new() -> Self {
        CallbackStore {
            callbacks: HashMap::new(),
        }
    }

    /// Insert a callback and return its code pointer address.
    pub fn insert(&mut self, callback: ActiveCallback) -> usize {
        let ptr = callback.code_ptr;
        self.callbacks.insert(ptr, callback);
        ptr
    }

    /// Remove and free a callback by its code pointer address.
    /// Returns true if the callback was found and freed.
    pub fn remove(&mut self, code_ptr: usize) -> bool {
        if let Some(cb) = self.callbacks.remove(&code_ptr) {
            free_callback(cb);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests;
