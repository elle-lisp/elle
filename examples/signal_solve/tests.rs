// audited: 2026-11-16
//! The rules on models built by hand: a ceiling that checks, each kind of violation, and a squelch, in both engines.
//!
//! docs/impl/solver.md
//!
//! An analyzed file reaches the solver only when the analyzer accepts its
//! ceilings, so no fixture can hold a violation. These models hold one each.

use elle::hir::solve::{bit_positions, lower, Model, Solution, VarId, VarKey};
use elle::signals::{CAP_MASK, SIG_ERROR, SIG_HALT, SIG_YIELD};

/// The bit value of a one-bit signal word.
fn bit(word: elle::value::fiber::SignalBits) -> u32 {
    word.raw().trailing_zeros()
}

/// Solve with both engines, require them to agree, and answer the model.
fn solve(model: &Model) -> Solution {
    let lowered = lower(model);
    let worklist = elle::hir::solve::solve(model, &lowered).solution(model);
    let datafrog = crate::crosscheck::solve(model, &lowered);
    assert_eq!(worklist, datafrog, "the two engines solve one model");
    worklist
}

/// A lambda of file 0 that declares a ceiling of `allowed` bits.
fn ceiled(model: &mut Model, id: u32, allowed: &[u32]) -> VarId {
    let lam = model.var(VarKey::Lam(0, id));
    model.facts.ceiled.insert(lam);
    for &b in allowed {
        model.facts.ceil.insert((lam, b));
    }
    lam
}

/// A ceiling wider than its body leaves the body's bits, and a caller reads
/// those. The counter-factual replaces the bits with the ceiling, so the
/// caller would read the :error the ceiling admits and the body never raises.
#[test]
fn a_ceiling_checks_and_does_not_replace() {
    let mut model = Model::default();
    let body = ceiled(&mut model, 1, &[bit(SIG_YIELD), bit(SIG_ERROR)]);
    model.facts.raw.insert((body, bit(SIG_YIELD)));
    let caller = model.var(VarKey::Lam(0, 2));
    let site = model.site();
    model.use_fact(caller, site, body, body, 0);
    let s = solve(&model);
    for v in [body, caller] {
        let bits: Vec<u32> = s.bits.iter().filter(|t| t.0 == v).map(|t| t.1).collect();
        assert_eq!(bits, vec![bit(SIG_YIELD)], "only the body's :yield");
    }
    assert!(s.viol.is_empty(), "the body fits its ceiling");
    assert!(s.vdep.is_empty(), "the body depends on nothing");
}

/// A body that raises a bit its ceiling does not admit violates it. The
/// counter-factual drops the check with the replacement, and finds nothing.
#[test]
fn a_bit_past_the_ceiling_is_a_violation() {
    let mut model = Model::default();
    let body = ceiled(&mut model, 1, &[]);
    model.facts.raw.insert((body, bit(SIG_YIELD)));
    let s = solve(&model);
    assert_eq!(
        s.viol.iter().copied().collect::<Vec<_>>(),
        vec![(body, bit(SIG_YIELD))]
    );
}

/// A ceiled body that calls its own parameter depends on what its caller
/// passes, which no ceiling admits. The counter-factual drops the body's
/// free variables under a ceiling, and finds nothing.
#[test]
fn a_ceiled_body_that_passes_on_a_parameter_is_a_violation() {
    let mut model = Model::default();
    let body = ceiled(&mut model, 1, &[]);
    model.nparams.insert(body, 1);
    let param = model.var(VarKey::Param(body, 0));
    let site = model.site();
    model.use_fact(body, site, param, param, 0);
    let s = solve(&model);
    assert_eq!(
        s.vdep.iter().copied().collect::<Vec<_>>(),
        vec![(body, param)]
    );
    assert!(
        s.dep.contains(&(body, param)),
        "a caller still sees the dependency"
    );
}

/// A squelch over a target that may raise every bit raises each user-facing
/// bit its mask does not hold, and `:error`. The mask here is what
/// `(attune :yield f)` blocks: every user-facing bit but `:yield`, and never
/// `:error` or `:halt`, which cross every boundary. The counter-factual lets
/// `TOP` pass the mask whole, because the mask lists bits one by one and never
/// names `TOP`, so the squelch raised every bit.
#[test]
fn a_squelch_over_every_bit_raises_what_its_mask_lets_through() {
    let mut model = Model::default();
    let target = model.unknown();
    let site = model.site();
    let squelch = model.var(VarKey::Sq(site));
    model.facts.sqof.insert((squelch, target));
    let blocked = CAP_MASK.subtract(SIG_YIELD.union(SIG_ERROR).union(SIG_HALT));
    for b in bit_positions(blocked.raw()) {
        model.facts.sqm.insert((squelch, b));
    }
    let s = solve(&model);
    let bits: Vec<u32> = s
        .bits
        .iter()
        .filter(|t| t.0 == squelch)
        .map(|t| t.1)
        .collect();
    assert_eq!(bits, vec![bit(SIG_ERROR), bit(SIG_YIELD), bit(SIG_HALT)]);
}
