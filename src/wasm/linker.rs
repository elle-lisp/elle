//! Host function registration for the Wasmtime linker.

use wasmtime::*;

use super::host::ElleHost;
use crate::value::repr::TAG_HEAP_START;
use crate::value::Value;

mod dataop;
pub use dataop::*;

mod create;
pub use create::*;

/// Execute a **bytecode closure** — one with no `wasm_func_idx` — by calling
/// back into the host VM's interpreter, returning `(tag, payload, signal)` for
/// the wasm caller. The full-module twin of the tiered linker's bytecode-closure
/// path (`src/wasm/lazy/env.rs`).
///
/// Only `stdlib.lisp` is compiled into the full module (see `wasm/mod.rs`
/// `eval_wasm_with_stdlib`); `core.lisp` and the prelude are loaded as
/// interpreter bytecode. So a compiled function — `map`'s list arm calling
/// `reverse`, a user HOF calling `last`/`fold`/`concat`, a callback bound at
/// runtime — routinely reaches a closure the module never compiled. Executing it
/// here via the VM makes the call transparent; without this the caller aborts
/// with "bytecode closure in WASM backend". Pinned by
/// `tests/elle/wasm-bytecode-closure-call.lisp`.
pub(crate) fn run_bytecode_closure(
    caller: &mut Caller<'_, ElleHost>,
    closure: &crate::value::Closure,
    func_val: Value,
    args: &[Value],
) -> (i64, i64, i64) {
    let vm = caller.data().vm;
    // Raw-pointer deref: the VM outlives the call, and it is a distinct object
    // from the `ElleHost` reached via `caller.data_mut()` below (the value/handle
    // marshalling touches the host, never the VM), so the two never alias.
    let vm_ref = unsafe { &mut *vm };
    let Some(env) = vm_ref.build_closure_env(closure, args) else {
        // `build_closure_env` rejected the args (bad keywords) and set the error
        // signal; surface it.
        let (bits, val) = vm_ref
            .fiber
            .signal
            .unwrap_or((crate::value::SIG_ERROR, Value::NIL));
        let (tag, payload) = caller.data_mut().value_to_wasm(val);
        return (tag, payload, bits.raw() as i64);
    };
    // Hand the callee its executing-closure register via the one-shot (the
    // WASM→interp entry boundary), so a self-reference in the body resolves to
    // the callee, not NIL — the same handoff the interpreter's `call_inner` and
    // the tiered fallback perform.
    vm_ref.pending_entry_closure = func_val;
    let bits = vm_ref
        .execute_bytecode_saving_stack(&closure.template.code(), &env)
        .bits;
    // A NIL `(halt)` is a normal return; every other terminal/suspending signal
    // rides back out on the signal word (mirrors `exec_result_to_jit_value`).
    let val = vm_ref
        .fiber
        .signal
        .as_ref()
        .map(|(_, v)| *v)
        .unwrap_or(Value::NIL);
    let signal = if bits.is_empty() || (bits == crate::value::SIG_HALT && val == Value::NIL) {
        crate::value::SIG_OK
    } else {
        bits
    };
    if bits.is_empty() || (bits == crate::value::SIG_HALT && val == Value::NIL) {
        vm_ref.fiber.signal.take();
    }
    let (tag, payload) = caller.data_mut().value_to_wasm(val);
    (tag, payload, signal.raw() as i64)
}

/// Apply a **callable collection** — a struct/array/set/string/bytes indexed by
/// a key (`(request :op)`, `(arr i)`, `(set x)`) — via the interpreter's shared
/// `call_collection`, returning `(tag, payload, signal)` for the wasm caller.
/// When `func_val` is not a collection either, produces the `cannot call …` type
/// error so both dispatch sites (`rt_call`, `rt_prepare_tail_call`) share one
/// tail. Mirrors the collection arm of the interpreter's `call_inner`.
///
/// Only stdlib compiles into the full module, so a compiled function routinely
/// applies a struct as a function — the async scheduler's `handle-wait` reads
/// `(request :op)` / `(request :fiber)` off the request struct a fiber emits via
/// `(emit :wait request)`. Without this the call aborts with "cannot call
/// struct" and no `ev/join`/`ev/scope`/futex wait ever resumes. Pinned by
/// `tests/elle/wasm-collection-call.lisp` and the `wasm_full_calls_*` /
/// `wasm_full_scheduler_resumes_joined_fiber` unit tests.
///
/// The full-module tier makes region instructions structural no-ops, so — like
/// the native-fn arm of `rt_call` — this skips the interpreter's Rule-5 escape
/// retain and mints a fresh `Alloc` only for the element/error it may build.
pub(crate) fn run_collection_call(
    caller: &mut Caller<'_, ElleHost>,
    func_val: Value,
    args: &[Value],
    what: &str,
) -> (i64, i64, i64) {
    let call_result = {
        let heap = unsafe { &mut *caller.data().heap_ptr() };
        let mut ctx = crate::primitives::ctx::Alloc::new(heap);
        crate::vm::call::call_collection(&func_val, args, &mut ctx)
    };
    let (value, signal) = match call_result {
        Some(Ok(value)) => (value, crate::value::SIG_OK.raw() as i64),
        Some(Err((kind, msg))) => {
            let heap = unsafe { &mut *caller.data().heap_ptr() };
            let ctx = crate::primitives::ctx::Alloc::new(heap);
            (ctx.error(kind, msg), crate::value::SIG_ERROR.raw() as i64)
        }
        None => {
            let heap = unsafe { &mut *caller.data().heap_ptr() };
            let ctx = crate::primitives::ctx::Alloc::new(heap);
            let err = ctx.error(
                "type-error",
                format!("{}: cannot call {}", what, func_val.type_name()),
            );
            (err, crate::value::SIG_ERROR.raw() as i64)
        }
    };
    let (tag, payload) = caller.data_mut().value_to_wasm(value);
    (tag, payload, signal)
}
