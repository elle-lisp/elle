// audited: 2026-11-22
//! A call to a primitive declared to run no Elle code keeps the guard's fact on a shared binding.
//!
//! src/hir/overview.md
//! docs/impl/typeinfer/facts.md
//!
//! Each program holds its guard and its op inside a lambda that captures a
//! binding its owner writes, so a call between them ends the fact unless the
//! callee runs no Elle code.

use super::super::{compile_booted, run_booted};
use crate::value::Value;

/// The source of each program, with `CALL` standing between the guard and
/// the op it proves.
const TYPED: &str = "(defn tagged [@x]
                       (assign x 1)
                       (fn [] (when (%not (%int? x)) (error :x)) CALL (%add x 1)))
                     ((tagged 0))";
const NONZERO: &str = "(defn q [@d]
                         (assign d 2)
                         (fn [] (if (%int? d) (if (%ne d 0) (begin CALL (%div 10 d)) 0) 0)))
                       ((q 0))";

/// `type-of` calls no closure and resumes no fiber, so the int fact and the
/// nonzero fact both stand at the op after it. The counter-factual: every call
/// ended the facts about a shared binding, and each op was refused.
#[test]
fn a_call_to_a_primitive_that_runs_no_elle_keeps_a_fact() {
    let typed = TYPED.replace("CALL", "(type-of :k)");
    assert_eq!(run_booted(&typed), Value::int(2), "{typed}");
    let nonzero = NONZERO.replace("CALL", "(type-of :k)");
    assert_eq!(run_booted(&nonzero), Value::int(5), "{nonzero}");
}

/// `length` may run a trait's method, and that method may resume the fiber
/// where the owner is parked. So a call to it still ends both facts, and each
/// op is refused. The counter-factual is a rule that kept every fact across a
/// call to any primitive.
#[test]
fn a_call_to_a_primitive_that_may_run_elle_ends_a_fact() {
    for (src, op) in [(TYPED, "%add"), (NONZERO, "%div")] {
        let src = src.replace("CALL", "(length [1])");
        let err = compile_booted(&src)
            .map(|_| ())
            .expect_err("the call may let the owner write the binding");
        assert!(err.contains(op), "{src}: {err}");
    }
}
