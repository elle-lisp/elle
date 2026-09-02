//! Unit tests (`super` is the parent impl module).

use super::*;
use crate::value::SymbolId;

#[test]
fn interning_the_same_name_twice_gives_one_id() {
    let mut table = SymbolTable::new();
    let id1 = table.intern("foo");
    let id2 = table.intern("bar");
    let id3 = table.intern("foo");

    assert_eq!(id1, id3);
    assert_ne!(id1, id2);
    assert_eq!(table.name(id1), Some("foo"));
    assert_eq!(table.name(id2), Some("bar"));
}

// The property the whole identity model exists for. Two tables built by
// different histories agree on every name — so a symbol value copied from one
// compilation to another still names the same symbol.
//
// Counter-factual: under mint-order ids, `first` assigns 0/1/2 and `second`
// assigns 2/1/0 to the same three names, and every assertion below fails.
#[test]
fn two_tables_built_in_opposite_orders_agree_on_every_name() {
    let mut first = SymbolTable::new();
    let mut second = SymbolTable::new();
    let names = ["zeta-xt", "mu-xt", "alpha-xt"];

    let forward: Vec<_> = names.iter().map(|n| first.intern(n)).collect();
    let backward: Vec<_> = names.iter().rev().map(|n| second.intern(n)).collect();

    for (i, id) in forward.iter().enumerate() {
        assert_eq!(*id, backward[names.len() - 1 - i]);
        assert_eq!(second.name(*id), Some(names[i]));
    }
}

// What separates a display memo from a registry, and the reason `send`,
// hydration and `ConstTemplate` decode all carry names: identity crosses on its
// own, a spelling does not. An instance answers for the symbols it has met.
//
// Counter-factual: back the names with one process-wide table and `b.name(id)`
// answers `Some` here, because `a` recorded it. Then nothing forces a name onto
// the wire, and a receiver that never met the symbol prints it wrong instead of
// admitting it does not know.
#[test]
fn a_name_learned_by_one_memo_is_unknown_to_another() {
    let mut a = SymbolTable::new();
    let b = SymbolTable::new();

    let id = a.intern("memo-local-xt");

    assert_eq!(a.name(id), Some("memo-local-xt"));
    assert_eq!(
        b.name(id),
        None,
        "a memo holds only the names its own instance learned; one that answers \
         for another instance's names is a registry"
    );
    assert_eq!(
        SymbolId::of("memo-local-xt"),
        id,
        "identity is the name's hash, so it needs no memo and crosses anyway"
    );
}

// Identity is a pure function of the name — the same one keywords use, so a
// symbol payload and a keyword payload are the same kind of number.
// `src/value/keyword/tests.rs` holds the independent digest oracle; this test
// pins that symbols share it rather than repeating the values.
#[test]
fn id_is_the_shared_name_hash() {
    for n in ["foo", "map", "+", "list?", "λ", ""] {
        assert_eq!(SymbolId::of(n).0, crate::value::keyword::keyword_hash(n));
    }
    assert_eq!(SymbolId::of("foo").0, 0x7eb4_f5db_4cbe_5ae7);
}

// `of` answers identity without recording anything; `intern` also records the
// name. A name nobody interned has an id but no recoverable spelling.
#[test]
fn of_computes_identity_without_recording_a_name() {
    let mut table = SymbolTable::new();
    let unrecorded = SymbolId::of("never-interned-anywhere-xt");
    assert_eq!(table.name(unrecorded), None);

    assert_eq!(table.intern("never-interned-anywhere-xt"), unrecorded);
    assert_eq!(table.name(unrecorded), Some("never-interned-anywhere-xt"));
}

#[test]
fn names_survive_later_interning() {
    let mut table = SymbolTable::new();
    let id = table.intern("persistent-xt");
    for i in 0..100 {
        table.intern(&format!("sym-xt-{}", i));
    }
    assert_eq!(table.name(id), Some("persistent-xt"));
    assert_eq!(table.intern("persistent-xt"), id);
}

