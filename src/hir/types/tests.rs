//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn join_same_type() {
    let i = TypeInterner::new();
    assert_eq!(
        i.join(TypeInterner::INT, TypeInterner::INT),
        TypeInterner::INT
    );
}

#[test]
fn join_int_float_is_number() {
    let i = TypeInterner::new();
    assert_eq!(
        i.join(TypeInterner::INT, TypeInterner::FLOAT),
        TypeInterner::NUMBER
    );
}

#[test]
fn join_int_string_is_top() {
    let i = TypeInterner::new();
    assert_eq!(
        i.join(TypeInterner::INT, TypeInterner::STRING),
        TypeInterner::TOP
    );
}

#[test]
fn join_bottom_t() {
    let i = TypeInterner::new();
    assert_eq!(
        i.join(TypeInterner::BOTTOM, TypeInterner::STRING),
        TypeInterner::STRING
    );
}

#[test]
fn subtype_int_number() {
    let i = TypeInterner::new();
    assert!(i.subtype(TypeInterner::INT, TypeInterner::NUMBER));
}

#[test]
fn subtype_number_not_int() {
    let i = TypeInterner::new();
    assert!(!i.subtype(TypeInterner::NUMBER, TypeInterner::INT));
}

#[test]
fn meet_number_int() {
    let i = TypeInterner::new();
    assert_eq!(
        i.meet(TypeInterner::NUMBER, TypeInterner::INT),
        TypeInterner::INT
    );
}

#[test]
fn meet_int_string_is_bottom() {
    let i = TypeInterner::new();
    assert_eq!(
        i.meet(TypeInterner::INT, TypeInterner::STRING),
        TypeInterner::BOTTOM
    );
}

#[test]
fn is_immediate_int() {
    let i = TypeInterner::new();
    assert!(i.is_immediate(TypeInterner::INT));
    assert!(i.is_immediate(TypeInterner::FLOAT));
    assert!(i.is_immediate(TypeInterner::BOOL));
    assert!(!i.is_immediate(TypeInterner::STRING));
    assert!(!i.is_immediate(TypeInterner::TOP));
}

#[test]
fn join_array_mutable_array_is_top() {
    let i = TypeInterner::new();
    assert_eq!(
        i.join(TypeInterner::ARRAY, TypeInterner::MUTABLE_ARRAY),
        TypeInterner::TOP
    );
}

#[test]
fn subtype_mutable_array_top() {
    let i = TypeInterner::new();
    assert!(i.subtype(TypeInterner::MUTABLE_ARRAY, TypeInterner::TOP));
}

#[test]
fn is_immediate_array_false() {
    let i = TypeInterner::new();
    assert!(!i.is_immediate(TypeInterner::ARRAY));
    assert!(!i.is_immediate(TypeInterner::MUTABLE_ARRAY));
    assert!(!i.is_immediate(TypeInterner::STRUCT));
    assert!(!i.is_immediate(TypeInterner::MUTABLE_STRUCT));
}

#[test]
fn is_stringifiable() {
    let i = TypeInterner::new();
    assert!(i.is_stringifiable(TypeInterner::INT));
    assert!(i.is_stringifiable(TypeInterner::STRING));
    assert!(i.is_stringifiable(TypeInterner::BOOL));
    assert!(i.is_stringifiable(TypeInterner::NIL));
    assert!(i.is_stringifiable(TypeInterner::KEYWORD));
    assert!(!i.is_stringifiable(TypeInterner::TOP));
    assert!(!i.is_stringifiable(TypeInterner::ARRAY));
}

#[test]
fn is_struct() {
    let i = TypeInterner::new();
    assert!(i.is_struct(TypeInterner::STRUCT));
    assert!(i.is_struct(TypeInterner::MUTABLE_STRUCT));
    assert!(!i.is_struct(TypeInterner::ARRAY));
    assert!(!i.is_struct(TypeInterner::TOP));
}
