//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn test_arity_matches() {
    assert!(Arity::Exact(2).matches(2));
    assert!(!Arity::Exact(2).matches(1));
    assert!(!Arity::Exact(2).matches(3));

    assert!(Arity::AtLeast(2).matches(2));
    assert!(Arity::AtLeast(2).matches(3));
    assert!(!Arity::AtLeast(2).matches(1));

    assert!(Arity::Range(1, 3).matches(1));
    assert!(Arity::Range(1, 3).matches(2));
    assert!(Arity::Range(1, 3).matches(3));
    assert!(!Arity::Range(1, 3).matches(0));
    assert!(!Arity::Range(1, 3).matches(4));
}

#[test]
fn test_arity_display() {
    assert_eq!(format!("{}", Arity::Exact(2)), "2");
    assert_eq!(format!("{}", Arity::AtLeast(1)), "1+");
    assert_eq!(format!("{}", Arity::Range(1, 3)), "1-3");
}

#[test]
fn test_symbol_id_display() {
    assert_eq!(format!("{}", SymbolId(42)), "Symbol(42)");
}
