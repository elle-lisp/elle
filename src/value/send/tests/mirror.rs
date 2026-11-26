// audited: 2026-11-25
//! The syntax mirror rebuilds a tree, and a received struct ascends in key order.
//!
//! docs/impl/syntax.md

use super::*;
use crate::value::TableKey;

// ── the syntax mirror ────────────────────────────────────────────────

/// A tree crosses the mirror and comes back with every kind, span, and scope
/// set intact, rebuilt in the destination's arena.
///
/// The trap this pins: a `Syntax` node's payloads are region pointers, so the
/// mirror must own the strings and children on the way out and rebuild them on
/// the way in. A mirror that carried the handles would hand the receiving
/// thread pointers into the sender's region store.
#[test]
fn a_syntax_tree_round_trips_through_the_send_mirror() {
    use crate::syntax::{thread_arena, ScopeId, Span, Syntax, SyntaxKind};

    let arena = thread_arena();
    let span = Span::new(3, 9, 2, 5).with_file("mirror.lisp");
    let inner = Syntax::symbol_scoped(&arena, "x", span, &[ScopeId::test(7), ScopeId::test(11)]);
    let tree = Syntax::list(
        &arena,
        &[
            Syntax::symbol(&arena, "quote", span),
            Syntax::string(&arena, "text", span),
            Syntax::new(SyntaxKind::Quasiquote(arena.node(inner)), span),
        ],
        span,
    );

    let wire = syntax::syntax_to_send(&tree).expect("every kind here is sendable");
    let back = syntax::send_to_syntax(&arena, wire);

    assert_eq!(back.span, span);
    assert_eq!(back.span.file(), Some("mirror.lisp"));
    let items = back.as_list().expect("a list");
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].as_symbol(), Some("quote"));
    assert!(matches!(items[1].kind, SyntaxKind::String(s) if s.as_str() == "text"));
    let SyntaxKind::Quasiquote(child) = items[2].kind else {
        panic!("expected a quasiquote");
    };
    assert_eq!(child.as_symbol(), Some("x"));
    assert_eq!(child.scopes(), &[ScopeId::test(7), ScopeId::test(11)]);
}

/// A `SyntaxLiteral` crosses too.
///
/// The trap: it is the one compound kind `SyntaxKind::children` reports as
/// childless, because the scope walks must not descend into it. A mirror
/// written on that shared walk would carry the node and silently drop the
/// symbol inside — which is why the mirror reaches its child by name.
#[test]
fn a_syntax_literal_crosses_the_send_mirror() {
    use crate::syntax::{thread_arena, ScopeId, Span, Syntax, SyntaxKind};

    let arena = thread_arena();
    let span = Span::synthetic();
    let captured = Syntax::symbol_scoped(&arena, "it", span, &[ScopeId::test(4)]);
    let literal = Syntax::new(SyntaxKind::SyntaxLiteral(arena.node(captured)), span);

    let wire = syntax::syntax_to_send(&literal).expect("a syntax literal is sendable");
    let back = syntax::send_to_syntax(&arena, wire);

    let SyntaxKind::SyntaxLiteral(child) = back.kind else {
        panic!("expected a syntax literal");
    };
    assert_eq!(child.as_symbol(), Some("it"));
    assert_eq!(child.scopes(), &[ScopeId::test(4)]);
}

// ── a received struct is sorted by key order ─────────────────────────

/// The keys of a received struct, as the received value holds them.
fn received_struct_keys(sv: SendValue, mutable: bool) -> (Value, Vec<TableKey>) {
    let v = into_value_in_region(|ctx| sv.into_value(ctx));
    let keys = if mutable {
        v.struct_to_vec()
            .expect("an @struct")
            .into_iter()
            .map(|(k, _)| k)
            .collect()
    } else {
        v.as_struct()
            .expect("a struct")
            .iter()
            .map(|(k, _)| *k)
            .collect()
    };
    (v, keys)
}

/// A received struct's entries ascend in `TableKey`'s order, the one order of
/// all values, and a lookup finds each key.
///
/// The trap: the wire map is ordered by `SendKey`'s derived `Ord`, which ranks
/// variants by declaration and puts a string before a keyword. The order of
/// all values puts the keyword first. Entries copied in the map's order are
/// all present, but the binary search over them misses the misplaced keys.
#[test]
fn a_received_struct_ascends_in_the_order_of_all_values() {
    use super::SendValue as SV;
    use std::collections::BTreeMap;

    for mutable in [false, true] {
        let mut map: BTreeMap<SendKey, SV> = BTreeMap::new();
        map.insert(SendKey::String("a".into()), SV::Immediate(Value::int(1)));
        map.insert(
            SendKey::Keyword(crate::value::keyword::keyword_hash("a")),
            SV::Immediate(Value::int(2)),
        );
        let traits = Box::new(SV::Immediate(Value::NIL));
        let sv = if mutable {
            SV::StructMut(map, traits)
        } else {
            SV::Struct(map, traits)
        };

        let (v, keys) = received_struct_keys(sv, mutable);
        assert!(
            keys.windows(2).all(|w| w[0] < w[1]),
            "the entries of a received {} ascend: {keys:?}",
            if mutable { "@struct" } else { "struct" }
        );
        let found = |key: TableKey| match v.as_struct() {
            Some(entries) => crate::value::sorted_struct_get(entries, &key).copied(),
            None => v.struct_mut_read().expect("an @struct").get(&key),
        };
        assert_eq!(
            found(TableKey::keyword("a")).and_then(|x| x.as_int()),
            Some(2)
        );
        let probe = into_value_in_region(|ctx| ctx.string("a"));
        assert_eq!(
            found(TableKey::String(probe)).and_then(|x| x.as_int()),
            Some(1)
        );
    }
}
