use super::*;

// =========================================================================
// H. TypeDesc struct layout properties
// =========================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(200))]

    // Struct size >= sum of field sizes
    #[test]
    fn struct_size_ge_field_sum(sd in arb_flat_struct()) {
        let desc = TypeDesc::Struct(sd.clone());
        let struct_size = desc.size().unwrap();
        let field_sum: usize = sd.fields.iter().map(|f| f.size().unwrap()).sum();
        prop_assert!(
            struct_size >= field_sum,
            "struct size {} < field sum {}",
            struct_size,
            field_sum
        );
    }

    // Struct alignment is max of field alignments
    #[test]
    fn struct_align_is_max_field_align(sd in arb_flat_struct()) {
        let desc = TypeDesc::Struct(sd.clone());
        let struct_align = desc.align().unwrap();
        let max_field_align = sd
            .fields
            .iter()
            .map(|f| f.align().unwrap())
            .max()
            .unwrap_or(1);
        prop_assert_eq!(
            struct_align, max_field_align,
            "struct align {} != max field align {}",
            struct_align, max_field_align
        );
    }

    // Struct size is divisible by alignment (tail padding)
    #[test]
    fn struct_size_aligned(sd in arb_flat_struct()) {
        let desc = TypeDesc::Struct(sd);
        let size = desc.size().unwrap();
        let align = desc.align().unwrap();
        prop_assert_eq!(
            size % align,
            0,
            "struct size {} not aligned to {}",
            size,
            align
        );
    }

    // Field offsets are sorted and non-overlapping
    #[test]
    fn field_offsets_sorted_non_overlapping(sd in arb_flat_struct()) {
        let (offsets, total_size) = sd.field_offsets().unwrap();
        for i in 0..offsets.len() {
            // Offset is aligned to field alignment
            let field_align = sd.fields[i].align().unwrap();
            prop_assert_eq!(
                offsets[i] % field_align,
                0,
                "field {} offset {} not aligned to {}",
                i,
                offsets[i],
                field_align
            );

            // Non-overlapping: offset[i] + size[i] <= offset[i+1]
            if i + 1 < offsets.len() {
                let field_end = offsets[i] + sd.fields[i].size().unwrap();
                prop_assert!(
                    field_end <= offsets[i + 1],
                    "field {} end {} overlaps field {} at {}",
                    i,
                    field_end,
                    i + 1,
                    offsets[i + 1]
                );
            }

            // Last field + size <= total
            if i == offsets.len() - 1 {
                let field_end = offsets[i] + sd.fields[i].size().unwrap();
                prop_assert!(
                    field_end <= total_size,
                    "last field end {} > total size {}",
                    field_end,
                    total_size
                );
            }
        }
    }

    // Field offsets total_size matches TypeDesc::size()
    #[test]
    fn field_offsets_total_matches_size(sd in arb_flat_struct()) {
        let desc = TypeDesc::Struct(sd.clone());
        let (_, total_size) = sd.field_offsets().unwrap();
        prop_assert_eq!(desc.size(), Some(total_size));
    }
}

