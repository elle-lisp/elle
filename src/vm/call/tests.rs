use super::*;
use crate::primitives::register_primitives;
use crate::symbol::SymbolTable;
use crate::value::Value;

fn make_vm_with_primitives() -> (VM, SymbolTable) {
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    register_primitives(&mut vm, &mut symbols);
    (vm, symbols)
}

/// Verify that call_closure with a trivial identity closure returns the argument.
#[test]
fn test_call_closure_identity() {
    use crate::pipeline::eval_syntax;
    use crate::syntax::Expander;

    let (mut vm, mut symbols) = make_vm_with_primitives();
    let arena = crate::syntax::SyntaxArena::mint(vm.heap());
    let mut expander = Expander::on_vm(&mut vm);
    expander.set_arena(arena);
    expander.load_prelude(&mut symbols, &mut vm).unwrap();

    // Compile (fn (x) x) to a closure
    let syntax = crate::reader::read_syntax(arena, "(fn (x) x)", "<test>").unwrap();
    let closure_val = eval_syntax(syntax, &mut expander, &mut symbols, &mut vm).unwrap();
    assert!(closure_val.as_closure().is_some(), "should be a closure");

    let arg = Value::int(42);
    let result = vm.call_closure(closure_val, &[arg]).unwrap();
    assert_eq!(result, Value::int(42));
}

/// Verify that call_closure propagates errors from the closure body.
#[test]
fn test_call_closure_error_propagation() {
    crate::value::arena::with_test_region(|| {
        use crate::pipeline::eval_syntax;
        use crate::syntax::Expander;

        let (mut vm, mut symbols) = make_vm_with_primitives();
        let arena = crate::syntax::SyntaxArena::mint(vm.heap());
        let mut expander = Expander::on_vm(&mut vm);
        expander.set_arena(arena);
        expander.set_eval_meta(crate::primitives::build_primitive_meta(&mut symbols));
        expander.load_prelude(&mut symbols, &mut vm).unwrap();

        // Compile (fn () (error "boom")) — always errors
        let syntax =
            crate::reader::read_syntax(arena, r#"(fn () (error "boom"))"#, "<test>").unwrap();
        let closure_val = eval_syntax(syntax, &mut expander, &mut symbols, &mut vm).unwrap();
        assert!(closure_val.as_closure().is_some(), "should be a closure");

        let result = vm.call_closure(closure_val, &[]);
        assert!(result.is_err(), "should propagate error from closure body");
    });
}

/// Verify that `vm.heap()` and `vm.heap_ptr` name the same heap instance
/// (pointer equality). The VM owns exactly one heap, reached via `vm.heap_ptr`,
/// so the heap a test reaches via the VM is `vm.heap_ptr`.
#[test]
fn test_vm_heap_is_tls_heap() {
    let mut vm = VM::new();
    let heap_ptr = vm.heap_ptr;
    let via_accessor: *const crate::value::fiberheap::FiberHeap = vm.heap() as *const _;
    assert_eq!(
        heap_ptr as usize, via_accessor as usize,
        "vm.heap() and vm.heap_ptr must name the same instance"
    );
}

/// Verify that a value allocated through a NativeCtx is visible on `vm.heap`'s
/// region store.
#[test]
fn test_native_alloc_visible_on_vm_heap() {
    let mut vm = VM::new();
    // Allocate a string through a ctx over a real runtime region minted from
    // the heap's pool (the native-call allocation discipline).
    let region = vm.heap().new_runtime_region();
    let val = {
        let ctx = crate::primitives::ctx::Alloc::with_region(region, unsafe { &mut *vm.heap_ptr });
        ctx.string("hello from native")
    };
    assert!(
        vm.heap().value_in_region_store(val),
        "NativeFn-allocated value must be visible on vm.heap"
    );
    vm.heap().decref_region_if_present(region);
}

/// Counterfactual: verify the identity test assertion fires if we break it.
#[test]
#[ignore = "counterfactual — run manually to verify assertion strength"]
fn test_call_closure_counterfactual() {
    use crate::pipeline::eval_syntax;
    use crate::syntax::Expander;

    let (mut vm, mut symbols) = make_vm_with_primitives();
    let arena = crate::syntax::SyntaxArena::mint(vm.heap());
    let mut expander = Expander::on_vm(&mut vm);
    expander.set_arena(arena);
    expander.load_prelude(&mut symbols, &mut vm).unwrap();

    let syntax = crate::reader::read_syntax(arena, "(fn (x) x)", "<test>").unwrap();
    let closure_val = eval_syntax(syntax, &mut expander, &mut symbols, &mut vm).unwrap();
    assert!(closure_val.as_closure().is_some(), "should be a closure");

    let result = vm.call_closure(closure_val, &[Value::int(42)]).unwrap();
    // This should fail — intentionally wrong:
    assert_eq!(result, Value::int(99), "counterfactual: should fail here");
}
