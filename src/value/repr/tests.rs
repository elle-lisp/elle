// audited: 2026-11-25
//! A scalar `Value` decodes from its wire form to itself or to an error, never to a panic or to another value.
//!
//! docs/impl/graft.md

use super::*;

/// A body fragment's bytes that do not decode are a body the payload does not
/// carry, and the call stays a call. The trap: routing the discriminant
/// through a plain `From<u8>` panics on an invalid tag, so one flipped byte
/// crashes the compile instead.
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
/// its tail against it, so it lands in a body fragment as a literal. The
/// stdlib's `+` dispatches on `(match args () 0 …)`, and its fragment carries
/// that literal.
///
/// The counter-factual: without its own arm, the empty list would fall through
/// to "unsupported scalar Value", and encoding `+`'s fragment would fail as
/// the stdlib compiles.
#[test]
fn the_empty_list_round_trips() {
    let bytes = bincode::serialize(&Value::EMPTY_LIST).expect("the empty list serializes");
    let back: Value = bincode::deserialize(&bytes).expect("deserializes");
    assert!(back.is_empty_list(), "came back as {back:?}");
}
