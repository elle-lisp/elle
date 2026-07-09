use super::*;

fn var(b: u32) -> HirPattern {
    HirPattern::Var(Binding(b))
}

#[test]
fn wildcard_does_not_allocate() {
    assert!(!HirPattern::Wildcard.allocates());
}

#[test]
fn var_does_not_allocate() {
    assert!(!var(0).allocates());
}

#[test]
fn list_rest_does_not_allocate() {
    // (rest list) returns the cdr pointer — no fresh allocation.
    let p = HirPattern::List {
        elements: vec![var(0)],
        rest: Some(Box::new(var(1))),
    };
    assert!(!p.allocates());
}

#[test]
fn array_rest_allocates() {
    let p = HirPattern::Array {
        elements: vec![var(0)],
        rest: Some(Box::new(var(1))),
    };
    assert!(p.allocates());
}

#[test]
fn tuple_rest_allocates() {
    let p = HirPattern::Tuple {
        elements: vec![var(0)],
        rest: Some(Box::new(var(1))),
    };
    assert!(p.allocates());
}

#[test]
fn struct_rest_allocates() {
    let p = HirPattern::Struct {
        entries: vec![(PatternKey::Keyword("a".to_string()), var(0))],
        rest: Some(Box::new(var(1))),
    };
    assert!(p.allocates());
}

#[test]
fn table_rest_allocates() {
    let p = HirPattern::Table {
        entries: vec![(PatternKey::Keyword("a".to_string()), var(0))],
        rest: Some(Box::new(var(1))),
    };
    assert!(p.allocates());
}

#[test]
fn nested_allocating_pattern_propagates() {
    // (match v ((:a (@[x & rest]))) ...) — inner Array.rest allocates,
    // so the outer Struct's allocates() must propagate the alloc out.
    let inner = HirPattern::Array {
        elements: vec![var(0)],
        rest: Some(Box::new(var(1))),
    };
    let outer = HirPattern::Struct {
        entries: vec![(PatternKey::Keyword("a".to_string()), inner)],
        rest: None,
    };
    assert!(outer.allocates());
}

#[test]
fn or_pattern_propagates_alloc() {
    let alts = vec![
        var(0),
        HirPattern::Array {
            elements: vec![],
            rest: Some(Box::new(var(1))),
        },
    ];
    assert!(HirPattern::Or(alts).allocates());
}
