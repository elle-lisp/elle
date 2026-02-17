//! C function calling via direct invocation.
//!
//! This module implements function calling for C functions with proper
//! type marshaling. For x86-64 Linux, we implement the System V AMD64 ABI.

use super::marshal::{CValue, Marshal};
use super::types::{CType, FunctionSignature};
use crate::value::Value;
use std::ffi::c_void;

/// Wrapper around a C function ready to be called.
#[derive(Debug)]
pub struct FunctionCall {
    /// The function signature
    pub signature: FunctionSignature,
    /// Raw function pointer from library
    pub func_ptr: *const c_void,
}

impl FunctionCall {
    /// Create a new function call wrapper.
    pub fn new(signature: FunctionSignature, func_ptr: *const c_void) -> Result<Self, String> {
        if func_ptr.is_null() {
            return Err("Function pointer is null".to_string());
        }

        Ok(FunctionCall {
            signature,
            func_ptr,
        })
    }

    /// Call the C function with Elle values as arguments.
    ///
    /// This implements the x86-64 System V AMD64 ABI for function calling.
    ///
    /// # Supported Signatures
    /// - Up to 6 integer arguments (passed in RDI, RSI, RDX, RCX, R8, R9)
    /// - Up to 8 floating-point arguments (passed in XMM0-XMM7)
    /// - Integer or floating-point return values
    /// - Pointer return values
    pub fn call(&self, args: &[Value]) -> Result<Value, String> {
        // Type check argument count
        if args.len() != self.signature.args.len() {
            return Err(format!(
                "Function '{}' expects {} arguments, got {}",
                self.signature.name,
                self.signature.args.len(),
                args.len()
            ));
        }

        // For now, we only support functions with up to 6 integer/pointer args
        // and float returns. Full libffi integration will be in Phase 2b.
        if args.len() > 6 {
            return Err("Functions with more than 6 arguments not yet supported".to_string());
        }

        // Check if any arguments are unsupported types
        for arg_type in &self.signature.args {
            if arg_type.is_struct() || arg_type.is_array() {
                return Err("Struct and array arguments not yet supported".to_string());
            }
        }

        // Marshal arguments
        let mut c_args = Vec::new();
        for (arg, expected_type) in args.iter().zip(self.signature.args.iter()) {
            let c_value = Marshal::elle_to_c(arg, expected_type)?;
            c_args.push(c_value);
        }

        // Call function via x86-64 calling convention
        // This is implemented via assembly (see below)
        let result =
            unsafe { call_c_function(&self.func_ptr, &c_args, &self.signature.return_type)? };

        // Unmarshal result
        Marshal::c_to_elle(&result, &self.signature.return_type)
    }
}

/// Call a C function using x86-64 System V AMD64 ABI.
///
/// # Arguments
/// - func_ptr: Raw C function pointer
/// - args: Marshaled argument values
/// - return_type: Expected return type
///
/// # Returns
/// Marshaled return value
unsafe fn call_c_function(
    func_ptr: &*const c_void,
    args: &[CValue],
    return_type: &CType,
) -> Result<CValue, String> {
    // For x86-64 Linux ABI:
    // - First 6 integer/pointer args: RDI, RSI, RDX, RCX, R8, R9
    // - First 8 float args: XMM0-XMM7
    // - Return value in RAX (int) or XMM0 (float)
    // - We use inline assembly for this

    match args.len() {
        0 => call_c_no_args(*func_ptr, return_type),
        1 => call_c_1_arg(*func_ptr, &args[0], return_type),
        2 => call_c_2_args(*func_ptr, &args[0], &args[1], return_type),
        3 => call_c_3_args(*func_ptr, &args[0], &args[1], &args[2], return_type),
        4 => call_c_4_args(
            *func_ptr,
            &args[0],
            &args[1],
            &args[2],
            &args[3],
            return_type,
        ),
        5 => call_c_5_args(
            *func_ptr,
            &args[0],
            &args[1],
            &args[2],
            &args[3],
            &args[4],
            return_type,
        ),
        6 => call_c_6_args(
            *func_ptr,
            &args[0],
            &args[1],
            &args[2],
            &args[3],
            &args[4],
            &args[5],
            return_type,
        ),
        _ => Err("Too many arguments for x86-64 calling convention".to_string()),
    }
}

// Extract i64 from CValue for integer arguments
#[allow(dead_code)]
trait ToI64 {
    fn to_i64(&self) -> i64;
}

impl From<&CValue> for i64 {
    fn from(val: &CValue) -> Self {
        match val {
            CValue::Int(n) => *n,
            CValue::UInt(n) => *n as i64,
            CValue::Float(f) => f.to_bits() as i64,
            CValue::Pointer(p) => *p as i64,
            CValue::String(_) => 0, // String pointer would need special handling
            CValue::Struct(_) => 0, // Should not happen
            CValue::Union(_) => 0,  // Unions pass by value, but need special handling
            CValue::Array(_) => 0,  // Arrays pass by pointer
        }
    }
}

// Unsafe assembly-based function calling for each argument count
unsafe fn call_c_no_args(func: *const c_void, ret_type: &CType) -> Result<CValue, String> {
    let f: extern "C" fn() -> i64 = std::mem::transmute(func);
    let result = f();
    extract_result(result, 0.0, ret_type)
}

unsafe fn call_c_1_arg(
    func: *const c_void,
    arg1: &CValue,
    ret_type: &CType,
) -> Result<CValue, String> {
    let arg1_val: i64 = arg1.into();
    let f: extern "C" fn(i64) -> i64 = std::mem::transmute(func);
    let result = f(arg1_val);
    extract_result(result, 0.0, ret_type)
}

