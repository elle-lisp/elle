//! Header parsing and enum definition primitives.

use crate::ffi::bindings::generate_elle_bindings;
use crate::ffi::header::HeaderParser;
use crate::ffi::types::{CType, EnumId, EnumLayout};
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::{error_val, Value};
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

    let header_path = if let Some(s) = args[0].as_string() {
        s
    } else {
        return Err("header-path must be a string".into());
    };

    let lib_path = if let Some(s) = args[1].as_string() {
        s
    } else {
        return Err("lib-path must be a string".into());
    };

    // Parse header
    let mut parser = HeaderParser::new();
    let parsed = parser.parse(header_path)?;

    // Generate bindings
    let _lisp_code = generate_elle_bindings(&parsed, lib_path);

    // In a full implementation, we would evaluate the generated Lisp code here
    // For now, return the library handle
    Ok(Value::string(lib_path))
}

/// (define-enum name ((variant-name value) ...)) -> enum-id
///
/// Defines a C enum type in Elle.
pub fn prim_define_enum(_vm: &mut VM, args: &[Value]) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("define-enum requires exactly 2 arguments".into());
    }

    let enum_name = if let Some(s) = args[0].as_string() {
        s
    } else {
        return Err("enum name must be a string".into());
    };

    // Parse variants from list
    let variants_list = &args[1];
    let variants = Vec::new();

    if !variants_list.is_nil() && !variants_list.is_empty_list() {
        let variant_vec = variants_list.list_to_vec()?;
        #[allow(clippy::never_loop)]
        for variant_val in variant_vec {
            if let Some(_cons) = variant_val.as_heap_ptr() {
                // This is a cons cell - need to extract it properly
                // For now, we'll need to handle this differently
                return Err("variant parsing not yet implemented for new Value API".into());
            } else {
                return Err("each variant must be a cons cell".into());
            }
        }
    }

    // Create enum layout
    static ENUM_ID_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let enum_id = EnumId::new(ENUM_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst));

    let _layout = EnumLayout::new(enum_id, enum_name.to_string(), variants, CType::Int);

    // Return enum ID as integer
    Ok(Value::int(enum_id.0 as i64))
}

pub fn prim_load_header_with_lib_wrapper(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val("arity-error", "load-header-with-lib: expected 2 arguments"),
        );
    }

    let header_path = if let Some(s) = args[0].as_string() {
        s
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                "load-header-with-lib: header-path must be a string",
            ),
        );
    };

    let lib_path = if let Some(s) = args[1].as_string() {
        s
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                "load-header-with-lib: lib-path must be a string",
            ),
        );
    };

    // Parse header
    let mut parser = HeaderParser::new();
    let parsed = match parser.parse(header_path) {
        Ok(p) => p,
        Err(e) => {
            return (SIG_ERROR, error_val("error", e.to_string()));
        }
    };

    // Generate bindings
    let _lisp_code = generate_elle_bindings(&parsed, lib_path);

    // Return library path (future: would evaluate generated code)
    (SIG_OK, Value::string(lib_path))
}

pub fn prim_define_enum_wrapper(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val("arity-error", "define-enum: expected 2 arguments"),
        );
    }

    let enum_name = if let Some(s) = args[0].as_string() {
        s
    } else {
        return (
            SIG_ERROR,
            error_val("type-error", "define-enum: enum name must be a string"),
        );
    };

    // Parse variants from list
    let variants_list = &args[1];
    let variants = Vec::new();

    if !variants_list.is_nil() && !variants_list.is_empty_list() {
        let variant_vec = match variants_list.list_to_vec() {
            Ok(v) => v,
            Err(e) => {
                return (
                    SIG_ERROR,
                    error_val("type-error", format!("define-enum: {}", e)),
                );
            }
        };
        #[allow(clippy::never_loop)]
        for variant_val in variant_vec {
            if let Some(_cons) = variant_val.as_heap_ptr() {
                // This is a cons cell - need to extract it properly
                // For now, we'll need to handle this differently
                return (
                    SIG_ERROR,
                    error_val(
                        "error",
                        "define-enum: variant parsing not yet implemented for new Value API",
                    ),
                );
            } else {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        "define-enum: each variant must be a cons cell",
                    ),
                );
            }
        }
    }

    // Create enum layout
    static ENUM_ID_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let enum_id = EnumId::new(ENUM_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst));

    let _layout = EnumLayout::new(enum_id, enum_name.to_string(), variants, CType::Int);

    // Return enum ID as integer
    (SIG_OK, Value::int(enum_id.0 as i64))
}
