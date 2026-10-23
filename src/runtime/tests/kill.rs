// audited: 2026-10-22
//! A pulled kill switch ends a program at its next charge, and a slack one
//! ends nothing.
//!
//! docs/runtime.md
//!
//! The kill lands inside a silent loop, under a protect, through a metered
//! fiber's mask, and in compiled code.

use super::*;
use crate::pipeline::compile_file_repl;
use crate::primitives::def::{PrimitiveDef, RegionEffect};
use crate::signals::Signal;
use crate::value::fiber::SignalBits;
use crate::value::types::Arity;
use crate::value::Value;

/// A silent loop of a thousand trips that pulls the running VM's own switch
/// on its tenth trip, so the kill lands mid-loop with no second thread. The
/// program first checks that the loop is silent, because the claim is that a
/// kill reaches code the meter never pauses.
const PROGRAM: &str = "
(defn spin-pull [] (def @i 0) (while (%lt i 1000) (when (%eq i 10) (pull!)) (assign i (%add i 1))) i)
(if (silent? spin-pull) nil (error {:error :not-silent}))";

/// A silent loop with no pull, for the slack case.
const SPIN: &str = "(defn spin [] (def @i 0) (while (%lt i 1000) (assign i (%add i 1))) i)";

fn prim_pull(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    ctx.vm().kill_switch().pull();
    (SignalBits::EMPTY, Value::NIL)
}

primitive!(
    /// `(pull!)`: pull the running VM's kill switch. Silent, so a loop that
    /// calls it stays silent.
    static PULL: PrimitiveDef = "pull!" => prim_pull {
        signal: Signal::silent(),
        arity: Arity::Exact(0),
        effect: RegionEffect::Immediate,
    }
);

/// Run `program` after `prelude` on a runtime that knows `pull!`, answering
/// what the root entry reports: the program's integer, or its error. Owned
/// data only, because the runtime is gone when the answer is read.
fn run(prelude: &str, program: &str) -> Result<Option<i64>, String> {
    let mut rt = Runtime::new();
    rt.register_primitive(&PULL);
    let src = format!("{prelude}\n{program}");
    let compiled = {
        let (_vm, symbols, cctx) = rt.parts();
        compile_file_repl(&src, symbols, cctx, "<kill>")
            .map_err(|e| format!("{e:?}"))?
            .0
    };
    let (vm, _symbols, cctx) = rt.parts();
    vm.execute_scheduled(&compiled, cctx).map(|v| v.as_int())
}

/// Whether the root entry reported the program killed.
fn killed(result: &Result<Option<i64>, String>) -> bool {
    matches!(result, Err(e) if e.contains("killed"))
}

#[test]
fn a_slack_switch_ends_nothing() {
    // Counter-factual: a compare the wrong way round, or a check that reads
    // the budget as the word, kills this loop at its first charge.
    assert_eq!(run(SPIN, "(spin)"), Ok(Some(1000)));
}

#[test]
fn a_pulled_switch_ends_a_silent_loop_at_its_next_charge() {
    // Counter-factual: a silent charge with no check spends on, and the
    // program answers 1000.
    let result = run(PROGRAM, "(spin-pull)");
    assert!(killed(&result), "the loop was not killed: {result:?}");
}

#[test]
fn a_kill_passes_a_protect() {
    // Counter-factual: a kill raised as a plain error is caught here, and
    // the program answers [false {:error :killed}].
    let result = run(PROGRAM, "(protect (spin-pull))");
    assert!(killed(&result), "the protect caught the kill: {result:?}");
}

#[test]
fn a_kill_passes_the_mask_of_a_metered_fiber() {
    // The fiber names :error in its mask, which catches any error but a
    // terminal one, and its budget is above the loop's spend.
    let result = run(
        PROGRAM,
        "(let [f (fiber/new (fn [] (spin-pull)) |:fuel :error|)]
           (fiber/set-fuel f 5000)
           (fiber/resume f)
           (fiber/status f))",
    );
    assert!(killed(&result), "the mask caught the kill: {result:?}");
}

#[cfg(feature = "jit")]
#[test]
fn a_pulled_switch_ends_a_compiled_silent_loop_at_its_next_charge() {
    // Counter-factual: a compiled silent charge with no check, or a check
    // whose cold block parks a frame the silent function never recorded,
    // either spins on or aborts the process.
    let result = run(PROGRAM, "(compile/run-on :jit spin-pull)");
    assert!(
        killed(&result),
        "the compiled loop was not killed: {result:?}"
    );
}

#[cfg(feature = "jit")]
#[test]
fn a_kill_in_compiled_code_passes_a_protect() {
    let result = run(PROGRAM, "(protect (compile/run-on :jit spin-pull))");
    assert!(killed(&result), "the protect caught the kill: {result:?}");
}
