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
    let signal = if bits.is_ok() || (bits == crate::value::SIG_HALT && val == Value::NIL) {
        crate::value::SIG_OK
    } else {
        bits
    };
    if bits.is_ok() || (bits == crate::value::SIG_HALT && val == Value::NIL) {
        vm_ref.fiber.signal.take();
    }
    let (tag, payload) = caller.data_mut().value_to_wasm(val);
    (tag, payload, signal.raw() as i64)
}
