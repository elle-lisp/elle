use super::*;

// =========================================================================
// C. Memory read-write roundtrip
// =========================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(200))]

    // i32 write-read roundtrip
    #[test]
    fn memory_roundtrip_i32(n in i32::MIN as i64..=i32::MAX as i64) {
        let alloc = prim_ffi_malloc(&[Value::int(4)]);
        prop_assert_eq!(alloc.0, SIG_OK);
        let ptr = alloc.1;

        let write = prim_ffi_write(&[ptr, Value::keyword("i32"), Value::int(n)]);
        prop_assert_eq!(write.0, SIG_OK);

        let read = prim_ffi_read(&[ptr, Value::keyword("i32")]);
        prop_assert_eq!(read.0, SIG_OK);
        prop_assert_eq!(read.1.as_int(), Some(n));

        prim_ffi_free(&[ptr]);
    }

    // i64 write-read roundtrip
    #[test]
    fn memory_roundtrip_i64(n in i64::MIN..=i64::MAX) {
        let alloc = prim_ffi_malloc(&[Value::int(8)]);
        prop_assert_eq!(alloc.0, SIG_OK);
        let ptr = alloc.1;

        let write = prim_ffi_write(&[ptr, Value::keyword("i64"), Value::int(n)]);
        prop_assert_eq!(write.0, SIG_OK);

        let read = prim_ffi_read(&[ptr, Value::keyword("i64")]);
        prop_assert_eq!(read.0, SIG_OK);
        prop_assert_eq!(read.1.as_int(), Some(n));

        prim_ffi_free(&[ptr]);
    }

    // double write-read roundtrip
    #[test]
    fn memory_roundtrip_double(f in prop::num::f64::NORMAL) {
        let alloc = prim_ffi_malloc(&[Value::int(8)]);
        prop_assert_eq!(alloc.0, SIG_OK);
        let ptr = alloc.1;

        let write = prim_ffi_write(&[ptr, Value::keyword("double"), Value::float(f)]);
        prop_assert_eq!(write.0, SIG_OK);

        let read = prim_ffi_read(&[ptr, Value::keyword("double")]);
        prop_assert_eq!(read.0, SIG_OK);
        let readback = read.1.as_float().unwrap();
        prop_assert_eq!(readback.to_bits(), f.to_bits(),
            "double roundtrip failed: wrote {} got {}", f, readback);

        prim_ffi_free(&[ptr]);
    }

    // u8 write-read roundtrip
    #[test]
    fn memory_roundtrip_u8(n in 0u8..=255) {
        let alloc = prim_ffi_malloc(&[Value::int(1)]);
        prop_assert_eq!(alloc.0, SIG_OK);
        let ptr = alloc.1;

        let write = prim_ffi_write(&[ptr, Value::keyword("u8"), Value::int(n as i64)]);
        prop_assert_eq!(write.0, SIG_OK);

        let read = prim_ffi_read(&[ptr, Value::keyword("u8")]);
        prop_assert_eq!(read.0, SIG_OK);
        prop_assert_eq!(read.1.as_int(), Some(n as i64));

        prim_ffi_free(&[ptr]);
    }

    // pointer write-read roundtrip
    #[test]
    fn memory_roundtrip_ptr(addr in 0usize..=0x0000_7FFF_FFFF_FFFFusize) {
        let alloc = prim_ffi_malloc(&[Value::int(8)]);
        prop_assert_eq!(alloc.0, SIG_OK);
        let ptr = alloc.1;

        // Write: nil for 0, pointer for nonzero
        let val = if addr == 0 { Value::NIL } else { Value::pointer(addr) };
        let write = prim_ffi_write(&[ptr, Value::keyword("ptr"), val]);
        prop_assert_eq!(write.0, SIG_OK);

        let read = prim_ffi_read(&[ptr, Value::keyword("ptr")]);
        prop_assert_eq!(read.0, SIG_OK);
        // NULL -> Value::pointer(0) -> Value::NIL
        if addr == 0 {
            prop_assert!(read.1.is_nil() || read.1.as_pointer() == Some(0),
                "reading NULL pointer should give nil, got {:?}", read.1);
        } else {
            prop_assert_eq!(read.1.as_pointer(), Some(addr));
        }

        prim_ffi_free(&[ptr]);
    }
}

