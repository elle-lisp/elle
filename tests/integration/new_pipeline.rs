// Integration tests for the new Syntax → HIR → LIR compilation pipeline
//
// These tests verify that code compiled through the new pipeline
// produces correct results when executed.

use crate::common::eval_source;
use elle::SymbolTable;

// Local `compile` shim preserving the pre-CompileCtx arity. Every call here is
// compile-only and stdlib-free, so a fresh `CompileCtx` per call (primitives +
// core + prelude, no stdlib) reproduces exactly what the old bare-symbols path
// gave — no shared compile state is needed across calls in this file.
fn compile(
    source: &str,
    symbols: &mut SymbolTable,
    source_name: &str,
) -> Result<elle::CompileResult, String> {
    let mut cctx = elle::pipeline::CompileCtx::new();
    elle::pipeline::compile(source, symbols, &mut cctx, source_name)
}

/// Helper that compiles but doesn't execute (for testing compilation only)
fn compiles(input: &str) -> bool {
    let mut symbols = SymbolTable::new();
    compile(input, &mut symbols, "<test>").is_ok()
}

// Tests split by feature area to keep each file small.
mod basics {
    include!("new_pipeline/basics.rs");
}
mod complex {
    include!("new_pipeline/complex.rs");
}
