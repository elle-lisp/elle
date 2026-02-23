//! Generates Elle Lisp bindings from parsed C headers.
//!
//! This module takes parsed C header information and generates
//! corresponding Elle Lisp code for convenient C library access.

use super::header::ParsedHeader;
use super::types::FunctionSignature;

/// Generates Elle Lisp code from a parsed C header.
///
/// # Arguments
/// * `parsed` - Parsed header information
/// * `library_name` - Name of the C library (.so file)
///
/// # Returns
/// Elle Lisp code as a string
pub fn generate_elle_bindings(parsed: &ParsedHeader, library_name: &str) -> String {
    let mut code = String::new();

    // Header comment
    code.push_str(&format!(
        ";;; Auto-generated Elle bindings from C header\n;;;  Library: {}\n;;;\n",
        library_name
    ));

    // Generate enums
    if !parsed.enums.is_empty() {
        code.push_str("\n;;; Enum definitions\n");
        for (enum_name, variants) in &parsed.enums {
            code.push_str(&generate_enum_definition(enum_name, variants));
        }
    }

    // Generate constants
    if !parsed.constants.is_empty() {
        code.push_str("\n;;; Constants\n");
        for (const_name, const_value) in &parsed.constants {
            code.push_str(&generate_constant_definition(const_name, const_value));
        }
    }

    // Generate function wrappers
    if !parsed.functions.is_empty() {
        code.push_str("\n;;; Function bindings\n");
        for (func_name, sig) in &parsed.functions {
            code.push_str(&generate_function_wrapper(func_name, sig, library_name));
        }
    }

    code
}

/// Generate Elle code for an enum definition.
fn generate_enum_definition(name: &str, variants: &[(String, i64)]) -> String {
    let mut code = String::new();

    code.push_str(&format!(";;; Enum: {}\n", name));
    for (variant_name, value) in variants {
        code.push_str(&format!("(def {} {})\n", variant_name, value));
    }
    code.push('\n');

    code
}

/// Generate Elle code for a constant definition.
fn generate_constant_definition(name: &str, value: &super::header::ConstantValue) -> String {
    use super::header::ConstantValue;

    let value_str = match value {
        ConstantValue::Int(n) => n.to_string(),
        ConstantValue::UInt(n) => n.to_string(),
        ConstantValue::Float(f) => f.clone(),
        ConstantValue::String(s) => format!("\"{}\"", s),
    };

    format!("(def {} {})\n", name, value_str)
}

/// Generate Elle code for a function wrapper.
fn generate_function_wrapper(
    func_name: &str,
    sig: &FunctionSignature,
    library_name: &str,
) -> String {
    let lisp_name = c_to_lisp_name(func_name);

    let arg_names: Vec<String> = (0..sig.args.len()).map(|i| format!("arg{}", i)).collect();

    let arg_list = arg_names.join(" ");

    let arg_types_str = sig
        .args
        .iter()
        .map(ctype_to_lisp_string)
        .collect::<Vec<_>>()
        .join(" ");

    let return_type_str = ctype_to_lisp_string(&sig.return_type);

    if arg_names.is_empty() {
        format!(
            "(def ({} )\n  (call-c-function \"{}\" \"{}\" {} ({}) ))\n\n",
            lisp_name, library_name, func_name, return_type_str, arg_types_str
        )
    } else {
        format!(
            "(def ({} {})\n  (call-c-function \"{}\" \"{}\" ({}) ({}) ))\n\n",
            lisp_name, arg_list, library_name, func_name, arg_types_str, arg_list
        )
    }
}

/// Convert C function name to Lisp function name.
/// E.g., gtk_window_new -> gtk-window-new
fn c_to_lisp_name(c_name: &str) -> String {
    c_name.replace('_', "-")
}

/// Convert a C type to Lisp type specification string.
fn ctype_to_lisp_string(ctype: &super::types::CType) -> String {
    use super::types::CType;

    match ctype {
        CType::Void => ":void".to_string(),
        CType::Bool => ":bool".to_string(),
        CType::Char => ":char".to_string(),
        CType::SChar => ":schar".to_string(),
        CType::UChar => ":uchar".to_string(),
        CType::Short => ":short".to_string(),
        CType::UShort => ":ushort".to_string(),
        CType::Int => ":int".to_string(),
        CType::UInt => ":uint".to_string(),
        CType::Long => ":long".to_string(),
        CType::ULong => ":ulong".to_string(),
        CType::LongLong => ":longlong".to_string(),
        CType::ULongLong => ":ulonglong".to_string(),
        CType::Float => ":float".to_string(),
        CType::Double => ":double".to_string(),
        CType::Pointer(inner) => {
            format!("(:pointer {})", ctype_to_lisp_string(inner))
        }
        CType::Struct(id) => format!(":struct{:?}", id),
        CType::Enum(id) => format!(":enum{:?}", id),
        CType::Union(id) => format!(":union{:?}", id),
        CType::Array(elem, count) => {
            format!("(:array {} {})", ctype_to_lisp_string(elem), count)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::header::{ConstantValue, ParsedHeader};
    use super::*;

    #[test]
    fn test_generate_bindings_empty() {
        let parsed = ParsedHeader::new();
        let bindings = generate_elle_bindings(&parsed, "libtest.so");
        assert!(bindings.contains("libtest.so"));
        assert!(bindings.contains("Auto-generated"));
    }

    #[test]
    fn test_c_to_lisp_name() {
        assert_eq!(c_to_lisp_name("gtk_window_new"), "gtk-window-new");
        assert_eq!(c_to_lisp_name("SDL_CreateWindow"), "SDL-CreateWindow");
        assert_eq!(c_to_lisp_name("strlen"), "strlen");
    }

    #[test]
    fn test_ctype_to_lisp() {
        use super::super::types::CType;

        assert_eq!(ctype_to_lisp_string(&CType::Int), ":int");
        assert_eq!(ctype_to_lisp_string(&CType::Long), ":long");
        assert_eq!(ctype_to_lisp_string(&CType::Void), ":void");
        assert!(ctype_to_lisp_string(&CType::Pointer(Box::new(CType::Char))).contains("pointer"));
    }

    #[test]
    fn test_generate_constant() {
        let result = generate_constant_definition("TEST_CONST", &ConstantValue::Int(42));
        assert!(result.contains("TEST_CONST"));
        assert!(result.contains("42"));
    }

    #[test]
    fn test_generate_function_wrapper() {
        use super::super::types::CType;
        use super::super::types::FunctionSignature;

        let sig = FunctionSignature::new(
            "strlen".to_string(),
            vec![CType::Pointer(Box::new(CType::Char))],
            CType::Long,
        );

        let wrapper = generate_function_wrapper("strlen", &sig, "libc.so.6");
        assert!(wrapper.contains("strlen"));
        assert!(wrapper.contains("libc.so.6"));
        assert!(wrapper.contains("def"));
    }
}
