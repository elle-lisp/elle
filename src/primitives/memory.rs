//! FFI memory management, typed access, and type construction primitives

use crate::ffi::types::TypeDesc;
use crate::primitives::def::RegionEffect;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

use super::ffi::{extract_pointer_addr, resolve_type_desc};

mod ffiops;
pub use ffiops::*;

// ── Struct/array type creation ──────────────────────────────────────

pub(crate) fn prim_ffi_struct(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    // Accept array, @array, or list of type descriptors
    let field_vals = if let Some(arr) = args[0].as_array() {
        arr.to_vec()
    } else if let Some(arr) = args[0].as_array_mut() {
        arr.borrow().clone()
    } else {
        match args[0].list_to_vec_in(ctx.heap_mut()) {
            Ok(v) => v,
            Err(_) => return type_error!(ctx, args[0], "ffi/struct", "array or list of types"),
        }
    };

    if field_vals.is_empty() {
        return (
            SIG_ERROR,
            ctx.error(
                "argument-error",
                "ffi/struct: struct must have at least one field",
            ),
        );
    }

    let mut fields = Vec::with_capacity(field_vals.len());
    for val in &field_vals {
        match resolve_type_desc(val, "ffi/struct", ctx) {
            Ok(desc) => {
                if matches!(desc, TypeDesc::Void) {
                    return (
                        SIG_ERROR,
                        ctx.error(
                            "argument-error",
                            "ffi/struct: void is not valid as a field type",
                        ),
                    );
                }
                fields.push(desc);
            }
            Err(e) => return e,
        }
    }

    let desc = TypeDesc::Struct(crate::ffi::types::StructDesc { fields });
    (SIG_OK, ctx.ffi_type(desc))
}

pub(crate) fn prim_ffi_array(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let elem_desc = match resolve_type_desc(&args[0], "ffi/array", ctx) {
        Ok(desc) => {
            if matches!(desc, TypeDesc::Void) {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "argument-error",
                        "ffi/array: void is not valid as element type",
                    ),
                );
            }
            desc
        }
        Err(e) => return e,
    };
    let count = match args[1].as_int() {
        Some(n) if n > 0 => n as usize,
        Some(0) => {
            return (
                SIG_ERROR,
                ctx.error("argument-error", "ffi/array: count must be positive"),
            )
        }
        Some(n) => {
            return (
                SIG_ERROR,
                ctx.error(
                    "argument-error",
                    format!("ffi/array: count must be positive, got {}", n),
                ),
            )
        }
        None => return type_error!(ctx, args[1], "ffi/array", "integer for count"),
    };
    let desc = TypeDesc::Array(Box::new(elem_desc), count);
    (SIG_OK, ctx.ffi_type(desc))
}

// ── Type introspection ──────────────────────────────────────────────

pub fn prim_ffi_size(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let desc = match resolve_type_desc(&args[0], "ffi/size", ctx) {
        Ok(t) => t,
        Err(e) => return e,
    };
    match desc.size() {
        Some(s) => (SIG_OK, Value::int(s as i64)),
        None => (SIG_OK, Value::NIL),
    }
}

pub fn prim_ffi_align(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let desc = match resolve_type_desc(&args[0], "ffi/align", ctx) {
        Ok(t) => t,
        Err(e) => return e,
    };
    match desc.align() {
        Some(a) => (SIG_OK, Value::int(a as i64)),
        None => (SIG_OK, Value::NIL),
    }
}

// ── Memory management ───────────────────────────────────────────────

pub fn prim_ffi_malloc(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let size = match args[0].as_int() {
        Some(n) if n > 0 => n as usize,
        Some(_) => {
            return (
                SIG_ERROR,
                ctx.error("argument-error", "ffi/malloc: size must be positive"),
            )
        }
        None => return type_error!(ctx, args[0], "ffi/malloc", "integer"),
    };
    let ptr = unsafe { libc::malloc(size) };
    if ptr.is_null() {
        (
            SIG_ERROR,
            ctx.error("ffi-error", "ffi/malloc: allocation failed"),
        )
    } else {
        (SIG_OK, ctx.managed_pointer(ptr as usize))
    }
}

