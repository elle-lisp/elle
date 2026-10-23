// audited: 2026-10-21
// The pipeline integration tests: the shared imports and setup, and one group per theme.
//
// docs/pipeline.md
//
// This file owns the shared `use` imports and the `setup`/`setup_with_stdlib`
// helpers, then pulls in each themed test group via `include!`. The included
// subfiles open with `use super::*;`, so they see these imports and helpers.
// The themes:
//
// - basics: compile, and basic eval of literals, forms and comparison.
// - eval: control flow, closures, higher-order fns, define/assign, intrinsics.
// - analyze: the `analyze` HIR shape, and mutual-recursion and purity
//   inference.
// - fiber: fiber/new, resume, status, emit, mask.
// - special: const, arity checks, and the `eval` special form.
// - solver: what an analysis hands a signal solver.

use elle::hir::HirKind;
use elle::pipeline::{analyze, compile, compile_file, eval, CompileCtx};
use elle::runtime::Runtime;
use elle::{SymbolTable, Value};

// Each test drives a `Runtime` (elle::runtime), the per-instance owner of the
// VM, symbol table, and per-instance `CompileCtx`. `rt.parts()` hands out the
// disjoint borrows the pipeline threads. The compile state each call names is
// this instance's own, threaded as a parameter. So stdlib exports loaded by
// `setup_with_stdlib` persist across that test's later compile/eval calls,
// because the SAME `cctx` is threaded through every one of them.

fn setup() -> Runtime {
    Runtime::without_stdlib()
}

fn setup_with_stdlib() -> Runtime {
    Runtime::new()
}

mod basics {
    include!("pipeline/basics.rs");
}

mod eval {
    include!("pipeline/eval.rs");
}

mod analyze {
    include!("pipeline/analyze.rs");
}

mod fiber {
    include!("pipeline/fiber.rs");
}

mod special {
    include!("pipeline/special.rs");
}

mod solver {
    include!("pipeline/solver.rs");
}
