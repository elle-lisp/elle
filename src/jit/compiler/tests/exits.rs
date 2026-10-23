// audited: 2026-10-23
// docs/impl/jit.md
// docs/impl/jit-fuel.md
// docs/impl/region/unwind.md
//! What a compiled call emits around itself: the fuel charge before it, the
//! exits after it, and the region-map pop before each exit.

use super::clif::{call_target_before, called_refs, func_refs};
use super::*;

/// fn(f) -> f(), with `signal`. In a function that may signal, a `Call` makes
/// the translator emit four exits: the charge's kill and its pause, the park
/// after the call's signal check, and the normal return.
fn make_call_lir(signal: Signal) -> LirOwned {
    use crate::hir::region::StaticRegion;
    LirFixture::new(Arity::Exact(1))
        .signal(signal)
        .block(
            0,
            &[
                InstrRef::LoadCapture {
                    dst: Reg(0),
                    index: 0,
                },
                InstrRef::Call {
                    dst: Reg(1),
                    func: Reg(0),
                    args: &[],
                    arity_checked: false,
                    region: StaticRegion::new(1).unwrap(),
                },
            ],
            Terminator::Return(Reg(1)),
        )
        .emitted_sites()
        .build()
}

/// The module function ids a rendered function calls, in emission order.
fn called_ids(clif: &[String]) -> Vec<u32> {
    let refs = func_refs(clif);
    called_refs(clif)
        .iter()
        .filter_map(|name| refs.get(name).copied())
        .collect()
}

/// Every `return` a compiled function emits is preceded by the call that pops
/// this activation's region-remap frame, so the prologue's push is balanced on
/// every path out (docs/impl/region/unwind.md).
#[test]
fn every_compiled_exit_pops_the_region_map() {
    // Trap: a missing pop is invisible to the compiled function itself. It
    // returns the right value; what it leaves behind is a map frame that
    // `last()` then names for the INTERPRETER activation above it, which parks
    // and releases against a frame that was never its own — and the remap stack
    // never shrinks back.
    //
    // Counter-factual: an exit that returns straight from the park helper.
    // Every corpus test that suspends through a compiled frame passes its own
    // assertions, and only the balance check in `execute_bytecode_saving_stack`
    // says anything at all: in a debug build, once the enclosing activation
    // returns.
    let compiler = verified();
    let pop_id = compiler.helpers.pop_region_map.as_u32();
    let clif = compiler
        .clif_text(&make_call_lir(Signal::yields()).view())
        .expect("Failed to translate");
    let refs = func_refs(&clif);

    let returns: Vec<usize> = clif
        .iter()
        .enumerate()
        .filter(|(_, line)| line.trim().starts_with("return"))
        .map(|(i, _)| i)
        .collect();
    // One per exit: the charge's kill, the charge's pause, the signal
    // check's park, and the terminator's.
    assert_eq!(
        returns.len(),
        4,
        "a Call in a function that may signal has four exits; got:\n{}",
        clif.join("\n")
    );

    for i in returns {
        let target = call_target_before(&clif, i).unwrap_or_else(|| {
            panic!(
                "`{}` is not preceded by any call in its block:\n{}",
                clif[i].trim(),
                clif.join("\n")
            )
        });
        assert_eq!(
            refs.get(&target).copied(),
            Some(pop_id),
            "`{}` is preceded by `{target}`, not by the region-map pop \
             (u0:{pop_id}) — this exit leaks its activation's remap frame:\n{}",
            clif[i].trim(),
            clif.join("\n")
        );
    }
}

/// A function that may signal charges fuel at a call and parks where the
/// budget runs out (docs/impl/jit-fuel.md).
#[test]
fn a_call_that_may_pause_reaches_the_fuel_pause() {
    // Counter-factual: a translator that charges nothing compiles every
    // program to the same answers, and a fueled fiber running the code never
    // pauses.
    let compiler = verified();
    let pause_id = compiler.helpers.fuel_pause.as_u32();
    let clif = compiler
        .clif_text(&make_call_lir(Signal::errors()).view())
        .expect("Failed to translate");
    let called = called_ids(&clif);
    assert!(
        called.contains(&pause_id),
        "a charge in a function that may signal must reach `elle_jit_fuel_pause` \
         (u0:{pause_id}); got {called:?} in:\n{}",
        clif.join("\n")
    );
}

/// A silent function's charge never leaves the function: it records no charge
/// site, so nothing in it reaches the fuel pause (docs/signals/inference.md).
#[test]
fn a_silent_functions_charge_never_pauses() {
    // Counter-factual: a pausing charge in a silent function parks a frame its
    // silent caller cannot park behind, since a silent function records no call
    // site, and the pause leaves the caller as a broken proof.
    let compiler = verified();
    let pause_id = compiler.helpers.fuel_pause.as_u32();
    let clif = compiler
        .clif_text(&make_call_lir(Signal::silent()).view())
        .expect("Failed to translate");
    let called = called_ids(&clif);
    assert!(
        !called.contains(&pause_id),
        "a silent function must not reach `elle_jit_fuel_pause` (u0:{pause_id}); \
         got {called:?} in:\n{}",
        clif.join("\n")
    );
}