#[test]
fn distinct_names_get_distinct_ids() {
    let mut table = SymbolTable::new();
    let ids: Vec<_> = (0..1000)
        .map(|i| table.intern(&format!("distinct-xt-{}", i)))
        .collect();
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len());
}

#[test]
fn special_characters_and_unicode_round_trip() {
    let mut table = SymbolTable::new();
    for sym in [
        "+",
        "-",
        "*",
        "/",
        "=",
        "<",
        ">",
        "<=",
        ">=",
        "!=",
        "list?",
        "nil?",
        "some-func-name",
        "CamelCase",
        "with_underscores",
        "λ",
        "こんにちは",
    ] {
        let id = table.intern(sym);
        assert_eq!(table.name(id), Some(sym));
    }
    let long = "a".repeat(1000);
    let id = table.intern(&long);
    assert_eq!(table.name(id), Some(long.as_str()));
}

// The sentinel is reserved: no real name may occupy it, so a compiler
// temporary can never be confused with a symbol somebody wrote.
#[test]
fn synthetic_is_outside_the_name_space() {
    let table = SymbolTable::new();
    assert_eq!(SymbolId::SYNTHETIC.0, u64::MAX);
    assert_eq!(table.name(SymbolId::SYNTHETIC), None);
}

// Two names on one hash would silently become one symbol everywhere at once.
// The memo refuses instead. Real FNV-1a collisions are not constructible at
// this scale, so the test drives the guard directly.
#[test]
#[should_panic(expected = "name hash collision")]
fn a_second_name_on_one_hash_panics() {
    let mut table = SymbolTable::new();
    let forged = SymbolId::of("collision-probe-xt");
    table.record(forged, "collision-probe-xt");
    table.record(forged, "a-different-name-entirely");
}

// The check lives at the learning site, not in one table, so it still fires for
// a name that arrives from somewhere else — a dump replayed into this instance,
// or a symbol sent from another thread. A memo that had never met the name is
// exactly the case a process-wide table cannot check, because it would have
// recorded the first spelling itself.
#[test]
#[should_panic(expected = "name hash collision")]
fn a_name_arriving_from_elsewhere_is_checked_against_this_memo() {
    let mut receiver = SymbolTable::new();
    let forged = SymbolId::of("hydrated-name-xt");
    receiver.intern("hydrated-name-xt");
    receiver.record(forged, "what-the-dump-called-it");
}

// ── keywords share the map ───────────────────────────────────────────

// The memo's domain is spellings, not values: `map` and `:map` differ by tag
// in the Value, so one entry serves both. Two maps would double the collision
// guard and let the two vocabularies drift.
#[test]
fn a_keyword_and_a_symbol_with_one_spelling_share_one_entry() {
    let mut table = SymbolTable::new();
    let sym = table.intern("shared-spelling-xt");
    let kw = table.keyword("shared-spelling-xt");

    assert_eq!(sym.0, kw, "one spelling, one hash, both vocabularies");
    assert_eq!(table.len(), 1, "one entry serves both vocabularies");
    assert_eq!(table.name(sym), Some("shared-spelling-xt"));
    assert_eq!(table.keyword_name(kw), Some("shared-spelling-xt"));
}

// The keyword analogue of `of_computes_identity_without_recording_a_name`:
// identity is pure, display is learned.
#[test]
fn keyword_name_answers_only_after_learning() {
    let mut table = SymbolTable::new();
    let hash = crate::value::keyword::keyword_hash("kw-unlearned-xt");

    assert_eq!(table.keyword_name(hash), None);
    assert_eq!(table.keyword("kw-unlearned-xt"), hash);
    assert_eq!(table.keyword_name(hash), Some("kw-unlearned-xt"));
}

// One map means one guard: a keyword spelling that collides with a symbol
// spelling is caught the moment the second one is learned, whichever
// vocabulary met the hash first.
#[test]
#[should_panic(expected = "name hash collision")]
fn the_collision_guard_spans_both_vocabularies() {
    let mut table = SymbolTable::new();
    table.intern("cross-vocab-xt");
    table.record_spelling(SymbolId::of("cross-vocab-xt").0, "some-other-spelling");
}
