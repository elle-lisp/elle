use elle::value::fiber::SignalBits;
// DEFENSE: Unit tests for closure and lambda primitives
// Tests the basic building blocks of closure and lambda functionality
use elle::primitives::register_primitives;
use elle::runtime::Runtime;
use elle::symbol::SymbolTable;
use elle::value::{Arity, Closure, TemplateProto, TemplateRef, Value};
use elle::vm::VM;
use std::rc::Rc;

fn setup() -> (VM, SymbolTable) {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    let _signals = register_primitives(&mut vm, &mut symbols);
    (vm, symbols)
}

/// Materialize a code object for `proto` into a fresh region of `heap` and name
/// it — the shape `MakeClosure` builds: a header over the blueprint's shared
/// payload (docs/impl/region/template.md).
fn template(
    heap: &mut elle::value::fiberheap::FiberHeap,
    proto: TemplateProto,
) -> TemplateRef {
    let region = heap.new_runtime_region();
    TemplateRef::region(elle::value::closure::materialize(
        heap,
        &Rc::new(proto),
        region,
    ))
}

// Sections 1-5: closure construction, type identity, arity, environment
// capture, constants/bytecode storage, and parameter binding.
mod construction {
    include!("closures_and_lambdas/construction.rs");
}

// Sections 6-10: equality/hashing, complex nested scenarios, accessor
// methods, scope behavior, and edge cases.
mod scenarios {
    include!("closures_and_lambdas/scenarios.rs");
}
