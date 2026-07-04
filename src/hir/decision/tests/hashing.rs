use super::*;
use crate::hir::decision::*;

// ── Hash/Eq consistency tests ──────────────────────────────────────

#[test]
fn test_pattern_literal_eq_implies_same_hash() {
    // Equal values must produce equal hashes (Hash contract).
    let pairs: &[(PatternLiteral, PatternLiteral)] = &[
        (PatternLiteral::Bool(true), PatternLiteral::Bool(true)),
        (PatternLiteral::Bool(false), PatternLiteral::Bool(false)),
        (PatternLiteral::Int(42), PatternLiteral::Int(42)),
        (PatternLiteral::Int(-1), PatternLiteral::Int(-1)),
        (PatternLiteral::Float(1.5), PatternLiteral::Float(1.5)),
        (
            PatternLiteral::String("hello".into()),
            PatternLiteral::String("hello".into()),
        ),
        (
            PatternLiteral::Keyword("ok".into()),
            PatternLiteral::Keyword("ok".into()),
        ),
    ];
    for (a, b) in pairs {
        assert_eq!(a, b, "equality failed for {:?}", a);
        assert_eq!(
            hash_of(a),
            hash_of(b),
            "hash mismatch for equal values {:?}",
            a
        );
    }
}
#[test]
fn test_pattern_literal_float_uses_bits() {
    // Hash must be derived from bit representation, not f64 equality.
    // Two values constructed from the same bits must have the same hash,
    // even for values that don't satisfy f64 equality (e.g. NaN).
    let nan = f64::NAN;
    let a = PatternLiteral::Float(nan);
    let b = PatternLiteral::Float(f64::from_bits(nan.to_bits()));
    // NaN != NaN under PartialEq, so we do NOT assert_eq!(a, b).
    // But their hashes must agree because to_bits() is the same.
    assert_eq!(hash_of(&a), hash_of(&b));

    // Normal floats: same bits → same hash, and they are also PartialEq-equal.
    let x = PatternLiteral::Float(2.5);
    let y = PatternLiteral::Float(2.5);
    assert_eq!(x, y);
    assert_eq!(hash_of(&x), hash_of(&y));
}
#[test]
fn test_pattern_literal_distinct_variants_differ() {
    // Different variants must not be equal (and their hashes are allowed to differ).
    assert_ne!(PatternLiteral::Int(1), PatternLiteral::Float(1.0));
    assert_ne!(
        PatternLiteral::String("x".into()),
        PatternLiteral::Keyword("x".into())
    );
}
#[test]
fn test_constructor_eq_implies_same_hash() {
    let pairs: &[(Constructor, Constructor)] = &[
        (Constructor::Nil, Constructor::Nil),
        (Constructor::EmptyList, Constructor::EmptyList),
        (Constructor::Pair, Constructor::Pair),
        (Constructor::Set, Constructor::Set),
        (Constructor::SetMut, Constructor::SetMut),
        (Constructor::Array(3), Constructor::Array(3)),
        (Constructor::ArrayMut(0), Constructor::ArrayMut(0)),
        (Constructor::ArrayRest(2), Constructor::ArrayRest(2)),
        (Constructor::ArrayMutRest(1), Constructor::ArrayMutRest(1)),
        (
            Constructor::Literal(PatternLiteral::Int(7)),
            Constructor::Literal(PatternLiteral::Int(7)),
        ),
        (
            Constructor::Literal(PatternLiteral::Float(2.5)),
            Constructor::Literal(PatternLiteral::Float(2.5)),
        ),
        (
            Constructor::Struct(vec![PatternKey::Keyword("a".into())]),
            Constructor::Struct(vec![PatternKey::Keyword("a".into())]),
        ),
        (
            Constructor::Table(vec![PatternKey::Keyword("b".into())]),
            Constructor::Table(vec![PatternKey::Keyword("b".into())]),
        ),
    ];
    for (a, b) in pairs {
        assert_eq!(a, b, "equality failed for {:?}", a);
        assert_eq!(
            hash_of(a),
            hash_of(b),
            "hash mismatch for equal Constructor {:?}",
            a
        );
    }
}
#[test]
fn test_constructor_float_literal_hash_consistency() {
    // The tricky case: float literals inside Constructor::Literal.
    // Use a value that won't trigger clippy::approx_constant.
    let v = 1.23456789_f64;
    let a = Constructor::Literal(PatternLiteral::Float(v));
    let b = Constructor::Literal(PatternLiteral::Float(v));
    assert_eq!(a, b);
    assert_eq!(hash_of(&a), hash_of(&b));
}
#[test]
fn test_constructor_in_hashset() {
    // Constructors can be stored and looked up in a HashSet correctly.
    let mut set = std::collections::HashSet::new();
    set.insert(Constructor::Literal(PatternLiteral::Int(1)));
    set.insert(Constructor::Literal(PatternLiteral::Int(2)));
    set.insert(Constructor::Pair);
    set.insert(Constructor::Pair); // duplicate — should not grow the set

    assert_eq!(set.len(), 3);
    assert!(set.contains(&Constructor::Literal(PatternLiteral::Int(1))));
    assert!(set.contains(&Constructor::Literal(PatternLiteral::Int(2))));
    assert!(set.contains(&Constructor::Pair));
    assert!(!set.contains(&Constructor::Nil));
}
#[test]
fn test_constructor_arity() {
    assert_eq!(Constructor::Literal(PatternLiteral::Int(1)).arity(), 0);
    assert_eq!(Constructor::Nil.arity(), 0);
    assert_eq!(Constructor::EmptyList.arity(), 0);
    assert_eq!(Constructor::Pair.arity(), 2);
    assert_eq!(Constructor::Array(3).arity(), 3);
    assert_eq!(Constructor::ArrayMut(2).arity(), 2);
    assert_eq!(
        Constructor::Struct(vec![
            PatternKey::Keyword("a".into()),
            PatternKey::Keyword("b".into())
        ])
        .arity(),
        2
    );
    assert_eq!(
        Constructor::Table(vec![PatternKey::Keyword("x".into())]).arity(),
        1
    );
}
