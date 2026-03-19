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

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::value::fiber::{SIG_ERROR, SIG_OK};
    use crate::value::Value;

    use super::super::calling::prim_ffi_call;
    use super::super::loading::{prim_ffi_native, prim_ffi_signature};
    use super::super::memory::{
        prim_ffi_align, prim_ffi_array, prim_ffi_free, prim_ffi_malloc, prim_ffi_read,
        prim_ffi_size, prim_ffi_string, prim_ffi_struct, prim_ffi_write, prim_ptr_add,
        prim_ptr_diff, prim_ptr_from_int, prim_ptr_to_int,
    };

    #[test]
    fn test_ffi_size() {
        let result = prim_ffi_size(&[Value::keyword("i32")]);
        assert_eq!(result.0, SIG_OK);
        assert_eq!(result.1.as_int(), Some(4));
    }

    #[test]
    fn test_ffi_size_void() {
        let result = prim_ffi_size(&[Value::keyword("void")]);
        assert_eq!(result.0, SIG_OK);
        assert!(result.1.is_nil());
    }

    #[test]
    fn test_ffi_size_unknown_type() {
        let result = prim_ffi_size(&[Value::keyword("nonsense")]);
        assert_eq!(result.0, SIG_ERROR);
    }

    #[test]
    fn test_ffi_align() {
        let result = prim_ffi_align(&[Value::keyword("double")]);
        assert_eq!(result.0, SIG_OK);
        assert_eq!(result.1.as_int(), Some(8));
    }

    #[test]
    fn test_ffi_signature() {
        let result = prim_ffi_signature(&[
            Value::keyword("double"),
            Value::array_mut(vec![Value::keyword("double")]),
        ]);
        assert_eq!(result.0, SIG_OK);
        assert!(result.1.as_ffi_signature().is_some());
    }

    #[test]
    fn test_ffi_signature_unknown_ret() {
        let result = prim_ffi_signature(&[Value::keyword("bad"), Value::array_mut(vec![])]);
        assert_eq!(result.0, SIG_ERROR);
    }

    #[test]
    fn test_ffi_malloc_free() {
        let result = prim_ffi_malloc(&[Value::int(100)]);
        assert_eq!(result.0, SIG_OK);
        assert!(result.1.as_managed_pointer().is_some());
        let ptr_val = result.1;

        let free_result = prim_ffi_free(&[ptr_val]);
        assert_eq!(free_result.0, SIG_OK);
    }

    #[test]
    fn test_ffi_free_nil() {
        let result = prim_ffi_free(&[Value::NIL]);
        assert_eq!(result.0, SIG_OK);
    }

    #[test]
    fn test_ffi_malloc_zero() {
        let result = prim_ffi_malloc(&[Value::int(0)]);
        assert_eq!(result.0, SIG_ERROR);
    }

    #[test]
    fn test_ffi_read_write_i32() {
        let alloc_result = prim_ffi_malloc(&[Value::int(4)]);
        assert_eq!(alloc_result.0, SIG_OK);
        let ptr = alloc_result.1;

        let write_result = prim_ffi_write(&[ptr, Value::keyword("i32"), Value::int(42)]);
        assert_eq!(write_result.0, SIG_OK);

        let read_result = prim_ffi_read(&[ptr, Value::keyword("i32")]);
        assert_eq!(read_result.0, SIG_OK);
        assert_eq!(read_result.1.as_int(), Some(42));

        prim_ffi_free(&[ptr]);
    }

    #[test]
    fn test_ffi_read_write_double() {
        let alloc_result = prim_ffi_malloc(&[Value::int(8)]);
        assert_eq!(alloc_result.0, SIG_OK);
        let ptr = alloc_result.1;

        let write_result = prim_ffi_write(&[ptr, Value::keyword("double"), Value::float(1.234)]);
        assert_eq!(write_result.0, SIG_OK);

        let read_result = prim_ffi_read(&[ptr, Value::keyword("double")]);
        assert_eq!(read_result.0, SIG_OK);
        assert_eq!(read_result.1.as_float(), Some(1.234));

        prim_ffi_free(&[ptr]);
    }

    #[test]
    fn test_ffi_read_null_error() {
        let result = prim_ffi_read(&[Value::NIL, Value::keyword("i32")]);
        assert_eq!(result.0, SIG_ERROR);
    }

    #[test]
    fn test_ffi_call_arity_error() {
        let result = prim_ffi_call(&[]);
        assert_eq!(result.0, SIG_ERROR);
    }

    #[test]
    fn test_ffi_native_wrong_type() {
        let result = prim_ffi_native(&[Value::int(42)]);
        assert_eq!(result.0, SIG_ERROR);
    }

    #[test]
    fn test_ffi_signature_variadic() {
        let result = prim_ffi_signature(&[
            Value::keyword("int"),
            Value::array_mut(vec![
                Value::keyword("ptr"),
                Value::keyword("string"),
                Value::keyword("int"),
            ]),
            Value::int(2),
        ]);
        assert_eq!(result.0, SIG_OK);
        let sig = result.1.as_ffi_signature().unwrap();
        assert_eq!(sig.fixed_args, Some(2));
    }

    #[test]
    fn test_ffi_signature_cif_caching() {
        let result = prim_ffi_signature(&[
            Value::keyword("int"),
            Value::array_mut(vec![Value::keyword("int")]),
        ]);
        assert_eq!(result.0, SIG_OK);
        let sig_val = result.1;

        // First access prepares the CIF
        let cif1 = sig_val.get_or_prepare_cif();
        assert!(cif1.is_some());
        drop(cif1);

        // Second access reuses the cached CIF
        let cif2 = sig_val.get_or_prepare_cif();
        assert!(cif2.is_some());
    }

    #[test]
    fn test_ffi_signature_variadic_out_of_range() {
        let result = prim_ffi_signature(&[
            Value::keyword("int"),
            Value::array_mut(vec![Value::keyword("int")]),
            Value::int(5), // 5 > 1 arg
        ]);
        assert_eq!(result.0, SIG_ERROR);
    }

    #[test]
    fn test_ffi_signature_variadic_bad_type() {
        let result = prim_ffi_signature(&[
            Value::keyword("int"),
            Value::array_mut(vec![Value::keyword("int")]),
            Value::string("bad"),
        ]);
        assert_eq!(result.0, SIG_ERROR);
    }

    #[test]
    fn test_ffi_string_null() {
        let result = prim_ffi_string(&[Value::NIL]);
        assert_eq!(result.0, SIG_OK);
        assert!(result.1.is_nil());
    }

    #[test]
    fn test_ffi_string_from_buffer() {
        // Allocate a buffer, write "hello\0" into it, read it back
        let alloc = prim_ffi_malloc(&[Value::int(16)]);
        assert_eq!(alloc.0, SIG_OK);
        let ptr = alloc.1;
        let addr = ptr.as_managed_pointer().unwrap().get().unwrap();
        unsafe {
            let p = addr as *mut u8;
            for (i, &b) in b"hello\0".iter().enumerate() {
                *p.add(i) = b;
            }
        }
        let result = prim_ffi_string(&[ptr]);
        assert_eq!(result.0, SIG_OK);
        assert_eq!(result.1.with_string(|s| s == "hello"), Some(true));

        // Also test with max-len
        let result2 = prim_ffi_string(&[ptr, Value::int(3)]);
        assert_eq!(result2.0, SIG_OK);
        assert_eq!(result2.1.with_string(|s| s == "hel"), Some(true));

        prim_ffi_free(&[ptr]);
    }

    #[test]
    fn test_ffi_string_wrong_type() {
        let result = prim_ffi_string(&[Value::int(42)]);
        assert_eq!(result.0, SIG_ERROR);
    }

    #[test]
    fn test_ffi_string_arity() {
        let result = prim_ffi_string(&[]);
        assert_eq!(result.0, SIG_ERROR);
    }

    #[test]
    fn test_ffi_struct_basic() {
        let result = prim_ffi_struct(&[Value::array_mut(vec![
            Value::keyword("i32"),
            Value::keyword("double"),
        ])]);
        assert_eq!(result.0, SIG_OK);
        assert!(result.1.as_ffi_type().is_some());
    }

    #[test]
    fn test_ffi_struct_nested() {
        // Create inner struct
        let inner_result = prim_ffi_struct(&[Value::array_mut(vec![
            Value::keyword("i8"),
            Value::keyword("i32"),
        ])]);
        assert_eq!(inner_result.0, SIG_OK);
        let inner = inner_result.1;

        // Create outer struct using inner
        let result = prim_ffi_struct(&[Value::array_mut(vec![Value::keyword("i64"), inner])]);
        assert_eq!(result.0, SIG_OK);
    }

    #[test]
    fn test_ffi_struct_empty() {
        let result = prim_ffi_struct(&[Value::array_mut(vec![])]);
        assert_eq!(result.0, SIG_ERROR);
    }

    #[test]
    fn test_ffi_struct_void_field() {
        let result = prim_ffi_struct(&[Value::array_mut(vec![Value::keyword("void")])]);
        assert_eq!(result.0, SIG_ERROR);
    }

    #[test]
    fn test_ffi_array_basic() {
        let result = prim_ffi_array(&[Value::keyword("i32"), Value::int(10)]);
        assert_eq!(result.0, SIG_OK);
        assert!(result.1.as_ffi_type().is_some());
    }

    #[test]
    fn test_ffi_array_zero_count() {
        let result = prim_ffi_array(&[Value::keyword("i32"), Value::int(0)]);
        assert_eq!(result.0, SIG_ERROR);
    }

    #[test]
    fn test_ffi_array_negative_count() {
        let result = prim_ffi_array(&[Value::keyword("i32"), Value::int(-5)]);
        assert_eq!(result.0, SIG_ERROR);
    }

    #[test]
    fn test_ffi_size_with_struct() {
        // Create a struct type
        let struct_result = prim_ffi_struct(&[Value::array_mut(vec![
            Value::keyword("i32"),
            Value::keyword("i32"),
        ])]);
        assert_eq!(struct_result.0, SIG_OK);
        let struct_type = struct_result.1;

        // Get its size
        let size_result = prim_ffi_size(&[struct_type]);
        assert_eq!(size_result.0, SIG_OK);
        assert_eq!(size_result.1.as_int(), Some(8));
    }

    #[test]
    fn test_ffi_align_with_struct() {
        let struct_result = prim_ffi_struct(&[Value::array_mut(vec![
            Value::keyword("i8"),
            Value::keyword("double"),
        ])]);
        assert_eq!(struct_result.0, SIG_OK);
        let struct_type = struct_result.1;

        let align_result = prim_ffi_align(&[struct_type]);
        assert_eq!(align_result.0, SIG_OK);
        assert_eq!(align_result.1.as_int(), Some(8));
    }

    #[test]
    fn test_ffi_signature_with_struct() {
        // Create a struct type for use in signature
        let struct_result = prim_ffi_struct(&[Value::array_mut(vec![
            Value::keyword("i32"),
            Value::keyword("double"),
        ])]);
        assert_eq!(struct_result.0, SIG_OK);
        let struct_type = struct_result.1;

        // Use struct as return type
        let sig_result =
            prim_ffi_signature(&[struct_type, Value::array_mut(vec![Value::keyword("ptr")])]);
        assert_eq!(sig_result.0, SIG_OK);

        // Use struct as argument type
        let sig_result2 =
            prim_ffi_signature(&[Value::keyword("void"), Value::array_mut(vec![struct_type])]);
        assert_eq!(sig_result2.0, SIG_OK);
    }

    #[test]
    fn test_ffi_read_write_struct() {
        // Create a struct type
        let struct_result = prim_ffi_struct(&[Value::array_mut(vec![
            Value::keyword("i32"),
            Value::keyword("double"),
        ])]);
        assert_eq!(struct_result.0, SIG_OK);
        let struct_type = struct_result.1;

        // Allocate memory for the struct
        let size = prim_ffi_size(&[struct_type]);
        let alloc_result = prim_ffi_malloc(&[size.1]);
        assert_eq!(alloc_result.0, SIG_OK);
        let ptr = alloc_result.1;

        // Write struct
        let test_float = 2.5;
        let struct_val = Value::array_mut(vec![Value::int(42), Value::float(test_float)]);
        let write_result = prim_ffi_write(&[ptr, struct_type, struct_val]);
        assert_eq!(write_result.0, SIG_OK);

        // Read struct back
        let read_result = prim_ffi_read(&[ptr, struct_type]);
        assert_eq!(read_result.0, SIG_OK);
        let arr = read_result.1.as_array_mut().unwrap();
        let arr = arr.borrow();
        assert_eq!(arr[0].as_int(), Some(42));
        assert!((arr[1].as_float().unwrap() - test_float).abs() < 1e-10);

        prim_ffi_free(&[ptr]);
    }

    // ── ptr/add ─────────────────────────────────────────────────────────

    fn error_kind(v: &crate::value::Value) -> Option<String> {
        use crate::value::heap::TableKey;
        v.as_struct()
            .and_then(|fields| fields.get(&TableKey::Keyword("error".into())))
            .and_then(|k| k.as_keyword_name())
    }

    #[test]
    fn test_ptr_add_basic() {
        let alloc = prim_ffi_malloc(&[Value::int(64)]);
        assert_eq!(alloc.0, SIG_OK);
        let buf = alloc.1;
        let addr = buf.as_managed_pointer().unwrap().get().unwrap();

        let result = prim_ptr_add(&[buf, Value::int(16)]);
        assert_eq!(result.0, SIG_OK);
        assert_eq!(result.1.as_pointer(), Some(addr + 16));

        prim_ffi_free(&[buf]);
    }

    #[test]
    fn test_ptr_add_negative() {
        let alloc = prim_ffi_malloc(&[Value::int(64)]);
        assert_eq!(alloc.0, SIG_OK);
        let buf = alloc.1;
        let addr = buf.as_managed_pointer().unwrap().get().unwrap();

        // Advance by 32, then retreat by 8 → net +24
        let p2 = prim_ptr_add(&[buf, Value::int(32)]);
        assert_eq!(p2.0, SIG_OK);
        let p3 = prim_ptr_add(&[p2.1, Value::int(-8)]);
        assert_eq!(p3.0, SIG_OK);
        assert_eq!(p3.1.as_pointer(), Some(addr + 24));

        prim_ffi_free(&[buf]);
    }

    #[test]
    fn test_ptr_add_null_error() {
        let result = prim_ptr_add(&[Value::NIL, Value::int(8)]);
        assert_eq!(result.0, SIG_ERROR);
        assert_eq!(error_kind(&result.1).as_deref(), Some("argument-error"));
    }

    #[test]
    fn test_ptr_add_freed_error() {
        let alloc = prim_ffi_malloc(&[Value::int(8)]);
        assert_eq!(alloc.0, SIG_OK);
        let buf = alloc.1;

        prim_ffi_free(&[buf]);

        let result = prim_ptr_add(&[buf, Value::int(4)]);
        assert_eq!(result.0, SIG_ERROR);
        assert_eq!(error_kind(&result.1).as_deref(), Some("use-after-free"));
    }

    #[test]
    fn test_ptr_add_wrong_type() {
        let result = prim_ptr_add(&[Value::int(42), Value::int(8)]);
        assert_eq!(result.0, SIG_ERROR);
        assert_eq!(error_kind(&result.1).as_deref(), Some("type-error"));
    }

    #[test]
    fn test_ptr_add_non_int_offset() {
        let alloc = prim_ffi_malloc(&[Value::int(8)]);
        assert_eq!(alloc.0, SIG_OK);
        let buf = alloc.1;

        let result = prim_ptr_add(&[buf, Value::string("hello")]);
        assert_eq!(result.0, SIG_ERROR);
        assert_eq!(error_kind(&result.1).as_deref(), Some("type-error"));

        prim_ffi_free(&[buf]);
    }

    #[test]
    fn test_ptr_add_overflow() {
        let alloc = prim_ffi_malloc(&[Value::int(8)]);
        assert_eq!(alloc.0, SIG_OK);
        let buf = alloc.1;

        // (1i64 << 47) - 1 == MAX_PTR. Any non-zero address plus this value
        // exceeds MAX_PTR, triggering the range check.
        const INT_MAX: i64 = (1i64 << 47) - 1;
        let result = prim_ptr_add(&[buf, Value::int(INT_MAX)]);
        assert_eq!(result.0, SIG_ERROR);
        // We expect argument-error (range exceeded), not overflow-error
        assert_eq!(error_kind(&result.1).as_deref(), Some("argument-error"));

        prim_ffi_free(&[buf]);
    }

    // ── ptr/diff ────────────────────────────────────────────────────────

    #[test]
    fn test_ptr_diff_basic() {
        let alloc = prim_ffi_malloc(&[Value::int(64)]);
        assert_eq!(alloc.0, SIG_OK);
        let buf = alloc.1;

        let p2 = prim_ptr_add(&[buf, Value::int(24)]);
        assert_eq!(p2.0, SIG_OK);

        let diff = prim_ptr_diff(&[p2.1, buf]);
        assert_eq!(diff.0, SIG_OK);
        assert_eq!(diff.1.as_int(), Some(24));

        prim_ffi_free(&[buf]);
    }

    #[test]
    fn test_ptr_diff_negative() {
        let alloc = prim_ffi_malloc(&[Value::int(64)]);
        assert_eq!(alloc.0, SIG_OK);
        let buf = alloc.1;

        let p2 = prim_ptr_add(&[buf, Value::int(24)]);
        assert_eq!(p2.0, SIG_OK);

        // Reverse order → negative
        let diff = prim_ptr_diff(&[buf, p2.1]);
        assert_eq!(diff.0, SIG_OK);
        assert_eq!(diff.1.as_int(), Some(-24));

        prim_ffi_free(&[buf]);
    }

    // ── ptr/to-int ──────────────────────────────────────────────────────

    #[test]
    fn test_ptr_to_int_basic() {
        let alloc = prim_ffi_malloc(&[Value::int(8)]);
        assert_eq!(alloc.0, SIG_OK);
        let buf = alloc.1;
        let expected = buf.as_managed_pointer().unwrap().get().unwrap() as i64;

        let result = prim_ptr_to_int(&[buf]);
        assert_eq!(result.0, SIG_OK);
        assert_eq!(result.1.as_int(), Some(expected));

        prim_ffi_free(&[buf]);
    }

    #[test]
    fn test_ptr_to_int_null_error() {
        let result = prim_ptr_to_int(&[Value::NIL]);
        assert_eq!(result.0, SIG_ERROR);
        assert_eq!(error_kind(&result.1).as_deref(), Some("argument-error"));
    }

    // ── ptr/from-int ────────────────────────────────────────────────────

    #[test]
    fn test_ptr_from_int_basic() {
        let alloc = prim_ffi_malloc(&[Value::int(8)]);
        assert_eq!(alloc.0, SIG_OK);
        let buf = alloc.1;
        let addr = buf.as_managed_pointer().unwrap().get().unwrap() as i64;

        let result = prim_ptr_from_int(&[Value::int(addr)]);
        assert_eq!(result.0, SIG_OK);
        assert_eq!(result.1.as_pointer(), Some(addr as usize));

        prim_ffi_free(&[buf]);
    }

    #[test]
    fn test_ptr_from_int_zero() {
        let result = prim_ptr_from_int(&[Value::int(0)]);
        assert_eq!(result.0, SIG_OK);
        // ptr/from-int 0 → nil (Value::pointer(0) == Value::NIL)
        assert!(result.1.is_nil());
    }

    #[test]
    fn test_ptr_from_int_negative() {
        let result = prim_ptr_from_int(&[Value::int(-1)]);
        assert_eq!(result.0, SIG_ERROR);
        assert_eq!(error_kind(&result.1).as_deref(), Some("argument-error"));
    }

    #[test]
    fn test_ptr_from_int_exceeds_47bit() {
        // (1i64 << 47) - 1 == MAX_PTR. The guard in prim_ptr_from_int is a
        // defensive invariant. We verify the boundary value is accepted.
        const INT_MAX: i64 = (1i64 << 47) - 1;
        let result = prim_ptr_from_int(&[Value::int(INT_MAX)]);
        assert_eq!(result.0, SIG_OK);
        assert_eq!(result.1.as_pointer(), Some(INT_MAX as usize));
    }
}
