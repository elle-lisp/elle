//! Unit tests (`super` is the parent impl module).

use super::*;
use crate::ffi::types::StructDesc;

/// Run `f` with an `Alloc` over a fresh region on the root heap, NOT releasing
/// it: a reconstructed heap value must outlive the call (the test reads it
/// afterward), so freeing the region would recycle the pages the returned value
/// points at (a UAF). The region simply stays resident for the test's duration.
fn read_in_region(
    f: impl FnOnce(&mut crate::primitives::ctx::Alloc) -> crate::error::LResult<Value>,
) -> crate::error::LResult<Value> {
    let heap_ptr = crate::value::arena::leaked_test_heap();
    let region = unsafe { (*heap_ptr).new_runtime_region() };
    let mut ctx = crate::primitives::ctx::Alloc::with_region(region, unsafe { &mut *heap_ptr });
    f(&mut ctx)
}

#[test]
fn test_to_libffi_type_primitives() {
    // Smoke test: these should not panic
    to_libffi_type(&TypeDesc::Void);
    to_libffi_type(&TypeDesc::Bool);
    to_libffi_type(&TypeDesc::I8);
    to_libffi_type(&TypeDesc::U8);
    to_libffi_type(&TypeDesc::I16);
    to_libffi_type(&TypeDesc::U16);
    to_libffi_type(&TypeDesc::I32);
    to_libffi_type(&TypeDesc::U32);
    to_libffi_type(&TypeDesc::I64);
    to_libffi_type(&TypeDesc::U64);
    to_libffi_type(&TypeDesc::Float);
    to_libffi_type(&TypeDesc::Double);
    to_libffi_type(&TypeDesc::Int);
    to_libffi_type(&TypeDesc::UInt);
    to_libffi_type(&TypeDesc::Long);
    to_libffi_type(&TypeDesc::ULong);
    to_libffi_type(&TypeDesc::Char);
    to_libffi_type(&TypeDesc::UChar);
    to_libffi_type(&TypeDesc::Short);
    to_libffi_type(&TypeDesc::UShort);
    to_libffi_type(&TypeDesc::Size);
    to_libffi_type(&TypeDesc::SSize);
    to_libffi_type(&TypeDesc::Ptr);
    to_libffi_type(&TypeDesc::Str);
}

#[test]
fn test_to_libffi_type_struct() {
    let desc = TypeDesc::Struct(StructDesc {
        fields: vec![TypeDesc::I32, TypeDesc::Double],
    });
    to_libffi_type(&desc);
}

#[test]
fn test_to_libffi_type_array() {
    let desc = TypeDesc::Array(Box::new(TypeDesc::I32), 4);
    to_libffi_type(&desc);
}

#[test]
fn test_marshal_int_types() {
    let val = Value::int(42);
    assert!(MarshalledArg::new(&val, &TypeDesc::I8).is_ok());
    assert!(MarshalledArg::new(&val, &TypeDesc::U8).is_ok());
    assert!(MarshalledArg::new(&val, &TypeDesc::I16).is_ok());
    assert!(MarshalledArg::new(&val, &TypeDesc::U16).is_ok());
    assert!(MarshalledArg::new(&val, &TypeDesc::I32).is_ok());
    assert!(MarshalledArg::new(&val, &TypeDesc::U32).is_ok());
    assert!(MarshalledArg::new(&val, &TypeDesc::I64).is_ok());
    assert!(MarshalledArg::new(&val, &TypeDesc::U64).is_ok());
    assert!(MarshalledArg::new(&val, &TypeDesc::Int).is_ok());
    assert!(MarshalledArg::new(&val, &TypeDesc::UInt).is_ok());
}

#[test]
fn test_marshal_int_range_error() {
    let val = Value::int(256);
    assert!(MarshalledArg::new(&val, &TypeDesc::I8).is_err());
    assert!(MarshalledArg::new(&val, &TypeDesc::U8).is_err());

    let neg = Value::int(-1);
    assert!(MarshalledArg::new(&neg, &TypeDesc::U8).is_err());
    assert!(MarshalledArg::new(&neg, &TypeDesc::U16).is_err());
    assert!(MarshalledArg::new(&neg, &TypeDesc::U32).is_err());
}

#[test]
fn test_marshal_float() {
    let val = Value::float(2.5);
    assert!(MarshalledArg::new(&val, &TypeDesc::Float).is_ok());
    assert!(MarshalledArg::new(&val, &TypeDesc::Double).is_ok());
}

#[test]
fn test_marshal_int_as_float() {
    let val = Value::int(42);
    assert!(MarshalledArg::new(&val, &TypeDesc::Float).is_ok());
    assert!(MarshalledArg::new(&val, &TypeDesc::Double).is_ok());
}

#[test]
fn test_marshal_bool() {
    let t = Value::bool(true);
    let f = Value::bool(false);
    assert!(MarshalledArg::new(&t, &TypeDesc::Bool).is_ok());
    assert!(MarshalledArg::new(&f, &TypeDesc::Bool).is_ok());
}

