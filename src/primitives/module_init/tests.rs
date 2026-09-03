//! Unit tests (`super` is the parent impl module).

use super::*;
use crate::pipeline::compile_file;
use crate::primitives::registration::register_primitives;

#[test]
fn build_closure_call_env_places_captures_before_locals() {
    // Regression test for Finding 4.
    //
    // `build_closure_call_env` constructs the env that the stdlib's
    // tail export closure receives at call time. The VM's
    // `LoadUpvalue` instruction indexes the env from zero, so the
    // captures must sit at the front. The old layout reserved
    // `num_locals` nil slots in front of the captures — invisible
    // while `num_locals == 0` (its assumed state for a trivial
    // `(fn [] {...})`), but the ANF lift introduces one local for
    // every allocating subexpression in the closure body. The stdlib
    // export closure contains an inline `(fn [port] ...)`, which the
    // lift names into a local. That local then occupied env[0] and
    // shifted every capture by one slot, so every capture read as
    // nil — which is why `(+ 1 2)` came back nil during
    // `init_stdlib`.
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    let _ = register_primitives(&mut vm, &mut symbols);

    // A captured outer binding (`outer`) and an allocating let
    // inside the returned closure (`(fn [x] x)` allocates a
    // closure → ANF lifts it into a local).
    let source = "(letrec [outer (fn [n] n)] \
                      (fn [] (let [inner (fn [x] x)] outer)))";
    let mut cctx = crate::pipeline::CompileCtx::new();
    let compiled =
        compile_file(source, &mut symbols, &mut cctx, "<test>").expect("source must compile");
    let closure_val = vm
        .execute(&compiled.bytecode)
        .expect("top-level execution must succeed");
    let closure = closure_val
        .as_closure()
        .expect("top-level must evaluate to a closure");

    assert!(
        closure.template.num_captures() >= 1,
        "the test source must produce a closure with captures; got num_captures={}",
        closure.template.num_captures()
    );
    assert!(
        closure.template.num_locals() >= 1,
        "the test source must produce a closure with at least one local \
             (otherwise the bug condition isn't exercised); got num_locals={}",
        closure.template.num_locals()
    );

    let env = build_closure_call_env(closure, &[]);
    assert!(
        !env[0].is_nil(),
        "env[0] must be the first capture — a nil here means locals \
             were placed before captures and `LoadUpvalue(0)` reads nil. \
             num_captures={}, num_locals={}, env[0]={}",
        closure.template.num_captures(),
        closure.template.num_locals(),
        env[0],
    );
}