// =========================================================================
// D. TypeDesc size/align consistency
// =========================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(100))]

    // Alignment is always a power of 2 (for non-void types)
    #[test]
    fn type_align_is_power_of_two(idx in 0usize..22) {
        let types = [
            TypeDesc::Bool, TypeDesc::I8, TypeDesc::U8, TypeDesc::I16, TypeDesc::U16,
            TypeDesc::I32, TypeDesc::U32, TypeDesc::I64, TypeDesc::U64,
            TypeDesc::Float, TypeDesc::Double,
            TypeDesc::Int, TypeDesc::UInt, TypeDesc::Long, TypeDesc::ULong,
            TypeDesc::Char, TypeDesc::UChar, TypeDesc::Short, TypeDesc::UShort,
            TypeDesc::Size, TypeDesc::SSize, TypeDesc::Ptr,
        ];
        if idx < types.len() {
            let align = types[idx].align().unwrap();
            prop_assert!(align.is_power_of_two(),
                "alignment of {:?} is {} (not power of 2)", types[idx], align);
        }
    }

    // Size >= alignment for all types
    #[test]
    fn type_size_ge_align(idx in 0usize..22) {
        let types = [
            TypeDesc::Bool, TypeDesc::I8, TypeDesc::U8, TypeDesc::I16, TypeDesc::U16,
            TypeDesc::I32, TypeDesc::U32, TypeDesc::I64, TypeDesc::U64,
            TypeDesc::Float, TypeDesc::Double,
            TypeDesc::Int, TypeDesc::UInt, TypeDesc::Long, TypeDesc::ULong,
            TypeDesc::Char, TypeDesc::UChar, TypeDesc::Short, TypeDesc::UShort,
            TypeDesc::Size, TypeDesc::SSize, TypeDesc::Ptr,
        ];
        if idx < types.len() {
            let size = types[idx].size().unwrap();
            let align = types[idx].align().unwrap();
            prop_assert!(size >= align,
                "{:?}: size {} < align {}", types[idx], size, align);
        }
    }
}

// =========================================================================
// E. String marshalling edge cases
// =========================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(500))]

    // Any ASCII string without nulls marshals successfully
    #[test]
    fn marshal_string_ascii(s in "[a-zA-Z0-9 ]{0,100}") {
        let h = elle::primitives::ctx::TestHeap::new();
        let v = h.ctx().string(s);
        prop_assert!(MarshalledArg::new(&v, &TypeDesc::Str).is_ok());
    }

    // Strings with embedded nulls are rejected
    #[test]
    fn marshal_string_with_null(
        prefix in "[a-zA-Z]{1,10}",
        suffix in "[a-zA-Z]{1,10}",
    ) {
        let h = elle::primitives::ctx::TestHeap::new();
        let s = format!("{}\0{}", prefix, suffix);
        let v = h.ctx().string(s);
        prop_assert!(MarshalledArg::new(&v, &TypeDesc::Str).is_err());
    }

    // Non-string values rejected for :string type
    #[test]
    fn marshal_string_rejects_int(n in i64::MIN..=i64::MAX) {
        let v = Value::int(n);
        prop_assert!(MarshalledArg::new(&v, &TypeDesc::Str).is_err());
    }
}

