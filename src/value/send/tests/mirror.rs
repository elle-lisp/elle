// audited: 2026-09-29
//! The serde mirror keeps each container kind apart, and the syntax mirror rebuilds a tree in the destination's arena.
//!
//! docs/impl/stdlib-cache.md
//! docs/impl/syntax.md

use super::*;

// ── the serde mirror keeps container kinds apart ────────────────────

/// `SendValue`'s serde is the stdlib cache's wire format, and eight variants
/// share three shapes: a sequence, a map, a byte run. Collapsing them loses
/// which one it was, so a `Tuple` returns as an `Array`, an `LSet` as an
/// `Array`, and a `Buffer` — a *mutable* @string — as immutable `Bytes`. The
/// value would be silently the wrong type on every reload.
#[test]
fn serde_round_trip_keeps_each_container_kind_distinct() {
    use super::SendValue as SV;

    fn nil() -> Box<SV> {
        Box::new(SV::Immediate(Value::NIL))
    }
    fn name(sv: &SV) -> &'static str {
        match sv {
            SV::Array(..) => "Array",
            SV::Tuple(..) => "Tuple",
            SV::LSet(..) => "LSet",
            SV::LSetMut(..) => "LSetMut",
            SV::Struct(..) => "Struct",
            SV::StructMut(..) => "StructMut",
            SV::Buffer(..) => "Buffer",
            SV::Bytes(..) => "Bytes",
            SV::Blob(..) => "Blob",
            _ => panic!("unexpected variant in this test"),
        }
    }

    let empty = std::collections::BTreeMap::new();
    let cases = vec![
        SV::Array(vec![], nil()),
        SV::Tuple(vec![], nil()),
        SV::LSet(vec![], nil()),
        SV::LSetMut(vec![], nil()),
        SV::Struct(empty.clone(), nil()),
        SV::StructMut(empty, nil()),
        SV::Buffer(vec![1, 2, 3], nil()),
        SV::Bytes(vec![1, 2, 3], nil()),
        SV::Blob(vec![1, 2, 3], nil()),
    ];

    for case in cases {
        let bytes = bincode::serialize(&case).expect("serializes");
        let back: SV = bincode::deserialize(&bytes).expect("deserializes");
        assert_eq!(
            name(&back),
            name(&case),
            "a {} must not come back as a {} — the cache would hand the runtime \
             a value of the wrong type on every reload",
            name(&case),
            name(&back)
        );
    }
}

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
    let inner = Syntax::symbol_scoped(&arena, "x", span, &[ScopeId(7), ScopeId(11)]);
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
    assert_eq!(child.scopes(), &[ScopeId(7), ScopeId(11)]);
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
    let captured = Syntax::symbol_scoped(&arena, "it", span, &[ScopeId(4)]);
    let literal = Syntax::new(SyntaxKind::SyntaxLiteral(arena.node(captured)), span);

    let wire = syntax::syntax_to_send(&literal).expect("a syntax literal is sendable");
    let back = syntax::send_to_syntax(&arena, wire);

    let SyntaxKind::SyntaxLiteral(child) = back.kind else {
        panic!("expected a syntax literal");
    };
    assert_eq!(child.as_symbol(), Some("it"));
    assert_eq!(child.scopes(), &[ScopeId(4)]);
}
