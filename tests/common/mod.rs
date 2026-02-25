//! Shared test helpers for the Elle test suite.
//!
//! Provides canonical eval and setup functions so test files don't need
//! to copy-paste their own variants.

use elle::ffi::primitives::context::{set_symbol_table, set_vm_context};
use elle::{eval_all, init_stdlib, register_primitives, SymbolTable, Value, VM};

/// Evaluate Elle source code through the full pipeline.
///
/// Handles both single-form and multi-form input via `eval_all`.
/// Initializes primitives, stdlib, and symbol table context.
///
/// This is the canonical test eval. Use this unless you have a specific
/// reason not to (e.g., testing without stdlib).
pub fn eval_source(input: &str) -> Result<Value, String> {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    let _effects = register_primitives(&mut vm, &mut symbols);
    init_stdlib(&mut vm, &mut symbols);
    // Thread-local pointers — safe because tests within a thread are sequential.
    // See src/ffi/primitives/context.rs: VM_CONTEXT and SYMBOL_TABLE are thread_local!.
    set_vm_context(&mut vm as *mut VM);
    set_symbol_table(&mut symbols as *mut SymbolTable);
    let result = eval_all(input, &mut symbols, &mut vm);
    // Clear context to avoid affecting other tests
    set_vm_context(std::ptr::null_mut());
    result
}

#[allow(dead_code)]
/// Set up a VM and SymbolTable with primitives and stdlib.
///
/// Returns (symbols, vm). Symbol table context is set.
pub fn setup() -> (SymbolTable, VM) {
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _effects = register_primitives(&mut vm, &mut symbols);
    init_stdlib(&mut vm, &mut symbols);
    set_symbol_table(&mut symbols as *mut SymbolTable);
    (symbols, vm)
}