pub fn prim_ffi_free(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if args[0].is_nil() {
        return (SIG_OK, Value::NIL); // free(NULL) is a no-op
    }
    // Managed pointer: check not already freed, then invalidate
    if let Some(cell) = args[0].as_managed_pointer() {
        return match cell.get() {
            Some(addr) => {
                cell.set(None);
                unsafe { libc::free(addr as *mut libc::c_void) };
                (SIG_OK, Value::NIL)
            }
            None => (
                SIG_ERROR,
                ctx.error("double-free", "ffi/free: pointer has already been freed"),
            ),
        };
    }
    // Raw CPointer: free without lifecycle tracking (backwards compat)
    let addr = prim_arg!(ctx, args, 0, as_pointer, "ffi/free", "pointer");
    unsafe { libc::free(addr as *mut libc::c_void) };
    (SIG_OK, Value::NIL)
}

// ── Typed memory access ─────────────────────────────────────────────

// ── String from pointer ─────────────────────────────────────────────

// ── Pointer arithmetic ──────────────────────────────────────────────

/// `(ptr/add pointer offset)` — Offset a pointer by a byte count.
///
/// Returns a raw C pointer (not managed). The result is a view into an
/// existing allocation; ownership remains with the original managed pointer.
/// The offset may be negative to move backwards.
pub fn prim_ptr_add(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let addr = match extract_pointer_addr(&args[0], "ptr/add", ctx) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let offset = prim_arg!(ctx, args, 1, as_int, "ptr/add", "integer for offset");
    // Use checked_add on i64 to detect overflow.
    let result = match (addr as i64).checked_add(offset) {
        Some(n) => n,
        None => {
            return (
                SIG_ERROR,
                ctx.error("overflow-error", "ptr/add: address arithmetic overflow"),
            )
        }
    };
    if result < 0 {
        return (
            SIG_ERROR,
            ctx.error("argument-error", "ptr/add: result address is negative"),
        );
    }
    let result_u64 = result as u64;
    // Value::pointer(0) returns NIL — treat null result as an error.
    if result_u64 == 0 {
        return (
            SIG_ERROR,
            ctx.error("argument-error", "ptr/add: result is null pointer"),
        );
    }
    // Validate the result fits in a usize (platform pointer width).
    if result_u64 > usize::MAX as u64 {
        return (
            SIG_ERROR,
            ctx.error(
                "argument-error",
                "ptr/add: result address exceeds pointer range",
            ),
        );
    }
    (SIG_OK, Value::pointer(result_u64 as usize))
}

/// `(ptr/diff pointer-a pointer-b)` — Compute signed byte distance between two pointers.
///
/// Returns `addr_a - addr_b` as a signed integer. Negative if `a < b`.
/// Both inputs may be raw or managed pointers.
pub fn prim_ptr_diff(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let addr_a = match extract_pointer_addr(&args[0], "ptr/diff", ctx) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let addr_b = match extract_pointer_addr(&args[1], "ptr/diff", ctx) {
        Ok(a) => a,
        Err(e) => return e,
    };
    // wrapping_sub handles the full usize range; result fits in i64 for
    // any realistic user-space address pair.
    let diff = (addr_a as i64).wrapping_sub(addr_b as i64);
    (SIG_OK, Value::int(diff))
}

/// `(ptr/to-int pointer)` — Extract the raw address of a pointer as an integer.
///
/// The address is at most 48 bits on current hardware, so it always fits
/// in a signed i64 (2^63-1 >> 2^48-1). The cast is safe.
pub fn prim_ptr_to_int(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let addr = match extract_pointer_addr(&args[0], "ptr/to-int", ctx) {
        Ok(a) => a,
        Err(e) => return e,
    };
    // User-space addresses fit comfortably in i64 on 64-bit platforms.
    (SIG_OK, Value::int(addr as i64))
}

/// `(ptr/from-int integer)` — Construct a raw C pointer from an integer address.
///
/// Returns `nil` if the address is 0 (consistent with `Value::pointer(0) == NIL`).
/// Validates the address fits in a usize before calling `Value::pointer`.
pub fn prim_ptr_from_int(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let n = prim_arg!(ctx, args, 0, as_int, "ptr/from-int", "integer");
    // Reinterpret as unsigned — negative values are valid C pointers
    // (e.g. SQLITE_TRANSIENT = (void(*)(void*))-1 = 0xFFFFFFFFFFFFFFFF).
    let addr = n as u64;
    // Validate the address fits in a usize (platform pointer width).
    if addr > usize::MAX as u64 {
        return (
            SIG_ERROR,
            ctx.error(
                "argument-error",
                "ptr/from-int: address exceeds pointer range",
            ),
        );
    }
    // Value::pointer(0) returns Value::NIL — a legitimate result for addr 0.
    (SIG_OK, Value::pointer(addr as usize))
}

