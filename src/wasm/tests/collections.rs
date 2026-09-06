// audited: 2026-09-06
// docs/impl/wasm.md
//! A collection applied as a function, dispatched host-side.
//!
//! A struct/array/set/string/bytes applied as a function — `(struct :k)`,
//! `(arr i)`, `(set x)` — is a callable collection: the interpreter routes it
//! through `call_collection` (src/vm/call/collection.rs). Only stdlib compiles
//! into the full module, so such a call reaches the `rt_call` /
//! `rt_prepare_tail_call` host functions, which must run the same fallback
//! (`run_collection_call`) or the value falls through to a `cannot call …`
//! error. The async scheduler depends on it: `make-async-scheduler`'s
//! `handle-wait` reads `(request :op)` off the struct a fiber emits with
//! `(emit :wait request)`, so without the fallback no `ev/join` under
//! `--wasm=full` ever resumes. The corpus twin is
//! tests/elle/wasm-collection-call.lisp (VM/JIT divergence + the marker).

use super::*;

#[test]
fn wasm_full_calls_struct_as_function() {
    // `(s :b)` in call position and `(m k)` in the tail position of a helper
    // both reach the host call functions with a struct callee.
    assert_eq!(
        eval_with_stdlib("(let [s {:a 1 :b 2}] (s :b))"),
        "2",
        "a struct applied as a function must index host-side under --wasm=full"
    );
    assert_eq!(
        eval_with_stdlib("(defn lookup [m k] (m k))\n(lookup {:a 1 :b 2} :a)"),
        "1",
        "a struct call in tail position must index via rt_prepare_tail_call"
    );
    assert_eq!(
        eval_with_stdlib("(let [s {:a 1}] (s :missing 99))"),
        "99",
        "a struct call's 2-arg form returns the default for a missing key"
    );
}

#[test]
fn wasm_full_calls_array_set_string_as_function() {
    assert_eq!(
        eval_with_stdlib("([10 20 30] 1)"),
        "20",
        "an array applied as a function must index host-side"
    );
    assert_eq!(
        eval_with_stdlib("(= ((set 2 4 6) 4) true)"),
        "true",
        "a set applied as a function must test membership host-side"
    );
    assert_eq!(
        eval_with_stdlib("(= (\"hello\" 1) \"e\")"),
        "true",
        "a string applied as a function must index a grapheme host-side"
    );
}
