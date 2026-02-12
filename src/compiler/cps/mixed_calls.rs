//! Mixed call support: native code calling CPS functions
//!
//! When pure (native) code calls a function that may yield,
//! we need a trampoline to handle the CPS execution.

use crate::compiler::cranelift::runtime_helpers::{decode_value_for_jit, encode_value_for_jit};
use crate::value::Value;

/// Call a CPS function from native code
///
/// This is the "implicit reset" - it runs the CPS function
/// in a trampoline until it completes or yields.
///
/// # Safety
/// - func_ptr must be a valid CPS function pointer
/// - args_ptr must point to args_len valid i64 values
/// - env_ptr must point to valid environment data
#[no_mangle]
pub unsafe extern "C" fn jit_call_cps_function(
    _func_ptr: *const u8,
    _args_ptr: *const i64,
    _args_len: i64,
    _env_ptr: *const i64,
) -> i64 {
    // For now, we don't have full CPS function compilation
    // This is a placeholder that will be filled in when E2 is complete

    // Return nil for now
    encode_value_for_jit(&Value::Nil)
}

/// Resume a suspended coroutine from native code
///
/// # Safety
/// - continuation must be a valid continuation pointer
/// - resume_value must be a valid encoded value
#[no_mangle]
pub unsafe extern "C" fn jit_resume_coroutine(_continuation: i64, _resume_value: i64) -> i64 {
    // Placeholder for coroutine resumption
    encode_value_for_jit(&Value::Nil)
}

/// Check if a value is a suspended coroutine
#[no_mangle]
pub extern "C" fn jit_is_suspended_coroutine(value: i64) -> i64 {
    let decoded = decode_value_for_jit(value);
    match decoded {
        Value::Coroutine(ref c) => {
            if matches!(c.state, crate::value::CoroutineState::Suspended) {
                1
            } else {
                0
            }
        }
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jit_is_suspended_coroutine_with_non_coroutine() {
        let val = encode_value_for_jit(&Value::Int(42));
        assert_eq!(jit_is_suspended_coroutine(val), 0);
    }

    #[test]
    fn test_jit_is_suspended_coroutine_with_nil() {
        let val = encode_value_for_jit(&Value::Nil);
        assert_eq!(jit_is_suspended_coroutine(val), 0);
    }
}
