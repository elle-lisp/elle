use elle::value::types::TableKey;
use elle::Value;

// ── from_value / to_value roundtrip ─────────────────────────────

#[test]
fn test_from_value_nil() {
    let key = TableKey::from_value(&Value::NIL).unwrap();
    assert_eq!(key, TableKey::Nil);
    assert_eq!(key.to_value(), Value::NIL);
}

#[test]
fn test_from_value_bool() {
    let key = TableKey::from_value(&Value::TRUE).unwrap();
    assert_eq!(key, TableKey::Bool(true));
    assert_eq!(key.to_value(), Value::TRUE);
}

#[test]
fn test_from_value_int() {
    let key = TableKey::from_value(&Value::int(42)).unwrap();
    assert_eq!(key, TableKey::Int(42));
    assert_eq!(key.to_value(), Value::int(42));
}

#[test]
fn test_from_value_keyword() {
    let val = Value::keyword("foo");
    let key = TableKey::from_value(&val).unwrap();
    assert!(matches!(key, TableKey::Keyword(ref s) if s == "foo"));
    // to_value produces an equivalent keyword
    assert_eq!(key.to_value().as_keyword_name().unwrap(), "foo");
}

#[test]
fn test_from_value_string() {
    let val = Value::string("hello");
    let key = TableKey::from_value(&val).unwrap();
    assert!(matches!(key, TableKey::String(ref s) if s == "hello"));
}

// ── Identity keys ───────────────────────────────────────────────

#[test]
fn test_from_value_external() {
    let ext = Value::external("test-type", 42u32);
    let key = TableKey::from_value(&ext);
    assert!(key.is_some(), "external should be accepted as key");
    let key = key.unwrap();
    assert!(matches!(key, TableKey::Identity(_)));
}

#[test]
fn test_external_key_roundtrip() {
    let ext = Value::external("test-type", 42u32);
    let key = TableKey::from_value(&ext).unwrap();
    let roundtripped = key.to_value();
    // Must be the exact same Value (bit-identical pointer)
    assert_eq!(
        roundtripped.to_bits(),
        ext.to_bits(),
        "to_value must return the original Value"
    );
}

#[test]
fn test_different_externals_produce_different_keys() {
    let ext1 = Value::external("test-type", 1u32);
    let ext2 = Value::external("test-type", 2u32);
    let key1 = TableKey::from_value(&ext1).unwrap();
    let key2 = TableKey::from_value(&ext2).unwrap();
    assert_ne!(key1, key2, "different externals must be different keys");
}

#[test]
fn test_same_external_produces_equal_key() {
    let ext = Value::external("test-type", 42u32);
    let key1 = TableKey::from_value(&ext).unwrap();
    let key2 = TableKey::from_value(&ext).unwrap();
    assert_eq!(key1, key2, "same external must produce equal keys");
}

// ── Rejected types ──────────────────────────────────────────────

#[test]
fn test_from_value_array_rejected() {
    let val = Value::array(vec![Value::int(1)]);
    assert!(TableKey::from_value(&val).is_none());
}

#[test]
fn test_from_value_table_rejected() {
    let val = Value::table();
    assert!(TableKey::from_value(&val).is_none());
}

// ── is_sendable ─────────────────────────────────────────────────

#[test]
fn test_is_sendable_value_keys() {
    assert!(TableKey::Nil.is_sendable());
    assert!(TableKey::Bool(true).is_sendable());
    assert!(TableKey::Int(42).is_sendable());
    assert!(TableKey::String("hello".to_string()).is_sendable());
    assert!(TableKey::Keyword("foo".to_string()).is_sendable());
}

#[test]
fn test_is_sendable_identity_key() {
    let ext = Value::external("test-type", 42u32);
    let key = TableKey::from_value(&ext).unwrap();
    assert!(!key.is_sendable(), "identity keys must not be sendable");
}