// =========================================================================
// F. Struct marshalling roundtrip
// =========================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(200))]

    // Struct write-read roundtrip: write a struct, read it back, values match
    #[test]
    fn struct_roundtrip((sd, val) in arb_struct_and_values()) {
        let h = elle::primitives::ctx::TestHeap::new();
        let desc = TypeDesc::Struct(sd.clone());
        let size = desc.size().unwrap();
        let alloc = prim_ffi_malloc(&[Value::int(size as i64)]);
        prop_assert_eq!(alloc.0, SIG_OK);
        let ptr = alloc.1;

        let type_val = h.ctx().ffi_type(desc.clone());
        let write = prim_ffi_write(&[ptr, type_val, val]);
        prop_assert_eq!(write.0, SIG_OK, "write failed");

        let read = prim_ffi_read(&[ptr, type_val]);
        prop_assert_eq!(read.0, SIG_OK, "read failed");

        // Compare field by field
        let original = val.as_array_mut().unwrap();
        let original = original.borrow();
        let result = read.1.as_array().unwrap();
        prop_assert_eq!(original.len(), result.len(), "field count mismatch");

        for (i, (field_desc, (orig, res))) in sd
            .fields
            .iter()
            .zip(original.iter().zip(result.iter()))
            .enumerate()
        {
            match field_desc {
                TypeDesc::Float => {
                    // Float roundtrip loses precision (f64→f32→f64)
                    let orig_f = orig
                        .as_float()
                        .or_else(|| orig.as_int().map(|i| i as f64))
                        .unwrap();
                    let res_f = res.as_float().unwrap();
                    let orig_f32 = orig_f as f32;
                    prop_assert_eq!(
                        orig_f32.to_bits(),
                        (res_f as f32).to_bits(),
                        "float field {} mismatch: {} vs {}",
                        i,
                        orig_f,
                        res_f
                    );
                }
                TypeDesc::Double => {
                    let orig_f = orig
                        .as_float()
                        .or_else(|| orig.as_int().map(|i| i as f64))
                        .unwrap();
                    let res_f = res.as_float().unwrap();
                    prop_assert_eq!(
                        orig_f.to_bits(),
                        res_f.to_bits(),
                        "double field {} mismatch: {} vs {}",
                        i,
                        orig_f,
                        res_f
                    );
                }
                TypeDesc::Ptr => {
                    // Pointer roundtrip: nil→nil, pointer→pointer
                    if orig.is_nil() {
                        prop_assert!(
                            res.is_nil() || res.as_pointer() == Some(0),
                            "null pointer field {} mismatch",
                            i
                        );
                    } else {
                        prop_assert_eq!(
                            orig.as_pointer(),
                            res.as_pointer(),
                            "pointer field {} mismatch",
                            i
                        );
                    }
                }
                _ => {
                    // Integer types: exact match
                    prop_assert_eq!(
                        orig.as_int(),
                        res.as_int(),
                        "integer field {} mismatch: {:?} vs {:?}",
                        i,
                        orig,
                        res
                    );
                }
            }
        }

        prim_ffi_free(&[ptr]);
    }
}

// =========================================================================
// G. Struct field count validation
// =========================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(100))]

    // Writing with wrong number of fields fails
    #[test]
    fn struct_wrong_field_count(sd in arb_flat_struct(), extra in 1usize..=3) {
        let h = elle::primitives::ctx::TestHeap::new();
        let desc = TypeDesc::Struct(sd.clone());
        let size = desc.size().unwrap();
        let alloc = prim_ffi_malloc(&[Value::int(size as i64)]);
        prop_assert_eq!(alloc.0, SIG_OK);
        let ptr = alloc.1;

        // Too few values
        if sd.fields.len() > 1 {
            let too_few = h.ctx().array_mut(vec![Value::int(0); sd.fields.len() - 1]);
            let write = prim_ffi_write(&[ptr, h.ctx().ffi_type(desc.clone()), too_few]);
            prop_assert_eq!(write.0, SIG_ERROR, "should reject too few fields");
        }

        // Too many values
        let too_many = h.ctx().array_mut(vec![Value::int(0); sd.fields.len() + extra]);
        let write = prim_ffi_write(&[ptr, h.ctx().ffi_type(desc), too_many]);
        prop_assert_eq!(write.0, SIG_ERROR, "should reject too many fields");

        prim_ffi_free(&[ptr]);
    }

    // Writing non-array value for struct fails
    #[test]
    fn struct_non_array_rejected(sd in arb_flat_struct()) {
        let h = elle::primitives::ctx::TestHeap::new();
        let desc = TypeDesc::Struct(sd);
        let size = desc.size().unwrap();
        let alloc = prim_ffi_malloc(&[Value::int(size as i64)]);
        prop_assert_eq!(alloc.0, SIG_OK);
        let ptr = alloc.1;

        let write = prim_ffi_write(&[ptr, h.ctx().ffi_type(desc), Value::int(42)]);
        prop_assert_eq!(write.0, SIG_ERROR, "should reject non-array");

        prim_ffi_free(&[ptr]);
    }
}
