// audited: 2026-11-21
//! The rules a native's declaration keeps, checked on hand-built declarations and on every registered one.
//!
//! docs/spec/signature.md

use super::{Kind::*, Mode::*, Place, Spec};
use crate::value::types::Arity;

/// Whether `check` refuses `spec` for a native of `arity`.
fn refused(spec: Spec, arity: Arity) -> bool {
    spec.check(arity).is_err()
}

#[test]
fn a_fixed_declaration_with_a_mode_per_parameter_is_kept() {
    let insert = Spec::fixed(&[Returned, Borrowed, Stored(Place::Param(0))], Itself(0));
    assert_eq!(insert.check(Arity::Exact(3)), Ok(()));
}

#[test]
fn a_rest_declaration_over_a_range_is_kept() {
    let resume = Spec::rest(&[Resumed], Crosses, Payload(0));
    assert_eq!(resume.check(Arity::Range(1, 2)), Ok(()));
}

#[test]
fn a_mode_count_unlike_the_arity_is_refused() {
    assert!(refused(
        Spec::fixed(&[Borrowed], Immediate),
        Arity::Exact(2)
    ));
    assert!(refused(
        Spec::rest(&[Borrowed], Borrowed, Fresh),
        Arity::AtLeast(0)
    ));
}

#[test]
fn a_rest_on_a_fixed_count_is_refused() {
    assert!(refused(
        Spec::rest(&[Borrowed], Borrowed, Immediate),
        Arity::Exact(1)
    ));
}

#[test]
fn an_arity_with_no_fixed_count_needs_a_rest() {
    assert!(refused(
        Spec::fixed(&[Borrowed], Immediate),
        Arity::AtLeast(1)
    ));
    assert!(refused(
        Spec::fixed(&[Borrowed], Immediate),
        Arity::Range(1, 2)
    ));
}

#[test]
fn itself_names_a_returned_fixed_parameter() {
    assert!(refused(
        Spec::fixed(&[Borrowed], Itself(0)),
        Arity::Exact(1)
    ));
    assert!(refused(
        Spec::fixed(&[Returned], Itself(1)),
        Arity::Exact(1)
    ));
}

#[test]
fn itself_names_the_one_returned_parameter() {
    assert!(refused(
        Spec::fixed(&[Returned, Returned], Itself(0)),
        Arity::Exact(2)
    ));
    assert!(refused(
        Spec::rest(&[Returned], Returned, Itself(0)),
        Arity::AtLeast(1)
    ));
}

#[test]
fn a_returned_parameter_needs_a_result_that_borrows() {
    assert!(refused(
        Spec::fixed(&[Returned], Immediate),
        Arity::Exact(1)
    ));
    assert!(refused(
        Spec::fixed(&[Returned], Payload(0)),
        Arity::Exact(1)
    ));
}

#[test]
fn an_alias_borrows_from_a_returned_parameter() {
    assert!(refused(Spec::fixed(&[Borrowed], Alias), Arity::Exact(1)));
}

#[test]
fn a_payload_names_a_fixed_parameter() {
    assert!(refused(
        Spec::fixed(&[Borrowed], Payload(1)),
        Arity::Exact(1)
    ));
}

#[test]
fn a_store_names_another_fixed_parameter() {
    assert!(refused(
        Spec::fixed(&[Stored(Place::Param(0))], Immediate),
        Arity::Exact(1)
    ));
    let past = Spec::fixed(&[Borrowed, Stored(Place::Param(2))], Immediate);
    assert!(refused(past, Arity::Exact(2)));
}

#[test]
fn a_store_into_the_result_needs_a_fresh_result() {
    let into = |result| Spec::rest(&[], Stored(Place::Result), result);
    assert!(refused(into(Immediate), Arity::AtLeast(0)));
    assert_eq!(into(Fresh).check(Arity::AtLeast(0)), Ok(()));
}

#[test]
fn every_registered_declaration_keeps_the_rules() {
    for def in crate::primitives::registration::canonical_defs() {
        if let Some(spec) = def.spec {
            assert_eq!(spec.check(def.arity), Ok(()), "{}", def.name);
        }
    }
}
