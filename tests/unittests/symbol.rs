// DEFENSE: Symbol interning must be fast and correct
use elle::symbol::SymbolTable;

#[test]
fn test_symbol_interning_basic() {
    let mut table = SymbolTable::new();

    let id1 = table.intern("foo");
    let id2 = table.intern("bar");
    let id3 = table.intern("foo"); // Same as id1

    assert_eq!(id1, id3);
    assert_ne!(id1, id2);
}

#[test]
fn test_symbol_names() {
    let mut table = SymbolTable::new();

    let id = table.intern("hello");
    assert_eq!(table.name(id), Some("hello"));
}

#[test]
fn test_symbol_lookup() {
    let mut table = SymbolTable::new();

    // Intern first
    let id1 = table.intern("test");

    // Lookup should return same ID
    let id2 = table.get("test");
    assert_eq!(Some(id1), id2);

    // Unknown symbol
    assert_eq!(None, table.get("unknown"));
}

#[test]
fn test_many_symbols() {
    let mut table = SymbolTable::new();

    // Intern 1000 unique symbols
    let ids: Vec<_> = (0..1000)
        .map(|i| table.intern(&format!("symbol-{}", i)))
        .collect();

    // All should be unique
    for i in 0..1000 {
        for j in 0..1000 {
            if i == j {
                assert_eq!(ids[i], ids[j]);
            } else {
                assert_ne!(ids[i], ids[j]);
            }
        }
    }
}

#[test]
fn test_symbol_persistence() {
    let mut table = SymbolTable::new();

    let id = table.intern("persistent");

    // Add more symbols
    for i in 0..100 {
        table.intern(&format!("sym-{}", i));
    }

    // Original symbol should still be valid
    assert_eq!(table.name(id), Some("persistent"));
    assert_eq!(table.intern("persistent"), id);
}

#[test]
fn test_special_characters() {
    let mut table = SymbolTable::new();

    // Lisp allows many special characters in symbols
    let symbols = vec![
        "+",
        "-",
        "*",
        "/",
        "=",
        "<",
        ">",
        "<=",
        ">=",
        "!=",
        "list?",
        "nil?",
        "number?",
        "some-func-name",
        "CamelCase",
        "with_underscores",
    ];

    for sym in symbols {
        let id = table.intern(sym);
        assert_eq!(table.name(id), Some(sym));
    }
}

#[test]
fn test_empty_symbol_table() {
    let table = SymbolTable::new();

    // Invalid ID should return None
    assert_eq!(table.name(elle::value::SymbolId(0)), None);
    assert_eq!(table.name(elle::value::SymbolId(9999)), None);
}

#[test]
fn test_symbol_ordering() {
    let mut table = SymbolTable::new();

    let id0 = table.intern("first");
    let id1 = table.intern("second");
    let id2 = table.intern("third");

    // IDs should be assigned sequentially
    assert_eq!(id0.0 + 1, id1.0);
    assert_eq!(id1.0 + 1, id2.0);
}

#[test]
fn test_unicode_symbols() {
    let mut table = SymbolTable::new();

    let id = table.intern("λ");
    assert_eq!(table.name(id), Some("λ"));

    let id2 = table.intern("こんにちは");
    assert_eq!(table.name(id2), Some("こんにちは"));
}

#[test]
fn test_long_symbol_names() {
    let mut table = SymbolTable::new();

    let long_name = "a".repeat(1000);
    let id = table.intern(&long_name);
    assert_eq!(table.name(id), Some(long_name.as_str()));
}