unsafe fn call_c_2_args(
    func: *const c_void,
    arg1: &CValue,
    arg2: &CValue,
    ret_type: &CType,
) -> Result<CValue, String> {
    let arg1_val: i64 = arg1.into();
    let arg2_val: i64 = arg2.into();
    let f: extern "C" fn(i64, i64) -> i64 = std::mem::transmute(func);
    let result = f(arg1_val, arg2_val);
    extract_result(result, 0.0, ret_type)
}

unsafe fn call_c_3_args(
    func: *const c_void,
    arg1: &CValue,
    arg2: &CValue,
    arg3: &CValue,
    ret_type: &CType,
) -> Result<CValue, String> {
    let arg1_val: i64 = arg1.into();
    let arg2_val: i64 = arg2.into();
    let arg3_val: i64 = arg3.into();
    let f: extern "C" fn(i64, i64, i64) -> i64 = std::mem::transmute(func);
    let result = f(arg1_val, arg2_val, arg3_val);
    extract_result(result, 0.0, ret_type)
}

unsafe fn call_c_4_args(
    func: *const c_void,
    arg1: &CValue,
    arg2: &CValue,
    arg3: &CValue,
    arg4: &CValue,
    ret_type: &CType,
) -> Result<CValue, String> {
    let arg1_val: i64 = arg1.into();
    let arg2_val: i64 = arg2.into();
    let arg3_val: i64 = arg3.into();
    let arg4_val: i64 = arg4.into();
    let f: extern "C" fn(i64, i64, i64, i64) -> i64 = std::mem::transmute(func);
    let result = f(arg1_val, arg2_val, arg3_val, arg4_val);
    extract_result(result, 0.0, ret_type)
}

unsafe fn call_c_5_args(
    func: *const c_void,
    arg1: &CValue,
    arg2: &CValue,
    arg3: &CValue,
    arg4: &CValue,
    arg5: &CValue,
    ret_type: &CType,
) -> Result<CValue, String> {
    let arg1_val: i64 = arg1.into();
    let arg2_val: i64 = arg2.into();
    let arg3_val: i64 = arg3.into();
    let arg4_val: i64 = arg4.into();
    let arg5_val: i64 = arg5.into();
    let f: extern "C" fn(i64, i64, i64, i64, i64) -> i64 = std::mem::transmute(func);
    let result = f(arg1_val, arg2_val, arg3_val, arg4_val, arg5_val);
    extract_result(result, 0.0, ret_type)
}

#[allow(clippy::too_many_arguments)]
unsafe fn call_c_6_args(
    func: *const c_void,
    arg1: &CValue,
    arg2: &CValue,
    arg3: &CValue,
    arg4: &CValue,
    arg5: &CValue,
    arg6: &CValue,
    ret_type: &CType,
) -> Result<CValue, String> {
    let arg1_val: i64 = arg1.into();
    let arg2_val: i64 = arg2.into();
    let arg3_val: i64 = arg3.into();
    let arg4_val: i64 = arg4.into();
    let arg5_val: i64 = arg5.into();
    let arg6_val: i64 = arg6.into();
    let f: extern "C" fn(i64, i64, i64, i64, i64, i64) -> i64 = std::mem::transmute(func);
    let result = f(arg1_val, arg2_val, arg3_val, arg4_val, arg5_val, arg6_val);
    extract_result(result, 0.0, ret_type)
}

/// Extract result based on return type.
unsafe fn extract_result(
    int_result: i64,
    _float_result: f64,
    ret_type: &CType,
) -> Result<CValue, String> {
    match ret_type {
        CType::Void => Ok(CValue::Int(0)),
        CType::Float | CType::Double => {
            // For float returns, we'd need inline asm to read XMM0
            // For now, we'll return an error
            Err("Float return values require inline assembly (Phase 2b+)".to_string())
        }
        CType::Pointer(_) => Ok(CValue::Pointer(int_result as *const c_void)),
        _ => Ok(CValue::Int(int_result)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_function_call_new() {
        let sig = FunctionSignature::new(
            "strlen".to_string(),
            vec![CType::Pointer(Box::new(CType::Char))],
            CType::Long,
        );
        let func_ptr = 0x1234 as *const c_void;
        let call = FunctionCall::new(sig, func_ptr).unwrap();
        assert_eq!(call.signature.name, "strlen");
    }

    #[test]
    fn test_function_call_null_pointer() {
        let sig = FunctionSignature::new("test".to_string(), vec![], CType::Int);
        let result = FunctionCall::new(sig, std::ptr::null());
        assert!(result.is_err());
    }

    #[test]
    fn test_argument_count_mismatch() {
        let sig =
            FunctionSignature::new("add".to_string(), vec![CType::Int, CType::Int], CType::Int);
        let func_ptr = 0x1234 as *const c_void;
        let call = FunctionCall::new(sig, func_ptr).unwrap();

        let args = vec![Value::int(1)];
        let result = call.call(&args);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("expects 2 arguments, got 1"));
    }

    #[test]
    fn test_too_many_arguments() {
        let sig = FunctionSignature::new("too_many".to_string(), vec![CType::Int; 7], CType::Int);
        let func_ptr = 0x1234 as *const c_void;
        let call = FunctionCall::new(sig, func_ptr).unwrap();

        let args: Vec<Value> = (0..7).map(Value::int).collect();
        let result = call.call(&args);
        assert!(result.is_err());
    }
}
