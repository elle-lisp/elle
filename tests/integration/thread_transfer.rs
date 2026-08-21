// Tests for thread transfer of closures with location data
//
// Verifies that closures spawned in new threads correctly preserve their
// LocationMap for error reporting.
//
// Coordinator (Recipe B): shared `use`s and the free `compile` helper live
// here; per-theme test bodies are pulled in via `include!` so each `super::*`
// in a subfile resolves to this module.

use crate::common::eval_source;
use elle::SymbolTable;
use proptest::prelude::*;

// Local `compile` shim preserving the pre-CompileCtx arity. The `compile` sites
// here are compile-only and stdlib-free, so a fresh `CompileCtx` per call
// (primitives + core + prelude) reproduces the old bare-symbols path.
fn compile(
    source: &str,
    symbols: &mut SymbolTable,
    source_name: &str,
) -> Result<elle::CompileResult, String> {
    let mut cctx = elle::pipeline::CompileCtx::new();
    elle::pipeline::compile(source, symbols, &mut cctx, source_name)
}

mod errors {
    include!("thread_transfer/errors.rs");
}
mod closures {
    include!("thread_transfer/closures.rs");
}
// The worker-heap slope lives in tests/worker_heap.rs, its own binary, because
// it reads a process-wide page counter (docs/analysis/testing.md
// § "Process-global state needs its own binary").
