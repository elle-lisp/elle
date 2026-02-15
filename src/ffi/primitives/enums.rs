//! Header parsing and enum definition primitives.

use crate::ffi::bindings::generate_elle_bindings;
use crate::ffi::header::HeaderParser;
use crate::ffi::types::{CType, EnumId, EnumLayout, EnumVariant};
use crate::value::Value;
use crate::vm::VM;

/// (load-header-with-lib header-path lib-path) -> library-handle
///
/// Loads a C header file, parses it, and generates Elle bindings.
///
/// # Arguments
/// - header-path: Path to C header file
/// - lib-path: Path to compiled library
pub fn prim_load_header_with_lib(_vm: &mut VM, args: &[Value]) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("load-header-with-lib requires exactly 2 arguments".into());
    }

    let header_path = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err("header-path must be a string".into()),
    };

    let lib_path = match &args[1] {
        Value::String(s) => s.as_ref(),
        _ => return Err("lib-path must be a string".into()),
    };

    // Parse header
    let mut parser = HeaderParser::new();
    let parsed = parser.parse(header_path)?;

    // Generate bindings
    let _lisp_code = generate_elle_bindings(&parsed, lib_path);

    // In a full implementation, we would evaluate the generated Lisp code here
    // For now, return the library handle
    Ok(Value::String(lib_path.into()))
}

/// (define-enum name ((variant-name value) ...)) -> enum-id
///
/// Defines a C enum type in Elle.
pub fn prim_define_enum(_vm: &mut VM, args: &[Value]) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("define-enum requires exactly 2 arguments".into());
    }

    let enum_name = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err("enum name must be a string".into()),
    };

    // Parse variants from list
    let variants_list = &args[1];
    let mut variants = Vec::new();

    match variants_list {
        Value::Cons(_) => {
            let variant_vec = variants_list.list_to_vec()?;
            for variant_val in variant_vec {
                match variant_val {
                    Value::Cons(cons) => {
                        let name = match &cons.first {
                            Value::String(n) => n.as_ref().to_string(),
                            _ => return Err("variant name must be a string".into()),
                        };

                        let value = match &cons.rest {
                            Value::Cons(rest_cons) => match &rest_cons.first {
                                Value::Int(n) => *n,
                                _ => return Err("variant value must be an integer".into()),
                            },
                            _ => return Err("variant must be (name value)".into()),
                        };

                        variants.push(EnumVariant { name, value });
                    }
                    _ => return Err("each variant must be a cons cell".into()),
                }
            }
        }
        Value::Nil => {}
        _ => return Err("variants must be a list".into()),
    }

    // Create enum layout
    static ENUM_ID_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let enum_id = EnumId::new(ENUM_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst));

    let _layout = EnumLayout::new(enum_id, enum_name.to_string(), variants, CType::Int);

    // Return enum ID as integer
    Ok(Value::Int(enum_id.0 as i64))
}

pub fn prim_load_header_with_lib_wrapper(args: &[Value]) -> crate::error::LResult<Value> {
    if args.len() != 2 {
        return Err("load-header-with-lib requires exactly 2 arguments".into());
    }

    let header_path = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err("header-path must be a string".into()),
    };

    let lib_path = match &args[1] {
        Value::String(s) => s.as_ref(),
        _ => return Err("lib-path must be a string".into()),
    };

    // Parse header
    let mut parser = HeaderParser::new();
    let parsed = parser.parse(header_path)?;

    // Generate bindings
    let _lisp_code = generate_elle_bindings(&parsed, lib_path);

    // Return library path (future: would evaluate generated code)
    Ok(Value::String(lib_path.into()))
}

pub fn prim_define_enum_wrapper(args: &[Value]) -> crate::error::LResult<Value> {
    if args.len() != 2 {
        return Err("define-enum requires exactly 2 arguments".into());
    }

    let enum_name = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err("enum name must be a string".into()),
    };

    // Parse variants from list
    let variants_list = &args[1];
    let mut variants = Vec::new();

    match variants_list {
        Value::Cons(_) => {
            let variant_vec = variants_list.list_to_vec()?;
            for variant_val in variant_vec {
                match variant_val {
                    Value::Cons(cons) => {
                        let name = match &cons.first {
                            Value::String(n) => n.as_ref().to_string(),
                            _ => return Err("variant name must be a string".into()),
                        };

                        let value = match &cons.rest {
                            Value::Cons(rest_cons) => match &rest_cons.first {
                                Value::Int(n) => *n,
                                _ => return Err("variant value must be an integer".into()),
                            },
                            _ => return Err("variant must be (name value)".into()),
                        };

                        variants.push(EnumVariant { name, value });
                    }
                    _ => return Err("each variant must be a cons cell".into()),
                }
            }
        }
        Value::Nil => {}
        _ => return Err("variants must be a list".into()),
    }

    // Create enum layout
    static ENUM_ID_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let enum_id = EnumId::new(ENUM_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst));

    let _layout = EnumLayout::new(enum_id, enum_name.to_string(), variants, CType::Int);

    // Return enum ID as integer
    Ok(Value::Int(enum_id.0 as i64))
}
