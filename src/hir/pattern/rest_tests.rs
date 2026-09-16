// audited: 2026-09-16
// ── What a rest sub-pattern builds, and who reaches it ───────────
//
// THE TRAP these guard. The solver and every lowering of a rest read these
// three predicates, and they read them for different purposes: one counts the
// builds, one names the collection's holders, one decides whether a build is
// emitted at all. A disagreement is a collection with no release route (an
// over-keep), a release route with no collection (nothing emitted), or — for
// the count — every later build keyed on another build's region.
//
// docs/impl/region/anchors.md

use super::*;

fn b(n: u32) -> Binding {
    Binding(n)
}

fn var(n: u32) -> HirPattern {
    HirPattern::Var(b(n))
}

fn key() -> PatternKey {
    PatternKey::Keyword("a".to_string())
}

fn wild() -> Option<Box<HirPattern>> {
    Some(Box::new(HirPattern::Wildcard))
}

#[test]
fn own_rest_builds_skips_a_wildcard_rest_wherever_the_check_is_made_elsewhere() {
    assert!(
        HirPattern::Tuple {
            elements: vec![var(1)],
            rest: Some(Box::new(var(2))),
        }
        .own_rest_builds(),
        "a named rest is read, so it is built"
    );
    assert!(
        !HirPattern::Tuple {
            elements: vec![var(1)],
            rest: wild(),
        }
        .own_rest_builds(),
        "`[a & _]` reads nothing, and the fixed element makes the type check"
    );
    assert!(
        HirPattern::Tuple {
            elements: vec![],
            rest: wild(),
        }
        .own_rest_builds(),
        "`[& _]` has no fixed element, so only the build checks the type"
    );
    assert!(
        !HirPattern::Array {
            elements: vec![var(1)],
            rest: wild(),
        }
        .own_rest_builds(),
        "an `@array` rest reaches the same opcode"
    );
    assert!(
        !HirPattern::Struct {
            entries: vec![],
            rest: wild(),
        }
        .own_rest_builds(),
        "`StructRest` signals on no input, so a keyed pattern always skips"
    );
    assert!(
        !HirPattern::Table {
            entries: vec![(key(), var(1))],
            rest: wild(),
        }
        .own_rest_builds(),
        "an `@struct` rest reaches the same opcode"
    );
    assert!(
        !HirPattern::List {
            elements: vec![],
            rest: Some(Box::new(var(2))),
        }
        .own_rest_builds(),
        "a list rest is the remaining cons tail, so it builds nothing at all"
    );
    assert!(
        !HirPattern::Tuple {
            elements: vec![var(1)],
            rest: None,
        }
        .own_rest_builds(),
        "no rest, nothing built"
    );
}

#[test]
fn the_allocating_rest_predicate_names_exactly_the_building_patterns() {
    let count = |p: &HirPattern| p.allocating_rest_bindings().len();

    assert_eq!(
        count(&HirPattern::Tuple {
            elements: vec![var(1)],
            rest: Some(Box::new(var(2))),
        }),
        1,
        "an array rest lowers to ArrayMutSliceFrom"
    );
    assert_eq!(
        count(&HirPattern::Array {
            elements: vec![],
            rest: Some(Box::new(var(2))),
        }),
        1,
        "an `@array` rest lowers to the same opcode"
    );
    assert_eq!(
        count(&HirPattern::Struct {
            entries: vec![],
            rest: Some(Box::new(var(2))),
        }),
        1,
        "a struct rest lowers to StructRest"
    );
    assert_eq!(
        count(&HirPattern::Table {
            entries: vec![],
            rest: Some(Box::new(var(2))),
        }),
        1,
        "an `@struct` rest lowers to StructRest"
    );
    assert_eq!(
        count(&HirPattern::List {
            elements: vec![var(1)],
            rest: Some(Box::new(var(2))),
        }),
        0,
        "a list rest is the remaining cons tail"
    );
    assert_eq!(
        count(&HirPattern::Tuple {
            elements: vec![var(1)],
            rest: None,
        }),
        0,
        "no rest, nothing built"
    );
    assert_eq!(
        count(&HirPattern::Tuple {
            elements: vec![],
            rest: wild(),
        }),
        0,
        "a wildcard rest binds no name to the collection"
    );
    assert_eq!(
        count(&HirPattern::Tuple {
            elements: vec![],
            rest: Some(Box::new(HirPattern::Tuple {
                elements: vec![var(1)],
                rest: Some(Box::new(var(2))),
            })),
        }),
        1,
        "a nested rest contributes only the inner bare name it does bind"
    );
    assert_eq!(
        count(&HirPattern::Tuple {
            elements: vec![HirPattern::Struct {
                entries: vec![],
                rest: Some(Box::new(var(1))),
            }],
            rest: Some(Box::new(var(2))),
        }),
        2,
        "a rest inside an element and the pattern's own rest are two builds"
    );
}

#[test]
fn the_building_rests_list_leaves_out_what_no_lowering_emits() {
    // THE COUNTER-FACTUAL. The lowerer takes the n-th placeholder at the n-th
    // build, so a rest it SKIPS must be absent here: counted, it would shift
    // every later build onto another one's region.
    let len = |p: &HirPattern| p.building_rests().len();

    assert_eq!(
        len(&HirPattern::Tuple {
            elements: vec![var(1)],
            rest: wild(),
        }),
        0,
        "`[a & _]` emits no build, so it contributes no placeholder"
    );
    assert_eq!(
        len(&HirPattern::Tuple {
            elements: vec![],
            rest: wild(),
        }),
        1,
        "`[& _]` keeps the build its type check needs"
    );
    assert_eq!(
        len(&HirPattern::Tuple {
            elements: vec![var(1)],
            rest: Some(Box::new(HirPattern::Tuple {
                elements: vec![],
                rest: Some(Box::new(var(2))),
            })),
        }),
        2,
        "`[a & [& q]]` builds the outer collection and the inner one"
    );
}

#[test]
fn the_holder_set_of_a_rest_sub_pattern_stops_at_a_further_build() {
    assert_eq!(
        var(1).rest_collection_holders(),
        vec![b(1)],
        "a bare name is the collection's one holder"
    );
    assert_eq!(
        HirPattern::Wildcard.rest_collection_holders(),
        vec![],
        "a wildcard binds nothing to key on"
    );
    assert_eq!(
        HirPattern::Tuple {
            elements: vec![var(1), var(2)],
            rest: None,
        }
        .rest_collection_holders(),
        vec![b(1), b(2)],
        "every name a further pattern binds projects the collection"
    );
    assert_eq!(
        HirPattern::Tuple {
            elements: vec![var(1)],
            rest: Some(Box::new(var(2))),
        }
        .rest_collection_holders(),
        vec![b(1)],
        "a further BUILD's names reach the inner collection, so the descent \
         stops there"
    );
    assert_eq!(
        HirPattern::Tuple {
            elements: vec![],
            rest: Some(Box::new(var(2))),
        }
        .rest_collection_holders(),
        vec![],
        "`[& q]` leaves the outer collection with no holder at all"
    );
    assert_eq!(
        HirPattern::List {
            elements: vec![var(1)],
            rest: Some(Box::new(var(2))),
        }
        .rest_collection_holders(),
        vec![b(1), b(2)],
        "a list rest is the remaining cons tail, so there is no build to stop at"
    );
    assert_eq!(
        HirPattern::Struct {
            entries: vec![(key(), var(1))],
            rest: Some(Box::new(var(2))),
        }
        .rest_collection_holders(),
        vec![b(1)],
        "`StructRest` builds exactly as `ArrayMutSliceFrom` does"
    );
}
