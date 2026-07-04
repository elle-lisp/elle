use elle::value::fiber::SignalBits;
// DEFENSE: Primitives are the building blocks - must be correct
use elle::error::LError;
use elle::pipeline::eval as pipeline_eval;
use elle::primitives::def::PrimitiveMeta;
use elle::primitives::register_primitives;
use elle::symbol::SymbolTable;
use elle::value::{Closure, ClosureTemplate, Value};
use elle::vm::VM;

use crate::common::eval_source;

fn setup() -> (VM, SymbolTable, PrimitiveMeta) {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    let meta = register_primitives(&mut vm, &mut symbols);
    (vm, symbols, meta)
}

fn get_primitive(meta: &PrimitiveMeta, symbols: &mut SymbolTable, name: &str) -> Value {
    let id = symbols.intern(name);
    *meta.functions.get(&id).expect("primitive not found")
}

#[allow(clippy::result_large_err)]
fn call_primitive(prim: &Value, args: &[Value]) -> Result<Value, LError> {
    if let Some(f) = prim.as_native_fn() {
        // Natives take a NativeCtx (docs/impl/region-ctx.md); the seam mints one
        // over a fresh region. Use the region-keeping variant: the returned
        // `value` (and the error struct `format_error` reads) is heap-backed and
        // inspected AFTER this call, so the region must not be released here —
        // releasing it would free the value on the root heap (a use-after-free).
        let (bits, value) = elle::primitives::ctx::with_test_ctx_keep_region(|ctx| f(ctx, args));
        if bits == elle::value::fiber::SIG_OK {
            Ok(value)
        } else {
            // SIG_ERROR or other — extract error message from error value
            let msg = elle::value::format_error(value);
            Err(LError::from(msg))
        }
    } else {
        panic!("Not a function");
    }
}

/// Like [`call_primitive`], but points the primitive's ctx VM at `symbols` so a
/// meta primitive that interns names (`gensym`) resolves into the caller's table
/// — the symbol table is threaded explicitly rather than installed ambiently.
#[allow(clippy::result_large_err)]
fn call_primitive_with_symbols(
    prim: &Value,
    args: &[Value],
    symbols: &mut SymbolTable,
) -> Result<Value, LError> {
    if let Some(f) = prim.as_native_fn() {
        let (bits, value) =
            elle::primitives::ctx::with_test_ctx_symbols(symbols as *mut SymbolTable, |ctx| {
                f(ctx, args)
            });
        if bits == elle::value::fiber::SIG_OK {
            Ok(value)
        } else {
            let msg = elle::value::format_error(value);
            Err(LError::from(msg))
        }
    } else {
        panic!("Not a function");
    }
}

/// Evaluate `input` and inspect the `Result` while its `Runtime` (and heap) is
/// still alive, tearing down only after `f` returns. A heap-valued result is a
/// tagged pointer into the runtime's region heap; handing it out past teardown
/// dangles (see the module note in `tests/common/mod.rs`). So inspect heap
/// structure inside `f` and return only OWNED data (scalars, `String`, counts).
///
/// Drives a `Runtime::new()` (primitives + stdlib, contexts installed, VM wired
/// to its CompileCtx). The stdlib exports are registered into this instance's
/// cctx, which is the SAME cctx threaded into the eval below — so SIG_QUERY
/// primitives and stdlib functions resolve correctly.
#[allow(clippy::result_large_err)]
fn eval_full<R>(input: &str, f: impl FnOnce(Result<Value, elle::error::LError>) -> R) -> R {
    let mut rt = elle::runtime::Runtime::new();
    let result = {
        let (vm, symbols, cctx) = rt.parts();
        pipeline_eval(input, symbols, vm, cctx, "<test>").map_err(elle::error::LError::from)
    };
    f(result)
}

fn run(input: &str) -> String {
    eval_full(input, |r| {
        r.map(|v| format!("{}", v))
            .unwrap_or_else(|e| panic!("eval failed: {}", e))
    })
}

// Themed test modules, wired via include! so the shared imports and helper
// fns above resolve as `super::*` from each subfile.
mod arithmetic {
    include!("primitives/arithmetic.rs");
}
mod strings {
    include!("primitives/strings.rs");
}
mod meta {
    include!("primitives/meta.rs");
}
mod json {
    include!("primitives/json.rs");
}
mod runtime {
    include!("primitives/runtime.rs");
}
