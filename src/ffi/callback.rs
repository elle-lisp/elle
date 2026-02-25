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
//! 2. Gets the VM from thread-local storage
//! 3. Calls the Elle closure via `execute_bytecode_saving_stack`
//! 4. Writes the return value back to the result buffer
//!
//! # Limitations
//!
//! - Callbacks can only be invoked on the thread that created them
//!   (same VM context). Single-threaded use only.
//! - If the Elle closure signals an error, the callback writes a
//!   zero return value and sets a thread-local error flag. The
//!   caller should check `take_callback_error` after the C function
//!   returns.

use crate::ffi::call::prepare_cif;
use crate::ffi::marshal::{read_value_from_buffer, write_value_to_buffer};
use crate::ffi::types::{Signature, TypeDesc};
use crate::value::{Closure, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::rc::Rc;

// ── Thread-local error flag ─────────────────────────────────────────

thread_local! {
    /// Error from the most recent callback invocation, if any.
    /// Set by the trampoline when the Elle closure signals an error.
    /// Checked by `ffi/call` after the C function returns.
    static CALLBACK_ERROR: RefCell<Option<Value>> = const { RefCell::new(None) };
}

/// Take the pending callback error, if any.
pub fn take_callback_error() -> Option<Value> {
    CALLBACK_ERROR.with(|e| e.borrow_mut().take())
}

fn set_callback_error(err: Value) {
    CALLBACK_ERROR.with(|e| *e.borrow_mut() = Some(err));
}

// ── Callback data ───────────────────────────────────────────────────

/// Data captured by an FFI callback trampoline.
///
/// Leaked onto the heap (via `Box::leak`) so the libffi closure can
/// reference it with `'static` lifetime. Recovered and dropped by
/// `free_callback`.
struct CallbackData {
    /// The Elle closure to invoke.
    closure: Rc<Closure>,
    /// The signature describing C argument and return types.
    signature: Signature,
}

/// An active callback that keeps the libffi closure alive.
///
/// Stored in `FFISubsystem::callbacks` keyed by code pointer address.
pub struct ActiveCallback {
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
/// # Coupling: VM context
///
/// This function depends on `crate::context::get_vm_context()` returning
/// a valid VM pointer. It is only safe to invoke callbacks on the thread
/// where the VM is running and the context is set.
unsafe extern "C" fn trampoline_callback(
    _cif: &libffi::low::ffi_cif,
    result: &mut c_void,
    args: *const *const c_void,
    userdata: &CallbackData,
) {
    let sig = &userdata.signature;
    let closure = &userdata.closure;

    // 1. Read C arguments into Elle Values
    let mut elle_args = Vec::with_capacity(sig.args.len());
    for (i, arg_desc) in sig.args.iter().enumerate() {
        let arg_ptr = *args.add(i);
        // libffi passes a pointer to each argument value.
        let value = match read_value_from_buffer(arg_ptr as *const u8, arg_desc) {
            Ok(v) => v,
            Err(e) => {
                set_callback_error(crate::value::error_val(
                    "ffi-error",
                    format!("callback: failed to read arg {}: {}", i, e),
                ));
                zero_result(result, &sig.ret);
                return;
            }
        };
        elle_args.push(value);
    }

    // 2. Get VM context
    let vm_ptr = match crate::context::get_vm_context() {
        Some(ptr) => ptr,
        None => {
            set_callback_error(crate::value::error_val(
                "ffi-error",
                "callback: no VM context (wrong thread?)",
            ));
            zero_result(result, &sig.ret);
            return;
        }
    };
    let vm = &mut *vm_ptr;

    // 3. Build closure environment and execute
    let new_env = build_callback_env(closure, &elle_args);
    let new_env_rc = Rc::new(new_env);

    vm.fiber.call_depth += 1;
    let (bits, _ip) =
        vm.execute_bytecode_saving_stack(&closure.bytecode, &closure.constants, &new_env_rc);
    vm.fiber.call_depth -= 1;

    // 4. Handle result
    use crate::value::fiber::{SIG_ERROR, SIG_OK};
    match bits {
        SIG_OK => {
            let (_, value) = vm.fiber.signal.take().unwrap_or((SIG_OK, Value::NIL));
            write_return_value(result, &value, &sig.ret);
        }
        SIG_ERROR => {
            let (_, err_value) = vm.fiber.signal.take().unwrap_or((SIG_ERROR, Value::NIL));
            set_callback_error(err_value);
            zero_result(result, &sig.ret);
        }
        _ => {
            // Yield or other signal inside a callback is not supported.
            set_callback_error(crate::value::error_val(
                "ffi-error",
                format!("callback: unexpected signal {} from closure", bits),
            ));
            zero_result(result, &sig.ret);
        }
    }
}

/// Write an Elle return value into the libffi result buffer.
///
/// For primitive types, writes directly to avoid going through
/// `write_value_to_buffer` which may have alignment concerns.
unsafe fn write_return_value(result: &mut c_void, value: &Value, ret: &TypeDesc) {
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
            } else {
                value.as_pointer().unwrap_or(0)
            };
            *(ptr as *mut usize) = p;
        }
        TypeDesc::Struct(_) | TypeDesc::Array(_, _) => {
            if let Err(e) = write_value_to_buffer(ptr, value, ret) {
                set_callback_error(crate::value::error_val(
                    "ffi-error",
                    format!("callback: failed to write return value: {}", e),
                ));
                zero_result(result, ret);
            }
        }
    }
}

