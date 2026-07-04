//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn test_symbol_interning() {
    let mut table = SymbolTable::new();
    let id1 = table.intern("foo");
    let id2 = table.intern("bar");
    let id3 = table.intern("foo");

    assert_eq!(id1, id3);
    assert_ne!(id1, id2);
    assert_eq!(table.name(id1), Some("foo"));
    assert_eq!(table.name(id2), Some("bar"));
}
