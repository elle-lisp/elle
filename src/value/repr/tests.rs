// audited: 2026-09-29
//! A scalar `Value` decodes from its wire form to itself or to an error, never to a panic or to another value.
//!
//! docs/impl/stdlib-cache.md

use super::*;

/// A corrupt cache file must surface as a decode error the cache layer can
/// turn into a miss. The trap: routing the discriminant through a plain
/// `From<u8>` panics on an invalid tag, so one flipped byte crashes startup
/// instead of falling back to a recompile.
#[test]
fn invalid_scalar_tag_is_an_error_not_a_panic() {
    let bytes = [200u8, 0, 0, 0, 0, 0, 0, 0];
    assert!(bincode::deserialize::<Value>(&bytes).is_err());
}

/// A symbol `Value`'s payload is its name's hash, the same number in every
/// process, so the scalar wire form carries it as it stands.
///
/// The counter-factual: a payload that was a per-process table index would bind
/// the constant to whatever symbol held that index in the loading process. The
/// round-trip would return a *different* symbol, and it would be silent,
/// because both sides are only integers.
#[test]
fn symbol_value_round_trips_as_its_name_hash() {
    let id = crate::value::SymbolId::of("a-pool-symbol");
    let bytes = bincode::serialize(&Value::symbol(id)).expect("a symbol scalar serializes");
    let back: Value = bincode::deserialize(&bytes).expect("deserializes");
    assert_eq!(back.as_symbol(), Some(id));
}

/// The empty list is an immediate, and a list pattern with no rest compares
/// its tail against it, so it lands in a function's constants: the stdlib's `+`
/// dispatches on `(match args () 0 …)`, and the cache stores that constant.
///
/// The counter-factual: the empty list fell through to "unsupported scalar
/// Value", the cache store failed, and every start compiled the stdlib again.
#[test]
fn the_empty_list_round_trips() {
    let bytes = bincode::serialize(&Value::EMPTY_LIST).expect("the empty list serializes");
    let back: Value = bincode::deserialize(&bytes).expect("deserializes");
    assert!(back.is_empty_list(), "came back as {back:?}");
}
