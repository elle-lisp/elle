// audited: 2026-09-23
// Programs whose result depends on regions freeing scoped values on time and keeping escaping ones alive.
// docs/regions.md
//
// Each test runs Elle source and asserts the observed value. A region freed too
// early, or a scope kept past its end and then reused, changes the result. The
// white-box pins on the inference and its emission live in src/hir/region/infer/tests
// and src/lir/lower/tests.

use crate::common::eval_source;
use elle::SymbolTable;
use elle::Value;

// Compile `source` with no stdlib, for the one test that only needs a tricky
// letrec to compile (its fixpoint converges). Each call is compile-only, so a
// fresh `CompileCtx` per call shares no state it would need.
fn compile(
    source: &str,
    symbols: &mut SymbolTable,
    source_name: &str,
) -> Result<elle::CompileResult, String> {
    let mut cctx = elle::pipeline::CompileCtx::new();
    elle::pipeline::compile(source, symbols, &mut cctx, source_name)
}

mod mutation {
    include!("escape/mutation.rs");
}
mod scopebind {
    include!("escape/scopebind.rs");
}
mod tailcall {
    include!("escape/tailcall.rs");
}
