use super::*;

// =========================================================================
// A. Pointer tagged-union invariants
// =========================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(1000))]

    // Pointer roundtrip: any 47-bit address survives the Value round-trip
    #[test]
    fn pointer_roundtrip(addr in 1usize..=0x0000_7FFF_FFFF_FFFFusize) {
        let v = Value::pointer(addr);
        prop_assert_eq!(v.as_pointer(), Some(addr));
    }

    // Pointer type discrimination: pointers are ONLY pointers
    #[test]
    fn pointer_is_only_pointer(addr in 1usize..=0x0000_7FFF_FFFF_FFFFusize) {
        let v = Value::pointer(addr);
        prop_assert!(v.is_pointer());
        prop_assert!(!v.is_int());
        prop_assert!(!v.is_float());
        prop_assert!(!v.is_nil());
        prop_assert!(!v.is_bool());
        prop_assert!(!v.is_symbol());
        prop_assert!(!v.is_keyword());
        prop_assert!(!v.is_heap());
        prop_assert!(!v.is_empty_list());
    }

    // Pointer truthiness: all non-null pointers are truthy
    #[test]
    fn pointer_is_truthy(addr in 1usize..=0x0000_7FFF_FFFF_FFFFusize) {
        prop_assert!(Value::pointer(addr).is_truthy());
    }

    // Pointer equality: same address -> equal values
    #[test]
    fn pointer_eq_same_addr(addr in 1usize..=0x0000_7FFF_FFFF_FFFFusize) {
        prop_assert_eq!(Value::pointer(addr), Value::pointer(addr));
    }

    // Pointer inequality: different addresses -> different values
    #[test]
    fn pointer_neq_diff_addr(
        a in 1usize..=0x0000_7FFF_FFFF_FFFFusize,
        b in 1usize..=0x0000_7FFF_FFFF_FFFFusize,
    ) {
        prop_assume!(a != b);
        prop_assert_ne!(Value::pointer(a), Value::pointer(b));
    }
}

// NULL pointer becomes NIL (not inside proptest -- deterministic)
#[test]
fn pointer_null_is_nil() {
    assert_eq!(Value::pointer(0), Value::NIL);
}

// NIL is NOT a pointer (as_pointer returns None)
#[test]
fn nil_is_not_pointer() {
    assert_eq!(Value::NIL.as_pointer(), None);
    assert!(!Value::NIL.is_pointer());
}

// =========================================================================
// B. Marshal integer range checking
// =========================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(500))]

    // i8 in-range: -128..127 accepted
    #[test]
    fn marshal_i8_in_range(n in -128i64..=127) {
        let v = Value::int(n);
        prop_assert!(MarshalledArg::new(&v, &TypeDesc::I8).is_ok());
    }

    // i8 out-of-range: rejected
    #[test]
    fn marshal_i8_out_of_range(n in prop_oneof![
        (i64::MIN..=-129i64),
        (128i64..=i64::MAX),
    ]) {
        let v = Value::int(n);
        prop_assert!(MarshalledArg::new(&v, &TypeDesc::I8).is_err());
    }

    // u8 in-range: 0..255 accepted
    #[test]
    fn marshal_u8_in_range(n in 0i64..=255) {
        let v = Value::int(n);
        prop_assert!(MarshalledArg::new(&v, &TypeDesc::U8).is_ok());
    }

    // u8 out-of-range: negative or >255
    #[test]
    fn marshal_u8_out_of_range(n in prop_oneof![
        (i64::MIN..=-1i64),
        (256i64..=i64::MAX),
    ]) {
        let v = Value::int(n);
        prop_assert!(MarshalledArg::new(&v, &TypeDesc::U8).is_err());
    }

    // i16 in-range
    #[test]
    fn marshal_i16_in_range(n in -32768i64..=32767) {
        let v = Value::int(n);
        prop_assert!(MarshalledArg::new(&v, &TypeDesc::I16).is_ok());
    }

    // u16 in-range
    #[test]
    fn marshal_u16_in_range(n in 0i64..=65535) {
        let v = Value::int(n);
        prop_assert!(MarshalledArg::new(&v, &TypeDesc::U16).is_ok());
    }

    // i32 in-range
    #[test]
    fn marshal_i32_in_range(n in i32::MIN as i64..=i32::MAX as i64) {
        let v = Value::int(n);
        prop_assert!(MarshalledArg::new(&v, &TypeDesc::I32).is_ok());
    }

    // i32 out-of-range
    #[test]
    fn marshal_i32_out_of_range(n in prop_oneof![
        (i64::MIN..=i32::MIN as i64 - 1),
        (i32::MAX as i64 + 1..=i64::MAX),
    ]) {
        let v = Value::int(n);
        prop_assert!(MarshalledArg::new(&v, &TypeDesc::I32).is_err());
    }

    // u32 in-range
    #[test]
    fn marshal_u32_in_range(n in 0i64..=u32::MAX as i64) {
        let v = Value::int(n);
        prop_assert!(MarshalledArg::new(&v, &TypeDesc::U32).is_ok());
    }

    // i64 always in-range (Elle ints are full i64)
    #[test]
    fn marshal_i64_always_ok(n in i64::MIN..=i64::MAX) {
        let v = Value::int(n);
        prop_assert!(MarshalledArg::new(&v, &TypeDesc::I64).is_ok());
    }

    // Float marshalling: any float is accepted
    #[test]
    fn marshal_float_from_float(f in prop::num::f64::NORMAL) {
        let v = Value::float(f);
        prop_assert!(MarshalledArg::new(&v, &TypeDesc::Float).is_ok());
        prop_assert!(MarshalledArg::new(&v, &TypeDesc::Double).is_ok());
    }

    // Float marshalling: integers also accepted as floats
    #[test]
    fn marshal_float_from_int(n in i64::MIN..=i64::MAX) {
        let v = Value::int(n);
        prop_assert!(MarshalledArg::new(&v, &TypeDesc::Float).is_ok());
        prop_assert!(MarshalledArg::new(&v, &TypeDesc::Double).is_ok());
    }

    // Bool marshalling: any value accepted (truthiness-based)
    #[test]
    fn marshal_bool_from_int(n in i64::MIN..=i64::MAX) {
        let v = Value::int(n);
        prop_assert!(MarshalledArg::new(&v, &TypeDesc::Bool).is_ok());
    }

    // Pointer marshalling: actual pointers accepted
    #[test]
    fn marshal_ptr_from_pointer(addr in 1usize..=0x0000_7FFF_FFFF_FFFFusize) {
        let v = Value::pointer(addr);
        prop_assert!(MarshalledArg::new(&v, &TypeDesc::Ptr).is_ok());
    }

    // Pointer marshalling: non-pointer/non-nil rejected
    #[test]
    fn marshal_ptr_from_int_rejected(n in i64::MIN..=i64::MAX) {
        let v = Value::int(n);
        prop_assert!(MarshalledArg::new(&v, &TypeDesc::Ptr).is_err());
    }
}

// Nil accepted as pointer (becomes NULL)
#[test]
fn marshal_ptr_nil_accepted() {
    assert!(MarshalledArg::new(&Value::NIL, &TypeDesc::Ptr).is_ok());
}
