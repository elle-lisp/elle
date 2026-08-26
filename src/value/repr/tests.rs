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

/// A symbol `Value` carries only a process-local id; no name travels with it,
/// so a loader could not re-intern it. Pool symbols cross via
/// `SendValue::Symbol { name, .. }` instead — this path must refuse, not
/// persist a raw id that binds to an arbitrary symbol on load.
#[test]
fn symbol_value_refuses_scalar_serialization() {
    let sym = Value::symbol(7);
    assert!(bincode::serialize(&sym).is_err());
}
