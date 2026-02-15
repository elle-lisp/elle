//! C type parsing utilities for FFI primitives.

use crate::ffi::types::CType;
use crate::value::Value;

/// Parse a C type from a keyword value.
pub fn parse_ctype(val: &Value) -> Result<CType, String> {
    match val {
        Value::Symbol(_) => {
            // We need to look up the symbol name, but we don't have access to SymbolTable
            // For now, we'll return an error indicating this needs symbol table integration
            Err("Symbol-based type specification not yet supported".into())
        }
        Value::String(s) => match s.as_ref() {
            "void" => Ok(CType::Void),
            "bool" => Ok(CType::Bool),
            "char" => Ok(CType::Char),
            "schar" => Ok(CType::SChar),
            "uchar" => Ok(CType::UChar),
            "short" => Ok(CType::Short),
            "ushort" => Ok(CType::UShort),
            "int" => Ok(CType::Int),
            "uint" => Ok(CType::UInt),
            "long" => Ok(CType::Long),
            "ulong" => Ok(CType::ULong),
            "longlong" => Ok(CType::LongLong),
            "ulonglong" => Ok(CType::ULongLong),
            "float" => Ok(CType::Float),
            "double" => Ok(CType::Double),
            "pointer" => Ok(CType::Pointer(Box::new(CType::Void))),
            _ => Err(format!("Unknown C type: {}", s)),
        },
        _ => Err("Type must be a string".into()),
    }
}
