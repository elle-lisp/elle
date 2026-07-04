//! Error value construction.
//!
//! Errors in Elle are structs: `{:error :keyword :message "message"}`. This module provides
//! a helper to construct them using interned keywords.

use super::heap::TableKey;
use super::repr::Value;
use super::types::sorted_struct_get;
use crate::hir::region::RuntimeRegion;
use crate::value::fiberheap::FiberHeap;

/// Build a rich error `(SIG_ERROR, {:error <kind> :message <msg> …fields})`
/// through `scope.error_extra` — the one region-coherent error routine
/// (docs/impl/region-errors.md). `scope` is `ctx` or the VM (`self`/`vm`): the
/// region source is explicit, so each error names the region it is born in.
///
/// Field values are written by the caller as `name = <expr>` — a string field
/// is `name = ctx.string(x)` so it is born in the error's region; keyword/int
/// fields are immediates; a pass-through `Value` is incref'd into the error's
/// region by `alloc`'s content scan. The macro only sugars the `(SIG_ERROR, …)`
/// tuple, the `:error <kind>` field, and the slice — it never builds a field
/// value for you, so it cannot misplace a region.
#[macro_export]
macro_rules! rich_error {
    ($scope:expr, $kind:expr, $msg:expr $(, $field:ident = $val:expr)* $(,)?) => {
        (
            $crate::value::SIG_ERROR,
            $scope.error_extra($kind, $msg, &[$((stringify!($field), $val)),*]),
        )
    };
}

/// Construct an error value: `{:error :keyword :message "message"}` in
/// `region` (Rule 3: born in the right region — the failing native's call
/// region). The kind string is interned as a keyword (immediate, no region).
pub fn error_val_in(
    heap: &mut FiberHeap,
    kind: &str,
    msg: impl Into<String>,
    region: RuntimeRegion,
) -> Value {
    crate::value::build::error(heap, kind, msg, region)
}

/// Region-explicit error value with extra context fields.
pub fn error_val_extra_in(
    heap: &mut FiberHeap,
    kind: &str,
    msg: impl Into<String>,
    extra: &[(&str, Value)],
    region: RuntimeRegion,
) -> Value {
    crate::value::build::error_extra(heap, kind, msg, extra, region)
}

/// Construct the runtime no-match error for `match` in `region`:
/// `{:error :match-error :message "..." :value <scrutinee>}`.
///
/// The single definition every backend (VM, JIT, WASM) raises — the
/// user-visible error contract for an unmatched scrutinee lives here
/// and nowhere else.
pub fn match_fail_error_in(heap: &mut FiberHeap, val: Value, region: RuntimeRegion) -> Value {
    crate::value::build::match_fail(heap, val, region)
}

/// Extract a human-readable error message from an error value.
///
/// Handles struct errors `{:error :keyword :message "string"}`, legacy array errors
/// `[:kind "msg"]` (for backward compatibility with user-constructed errors),
/// plain string errors, and arbitrary values.
/// Returns the formatted string representation.
pub fn format_error(value: Value) -> String {
    // Struct error: {:error :keyword :message "string"}
    if let Some(fields) = value.as_struct() {
        let error = sorted_struct_get(fields, &TableKey::Keyword("error".into()));
        let msg = sorted_struct_get(fields, &TableKey::Keyword("message".into()));
        if let (Some(error_val), Some(msg_val)) = (error, msg) {
            if let (Some(name), Some(text)) = (
                error_val.as_keyword_name(),
                msg_val.with_string(|s| s.to_string()),
            ) {
                return format!("{}: {}", name, text);
            }
        }
    }

    // Legacy array error: [:error "msg"] (backward compat for user-constructed errors)
    if let Some(elems) = value.as_array() {
        if elems.len() == 2 {
            if let Some(msg) = elems[1].with_string(|s| s.to_string()) {
                if let Some(name) = elems[0].as_keyword_name() {
                    return format!("{}: {}", name, msg);
                }
                return msg;
            }
        }
    }

    // Plain string error
    if let Some(s) = value.with_string(|s| s.to_string()) {
        return s;
    }

    // Fallback: display the value
    format!("{}", value)
}

#[cfg(test)]
mod tests;
