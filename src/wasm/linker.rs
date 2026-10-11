// audited: 2026-10-06
//! Host function registration for the Wasmtime linker, and the collection-call
//! fallback both dispatch sites share.
//!
//! docs/impl/wasm.md
//!
//! `linker/bytecode.rs` holds the other fallback, the bridge to a closure the
//! module never compiled.

use wasmtime::*;

use super::host::ElleHost;
use crate::value::repr::TAG_HEAP_START;
use crate::value::Value;
use crate::wasm::outcome::CallOutcome;

mod dataop;
pub use dataop::*;

mod create;
pub use create::*;

mod bytecode;
pub(in crate::wasm) use bytecode::{resume_bytecode_park, run_bytecode_closure};

/// Apply a **callable collection** — a struct/array/set/string/bytes indexed by
/// a key (`(request :op)`, `(arr i)`, `(set x)`) — via the interpreter's shared
/// `call_collection`, returning a [`CallOutcome`] for the wasm caller.
/// When `func_val` is not a collection either, produces the `cannot call …` type
/// error so both dispatch sites (`rt_call`, `rt_prepare_tail_call`) share one
/// tail. Mirrors the collection arm of the interpreter's `call_inner`.
///
/// Only stdlib compiles into the full module, so a compiled function routinely
/// applies a struct as a function — the async scheduler's `handle-wait` reads
/// `(request :op)` / `(request :fiber)` off the request struct a fiber emits via
/// `(emit :wait request)`. Without this the call aborts with "cannot call
/// struct" and no `ev/join`/`ev/scope`/futex wait ever resumes. Pinned by
/// `tests/lang/wasm-collection-call.lisp` and the `wasm_full_calls_*` /
/// `wasm_full_scheduler_resumes_joined_fiber` unit tests.
///
/// The full-module tier makes region instructions structural no-ops, so — like
/// the native-fn arm of `rt_call` — this skips the interpreter's Rule-5 escape
/// retain and mints a fresh `Alloc` only for the element/error it may build.
pub(in crate::wasm) fn run_collection_call(
    caller: &mut Caller<'_, ElleHost>,
    func_val: Value,
    args: &[Value],
    what: &str,
) -> CallOutcome {
    let call_result = {
        let gen = unsafe { &*caller.data().vm }.unicode_generation();
        let heap = unsafe { &mut *caller.data().heap_ptr() };
        let mut ctx = crate::primitives::ctx::Alloc::new(heap);
        crate::vm::call::call_collection(&func_val, args, gen, &mut ctx)
    };
    let (value, signal) = match call_result {
        Some(Ok(value)) => (value, crate::value::SIG_OK),
        Some(Err((kind, msg))) => {
            let heap = unsafe { &mut *caller.data().heap_ptr() };
            let ctx = crate::primitives::ctx::Alloc::new(heap);
            (ctx.error(kind, msg), crate::value::SIG_ERROR)
        }
        None => {
            let heap = unsafe { &mut *caller.data().heap_ptr() };
            let ctx = crate::primitives::ctx::Alloc::new(heap);
            let err = ctx.error(
                "type-error",
                format!("{}: cannot call {}", what, func_val.type_name()),
            );
            (err, crate::value::SIG_ERROR)
        }
    };
    let (tag, payload) = caller.data_mut().value_to_wasm(value);
    CallOutcome::signalled(tag, payload, signal)
}