#[test]
fn test_marshal_ptr_nil() {
    let nil = Value::NIL;
    assert!(MarshalledArg::new(&nil, &TypeDesc::Ptr).is_ok());
}

#[test]
fn test_marshal_ptr_value() {
    let ptr = Value::pointer(0x1234);
    assert!(MarshalledArg::new(&ptr, &TypeDesc::Ptr).is_ok());
}

#[test]
fn test_marshal_ptr_type_error() {
    let val = Value::int(42);
    assert!(MarshalledArg::new(&val, &TypeDesc::Ptr).is_err());
}

#[test]
fn test_marshal_string() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let val = h.ctx().string("hello");
        assert!(MarshalledArg::new(&val, &TypeDesc::Str).is_ok());
    });
}

#[test]
fn test_marshal_string_interior_null() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let val = h.ctx().string("hel\0lo");
        assert!(MarshalledArg::new(&val, &TypeDesc::Str).is_err());
    });
}

#[test]
fn test_marshal_void_error() {
    let val = Value::NIL;
    assert!(MarshalledArg::new(&val, &TypeDesc::Void).is_err());
}

#[test]
fn test_marshal_struct() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let desc = TypeDesc::Struct(StructDesc {
            fields: vec![TypeDesc::I32, TypeDesc::Double],
        });
        let val = h.ctx().array_mut(vec![Value::int(42), Value::float(1.5)]);
        let m = MarshalledArg::new(&val, &desc).unwrap();
        let _ = m.as_arg();
    });
}

#[test]
fn test_marshal_struct_wrong_count() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let desc = TypeDesc::Struct(StructDesc {
            fields: vec![TypeDesc::I32, TypeDesc::Double],
        });
        let val = h.ctx().array_mut(vec![Value::int(42)]);
        assert!(MarshalledArg::new(&val, &desc).is_err());
    });
}

#[test]
fn test_marshal_struct_wrong_type() {
    let desc = TypeDesc::Struct(StructDesc {
        fields: vec![TypeDesc::I32],
    });
    let val = Value::int(42); // Not an array
    assert!(MarshalledArg::new(&val, &desc).is_err());
}

#[test]
fn test_marshal_array() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let desc = TypeDesc::Array(Box::new(TypeDesc::I32), 3);
        let val = h
            .ctx()
            .array_mut(vec![Value::int(1), Value::int(2), Value::int(3)]);
        let m = MarshalledArg::new(&val, &desc).unwrap();
        let _ = m.as_arg();
    });
}

#[test]
fn test_marshal_array_wrong_count() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let desc = TypeDesc::Array(Box::new(TypeDesc::I32), 3);
        let val = h.ctx().array_mut(vec![Value::int(1), Value::int(2)]);
        assert!(MarshalledArg::new(&val, &desc).is_err());
    });
}

#[test]
fn test_read_write_struct_roundtrip() {
    crate::value::arena::with_test_region(|| {
        let sd = StructDesc {
            fields: vec![TypeDesc::I32, TypeDesc::Double, TypeDesc::I64],
        };
        let desc = TypeDesc::Struct(sd.clone());
        let h = crate::primitives::ctx::TestHeap::new();
        let values = h
            .ctx()
            .array_mut(vec![Value::int(42), Value::float(1.5), Value::int(-100)]);

        let (offsets, total_size) = sd.field_offsets().unwrap();
        let align = desc.align().unwrap();
        let buf = AlignedBuffer::new(total_size, align);

        let arr = values.as_array_mut().unwrap();
        let elems = arr.borrow();
        for (i, (field_desc, &offset)) in sd.fields.iter().zip(offsets.iter()).enumerate() {
            let _ = write_value_to_buffer(
                unsafe { buf.as_mut_ptr().add(offset) },
                &elems[i],
                field_desc,
            )
            .unwrap();
        }

        let result =
            read_in_region(|ctx| read_value_from_buffer(buf.as_mut_ptr(), &desc, ctx)).unwrap();
        let result_elems = result.as_array().unwrap();
        assert_eq!(result_elems[0].as_int(), Some(42));
        assert!((result_elems[1].as_float().unwrap() - 1.5).abs() < 1e-10);
        assert_eq!(result_elems[2].as_int(), Some(-100));
    });
}

