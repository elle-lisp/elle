//! Shared test helpers for the Elle test suite.
//!
//! Provides canonical eval and setup functions so test files don't need
//! to copy-paste their own variants.

use elle::context::{set_symbol_table, set_vm_context};
use elle::{eval_all, init_stdlib, register_primitives, SymbolTable, Value, VM};

/// Evaluate Elle source code through the pipeline WITHOUT stdlib.
///
/// Identical to `eval_source` except it skips `init_stdlib`. Use this
/// for tests that never call stdlib functions (map, filter, fold, etc.).
/// Prelude macros (defn, let*, ->, ->>, when, unless, try/catch, etc.)
/// are still available — they're loaded by `compile_all`'s internal
/// `Expander::load_prelude`, not by `init_stdlib`.
#[allow(dead_code)]
pub fn eval_source_bare(input: &str) -> Result<Value, String> {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    let _effects = register_primitives(&mut vm, &mut symbols);
    set_vm_context(&mut vm as *mut VM);
    set_symbol_table(&mut symbols as *mut SymbolTable);
    // No init_stdlib — tests using this must not depend on stdlib functions
    let result = eval_all(input, &mut symbols, &mut vm);
    set_vm_context(std::ptr::null_mut());
    result
}

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
    // Set symbol table context before stdlib init so that macros using
    // gensym (each, ffi/defbind) work during init_stdlib's eval() calls.
    set_vm_context(&mut vm as *mut VM);
    set_symbol_table(&mut symbols as *mut SymbolTable);
    init_stdlib(&mut vm, &mut symbols);
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
    set_symbol_table(&mut symbols as *mut SymbolTable);
    init_stdlib(&mut vm, &mut symbols);
    (symbols, vm)
}

/// Create a proptest config that respects the PROPTEST_CASES env var.
///
/// When PROPTEST_CASES is set, its value overrides the given default.
/// This lets CI and local development control case counts uniformly:
///
///   PROPTEST_CASES=8 cargo test    # fast smoke
///   cargo test                     # use per-test defaults
#[allow(dead_code)]
pub fn proptest_cases(default: u32) -> proptest::prelude::ProptestConfig {
    let cases = std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default);
    proptest::prelude::ProptestConfig::with_cases(cases)
}
