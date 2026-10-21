// audited: 2026-10-21
// Signal inference across function boundaries: what a call to a direct, higher-order, reassigned or unknown callee signals.
//
// docs/signals/inference.md
//
// This file holds the shared imports and helpers, then `include!`s the themed
// test files. Each opens with `use super::*;`, so `super::` in a test body
// names the items here.

use elle::hir::HirKind;
use elle::primitives::register_primitives;
use elle::signals::{Signal, SIG_ERROR, SIG_YIELD};
use elle::symbol::SymbolTable;
use elle::vm::VM;

fn setup() -> (SymbolTable, VM) {
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);
    (symbols, vm)
}

// Analyze `source` without the stdlib. The tests assume that environment:
// `test_signal_unknown_global_call_is_unknown` needs `map` to be an unknown
// global. A fresh `CompileCtx` per call holds the primitives, core and
// prelude, and no analysis here shares one.
fn analyze(
    source: &str,
    symbols: &mut SymbolTable,
    vm: &mut VM,
    source_name: &str,
) -> Result<elle::pipeline::AnalyzeResult, String> {
    let mut cctx = elle::pipeline::CompileCtx::new();
    elle::pipeline::analyze(source, symbols, vm, &mut cctx, source_name)
}

// Like `analyze`, but with the stdlib loaded, for a source that names a stdlib
// function such as `number?`. Without the stdlib that name is an unknown
// global; with it, the call carries the function's inferred signal.
fn analyze_with_stdlib(
    source: &str,
    symbols: &mut SymbolTable,
    vm: &mut VM,
    source_name: &str,
) -> Result<elle::pipeline::AnalyzeResult, String> {
    let mut cctx = elle::pipeline::CompileCtx::new();
    vm.set_symbols(symbols as *mut SymbolTable);
    elle::init_stdlib(
        vm,
        symbols,
        &mut cctx,
        &elle::compiler::stdlib_cache::StdlibCache::Off,
    );
    elle::pipeline::analyze(source, symbols, vm, &mut cctx, source_name)
}

mod propagation {
    include!("signal_enforcement/propagation.rs");
}

mod primitives {
    include!("signal_enforcement/primitives.rs");
}

mod inference {
    include!("signal_enforcement/inference.rs");
}
