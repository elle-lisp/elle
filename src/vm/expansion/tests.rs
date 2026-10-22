// audited: 2026-10-22
//! The expansion budget, one count that nested expansions share apart from
//! fiber fuel, and the capabilities an expansion withholds.
//!
//! docs/runtime.md
//! docs/macros.md
//!
//! Each test drives `at_expansion` with a body written in Rust. A nested
//! expansion written in Elle needs a transformer whose `eval` expands a macro,
//! and the expander cannot nest the allocation scope such a call opens.

use super::*;
use crate::syntax::UnitTag;
use crate::value::fiber::Fuel;

/// A unit for an expansion that calls no `gensym`.
fn unit() -> Unit {
    Unit::begin(UnitTag::of("<budget>", &[]))
}

/// An expansion nested inside another spends the outer one's budget, and only
/// the outermost closes it. The counter-factual opens a fresh budget for each
/// expansion: the inner one then finds the whole budget again.
#[test]
fn a_nested_expansion_spends_the_outer_budget() {
    let mut vm = VM::new();
    assert!(!vm.expanding(), "no expansion runs on a new VM");
    let mut inner_left = None;
    let mut open_after_inner = false;
    let outer = vm.at_expansion(&mut unit(), |vm| {
        for _ in 0..10 {
            assert!(vm.expansion_budget.charge(), "the budget holds ten units");
        }
        let inner = vm.at_expansion(&mut unit(), |vm| {
            inner_left = vm.expansion_budget.remaining();
            SignalBits::EMPTY
        });
        assert_eq!(inner, Ok(Value::NIL), "the inner expansion answers");
        open_after_inner = vm.expanding();
        SignalBits::EMPTY
    });
    assert_eq!(outer, Ok(Value::NIL), "the outer expansion answers");
    assert_eq!(
        inner_left,
        Some(EXPANSION_FUEL - 10),
        "the inner expansion finds what the outer one left"
    );
    assert!(
        open_after_inner,
        "the inner expansion leaves the budget open"
    );
    assert!(!vm.expanding(), "the outermost expansion closes the budget");
}

/// The fiber an expansion runs on has no budget of its own while the
/// expansion runs, and gets its budget back unspent. The counter-factual
/// charges the expansion to the fiber, which a fuel stop could then pause.
#[test]
fn an_expansion_puts_the_fibers_fuel_aside() {
    let mut vm = VM::new();
    vm.fiber.fuel = Fuel::budget(5).expect("a small budget");
    let mut during = Some(0);
    let answer = vm.at_expansion(&mut unit(), |vm| {
        during = vm.fiber.fuel.remaining();
        assert!(vm.expansion_budget.charge(), "the expansion spends its own");
        SignalBits::EMPTY
    });
    assert_eq!(answer, Ok(Value::NIL));
    assert_eq!(during, None, "the fiber has no budget during the expansion");
    assert_eq!(vm.fiber.fuel.remaining(), Some(5), "and gets its own back");
}

/// A charge that finds the budget spent halts the expansion, and the halt
/// answers the fuel fault rather than an error the compile would print as
/// raised. The counter-factual reads the halt's payload as a raised error.
#[test]
fn a_spent_budget_answers_the_fuel_fault() {
    let mut vm = VM::new();
    let answer = vm.at_expansion(&mut unit(), |vm| vm.halt_spent_expansion(0).0);
    assert_eq!(answer, Err(CompileTimeFault::Fuel));
    assert!(!vm.expanding(), "the halted expansion closes the budget");
}

/// An expansion withholds the effect capabilities and `:ambient` beside what its
/// fiber withholds, and the fiber gets its own set back when the expansion ends.
/// The counter-factual runs the transformer with the fiber's set alone, so a
/// macro reads a file wherever no fiber denied `:fs`, and the environment
/// wherever no fiber denied `:ambient`.
#[test]
fn an_expansion_withholds_the_effect_capabilities_until_it_ends() {
    use crate::signals::{SIG_EXEC, SIG_FFI, SIG_FS, SIG_GPU, SIG_IO, SIG_NET, SIG_OS_SIGNAL};
    let effects = [
        SIG_FFI,
        SIG_IO,
        SIG_EXEC,
        SIG_GPU,
        SIG_OS_SIGNAL,
        SIG_FS,
        SIG_NET,
        crate::signals::registry::SignalRegistry::with_builtins()
            .to_signal_bits("ambient")
            .expect("the registry names :ambient"),
    ];
    let mut vm = VM::new();
    vm.fiber.withheld = SIG_ERROR;
    let mut during = SignalBits::EMPTY;
    let answer = vm.at_expansion(&mut unit(), |vm| {
        during = vm.fiber.withheld;
        SignalBits::EMPTY
    });
    assert_eq!(answer, Ok(Value::NIL));
    for bit in effects {
        assert!(during.intersects(bit), "the expansion withholds {bit}");
    }
    assert!(during.intersects(SIG_ERROR), "and what the fiber withholds");
    assert_eq!(
        vm.fiber.withheld, SIG_ERROR,
        "the fiber gets its own set back"
    );
}