/// Build a closure environment for a callback invocation.
///
/// Mirrors `VM::build_closure_env` but without needing `&mut VM`.
/// The callback runs during a C call, so we build the env directly.
fn build_callback_env(closure: &Closure, args: &[Value]) -> Vec<Value> {
    let needed = closure.env_capacity();
    let mut env = Vec::with_capacity(needed);

    // Copy captured upvalues
    env.extend(closure.env.iter().copied());

    // Add parameters with cell wrapping as needed
    match closure.arity {
        crate::value::Arity::AtLeast(n) => {
            for (i, arg) in args[..n.min(args.len())].iter().enumerate() {
                if i < 64 && (closure.cell_params_mask & (1 << i)) != 0 {
                    env.push(Value::local_cell(*arg));
                } else {
                    env.push(*arg);
                }
            }
            // Collect remaining args into a list for the rest slot
            let rest_args = if args.len() > n { &args[n..] } else { &[] };
            let rest = args_to_list(rest_args);
            let rest_idx = n;
            if rest_idx < 64 && (closure.cell_params_mask & (1 << rest_idx)) != 0 {
                env.push(Value::local_cell(rest));
            } else {
                env.push(rest);
            }
        }
        _ => {
            for (i, arg) in args.iter().enumerate() {
                if i < 64 && (closure.cell_params_mask & (1 << i)) != 0 {
                    env.push(Value::local_cell(*arg));
                } else {
                    env.push(*arg);
                }
            }
        }
    }

    // Add empty LocalCells for locally-defined variables
    let num_param_slots = match closure.arity {
        crate::value::Arity::Exact(n) => n,
        crate::value::Arity::AtLeast(n) => n + 1,
        crate::value::Arity::Range(min, _) => min,
    };
    let num_locally_defined = closure.num_locals.saturating_sub(num_param_slots);
    for _ in 0..num_locally_defined {
        env.push(Value::local_cell(Value::NIL));
    }

    env
}

