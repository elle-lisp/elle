//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn test_keyword_parsing() {
    assert_eq!(TypeDesc::from_keyword("void"), Some(TypeDesc::Void));
    assert_eq!(TypeDesc::from_keyword("i32"), Some(TypeDesc::I32));
    assert_eq!(TypeDesc::from_keyword("ptr"), Some(TypeDesc::Ptr));
    assert_eq!(TypeDesc::from_keyword("string"), Some(TypeDesc::Str));
    assert_eq!(TypeDesc::from_keyword("size"), Some(TypeDesc::Size));
    assert_eq!(TypeDesc::from_keyword("nonsense"), None);
}

#[test]
fn test_primitive_sizes() {
    assert_eq!(TypeDesc::Void.size(), None);
    assert_eq!(TypeDesc::I8.size(), Some(1));
    assert_eq!(TypeDesc::I16.size(), Some(2));
    assert_eq!(TypeDesc::I32.size(), Some(4));
    assert_eq!(TypeDesc::I64.size(), Some(8));
    assert_eq!(TypeDesc::Float.size(), Some(4));
    assert_eq!(TypeDesc::Double.size(), Some(8));
    assert_eq!(TypeDesc::Ptr.size(), Some(8)); // 64-bit platform
}

#[test]
fn test_struct_size_and_align() {
    // Two i32 fields: no padding needed
    let s = TypeDesc::Struct(StructDesc {
        fields: vec![TypeDesc::I32, TypeDesc::I32],
    });
    assert_eq!(s.size(), Some(8));
    assert_eq!(s.align(), Some(4));

    // i8 + i32: padding after i8
    let s2 = TypeDesc::Struct(StructDesc {
        fields: vec![TypeDesc::I8, TypeDesc::I32],
    });
    assert_eq!(s2.size(), Some(8)); // 1 + 3 padding + 4
    assert_eq!(s2.align(), Some(4));
}

#[test]
fn test_array_size() {
    let a = TypeDesc::Array(Box::new(TypeDesc::I32), 10);
    assert_eq!(a.size(), Some(40));
    assert_eq!(a.align(), Some(4));
}

#[test]
fn test_calling_convention() {
    assert_eq!(
        CallingConvention::from_keyword("default"),
        Some(CallingConvention::Default)
    );
    assert_eq!(CallingConvention::from_keyword("sysv64"), None);
}

#[test]
fn test_field_offsets_simple() {
    let desc = StructDesc {
        fields: vec![TypeDesc::I32, TypeDesc::I32],
    };
    let (offsets, total) = desc.field_offsets().unwrap();
    assert_eq!(offsets, vec![0, 4]);
    assert_eq!(total, 8);
}

#[test]
fn test_field_offsets_padding() {
    // i8 at 0, then i32 needs 4-byte alignment → padding
    let desc = StructDesc {
        fields: vec![TypeDesc::I8, TypeDesc::I32],
    };
    let (offsets, total) = desc.field_offsets().unwrap();
    assert_eq!(offsets, vec![0, 4]);
    assert_eq!(total, 8);
}

#[test]
fn test_field_offsets_tail_padding() {
    // i32 at 0, i8 at 4, tail padding to align to 4
    let desc = StructDesc {
        fields: vec![TypeDesc::I32, TypeDesc::I8],
    };
    let (offsets, total) = desc.field_offsets().unwrap();
    assert_eq!(offsets, vec![0, 4]);
    assert_eq!(total, 8); // 5 bytes + 3 padding
}

#[test]
fn test_field_offsets_mixed() {
    // i8 at 0, double at 8, i32 at 16, ptr at 24
    let desc = StructDesc {
        fields: vec![TypeDesc::I8, TypeDesc::Double, TypeDesc::I32, TypeDesc::Ptr],
    };
    let (offsets, total) = desc.field_offsets().unwrap();
    assert_eq!(offsets, vec![0, 8, 16, 24]);
    assert_eq!(total, 32);
}

#[test]
fn test_field_offsets_empty() {
    let desc = StructDesc { fields: vec![] };
    let (offsets, total) = desc.field_offsets().unwrap();
    assert_eq!(offsets, Vec::<usize>::new());
    assert_eq!(total, 0);
}

#[test]
fn test_field_offsets_nested_struct() {
    let inner = StructDesc {
        fields: vec![TypeDesc::I8, TypeDesc::I32],
    };
    // inner struct is 8 bytes, align 4
    let outer = StructDesc {
        fields: vec![TypeDesc::I8, TypeDesc::Struct(inner)],
    };
    let (offsets, total) = outer.field_offsets().unwrap();
    assert_eq!(offsets, vec![0, 4]); // inner aligns to 4
    assert_eq!(total, 12); // 4 + 8 = 12
}
