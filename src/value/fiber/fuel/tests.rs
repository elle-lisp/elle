// audited: 2026-10-12
//! A budget holds any count a program can write, counts down one unit per
//! charge, and refuses the charge once spent; a silent function's spend never
//! refuses; no budget never runs out.
//!
//! docs/runtime.md

use super::*;

/// The largest integer, the largest budget an Elle program can write.
const LARGEST: u64 = i64::MAX as u64;

#[test]
fn the_largest_integer_is_a_budget_that_reads_back_whole() {
    // Counter-factual: a budget kept in 32 bits reads this back as
    // 4294967295, and 2^32 as 0, so a fiber given it pauses at once.
    let fuel = Fuel::budget(LARGEST).expect("the largest integer is a budget");
    assert_eq!(fuel.remaining(), Some(LARGEST));
    let fuel = Fuel::budget(1 << 32).expect("2^32 is a budget");
    assert_eq!(fuel.remaining(), Some(1 << 32));
}

#[test]
fn a_count_past_the_largest_integer_is_no_budget() {
    // No program can write it, and a word that high reads as unmetered.
    assert_eq!(Fuel::budget(LARGEST + 1), None);
    assert_eq!(Fuel::budget(u64::MAX), None);
}

#[test]
fn each_charge_takes_one_unit_until_the_budget_is_spent() {
    let mut fuel = Fuel::budget(2).expect("a budget of 2");
    assert!(fuel.charge());
    assert_eq!(fuel.remaining(), Some(1));
    assert!(fuel.charge());
    assert_eq!(fuel.remaining(), Some(0));
    assert!(!fuel.charge(), "a spent budget refuses the charge");
    assert_eq!(fuel.remaining(), Some(0), "a refused charge takes nothing");
}

#[test]
fn a_charge_from_the_largest_budget_leaves_one_less() {
    let mut fuel = Fuel::budget(LARGEST).expect("the largest integer is a budget");
    assert!(fuel.charge());
    assert_eq!(fuel.remaining(), Some(LARGEST - 1));
}

#[test]
fn an_unmetered_fiber_accepts_every_charge_and_stays_unmetered() {
    let mut fuel = Fuel::unmetered();
    for _ in 0..1000 {
        assert!(fuel.charge());
    }
    assert_eq!(fuel.remaining(), None);
    assert_eq!(Fuel::default(), Fuel::unmetered());
}

#[test]
fn a_spend_takes_one_unit_while_one_is_left_and_never_refuses() {
    // Counter-factual: a silent function that spends nothing leaves the budget
    // where its caller's charge put it, so the pause that should follow the
    // silent call never comes.
    let mut fuel = Fuel::budget(2).expect("a budget of 2");
    fuel.spend();
    assert_eq!(fuel.remaining(), Some(1));
    fuel.spend();
    assert_eq!(fuel.remaining(), Some(0));
    fuel.spend();
    assert_eq!(fuel.remaining(), Some(0), "a spent budget stays at zero");
    assert!(!fuel.charge(), "and the next charge refuses");
}

#[test]
fn an_unmetered_fiber_stays_unmetered_whatever_it_spends() {
    let mut fuel = Fuel::unmetered();
    for _ in 0..1000 {
        fuel.spend();
    }
    assert_eq!(fuel.remaining(), None);
}
