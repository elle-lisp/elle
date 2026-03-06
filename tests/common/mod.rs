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
/// are still available — they're loaded by `compile_file`'s internal
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
///
/// Regression files are persisted to `tests/proptest-regressions/`.
#[allow(dead_code)]
pub fn proptest_cases(default: u32) -> proptest::prelude::ProptestConfig {
    use proptest::test_runner::FileFailurePersistence;

    let cases = std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default);

    proptest::prelude::ProptestConfig {
        cases,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "tests/proptest-regressions",
        ))),
        ..proptest::prelude::ProptestConfig::default()
    }
}

// ---------------------------------------------------------------------------
// Cached eval helpers for property tests
// ---------------------------------------------------------------------------
//
// These reuse a thread-local (VM, SymbolTable) pair across proptest cases,
// eliminating per-case bootstrap cost (VM creation, primitive registration,
// stdlib loading). Between cases the fiber is reset and globals are restored
// to their post-initialization snapshot.
//
// Use `eval_reuse_bare` for tests that don't need stdlib (the common case).
// Use `eval_reuse` for tests that need stdlib functions (map, filter, etc.).
//
// The old `eval_source` / `eval_source_bare` remain available for tests that
// need a guaranteed-fresh VM (none currently do, but the option exists).

use std::cell::RefCell;
use std::thread::LocalKey;

struct TestCache {
    vm: VM,
    symbols: SymbolTable,
    globals_snapshot: Vec<Value>,
    defined_globals_snapshot: Vec<bool>,
}

thread_local! {
    static BARE_CACHE: RefCell<Option<TestCache>> = const { RefCell::new(None) };
    static FULL_CACHE: RefCell<Option<TestCache>> = const { RefCell::new(None) };
}

fn eval_with_cache(
    input: &str,
    cache: &'static LocalKey<RefCell<Option<TestCache>>>,
    init: fn(&mut VM, &mut SymbolTable),
) -> Result<Value, String> {
    cache.with(|cell| {
        let mut borrow = cell.borrow_mut();
        let c = borrow.get_or_insert_with(|| {
            let mut vm = VM::new();
            let mut symbols = SymbolTable::new();
            let _effects = register_primitives(&mut vm, &mut symbols);
            // Context pointers needed during init (stdlib loading uses gensym).
            set_vm_context(&mut vm as *mut VM);
            set_symbol_table(&mut symbols as *mut SymbolTable);
            init(&mut vm, &mut symbols);
            let globals_snapshot = vm.globals.clone();
            let defined_globals_snapshot = vm.defined_globals.clone();
            TestCache {
                vm,
                symbols,
                globals_snapshot,
                defined_globals_snapshot,
            }
        });

        // Reset per-case state
        c.vm.reset_fiber();
        c.vm.jit_cache.clear();
        c.vm.globals.truncate(c.globals_snapshot.len());
        c.vm.globals.copy_from_slice(&c.globals_snapshot);
        c.vm.defined_globals
            .truncate(c.defined_globals_snapshot.len());
        c.vm.defined_globals
            .copy_from_slice(&c.defined_globals_snapshot);

        // Set context pointers (may have been cleared after previous eval)
        set_vm_context(&mut c.vm as *mut VM);
        set_symbol_table(&mut c.symbols as *mut SymbolTable);

        let result = eval_all(input, &mut c.symbols, &mut c.vm);

        set_vm_context(std::ptr::null_mut());

        result
    })
}

/// Evaluate Elle source with a cached VM (primitives only, no stdlib).
///
/// Drop-in replacement for `eval_source_bare` in property tests. The VM
/// is created once per thread and reused across proptest cases. Between
/// cases, the fiber is reset and globals are restored to their
/// post-initialization values.
#[allow(dead_code)]
pub fn eval_reuse_bare(input: &str) -> Result<Value, String> {
    eval_with_cache(input, &BARE_CACHE, |_, _| {})
}

/// Evaluate Elle source with a cached VM (primitives + stdlib).
///
/// Drop-in replacement for `eval_source` in property tests. The VM
/// is created once per thread and reused across proptest cases. Between
/// cases, the fiber is reset and globals are restored to their
/// post-initialization values.
#[allow(dead_code)]
pub fn eval_reuse(input: &str) -> Result<Value, String> {
    eval_with_cache(input, &FULL_CACHE, |vm, symbols| {
        init_stdlib(vm, symbols);
    })
}
