// Integration tests for file-scope compilation.
// Tests immutable vs mutable capture behavior at runtime.

use crate::common::eval_source;
use elle::Value;

// ============================================================================
// SECTION 0: File-as-letrec pipeline (eval_file, compile_file, analyze_file)
// ============================================================================

/// Helper: evaluate source through the file-as-letrec pipeline (no stdlib) and
/// inspect the `Result` while its `Runtime` (and heap) is still alive, tearing
/// down only after `f` returns. A heap-valued result is a tagged pointer into the
/// runtime's region heap; handing it out past teardown dangles (see the module
/// note in `tests/common/mod.rs`). So inspect heap structure inside `f` and
/// return only OWNED data (scalars, `String`, counts).
///
/// Drives a `Runtime` so the per-instance `CompileCtx` is heap-stable and the
/// VM is already wired to it — these tests evaluate `(eval …)`/`(import-file …)`
/// runtime forms, which reach the cctx through the VM's pointer, so the cctx
/// must outlive the call (a moved local cctx would dangle).
fn eval_file_source<R>(input: &str, f: impl FnOnce(Result<Value, String>) -> R) -> R {
    use elle::runtime::Runtime;

    let mut rt = Runtime::without_stdlib();
    let result = {
        let (vm, symbols, cctx) = rt.parts();
        elle::eval_file(input, symbols, vm, cctx, "<test>")
    };
    f(result)
}

/// Like `eval_file_source`, but with the stdlib loaded — for tests that call
/// library functions (e.g. `push`).
fn eval_file_source_stdlib<R>(input: &str, f: impl FnOnce(Result<Value, String>) -> R) -> R {
    use elle::runtime::Runtime;

    let mut rt = Runtime::new();
    let result = {
        let (vm, symbols, cctx) = rt.parts();
        elle::eval_file(input, symbols, vm, cctx, "<test>")
    };
    f(result)
}

/// Helper: compile source through the file-as-letrec pipeline.
fn compile_file_source(input: &str) -> Result<elle::CompileResult, String> {
    use elle::runtime::Runtime;

    let mut rt = Runtime::without_stdlib();
    let (_, symbols, cctx) = rt.parts();
    elle::compile_file(input, symbols, cctx, "<test>")
}

/// Helper: evaluate source through the file-as-letrec pipeline with stdlib,
/// inspecting the `Result` while the `Runtime` (and heap) is still alive — same
/// scoped-callback discipline as `eval_file_source` (return only OWNED data).
///
/// Drives a `Runtime::new()` (primitives + stdlib): the stdlib exports are
/// registered into this instance's `CompileCtx`, which is the SAME cctx threaded
/// into `eval_file` here — so stdlib functions stay visible. The Runtime also
/// keeps the cctx heap-stable for the `(import-file …)`/`(eval …)` runtime forms.
fn eval_file_source_with_stdlib<R>(input: &str, f: impl FnOnce(Result<Value, String>) -> R) -> R {
    use elle::runtime::Runtime;

    let mut rt = Runtime::new();
    let result = {
        let (vm, symbols, cctx) = rt.parts();
        elle::eval_file(input, symbols, vm, cctx, "<test>")
    };
    f(result)
}

// Tests are split into themed subfiles wired via include!; each subfile
// opens with `use super::*;` so it sees the shared helpers and imports above.
mod pipeline {
    include!("file_scope/pipeline.rs");
}

mod captures {
    include!("file_scope/captures.rs");
}
