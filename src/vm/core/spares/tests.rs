// audited: 2026-11-25
//! A spare comes back empty, and each pool keeps only what its bounds admit.
//!
//! docs/impl/spares.md

use super::{Spares, KEPT, ROOM};
use crate::value::Value;
use std::rc::Rc;

/// A vector with room for `room` values, holding one.
fn vec_with_room(room: usize) -> Vec<Value> {
    let mut v = Vec::with_capacity(room);
    v.push(Value::int(1));
    v
}

// The counter-factual for the next two is a pool that keeps nothing: every
// take is then a new vector, with no room.

#[test]
fn a_given_vec_comes_back_empty() {
    let mut spares = Spares::default();
    spares.give_vec(vec_with_room(8));
    let taken = spares.vec();
    assert!(taken.is_empty(), "a spare comes back empty");
    assert!(taken.capacity() >= 8, "the spare is the vector given back");
}

#[test]
fn an_unshared_env_comes_back_empty() {
    let mut spares = Spares::default();
    spares.give_env(Rc::new(vec_with_room(8)));
    let mut taken = spares.env();
    let env = Rc::get_mut(&mut taken).expect("nothing else holds a spare");
    assert!(env.is_empty(), "a spare comes back empty");
    assert!(
        env.capacity() >= 8,
        "the spare is the environment given back"
    );
}

// The counter-factual for the bounds is a pool that keeps whatever it is
// given: a shared environment would then be cleared under its other holder,
// and a deep recursion's stacks would stay held after it returns.

#[test]
fn a_shared_env_is_not_kept() {
    let mut spares = Spares::default();
    let given = Rc::new(vec![Value::int(1)]);
    let holder = given.clone();
    spares.give_env(given);
    assert_eq!(
        *holder,
        [Value::int(1)],
        "the other holder's environment is intact"
    );
    let taken = spares.env();
    assert!(
        !Rc::ptr_eq(&taken, &holder),
        "a shared environment is no spare"
    );
}

#[test]
fn a_vec_past_the_room_bound_is_freed() {
    let mut spares = Spares::default();
    spares.give_vec(vec_with_room(ROOM + 1));
    assert_eq!(spares.vec().capacity(), 0, "the oversized vector was kept");
}

#[test]
fn a_pool_keeps_at_most_its_bound() {
    let mut spares = Spares::default();
    for _ in 0..KEPT + 1 {
        spares.give_vec(vec_with_room(4));
    }
    let kept = std::iter::repeat_with(|| spares.vec())
        .take(KEPT + 1)
        .filter(|v| v.capacity() > 0)
        .count();
    assert_eq!(kept, KEPT, "the pool kept {kept} vectors");
}
