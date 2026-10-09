// audited: 2026-10-06
//! A symbol or keyword that crosses the send boundary names the same thing on the receiving side.
//!
//! docs/impl/symbol.md

use super::*;

// ── symbol identity survives the boundary verbatim ──────────────────
//
// The three tests below cover the three ways a symbol id crosses `send`: as a
// value, as a struct key, and as a LIR constant. A symbol id is its name's hash,
// so all three cross verbatim, and each test asserts a name resolves on the
// receiving side.

/// A receiver's table whose history differs from the sender's. With mint-order
/// ids, the four decoys would make every shared name disagree.
fn skewed_receiver() -> crate::symbol::SymbolTable {
    let mut receiver = crate::symbol::SymbolTable::new();
    for n in ["skew-w", "skew-x", "skew-y", "skew-z"] {
        let _ = receiver.intern(n);
    }
    receiver
}

#[test]
fn a_symbol_value_names_the_same_symbol_on_both_sides() {
    use crate::symbol::SymbolTable;

    let mut sender = SymbolTable::new();
    let _ = sender.intern("send-aaa");
    let _ = sender.intern("send-bbb");
    let begin = sender.intern("begin");

    // A symbol is immediate, so serialization allocates nothing; any heap serves.
    let sv = SendBundle::from_value(
        Value::symbol(begin),
        unsafe { &*crate::value::arena::leaked_test_heap() },
        Some(&sender),
    )
    .expect("symbol is sendable");

    let mut receiver = skewed_receiver();
    let got = {
        let heap_ptr = crate::value::arena::leaked_test_heap();
        let region = unsafe { (*heap_ptr).new_runtime_region() };
        let mut alloc =
            crate::primitives::ctx::Alloc::with_region(region, unsafe { &mut *heap_ptr });
        sv.into_value(&mut alloc, Some(&mut receiver))
    };

    let got_id = got.as_symbol().expect("reconstructed value is a symbol");
    assert_eq!(got_id, begin, "the id crosses unchanged");
    assert_eq!(receiver.name(got_id), Some("begin"));
}

// `SendValue::Struct` clones each `TableKey` verbatim, so a symbol key crosses
// as a bare id with no name beside it. The receiver must still read the key the
// sender wrote.
//
// Counter-factual: with mint-order ids,
// `(get (sys/join (sys/spawn (fn [] {'alpha 1}))) 'alpha)` returns the wrong
// entry. `tests/lang/symbol-identity.lisp` pins the end-to-end shape.
#[test]
fn a_symbol_struct_key_names_the_same_symbol_on_both_sides() {
    use crate::symbol::SymbolTable;
    use crate::value::heap::TableKey;

    crate::value::arena::with_test_region(|| {
        let mut sender = SymbolTable::new();
        let _ = sender.intern("send-aaa");
        let alpha = sender.intern("key-alpha");
        let beta = sender.intern("key-beta");

        let h = crate::primitives::ctx::TestHeap::new();
        let mut entries = vec![
            (TableKey::Symbol(alpha), Value::int(7)),
            (TableKey::Symbol(beta), Value::int(8)),
        ];
        entries.sort_by_key(|(a, _)| *a);
        let s = h.ctx().struct_from_sorted(entries);
        let bundle =
            SendBundle::from_value(s, h.heap(), Some(&sender)).expect("struct is sendable");

        let mut receiver = skewed_receiver();
        let probe = TableKey::Symbol(receiver.intern("key-alpha"));
        let got = {
            let heap_ptr = crate::value::arena::leaked_test_heap();
            let region = unsafe { (*heap_ptr).new_runtime_region() };
            let mut alloc =
                crate::primitives::ctx::Alloc::with_region(region, unsafe { &mut *heap_ptr });
            bundle.into_value(&mut alloc, Some(&mut receiver))
        };

        let entries = got.as_struct().expect("reconstructed value is a struct");
        assert_eq!(
            crate::value::types::sorted_struct_get(entries, &probe),
            Some(&crate::value::Value::int(7)),
            "the receiver's own key must find the entry the sender stored"
        );
        // `key-beta` was never interned on this side: its name reaches the
        // receiver only through the bundle's name table, from the struct-key
        // noting site (keys are cloned as `TableKey`, never routed through
        // `from_value_inner`).
        assert_eq!(
            receiver.name(beta),
            Some("key-beta"),
            "a struct key's name must cross inside the bundle"
        );
    });
}