// Declarative primitive definitions for FFI memory operations.
primitive! {
    "ffi/size" => prim_ffi_size {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get the size of a C type in bytes.",
        params: &["type"],
        category: "ffi",
        example: "(ffi/size :i32) #=> 4",
        effect: RegionEffect::Immediate,
    }
    "ffi/align" => prim_ffi_align {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get the alignment of a C type in bytes.",
        params: &["type"],
        category: "ffi",
        example: "(ffi/align :double) #=> 8",
        effect: RegionEffect::Immediate,
    }
    "ffi/malloc" => prim_ffi_malloc {
        signal: Signal::ffi_errors(),
        arity: Arity::Exact(1),
        doc: "Allocate C memory.",
        params: &["size"],
        category: "ffi",
        example: "(ffi/malloc 100)",
        effect: RegionEffect::Fresh,
    }
    "ffi/free" => prim_ffi_free {
        signal: Signal::ffi_errors(),
        arity: Arity::Exact(1),
        doc: "Free C memory.",
        params: &["ptr"],
        category: "ffi",
        example: "(ffi/free ptr)",
        effect: RegionEffect::Immediate,
    }
    "ffi/read" => prim_ffi_read {
        signal: Signal::ffi_errors(),
        arity: Arity::Exact(2),
        doc: "Read a typed value from C memory.",
        params: &["ptr", "type"],
        category: "ffi",
        example: "(ffi/read ptr :i32)",
        effect: RegionEffect::Fresh,
    }
    "ffi/write" => prim_ffi_write {
        signal: Signal::ffi_errors(),
        arity: Arity::Exact(3),
        doc: "Write a typed value to C memory.",
        params: &["ptr", "type", "value"],
        category: "ffi",
        example: "(ffi/write ptr :i32 42)",
        effect: RegionEffect::Immediate,
    }
    "ffi/string" => prim_ffi_string {
        signal: Signal::ffi_errors(),
        arity: Arity::Range(1, 2),
        doc: "Read a null-terminated C string from a pointer.",
        params: &["ptr", "max-len"],
        category: "ffi",
        example: "(ffi/string ptr)",
        effect: RegionEffect::Fresh,
    }
    "ffi/struct" => prim_ffi_struct {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Create a struct type descriptor from field types.",
        params: &["fields"],
        category: "ffi",
        example: "(ffi/struct [:i32 :double :ptr])",
        effect: RegionEffect::Fresh,
    }
    "ffi/array" => prim_ffi_array {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Create an array type descriptor from element type and count.",
        params: &["elem-type", "count"],
        category: "ffi",
        example: "(ffi/array :i32 10)",
        effect: RegionEffect::Fresh,
    }
    "ptr/add" => prim_ptr_add {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Offset a pointer by a byte count. Returns a raw pointer. Offset may be negative.",
        params: &["pointer", "offset"],
        category: "ptr",
        example: "(ptr/add buf 16)",
        effect: RegionEffect::Immediate,
    }
    "ptr/diff" => prim_ptr_diff {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Compute the signed byte distance between two pointers (a - b).",
        params: &["pointer-a", "pointer-b"],
        category: "ptr",
        example: "(ptr/diff p2 p1)",
        effect: RegionEffect::Immediate,
    }
    "ptr/to-int" => prim_ptr_to_int {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Extract the raw address of a pointer as an integer.",
        params: &["pointer"],
        category: "ptr",
        example: "(ptr/to-int buf)",
        effect: RegionEffect::Immediate,
    }
    "ptr/from-int" => prim_ptr_from_int {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Construct a raw C pointer from an integer address. Returns nil if address is 0.",
        params: &["integer"],
        category: "ptr",
        example: "(ptr/from-int addr)",
        effect: RegionEffect::Immediate,
    }
}
