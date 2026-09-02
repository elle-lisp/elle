// DEFENSE: an embedder sees stable, table-independent symbol identity.
//
// The mechanism's own tests live in `src/symbol/tests.rs`. These pin what only
// a consumer of the public API can see: that `elle::symbol` hands out the same
// id for a name no matter which handle asks, and that an id computed without a
// handle is the id interning produces.
use elle::symbol::SymbolTable;
use elle::value::SymbolId;

#[test]
fn every_handle_agrees_on_a_name() {
    let mut first = SymbolTable::new();
    let mut second = SymbolTable::new();

    let a = first.intern("embedder-alpha");
    let _ = second.intern("embedder-decoy-1");
    let _ = second.intern("embedder-decoy-2");
    let b = second.intern("embedder-alpha");

    assert_eq!(a, b);
    assert_eq!(first.name(b), Some("embedder-alpha"));
    assert_eq!(second.name(a), Some("embedder-alpha"));
}

#[test]
fn identity_needs_no_handle() {
    let mut table = SymbolTable::new();
    assert_eq!(table.intern("embedder-beta"), SymbolId::of("embedder-beta"));
}

#[test]
fn an_unrecorded_id_has_no_name() {
    let table = SymbolTable::new();
    assert_eq!(table.name(SymbolId::of("embedder-never-interned")), None);
    assert_eq!(table.name(SymbolId::SYNTHETIC), None);
}
