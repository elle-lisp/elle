//! Unit tests (`super` is the parent impl module).

use crate::primitives::register_primitives;
use crate::reader::read_syntax;
use crate::symbol::SymbolTable;
use crate::syntax::{Expander, SyntaxArena};
use crate::vm::VM;

/// A symbol table, a VM, an expander over that VM's heap, and the working
/// arena the expander and the reader share for the test.
fn setup() -> (SymbolTable, VM, Expander, SyntaxArena) {
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    register_primitives(&mut vm, &mut symbols);
    let arena = SyntaxArena::mint(vm.heap());
    let mut expander = Expander::on_vm(&mut vm);
    expander.set_arena(arena);
    (symbols, vm, expander, arena)
}

#[test]
fn bfs_non_def_rejected() {
    crate::value::arena::with_test_region(|| {
        // (begin-for-syntax (+ 1 2)) must fail
        let (mut symbols, mut vm, mut expander, arena) = setup();
        expander.load_prelude(&mut symbols, &mut vm).unwrap();

        let src = "(begin-for-syntax (+ 1 2))";
        let syn = read_syntax(arena, src, "<test>").unwrap();
        let result = expander.expand(syn, &mut symbols, &mut vm);
        assert!(result.is_err(), "non-def form should be rejected");
        let msg = result.unwrap_err();
        assert!(
            msg.contains("begin-for-syntax"),
            "error should mention begin-for-syntax: {}",
            msg
        );
    });
}

#[test]
fn bfs_destructuring_def_rejected() {
    crate::value::arena::with_test_region(|| {
        // (begin-for-syntax (def (a b) 42)) must fail
        let (mut symbols, mut vm, mut expander, arena) = setup();
        expander.load_prelude(&mut symbols, &mut vm).unwrap();

        let src = "(begin-for-syntax (def (a b) 42))";
        let syn = read_syntax(arena, src, "<test>").unwrap();
        let result = expander.expand(syn, &mut symbols, &mut vm);
        assert!(result.is_err());
    });
}

#[test]
fn bfs_stores_value_in_env() {
    crate::value::arena::with_test_region(|| {
        // (begin-for-syntax (def my-val 42)) should store "my-val" in compile_time_env
        let (mut symbols, mut vm, mut expander, arena) = setup();
        expander.load_prelude(&mut symbols, &mut vm).unwrap();

        let src = "(begin-for-syntax (def my-val 42))";
        let syn = read_syntax(arena, src, "<test>").unwrap();
        expander.expand(syn, &mut symbols, &mut vm).unwrap();

        assert!(
            expander.compile_time_env.contains_key("my-val"),
            "compile_time_env should contain my-val"
        );
        let val = expander.compile_time_env["my-val"];
        assert_eq!(val, crate::value::Value::int(42));
    });
}

#[test]
fn bfs_clone_resets_env() {
    crate::value::arena::with_test_region(|| {
        // Cloning an Expander that has compile_time_env entries should
        // produce an Expander with an empty compile_time_env.
        let (mut symbols, mut vm, mut expander, arena) = setup();
        expander.load_prelude(&mut symbols, &mut vm).unwrap();

        let src = "(begin-for-syntax (def helper 99))";
        let syn = read_syntax(arena, src, "<test>").unwrap();
        expander.expand(syn, &mut symbols, &mut vm).unwrap();
        assert!(!expander.compile_time_env.is_empty());

        let cloned = expander.clone();
        assert!(
            cloned.compile_time_env.is_empty(),
            "cloned Expander should have empty compile_time_env"
        );
    });
}
