// Coordinator for the pipeline integration tests. This file owns the shared
// `use` imports and the `setup`/`setup_with_stdlib` helpers, then pulls in each
// themed test group via `include!`. The included subfiles open with
// `use super::*;`, so they see these imports and helpers as `super::*`.
//
// Themes:
//   basics  — compile + basic eval of literals, forms, comparison
//   eval    — control flow, closures, higher-order fns, define/assign, intrinsics
//   analyze — `analyze` HIR shape + mutual-recursion / purity inference
//   fiber   — fiber/new, resume, status, emit, mask
//   special — const, arity checks, and the `eval` special form

use elle::hir::HirKind;
use elle::pipeline::{analyze, compile, compile_file, eval, CompileCtx};
use elle::runtime::Runtime;
use elle::{SymbolTable, Value};

// Each test drives a `Runtime` (elle::runtime), the per-instance owner of the
// VM, symbol table, and per-instance `CompileCtx`. `rt.parts()` hands out the
// disjoint borrows the pipeline threads. The compile state each call names is
// this instance's own, threaded as a parameter, so stdlib exports loaded
// by `setup_with_stdlib` persist across that test's later compile/eval calls
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
