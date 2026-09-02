// Tests migrated to tests/elle/value-repr.lisp

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
/// The trap this replaced: while the payload was a per-process table index,
/// persisting it bound the constant to whatever symbol held that index in the
/// loading process, so this path had to refuse. The counter-factual now is a
/// round-trip that returns a *different* symbol — and it would be silent,
/// because both sides are only integers.
#[test]
fn symbol_value_round_trips_as_its_name_hash() {
    let id = crate::value::SymbolId::of("a-pool-symbol");
    let bytes = bincode::serialize(&Value::symbol(id)).expect("a symbol scalar serializes");
    let back: Value = bincode::deserialize(&bytes).expect("deserializes");
    assert_eq!(back.as_symbol(), Some(id));
}
