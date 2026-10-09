// audited: 2026-10-06
// Closures sent to a new thread keep their location table, so an error there
// still names the form that raised it.
//
// docs/threads.md
//
// Shared `use`s and the free `compile` helper live here; per-theme test bodies
// are pulled in via `include!` so each `super::*` in a subfile resolves to
// this module.

use crate::common::eval_source;
use elle::SymbolTable;
use proptest::prelude::*;

// A local `compile`: the `compile` sites here are compile-only and
// stdlib-free, so a fresh `CompileCtx` per call (primitives + core + prelude)
// is all they need.
fn compile(
    source: &str,
    symbols: &mut SymbolTable,
    source_name: &str,
) -> Result<elle::CodeUnit, String> {
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
