// audited: 2026-11-24
//! Placement pins for a tail call a `break` carries: each release on the break's path goes ahead of the call.
//!
//! docs/impl/region/breakpoint.md

use super::*;

// A `break` out of a function's tail block walks its value as a tail value, so
// a closure call there replaces the frame before the jump. The releases the
// break's path owes, those the jump passes over and those past the block's
// exit label, run on the closure path only as replicas ahead of that call. The
// counts are the same with or without the replica on the native path, so only
// where each release sits tells the two apart.

/// The function `rd` the subject defines, which must make a `TailCall`: a
/// subject with none releases nothing ahead of a call, and would pass or fail
/// for that reason alone.
fn carrying_function(module: &FrozenModule) -> &LirOwned {
    let rd = named(module, "rd");
    assert!(
        tail_call_blocks_of(rd).next().is_some(),
        "the call the break carries was not lowered as a TailCall",
    );
    rd
}

/// The local slots `func`'s `TailCall` blocks release by value ahead of the
/// call, and the slots its other blocks release.
fn ahead_of_the_call_and_elsewhere(func: &LirOwned) -> (Vec<u16>, Vec<u16>) {
    let (mut ahead, mut elsewhere) = (Vec::new(), Vec::new());
    for b in func.view().blocks() {
        match tail_call_at(&b) {
            Some(at) => ahead.extend(
                value_releases(b.instrs())
                    .into_iter()
                    .filter(|&(idx, _)| idx < at)
                    .map(|(_, slot)| slot),
            ),
            None => elsewhere.extend(value_released(b.instrs())),
        }
    }
    (ahead, elsewhere)
}

/// The local slots `func`'s `TailCall`s load their operands from.
fn tail_call_operand_slots(func: &LirOwned) -> Vec<u16> {
    let mut out = Vec::new();
    for b in func.view().blocks() {
        let mut loaded: rustc_hash::FxHashMap<Reg, u16> = rustc_hash::FxHashMap::default();
        for i in b.instrs() {
            match i {
                InstrRef::LoadLocal { dst, slot } => {
                    loaded.insert(dst, slot);
                }
                InstrRef::TailCall { args, .. } => {
                    out.extend(args.iter().filter_map(|r| loaded.get(r).copied()));
                }
                _ => {}
            }
        }
    }
    out
}

#[test]
fn a_release_past_the_exit_label_goes_ahead_of_the_carried_call() {
    // `held` is bound outside the loop, so its release lands at the `let`'s
    // scope exit, past the block's exit label. The closure path of `(h m)`
    // never gets there.
    let module = compile_to_lir(
        "(begin (def mk (fn () {:type :data :n 1}))
                (def rd (fn (h m)
                  (let [held (mk)]
                    (while true
                      (if (= (get held :n) m) (break (h m)) nil)))))
                (rd (fn (x) x) 1))",
    );
    let (ahead, elsewhere) = ahead_of_the_call_and_elsewhere(carrying_function(&module));
    assert!(
        ahead.iter().any(|s| elsewhere.contains(s)),
        "no release past the exit label went ahead of the call the break \
         carries — the closure path strands the region once per call \
         (ahead {ahead:?}, elsewhere {elsewhere:?})",
    );
}

#[test]
fn a_release_the_break_jumps_over_goes_ahead_of_the_carried_call() {
    // `x` is made on each pass and released after the branch, a position the
    // break jumps over. The break's own point sits after the call, which the
    // closure path never reaches.
    let module = compile_to_lir(
        "(begin (def mk (fn () {:type :data :n 1}))
                (def rd (fn (h m)
                  (while true
                    (let [x (mk)]
                      (if (= m 1) (break (h m)) nil)
                      (get x :n)))))
                (rd (fn (y) y) 1))",
    );
    let (ahead, elsewhere) = ahead_of_the_call_and_elsewhere(carrying_function(&module));
    assert!(
        ahead.iter().any(|s| elsewhere.contains(s)),
        "no release the break jumps over went ahead of the call it carries — \
         the breaking pass strands its value on the closure path \
         (ahead {ahead:?}, elsewhere {elsewhere:?})",
    );
}

#[test]
fn an_operand_of_the_carried_call_keeps_its_release_behind_it() {
    // The exemption. `held` is the call's argument, so the release behind the
    // call is the ownership move the callee's parameter release consumes. A
    // copy ahead of the call frees what the callee is about to take.
    let module = compile_to_lir(
        "(begin (def mk (fn () {:type :data :n 1}))
                (def rd (fn (h m)
                  (let [held (mk)]
                    (while true
                      (if (= (get held :n) m) (break (h held)) nil)))))
                (rd (fn (x) x) 1))",
    );
    let rd = carrying_function(&module);
    let operands = tail_call_operand_slots(rd);
    let (ahead, _) = ahead_of_the_call_and_elsewhere(rd);
    assert!(
        !operands.is_empty(),
        "the subject makes no tail call with a slot operand",
    );
    assert!(
        !ahead.iter().any(|s| operands.contains(s)),
        "an operand of the call the break carries was released ahead of it — \
         the callee takes a freed value (operands {operands:?}, ahead {ahead:?})",
    );
}