// =========================================================================
// I. Array type properties
// =========================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(100))]

    // Array size = element_size * count
    #[test]
    fn array_size_is_elem_times_count(
        elem in arb_primitive_type(),
        count in 1usize..=10,
    ) {
        let desc = TypeDesc::Array(Box::new(elem.clone()), count);
        let expected = elem.size().unwrap() * count;
        prop_assert_eq!(desc.size(), Some(expected));
    }

    // Array write-read roundtrip
    #[test]
    fn array_roundtrip(
        elem_desc in arb_primitive_type(),
        count in 1usize..=5,
    ) {
        // Skip pointer and float for simpler comparison
        prop_assume!(!matches!(elem_desc, TypeDesc::Ptr | TypeDesc::Float));

        let h = elle::primitives::ctx::TestHeap::new();
        let desc = TypeDesc::Array(Box::new(elem_desc.clone()), count);
        // Generate deterministic values
        let vals: Vec<Value> = (0..count)
            .map(|i| match &elem_desc {
                TypeDesc::I8 => Value::int((i as i64) % 127),
                TypeDesc::U8 => Value::int(i as i64),
                TypeDesc::I16 => Value::int(i as i64 * 100),
                TypeDesc::U16 => Value::int(i as i64 * 100),
                TypeDesc::I32 => Value::int(i as i64 * 10000),
                TypeDesc::U32 => Value::int(i as i64 * 10000),
                TypeDesc::I64 => Value::int(i as i64 * 100000),
                TypeDesc::U64 => Value::int(i as i64 * 100000),
                TypeDesc::Double => Value::float(i as f64 * 1.5),
                _ => Value::int(i as i64),
            })
            .collect();
        let val = h.ctx().array_mut(vals.clone());

        let size = desc.size().unwrap();
        let alloc = prim_ffi_malloc(&[Value::int(size as i64)]);
        prop_assert_eq!(alloc.0, SIG_OK);
        let ptr = alloc.1;

        let type_val = h.ctx().ffi_type(desc);
        let write = prim_ffi_write(&[ptr, type_val, val]);
        prop_assert_eq!(write.0, SIG_OK, "write failed");

        let read = prim_ffi_read(&[ptr, type_val]);
        prop_assert_eq!(read.0, SIG_OK, "read failed");

        // u8/i8 arrays return bytes; other element types return immutable arrays.
        if matches!(
            elem_desc,
            TypeDesc::U8 | TypeDesc::UChar | TypeDesc::I8 | TypeDesc::Char
        ) {
            let data = read.1.as_bytes().expect("u8 array should return bytes");
            prop_assert_eq!(data.len(), count, "bytes length mismatch");
            for (i, (orig, &res)) in vals.iter().zip(data.iter()).enumerate() {
                prop_assert_eq!(
                    orig.as_int(),
                    Some(res as i64),
                    "byte element {} mismatch",
                    i
                );
            }
        } else {
            let result = read.1.as_array().expect("non-u8 array should return immutable array");
            prop_assert_eq!(result.len(), count, "element count mismatch");

            for (i, (orig, res)) in vals.iter().zip(result.iter()).enumerate() {
                if matches!(elem_desc, TypeDesc::Double) {
                    let orig_f = orig.as_float().unwrap();
                    let res_f = res.as_float().unwrap();
                    prop_assert_eq!(
                        orig_f.to_bits(),
                        res_f.to_bits(),
                        "double element {} mismatch",
                        i
                    );
                } else {
                    prop_assert_eq!(orig.as_int(), res.as_int(), "element {} mismatch", i);
                }
            }
        }

        prim_ffi_free(&[ptr]);
    }
}

// =========================================================================
// J. FFIType value properties
// =========================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(100))]

    // FFIType structural equality
    #[test]
    fn ffi_type_structural_eq(sd in arb_flat_struct()) {
        let h = elle::primitives::ctx::TestHeap::new();
        let desc1 = TypeDesc::Struct(sd.clone());
        let desc2 = TypeDesc::Struct(sd);
        prop_assert_eq!(h.ctx().ffi_type(desc1), h.ctx().ffi_type(desc2));
    }

    // FFIType type name is always "ffi-type"
    #[test]
    fn ffi_type_name(sd in arb_flat_struct()) {
        let h = elle::primitives::ctx::TestHeap::new();
        let v = h.ctx().ffi_type(TypeDesc::Struct(sd));
        prop_assert_eq!(v.type_name(), "ffi-type");
    }

    // FFIType roundtrip through as_ffi_type
    #[test]
    fn ffi_type_accessor_roundtrip(sd in arb_flat_struct()) {
        let h = elle::primitives::ctx::TestHeap::new();
        let desc = TypeDesc::Struct(sd);
        let v = h.ctx().ffi_type(desc.clone());
        prop_assert_eq!(v.as_ffi_type(), Some(&desc));
    }

    // ffi/size matches TypeDesc::size() for structs
    #[test]
    fn ffi_size_matches_type_desc(sd in arb_flat_struct()) {
        let h = elle::primitives::ctx::TestHeap::new();
        let desc = TypeDesc::Struct(sd);
        let expected = desc.size().unwrap();
        let result = prim_ffi_size(&[h.ctx().ffi_type(desc)]);
        prop_assert_eq!(result.0, SIG_OK);
        prop_assert_eq!(result.1.as_int(), Some(expected as i64));
    }

    // ffi/align matches TypeDesc::align() for structs
    #[test]
    fn ffi_align_matches_type_desc(sd in arb_flat_struct()) {
        let h = elle::primitives::ctx::TestHeap::new();
        let desc = TypeDesc::Struct(sd);
        let expected = desc.align().unwrap();
        let result = prim_ffi_align(&[h.ctx().ffi_type(desc)]);
        prop_assert_eq!(result.0, SIG_OK);
        prop_assert_eq!(result.1.as_int(), Some(expected as i64));
    }
}
