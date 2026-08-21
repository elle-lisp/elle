//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn test_builtin_arity() {
    use crate::value::Arity;
    // +, pair moved to stdlib; test with remaining Rust primitives
    assert_eq!(builtin_arity("length"), Some(Arity::Exact(1)));
    assert_eq!(builtin_arity("list"), Some(Arity::AtLeast(0)));
    assert_eq!(builtin_arity("undefined"), None);
}

#[test]
fn test_variadic_builtins_no_false_w002() {
    // list is variadic (AtLeast(0)); calling with multiple args must not produce W002
    let mut symbols = crate::SymbolTable::new();
    let mut diagnostics = Vec::new();

    let list = symbols.intern("list");
    check_call_arity(list, 3, &None, &symbols, &mut diagnostics);
    assert!(
        diagnostics.is_empty(),
        "W002 false positive for (list 1 2 3)"
    );

    check_call_arity(list, 5, &None, &symbols, &mut diagnostics);
    assert!(
        diagnostics.is_empty(),
        "W002 false positive for (list 1 2 3 4 5)"
    );
}

#[test]
fn test_exact_arity_still_warns() {
    // length expects exactly 1 arg
    let mut symbols = crate::SymbolTable::new();
    let mut diagnostics = Vec::new();

    let length = symbols.intern("length");
    check_call_arity(length, 0, &None, &symbols, &mut diagnostics);
    assert_eq!(diagnostics.len(), 1, "W002 should fire for (length)");

    diagnostics.clear();
    check_call_arity(length, 2, &None, &symbols, &mut diagnostics);
    assert_eq!(diagnostics.len(), 1, "W002 should fire for (length 1 2)");
}
