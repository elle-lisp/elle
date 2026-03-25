//! Marshalling between Elle Values and C-typed data for libffi calls.
//!
//! Shared types and helpers. Value→C conversion is in `to_c.rs`,
//! C→Value conversion is in `from_c.rs`.

use crate::error::{LError, LResult};
use crate::ffi::types::TypeDesc;
use crate::value::Value;
use libffi::middle::Type;

// Re-export moved items so existing callers don't break.
pub(crate) use crate::ffi::from_c::read_value_from_buffer;
pub(crate) use crate::ffi::to_c::write_value_to_buffer;
pub use crate::ffi::to_c::MarshalledArg;

/// Convert a `TypeDesc` to the corresponding `libffi::middle::Type`.
pub(crate) fn to_libffi_type(desc: &TypeDesc) -> Type {
    match desc {
        TypeDesc::Void => Type::void(),
        TypeDesc::Bool => Type::c_int(),
        TypeDesc::I8 => Type::i8(),
        TypeDesc::U8 => Type::u8(),
        TypeDesc::I16 => Type::i16(),
        TypeDesc::U16 => Type::u16(),
        TypeDesc::I32 => Type::i32(),
        TypeDesc::U32 => Type::u32(),
        TypeDesc::I64 => Type::i64(),
        TypeDesc::U64 => Type::u64(),
        TypeDesc::Float => Type::f32(),
        TypeDesc::Double => Type::f64(),
        TypeDesc::Int => Type::c_int(),
        TypeDesc::UInt => Type::c_uint(),
        TypeDesc::Long => Type::c_long(),
        TypeDesc::ULong => Type::c_ulong(),
        TypeDesc::Char => Type::i8(),
        TypeDesc::UChar => Type::u8(),
        TypeDesc::Short => Type::c_short(),
        TypeDesc::UShort => Type::c_ushort(),
        TypeDesc::Size => Type::usize(),
        TypeDesc::SSize => Type::isize(),
        TypeDesc::Ptr | TypeDesc::Str => Type::pointer(),
        TypeDesc::Struct(desc) => {
            let fields: Vec<Type> = desc.fields.iter().map(to_libffi_type).collect();
            Type::structure(fields)
        }
        TypeDesc::Array(elem, count) => {
            let elem_type = to_libffi_type(elem);
            let fields: Vec<Type> = (0..*count).map(|_| elem_type.clone()).collect();
            Type::structure(fields)
        }
    }
}

// ── Aligned buffer ──────────────────────────────────────────────────

/// Heap-allocated buffer with guaranteed alignment for FFI struct data.
///
/// Used to hold C struct/array data that libffi reads from (arguments)
/// or writes into (return values). The buffer is zero-initialized.
pub(crate) struct AlignedBuffer {
    ptr: *mut u8,
    layout: std::alloc::Layout,
}

impl AlignedBuffer {
    /// Allocate a zero-initialized buffer of `size` bytes with `align` alignment.
    ///
    /// Panics if align is 0 or not a power of two, or if size overflows
    /// the layout constraints.
    pub(crate) fn new(size: usize, align: usize) -> Self {
        // std::alloc::alloc with size 0 is UB; use at least 1 byte.
        let effective_size = size.max(1);
        let layout = std::alloc::Layout::from_size_align(effective_size, align)
            .expect("invalid layout for AlignedBuffer");
        let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
        if ptr.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        AlignedBuffer { ptr, layout }
    }

    /// Raw pointer to the buffer data.
    pub(crate) fn as_mut_ptr(&self) -> *mut u8 {
        self.ptr
    }
}

