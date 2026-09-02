// Integration tests for interprocedural signal tracking and enforcement
//
// These tests verify that signals propagate correctly across function boundaries:
// - Direct yield has Yields signal
// - Calling a yielding function propagates Yields signal
// - Polymorphic signals (like map) resolve based on argument signals
// - Silent functions remain silent
// - assign invalidates signal tracking
// - Unknown callees use Signal::unknown() (sound conservative)
// - Parameter calls are purely polymorphic (no inherent error)
//
// This file is a coordinator: it holds the shared `use` imports and free
// helpers, then `include!`s the themed test subfiles. Each subfile opens with
// `use super::*;`, so `super::` from a test body resolves to this coordinator's
// items (setup/analyze/analyze_with_stdlib and the imports below).

use elle::hir::HirKind;
use elle::primitives::register_primitives;
use elle::signals::{Signal, SIG_OK, SIG_YIELD};
use elle::symbol::SymbolTable;
use elle::vm::VM;

fn setup() -> (SymbolTable, VM) {
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);
    (symbols, vm)
}

// Local `analyze` shim preserving the pre-CompileCtx arity. These analyses are
// the deliberately stdlib-free path the file's tests assume — e.g.
// `test_signal_polymorphic_with_pure_arg` requires `map` to be an *unknown*
// global. A fresh per-call `CompileCtx` (primitives + core + prelude, no stdlib)
// is exactly that environment; signal inference reads only the instance's
// compile-time metadata, none of which is shared across these independent
// analyses (the no-ambient-compile-cache model, docs/impl/region/ctx.md).
fn analyze(
    source: &str,
    symbols: &mut SymbolTable,
    vm: &mut VM,
    source_name: &str,
) -> Result<elle::pipeline::AnalyzeResult, String> {
    let mut cctx = elle::pipeline::CompileCtx::new();
    elle::pipeline::analyze(source, symbols, vm, &mut cctx, source_name)
}

// Like `analyze`, but with the stdlib loaded — for tests whose source names
// stdlib functions (e.g. `number?`, `string?`, defined in stdlib.lisp via
// `type-of`). Without stdlib those resolve as *unknown* globals; with it they
// resolve to their definitions and carry their inferred signals.
fn analyze_with_stdlib(
    source: &str,
    symbols: &mut SymbolTable,
    vm: &mut VM,
    source_name: &str,
) -> Result<elle::pipeline::AnalyzeResult, String> {
    let mut cctx = elle::pipeline::CompileCtx::new();
    vm.set_symbols(symbols as *mut SymbolTable);
    elle::init_stdlib(vm, symbols, &mut cctx, &elle::compiler::stdlib_cache::StdlibCache::Off);
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