#[test]
fn test_read_write_array_roundtrip() {
    crate::value::arena::with_test_region(|| {
        let desc = TypeDesc::Array(Box::new(TypeDesc::I32), 4);
        let h = crate::primitives::ctx::TestHeap::new();
        let values = h.ctx().array_mut(vec![
            Value::int(10),
            Value::int(20),
            Value::int(30),
            Value::int(40),
        ]);

        let elem_size = TypeDesc::I32.size().unwrap();
        let total_size = elem_size * 4;
        let align = TypeDesc::I32.align().unwrap();
        let buf = AlignedBuffer::new(total_size, align);

        let arr = values.as_array_mut().unwrap();
        let elems = arr.borrow();
        for (i, elem_val) in elems.iter().enumerate() {
            let _ = write_value_to_buffer(
                unsafe { buf.as_mut_ptr().add(i * elem_size) },
                elem_val,
                &TypeDesc::I32,
            )
            .unwrap();
        }

        let result =
            read_in_region(|ctx| read_value_from_buffer(buf.as_mut_ptr(), &desc, ctx)).unwrap();
        let result_elems = result.as_array().unwrap();
        assert_eq!(result_elems.len(), 4);
        assert_eq!(result_elems[0].as_int(), Some(10));
        assert_eq!(result_elems[1].as_int(), Some(20));
        assert_eq!(result_elems[2].as_int(), Some(30));
        assert_eq!(result_elems[3].as_int(), Some(40));
    });
}

#[test]
fn test_u64_roundtrip_large_value() {
    // A u64 value above i64::MAX must survive write → read without truncation.
    // This is the core invariant of the u64 bit-reinterpret convention.
    let large: u64 = u64::MAX - 1; // 0xFFFFFFFFFFFFFFFE
    let buf = AlignedBuffer::new(8, 8);

    // Write: Elle int holding the bit pattern
    let val = Value::int(large as i64);
    write_value_to_buffer(buf.as_mut_ptr(), &val, &TypeDesc::U64).unwrap();

    // Read: should recover the same bit pattern
    let result = crate::primitives::ctx::with_test_ctx(|ctx| {
        read_value_from_buffer(buf.as_mut_ptr(), &TypeDesc::U64, ctx)
    })
    .unwrap();
    let recovered = result.as_int().unwrap() as u64;
    assert_eq!(recovered, large);
}

#[test]
fn test_u64_roundtrip_boundary() {
    // i64::MAX + 1 wraps to i64::MIN; verify the u64 roundtrip is lossless.
    let boundary: u64 = i64::MAX as u64 + 1; // 0x8000000000000000
    let buf = AlignedBuffer::new(8, 8);

    let val = Value::int(boundary as i64);
    write_value_to_buffer(buf.as_mut_ptr(), &val, &TypeDesc::U64).unwrap();

    let result = crate::primitives::ctx::with_test_ctx(|ctx| {
        read_value_from_buffer(buf.as_mut_ptr(), &TypeDesc::U64, ctx)
    })
    .unwrap();
    let recovered = result.as_int().unwrap() as u64;
    assert_eq!(recovered, boundary);
}

#[test]
fn test_read_write_nested_struct_roundtrip() {
    crate::value::arena::with_test_region(|| {
        let inner_sd = StructDesc {
            fields: vec![TypeDesc::I8, TypeDesc::I32],
        };
        let outer_sd = StructDesc {
            fields: vec![TypeDesc::I64, TypeDesc::Struct(inner_sd)],
        };
        let desc = TypeDesc::Struct(outer_sd.clone());

        let h = crate::primitives::ctx::TestHeap::new();
        let inner_val = h.ctx().array_mut(vec![Value::int(7), Value::int(999)]);
        let outer_val = h.ctx().array_mut(vec![Value::int(123456), inner_val]);

        let m = MarshalledArg::new(&outer_val, &desc).unwrap();
        let _ = m.as_arg();

        let (offsets, total_size) = outer_sd.field_offsets().unwrap();
        let align = desc.align().unwrap();
        let buf = AlignedBuffer::new(total_size, align);

        let arr = outer_val.as_array_mut().unwrap();
        let elems = arr.borrow();
        for (i, (field_desc, &offset)) in outer_sd.fields.iter().zip(offsets.iter()).enumerate() {
            let _ = write_value_to_buffer(
                unsafe { buf.as_mut_ptr().add(offset) },
                &elems[i],
                field_desc,
            )
            .unwrap();
        }

        let result =
            read_in_region(|ctx| read_value_from_buffer(buf.as_mut_ptr(), &desc, ctx)).unwrap();
        let result_elems = result.as_array().unwrap();
        assert_eq!(result_elems[0].as_int(), Some(123456));

        let inner_elems = result_elems[1].as_array().unwrap();
        assert_eq!(inner_elems[0].as_int(), Some(7));
        assert_eq!(inner_elems[1].as_int(), Some(999));
    });
}

#[test]
fn test_as_arg_does_not_panic() {
    crate::value::arena::with_test_region(|| {
        let val = Value::int(42);
        let m = MarshalledArg::new(&val, &TypeDesc::I32).unwrap();
        let _ = m.as_arg();

        let fval = Value::float(1.5);
        let m2 = MarshalledArg::new(&fval, &TypeDesc::Double).unwrap();
        let _ = m2.as_arg();

        let h = crate::primitives::ctx::TestHeap::new();
        let sval = h.ctx().string("test");
        let m3 = MarshalledArg::new(&sval, &TypeDesc::Str).unwrap();
        let _ = m3.as_arg();
    });
}
