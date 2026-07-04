// End-to-end escape/region behavior tests.
//
// History: this file once asserted the *scope-emission* model of escape
// analysis by disassembling bytecode and matching `RegionEnter`/`RegionExit`
// opcodes. The s11 overhaul replaced that model with per-value region
// refcounting (`IncrefValueRegion`/`DecrefValueRegion`/`AdoptRegion`); the old
// opcodes no longer exist and several of the old probe sources no longer
// compile. Those ~130 white-box tests were retired — the scope/region inference
// they covered is now verified against the current compiler in
// `src/hir/regions/tests` and `src/lir/lower/tests`.
//
// What remains here is the behavioral core: programs whose correctness depends
// on regions reclaiming scoped allocations at the right time (and NOT
// reclaiming values that escape). Each test runs Elle source and asserts the
// observed value — if region inference freed something too early (or kept a
// scope it should have reclaimed, corrupting reuse), the result would differ.

use crate::common::eval_source;
use elle::SymbolTable;
use elle::Value;

// Local `compile` shim preserving the pre-`CompileCtx` arity, for the one test
// that only needs to confirm a tricky letrec compiles (fixpoint converges). A
// fresh `CompileCtx` per call is correct here — every call is compile-only and
// stdlib-free, so no shared compile state is needed.
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
