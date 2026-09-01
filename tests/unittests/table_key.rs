use elle::primitives::ctx::with_test_ctx;
use elle::value::types::TableKey;
use elle::Value;

// ── from_value / to_value roundtrip ─────────────────────────────

#[test]
fn test_from_value_nil() {
    let key = TableKey::from_value(&Value::NIL).unwrap();
    assert_eq!(key, TableKey::Nil);
    assert_eq!(with_test_ctx(|ctx| key.to_value(ctx)), Value::NIL);
}

#[test]
fn test_from_value_bool() {
    let key = TableKey::from_value(&Value::TRUE).unwrap();
    assert_eq!(key, TableKey::Bool(true));
    assert_eq!(with_test_ctx(|ctx| key.to_value(ctx)), Value::TRUE);
}

#[test]
fn test_from_value_int() {
    let key = TableKey::from_value(&Value::int(42)).unwrap();
    assert_eq!(key, TableKey::Int(42));
    assert_eq!(with_test_ctx(|ctx| key.to_value(ctx)), Value::int(42));
}

#[test]
fn test_from_value_keyword() {
    let val = Value::keyword("foo");
    let key = TableKey::from_value(&val).unwrap();
    assert!(
        matches!(key, TableKey::Keyword(h) if h == elle::value::keyword::keyword_hash("foo"))
    );
    // to_value produces an equivalent keyword
    assert!(with_test_ctx(|ctx| key.to_value(ctx)).is_keyword_named("foo"));
}

#[test]
fn test_from_value_string() {
    let h = elle::primitives::ctx::TestHeap::new();
    let val = h.ctx().string("hello");
    let key = TableKey::from_value(&val).unwrap();
    assert!(matches!(key, TableKey::String(ref s) if s == "hello"));
}

// ── EmptyList key ───────────────────────────────────────────

#[test]
fn test_from_value_empty_list() {
    let key = TableKey::from_value(&Value::EMPTY_LIST).unwrap();
    assert_eq!(key, TableKey::EmptyList);
    assert_eq!(with_test_ctx(|ctx| key.to_value(ctx)), Value::EMPTY_LIST);
}

#[test]
fn test_empty_list_is_sendable() {
    assert!(TableKey::EmptyList.is_sendable());
}

// ── Heap keys ───────────────────────────────────────────────

#[test]
fn test_from_value_external() {
    let h = elle::primitives::ctx::TestHeap::new();
    let ext = h.ctx().external("test-type", 42u32);
    let key = TableKey::from_value(&ext);
    assert!(key.is_some(), "external should be accepted as key");
    let key = key.unwrap();
    assert!(matches!(key, TableKey::Heap(_)));
}

#[test]
fn test_external_key_roundtrip() {
    let h = elle::primitives::ctx::TestHeap::new();
    let ext = h.ctx().external("test-type", 42u32);
    let key = TableKey::from_value(&ext).unwrap();
    let roundtripped = with_test_ctx(|ctx| key.to_value(ctx));
    // Must be the exact same Value (same tag and pointer payload)
    assert_eq!(roundtripped, ext, "to_value must return the original Value");
}

#[test]
fn test_different_externals_produce_different_keys() {
    let h = elle::primitives::ctx::TestHeap::new();
    let ext1 = h.ctx().external("test-type", 1u32);
    let ext2 = h.ctx().external("test-type", 2u32);
    let key1 = TableKey::from_value(&ext1).unwrap();
    let key2 = TableKey::from_value(&ext2).unwrap();
    assert_ne!(key1, key2, "different externals must be different keys");
}

#[test]
fn test_same_external_produces_equal_key() {
    let h = elle::primitives::ctx::TestHeap::new();
    let ext = h.ctx().external("test-type", 42u32);
    let key1 = TableKey::from_value(&ext).unwrap();
    let key2 = TableKey::from_value(&ext).unwrap();
    assert_eq!(key1, key2, "same external must produce equal keys");
}

// ── Rejected types ──────────────────────────────────────────────

#[test]
fn test_from_value_array_rejected() {
    let h = elle::primitives::ctx::TestHeap::new();
    let val = h.ctx().array_mut(vec![Value::int(1)]);
    assert!(TableKey::from_value(&val).is_none());
}

#[test]
fn test_from_value_table_rejected() {
    let h = elle::primitives::ctx::TestHeap::new();
    let val = h.ctx().struct_mut();
    assert!(TableKey::from_value(&val).is_none());
}

// ── is_sendable ─────────────────────────────────────────────────

#[test]
fn test_is_sendable_value_keys() {
    assert!(TableKey::Nil.is_sendable());
    assert!(TableKey::Bool(true).is_sendable());
    assert!(TableKey::Int(42).is_sendable());
    assert!(TableKey::String("hello".to_string()).is_sendable());
    assert!(TableKey::keyword("foo").is_sendable());
    assert!(TableKey::EmptyList.is_sendable());
}

#[test]
fn test_is_sendable_heap_key() {
    let h = elle::primitives::ctx::TestHeap::new();
    let ext = h.ctx().external("test-type", 42u32);
    let key = TableKey::from_value(&ext).unwrap();
    assert!(!key.is_sendable(), "heap keys must not be sendable");
}
