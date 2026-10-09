// audited: 2026-10-06
// The Syntax → HIR → LIR → bytecode pipeline end to end: what compiles, and
// what the compiled code answers when it runs.
//
// src/pipeline/AGENTS.md

use crate::common::eval_source;
use elle::SymbolTable;

// A local `compile`: every call here is compile-only and stdlib-free, so a
// fresh `CompileCtx` per call (primitives + core + prelude, no stdlib) is all
// it needs, and no compile state is shared across calls in this file.
fn compile(
    source: &str,
    symbols: &mut SymbolTable,
    source_name: &str,
) -> Result<elle::CodeUnit, String> {
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