impl Drop for AlignedBuffer {
    fn drop(&mut self) {
        unsafe {
            std::alloc::dealloc(self.ptr, self.layout);
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────

pub(crate) fn extract_int(value: &Value, type_name: &str) -> LResult<i64> {
    value.as_int().ok_or_else(|| {
        LError::ffi_type_error(
            type_name,
            format!("expected integer, got {}", value.type_name()),
        )
    })
}

pub(crate) fn range_check(n: i64, min: i64, max: i64, type_name: &str) -> LResult<()> {
    if n < min || n > max {
        Err(LError::ffi_type_error(
            type_name,
            format!("value {} out of range [{}, {}]", n, min, max),
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn desc_name(desc: &TypeDesc) -> &'static str {
    match desc {
        TypeDesc::UChar => "uchar",
        TypeDesc::U8 => "u8",
        _ => "unknown",
    }
}

pub(crate) fn desc_name_full(desc: &TypeDesc) -> &'static str {
    match desc {
        TypeDesc::I8 => "i8",
        TypeDesc::U8 => "u8",
        TypeDesc::I16 => "i16",
        TypeDesc::U16 => "u16",
        TypeDesc::I32 => "i32",
        TypeDesc::U32 => "u32",
        TypeDesc::I64 => "i64",
        TypeDesc::U64 => "u64",
        TypeDesc::Int => "int",
        TypeDesc::UInt => "uint",
        TypeDesc::Long => "long",
        TypeDesc::ULong => "ulong",
        TypeDesc::Char => "char",
        TypeDesc::UChar => "uchar",
        TypeDesc::Short => "short",
        TypeDesc::UShort => "ushort",
        TypeDesc::Size => "size",
        TypeDesc::SSize => "ssize",
        TypeDesc::Float => "float",
        TypeDesc::Double => "double",
        TypeDesc::Ptr => "ptr",
        TypeDesc::Str => "string",
        TypeDesc::Bool => "bool",
        TypeDesc::Void => "void",
        TypeDesc::Struct(_) => "struct",
        TypeDesc::Array(_, _) => "array",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::types::StructDesc;

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
        let val = Value::string("hello");
        assert!(MarshalledArg::new(&val, &TypeDesc::Str).is_ok());
    }

    #[test]
    fn test_marshal_string_interior_null() {
        let val = Value::string("hel\0lo");
        assert!(MarshalledArg::new(&val, &TypeDesc::Str).is_err());
    }

    #[test]
    fn test_marshal_void_error() {
        let val = Value::NIL;
        assert!(MarshalledArg::new(&val, &TypeDesc::Void).is_err());
    }

    #[test]
    fn test_marshal_struct() {
        let desc = TypeDesc::Struct(StructDesc {
            fields: vec![TypeDesc::I32, TypeDesc::Double],
        });
        let val = Value::array_mut(vec![Value::int(42), Value::float(1.5)]);
        let m = MarshalledArg::new(&val, &desc).unwrap();
        let _ = m.as_arg(); // Should not panic
    }

    #[test]
    fn test_marshal_struct_wrong_count() {
        let desc = TypeDesc::Struct(StructDesc {
            fields: vec![TypeDesc::I32, TypeDesc::Double],
        });
        let val = Value::array_mut(vec![Value::int(42)]); // Only 1 value for 2 fields
        assert!(MarshalledArg::new(&val, &desc).is_err());
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
        let desc = TypeDesc::Array(Box::new(TypeDesc::I32), 3);
        let val = Value::array_mut(vec![Value::int(1), Value::int(2), Value::int(3)]);
        let m = MarshalledArg::new(&val, &desc).unwrap();
        let _ = m.as_arg();
    }

    #[test]
    fn test_marshal_array_wrong_count() {
        let desc = TypeDesc::Array(Box::new(TypeDesc::I32), 3);
        let val = Value::array_mut(vec![Value::int(1), Value::int(2)]);
        assert!(MarshalledArg::new(&val, &desc).is_err());
    }

    #[test]
    fn test_read_write_struct_roundtrip() {
        let sd = StructDesc {
            fields: vec![TypeDesc::I32, TypeDesc::Double, TypeDesc::I64],
        };
        let desc = TypeDesc::Struct(sd.clone());
        let values = Value::array_mut(vec![Value::int(42), Value::float(1.5), Value::int(-100)]);

        let (offsets, total_size) = sd.field_offsets().unwrap();
        let align = desc.align().unwrap();
        let buf = AlignedBuffer::new(total_size, align);

        // Write each field
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

        // Read back — returns immutable array
        let result = read_value_from_buffer(buf.as_mut_ptr(), &desc).unwrap();
        let result_elems = result.as_array().unwrap();
        assert_eq!(result_elems[0].as_int(), Some(42));
        assert!((result_elems[1].as_float().unwrap() - 1.5).abs() < 1e-10);
        assert_eq!(result_elems[2].as_int(), Some(-100));
    }

    #[test]
    fn test_read_write_array_roundtrip() {
        let desc = TypeDesc::Array(Box::new(TypeDesc::I32), 4);
        let values = Value::array_mut(vec![
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

        // i32 array → returns immutable array (not bytes, since element type is i32)
        let result = read_value_from_buffer(buf.as_mut_ptr(), &desc).unwrap();
        let result_elems = result.as_array().unwrap();
        assert_eq!(result_elems.len(), 4);
        assert_eq!(result_elems[0].as_int(), Some(10));
        assert_eq!(result_elems[1].as_int(), Some(20));
        assert_eq!(result_elems[2].as_int(), Some(30));
        assert_eq!(result_elems[3].as_int(), Some(40));
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
        let result = read_value_from_buffer(buf.as_mut_ptr(), &TypeDesc::U64).unwrap();
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

        let result = read_value_from_buffer(buf.as_mut_ptr(), &TypeDesc::U64).unwrap();
        let recovered = result.as_int().unwrap() as u64;
        assert_eq!(recovered, boundary);
    }

    #[test]
    fn test_read_write_nested_struct_roundtrip() {
        let inner_sd = StructDesc {
            fields: vec![TypeDesc::I8, TypeDesc::I32],
        };
        let outer_sd = StructDesc {
            fields: vec![TypeDesc::I64, TypeDesc::Struct(inner_sd)],
        };
        let desc = TypeDesc::Struct(outer_sd.clone());

        let inner_val = Value::array_mut(vec![Value::int(7), Value::int(999)]);
        let outer_val = Value::array_mut(vec![Value::int(123456), inner_val]);

        // Marshal via MarshalledArg
        let m = MarshalledArg::new(&outer_val, &desc).unwrap();
        let _ = m.as_arg();

        // Also test roundtrip through write/read
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

        let result = read_value_from_buffer(buf.as_mut_ptr(), &desc).unwrap();
        let result_elems = result.as_array().unwrap();
        assert_eq!(result_elems[0].as_int(), Some(123456));

        let inner_elems = result_elems[1].as_array().unwrap();
        assert_eq!(inner_elems[0].as_int(), Some(7));
        assert_eq!(inner_elems[1].as_int(), Some(999));
    }

    #[test]
    fn test_as_arg_does_not_panic() {
        let val = Value::int(42);
        let m = MarshalledArg::new(&val, &TypeDesc::I32).unwrap();
        let _ = m.as_arg();

        let fval = Value::float(1.5);
        let m2 = MarshalledArg::new(&fval, &TypeDesc::Double).unwrap();
        let _ = m2.as_arg();

        let sval = Value::string("test");
        let m3 = MarshalledArg::new(&sval, &TypeDesc::Str).unwrap();
        let _ = m3.as_arg();
    }
}
