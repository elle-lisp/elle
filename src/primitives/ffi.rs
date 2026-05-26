//! FFI type resolution helpers and tests

use crate::ffi::types::TypeDesc;
use crate::value::fiber::{SignalBits, SIG_ERROR};
use crate::value::{error_val, Value};

// ── Type descriptor resolution ──────────────────────────────────────

/// Resolve a type descriptor from a keyword or FFIType value.
///
/// Used by ffi/read, ffi/write, ffi/size, ffi/align, ffi/signature.
/// Returns the TypeDesc or an error array.
pub(crate) fn resolve_type_desc(
    value: &Value,
    context: &str,
) -> Result<TypeDesc, (SignalBits, Value)> {
    // First try keyword
    if let Some(name) = value.as_keyword_name() {
        return TypeDesc::from_keyword(&name).ok_or_else(|| {
            (
                SIG_ERROR,
                error_val("ffi-error", format!("{}: unknown type :{}", context, name)),
            )
        });
    }
    // Then try FFIType value
    if let Some(desc) = value.as_ffi_type() {
        return Ok(desc.clone());
    }
    Err((
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "{}: expected keyword or ffi-type, got {}",
                context,
                value.type_name()
            ),
        ),
    ))
}

// ── Pointer extraction helper ───────────────────────────────────────

/// Extract a raw pointer address from a Value that is either a raw CPointer
/// or a managed pointer. Returns an error for nil, freed, or wrong-type values.
pub(crate) fn extract_pointer_addr(
    value: &Value,
    context: &str,
) -> Result<usize, (SignalBits, Value)> {
    if value.is_nil() {
        return Err((
            SIG_ERROR,
            error_val(
                "argument-error",
                format!("{}: cannot use null pointer", context),
            ),
        ));
    }
    // Raw CPointer (unmanaged — from ffi/lookup, ffi/call returns, etc.)
    if let Some(addr) = value.as_pointer() {
        return Ok(addr);
    }
    // Managed pointer (from ffi/malloc)
    if let Some(cell) = value.as_managed_pointer() {
        return match cell.get() {
            Some(addr) => Ok(addr),
            None => Err((
                SIG_ERROR,
                error_val(
                    "use-after-free",
                    format!("{}: pointer has been freed", context),
                ),
            )),
        };
    }
    Err((
        SIG_ERROR,
        error_val(
            "type-error",
            format!("{}: expected pointer, got {}", context, value.type_name()),
        ),
    ))
}

// Tests migrated to tests/elle/prim-ffi.lisp