// A symbol constant ships inside the frozen LIR with no name and no rewrite, so
// the id the sender lowered is the id the worker re-emits into its own constant
// pool.
#[test]
fn a_lir_symbol_const_names_the_same_symbol_on_both_sides() {
    use crate::lir::code::ConstRef;
    use crate::lir::InstrRef;
    use crate::symbol::SymbolTable;

    let mut sender = SymbolTable::new();
    let _ = sender.intern("send-aaa");
    let alpha = sender.intern("lir-alpha");

    let lir = LirFixture::new(Arity::Exact(0))
        .block(
            0,
            &[InstrRef::Const {
                dst: Reg(0),
                value: ConstRef::Symbol(alpha),
            }],
            Terminator::Return(Reg(0)),
        )
        .build();
    let shipped = match lir.view().block(0).node(0).instr() {
        InstrRef::Const {
            value: ConstRef::Symbol(id),
            ..
        } => id,
        other => panic!("a symbol freezes to a symbol constant, got {:?}", other),
    };

    let mut receiver = skewed_receiver();
    let _ = receiver.intern("lir-alpha");
    assert_eq!(
        receiver.name(shipped),
        Some("lir-alpha"),
        "the worker re-emits this id verbatim; it must name the sender's symbol"
    );
}

// A keyword crosses as an immediate; its spelling rides the bundle's name
// table with the symbols', so the receiving instance can print both the
// keyword value and a keyword struct key it never learned itself.
#[test]
fn a_keyword_names_the_same_keyword_on_both_sides() {
    use crate::symbol::SymbolTable;
    use crate::value::heap::TableKey;

    crate::value::arena::with_test_region(|| {
        let mut sender = SymbolTable::new();
        let kw_hash = sender.keyword("kw-send-xt");
        let key_hash = sender.keyword("kw-send-key-xt");

        let h = crate::primitives::ctx::TestHeap::new();
        let s = h.ctx().struct_from_sorted(vec![(
            TableKey::Keyword(key_hash),
            Value::keyword("kw-send-xt"),
        )]);
        let bundle =
            SendBundle::from_value(s, h.heap(), Some(&sender)).expect("struct is sendable");

        let mut receiver = SymbolTable::new();
        let got = {
            let heap_ptr = crate::value::arena::leaked_test_heap();
            let region = unsafe { (*heap_ptr).new_runtime_region() };
            let mut alloc =
                crate::primitives::ctx::Alloc::with_region(region, unsafe { &mut *heap_ptr });
            bundle.into_value(&mut alloc, Some(&mut receiver))
        };

        assert_eq!(
            format!("{}", got.display_with(Some(&receiver))),
            "{:kw-send-key-xt :kw-send-xt}",
            "both the key's and the value's spellings crossed in the bundle"
        );
        assert_eq!(receiver.keyword_name(kw_hash), Some("kw-send-xt"));
    });
}

// ── a LIR symbol constant crosses unchanged ─────────────────────────

/// The JIT materializes a symbol constant straight into a `Value::symbol`,
/// so the id that crosses the boundary must name the same symbol on the other
/// side. It does, with no translation step: the id is the name's hash.
///
/// The counter-factual: with per-process table indices as ids, the loading side
/// would remap them against a stored name map, and an id the map missed would
/// silently become whatever symbol held that index there. The round-trip would
/// yield a different `SymbolId`, and the JIT would compile a comparison against
/// a symbol no source text spells.
#[test]
fn a_lir_symbol_constant_survives_serialization_as_the_name_hash() {
    use crate::lir::code::ConstRef;
    use crate::lir::{InstrRef, LirCode, LirOwned, Reg, Terminator};
    use crate::value::SymbolId;

    let lir = LirFixture::new(Arity::Exact(0))
        .block(
            0,
            &[InstrRef::Const {
                dst: Reg(0),
                value: ConstRef::Symbol(SymbolId::of("answerish")),
            }],
            Terminator::Return(Reg(0)),
        )
        .build();

    let bytes = bincode::serialize(lir.code()).expect("LIR serializes");
    let back: LirCode = bincode::deserialize(&bytes).expect("deserializes");
    let back = LirOwned::from_parts(back, Vec::new()).expect("the LIR loads no values");

    let mut ids = Vec::new();
    for node in back.view().nodes() {
        if let InstrRef::Const {
            value: ConstRef::Symbol(sid),
            ..
        } = node.instr()
        {
            ids.push(sid);
        }
    }
    assert_eq!(
        ids,
        vec![SymbolId::of("answerish")],
        "the stored id must still be the hash of `answerish`"
    );
}