/// Build a cons-list from a slice of values.
fn args_to_list(args: &[Value]) -> Value {
    let mut list = Value::EMPTY_LIST;
    for arg in args.iter().rev() {
        list = Value::cons(*arg, list);
    }
    list
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
pub fn create_callback(
    closure: Rc<Closure>,
    signature: Signature,
) -> Result<ActiveCallback, String> {
    // Validate: signature must not be variadic (callbacks can't be variadic)
    if signature.fixed_args.is_some() {
        return Err("ffi/callback: variadic signatures are not supported for callbacks".into());
    }

    // Build the libffi CIF
    let cif = prepare_cif(&signature);

    // Leak the userdata so the closure has 'static lifetime
    let userdata = Box::new(CallbackData { closure, signature });
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
pub fn free_callback(callback: ActiveCallback) {
    // Recover the leaked Box and drop it
    unsafe {
        drop(Box::from_raw(callback.userdata_ptr));
    }
    // The libffi closure (_closure) is dropped automatically
}

// ── Callback storage ────────────────────────────────────────────────

/// Storage for active callbacks, keyed by code pointer address.
#[derive(Default)]
pub struct CallbackStore {
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
mod tests {
    use super::*;
    use crate::ffi::types::{CallingConvention, Signature, TypeDesc};
    use crate::value::Closure;
    use std::collections::HashMap;

    /// Create a minimal closure for testing.
    /// This closure has empty bytecode — it won't execute correctly,
    /// but it's enough to test callback creation/destruction.
    fn test_closure(arity: usize) -> Rc<Closure> {
        use crate::effects::Effect;
        use crate::error::LocationMap;
        use crate::value::types::Arity;
        Rc::new(Closure {
            bytecode: Rc::new(vec![]),
            arity: Arity::Exact(arity),
            env: Rc::new(vec![]),
            num_locals: arity,
            num_captures: 0,
            constants: Rc::new(vec![]),
            effect: Effect::none(),
            cell_params_mask: 0,
            symbol_names: Rc::new(HashMap::new()),
            location_map: Rc::new(LocationMap::new()),
            jit_code: None,
            lir_function: None,
        })
    }

    #[test]
    fn test_create_and_free_callback() {
        let closure = test_closure(2);
        let sig = Signature {
            convention: CallingConvention::Default,
            ret: TypeDesc::I32,
            args: vec![TypeDesc::Ptr, TypeDesc::Ptr],
            fixed_args: None,
        };
        let cb = create_callback(closure, sig).unwrap();
        assert_ne!(cb.code_ptr, 0);
        free_callback(cb);
    }

    #[test]
    fn test_variadic_callback_rejected() {
        let closure = test_closure(2);
        let sig = Signature {
            convention: CallingConvention::Default,
            ret: TypeDesc::I32,
            args: vec![TypeDesc::Ptr, TypeDesc::I32],
            fixed_args: Some(1),
        };
        let result = create_callback(closure, sig);
        assert!(result.is_err());
    }

    #[test]
    fn test_callback_store() {
        let closure = test_closure(1);
        let sig = Signature {
            convention: CallingConvention::Default,
            ret: TypeDesc::Void,
            args: vec![TypeDesc::I32],
            fixed_args: None,
        };
        let mut store = CallbackStore::new();
        let cb = create_callback(closure, sig).unwrap();
        let ptr = store.insert(cb);
        assert_ne!(ptr, 0);
        assert!(store.remove(ptr));
        assert!(!store.remove(ptr)); // Already removed
    }

    #[test]
    fn test_callback_error_flag() {
        // Ensure the error flag starts empty
        assert!(take_callback_error().is_none());

        // Set an error
        set_callback_error(crate::value::error_val("test", "test error"));
        let err = take_callback_error();
        assert!(err.is_some());

        // Flag should be cleared after take
        assert!(take_callback_error().is_none());
    }

    #[test]
    fn test_build_callback_env_exact_arity() {
        let closure = test_closure(2);
        let args = vec![Value::int(10), Value::int(20)];
        let env = build_callback_env(&closure, &args);
        // 0 captures + 2 params + 0 locals = 2
        assert_eq!(env.len(), 2);
        assert_eq!(env[0].as_int(), Some(10));
        assert_eq!(env[1].as_int(), Some(20));
    }

    #[test]
    fn test_build_callback_env_with_captures() {
        use crate::effects::Effect;
        use crate::error::LocationMap;
        use crate::value::types::Arity;

        let closure = Rc::new(Closure {
            bytecode: Rc::new(vec![]),
            arity: Arity::Exact(1),
            env: Rc::new(vec![Value::int(99)]), // 1 capture
            num_locals: 2,                      // 1 param + 1 local
            num_captures: 1,
            constants: Rc::new(vec![]),
            effect: Effect::none(),
            cell_params_mask: 0,
            symbol_names: Rc::new(HashMap::new()),
            location_map: Rc::new(LocationMap::new()),
            jit_code: None,
            lir_function: None,
        });
        let args = vec![Value::int(42)];
        let env = build_callback_env(&closure, &args);
        // 1 capture + 1 param + 1 local = 3
        assert_eq!(env.len(), 3);
        assert_eq!(env[0].as_int(), Some(99)); // capture
        assert_eq!(env[1].as_int(), Some(42)); // param
                                               // env[2] is a LocalCell(NIL) for the local variable
    }

    #[test]
    fn test_zero_result_does_not_crash() {
        // Allocate a buffer and verify zero_result writes zeros
        let mut buf = [0xFFu8; 16];
        unsafe {
            zero_result(&mut *buf.as_mut_ptr().cast::<c_void>(), &TypeDesc::I32);
        }
        // First 4 bytes should be zero (i32 size)
        assert_eq!(&buf[..4], &[0, 0, 0, 0]);
    }

    #[test]
    fn test_zero_result_void() {
        // Void has no size — zero_result should be a no-op
        let mut buf = [0xFFu8; 8];
        unsafe {
            zero_result(&mut *buf.as_mut_ptr().cast::<c_void>(), &TypeDesc::Void);
        }
        // Buffer should be unchanged
        assert_eq!(&buf, &[0xFF; 8]);
    }
}
