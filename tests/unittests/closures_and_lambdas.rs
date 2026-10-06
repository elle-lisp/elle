// audited: 2026-10-06
// Closures and lambdas: construction, arity, captured environments, and the
// code object's accessors.
//
// docs/functions.md
use elle::primitives::register_primitives;
use elle::runtime::Runtime;
use elle::symbol::SymbolTable;
use elle::value::fiber::SignalBits;
use elle::value::{Arity, Closure, CodeBuilder, TemplateRef, Value};
use elle::vm::VM;
use std::rc::Rc;

fn setup() -> (VM, SymbolTable) {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    let _signals = register_primitives(&mut vm, &mut symbols);
    (vm, symbols)
}

/// Write `code` into a fresh code region of `heap` and name its header
/// (docs/impl/region/template.md).
fn template(heap: &mut elle::value::fiberheap::FiberHeap, code: CodeBuilder) -> TemplateRef {
    code.build(heap)
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
