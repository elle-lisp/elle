//! Unit tests (`super` is the parent impl module).

use crate::primitives::register_primitives;
use crate::reader::read_syntax;
use crate::symbol::SymbolTable;
use crate::syntax::Expander;
use crate::vm::VM;

fn setup() -> (SymbolTable, VM) {
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    register_primitives(&mut vm, &mut symbols);
    (symbols, vm)
}

#[test]
fn arity_error_no_args() {
    crate::value::arena::with_test_region(|| {
        let mut expander = Expander::new();
        let (mut symbols, mut vm) = setup();
        expander.load_prelude(&mut symbols, &mut vm).unwrap();

        let syn = read_syntax("(syntax-case)", "<test>").unwrap();
        let result = expander.expand(syn, &mut symbols, &mut vm);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("syntax-case requires"));
    });
}

#[test]
fn arity_error_no_clauses() {
    crate::value::arena::with_test_region(|| {
        let mut expander = Expander::new();
        let (mut symbols, mut vm) = setup();
        expander.load_prelude(&mut symbols, &mut vm).unwrap();

        let syn = read_syntax("(syntax-case stx)", "<test>").unwrap();
        let result = expander.expand(syn, &mut symbols, &mut vm);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("syntax-case requires"));
    });
}

#[test]
fn bad_clause_not_list() {
    crate::value::arena::with_test_region(|| {
        let mut expander = Expander::new();
        let (mut symbols, mut vm) = setup();
        expander.load_prelude(&mut symbols, &mut vm).unwrap();

        let syn = read_syntax("(syntax-case stx 42)", "<test>").unwrap();
        let result = expander.expand(syn, &mut symbols, &mut vm);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("clause must be a list"));
    });
}

#[test]
fn duplicate_pattern_variable() {
    crate::value::arena::with_test_region(|| {
        let mut expander = Expander::new();
        let (mut symbols, mut vm) = setup();
        expander.load_prelude(&mut symbols, &mut vm).unwrap();

        let syn = read_syntax("(syntax-case stx ((x x) :body))", "<test>").unwrap();
        let result = expander.expand(syn, &mut symbols, &mut vm);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("duplicate pattern variable"));
    });
}

#[test]
fn literal_wrong_arity() {
    crate::value::arena::with_test_region(|| {
        let mut expander = Expander::new();
        let (mut symbols, mut vm) = setup();
        expander.load_prelude(&mut symbols, &mut vm).unwrap();

        let syn = read_syntax("(syntax-case stx ((literal) :body))", "<test>").unwrap();
        let result = expander.expand(syn, &mut symbols, &mut vm);
        assert!(result.is_err());
    });
}
