// audited: 2026-10-13
//! A constant template survives its bytecode encoding whole, scope ids included.
//!
//! docs/impl/syntax.md

use super::*;
use crate::syntax::ScopeId;

fn roundtrip(t: &ConstTemplate) -> ConstTemplate {
    let mut buf = Vec::new();
    t.encode(&mut buf);
    let mut ip = 0;
    let decoded = ConstTemplate::decode(&buf, &mut ip);
    assert_eq!(
        ip,
        buf.len(),
        "decode must consume exactly the encoded bytes"
    );
    decoded
}

#[test]
fn encode_decode_roundtrips_every_variant() {
    // The bytecode encoding must reproduce the template exactly — a decode
    // that drops or reshapes a node materializes the wrong quoted datum.
    let nested = ConstTemplate::Pair(
        Box::new(ConstTemplate::Int(1)),
        Box::new(ConstTemplate::Pair(
            Box::new(ConstTemplate::String("two".into())),
            Box::new(ConstTemplate::Pair(
                Box::new(ConstTemplate::Array(vec![
                    ConstTemplate::Int(3),
                    ConstTemplate::Symbol("a-sym".into()),
                    ConstTemplate::Keyword("k".into()),
                    ConstTemplate::Bool(true),
                    ConstTemplate::Nil,
                    ConstTemplate::Float(2.5),
                    ConstTemplate::StringMut("m".into()),
                    ConstTemplate::ArrayMut(vec![ConstTemplate::EmptyList]),
                ])),
                Box::new(ConstTemplate::EmptyList),
            )),
        )),
    );
    assert_eq!(roundtrip(&nested), nested);
}

#[test]
fn syntax_symbol_roundtrips_with_scopes_and_span() {
    // A decode that drops a scope id, or the high half of one, breaks hygiene
    // in code that was compiled before it was loaded. The last id sets bits
    // above 32, which an encoding four bytes wide would cut off.
    let span = crate::syntax::Span::new(10, 20, 3, 7).with_file("macro.elle");
    let t = ConstTemplate::SyntaxSymbol {
        name: "template-sym".into(),
        scopes: vec![
            ScopeId::test(1),
            ScopeId::test(4),
            ScopeId::test(9),
            ScopeId::from_bits(0xdead_beef_8000_0010),
        ],
        span,
        scope_exempt: true,
    };
    assert_eq!(roundtrip(&t), t);

    // Fileless span + empty scope set + non-exempt — the other end of the range.
    let bare = ConstTemplate::SyntaxSymbol {
        name: "x".into(),
        scopes: vec![],
        span: crate::syntax::Span::synthetic(),
        scope_exempt: false,
    };
    assert_eq!(roundtrip(&bare), bare);
}

#[test]
fn syntax_symbol_is_not_immediate() {
    // It allocates a `Value::syntax`, so it must NOT report an immediate value
    // — a pure-immediate MaterializeConst would leave its region with no
    // RC-raising allocation (a Rule 2 violation; see `immediate_value`).
    let mut symbols = crate::symbol::SymbolTable::new();
    let t = ConstTemplate::SyntaxSymbol {
        name: "s".into(),
        scopes: vec![ScopeId::test(2)],
        span: crate::syntax::Span::synthetic(),
        scope_exempt: false,
    };
    assert!(t.immediate_value(&mut symbols).is_none());
}
