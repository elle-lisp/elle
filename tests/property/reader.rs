// Property tests for the reader/parser.
//
// Tests the fundamental roundtrip property: read(display(read(s))) == read(s)
// for structurally valid source code. Also tests that the reader never panics
// on arbitrary input.

use elle::reader::read_syntax;
use elle::syntax::{Syntax, SyntaxKind};
use elle::value::repr::{INT_MAX, INT_MIN};
use proptest::prelude::*;

/// Compare two Syntax trees structurally, ignoring spans and scopes.
fn syntax_eq(a: &Syntax, b: &Syntax) -> bool {
    kind_eq(&a.kind, &b.kind)
}

/// Compare two SyntaxKind values structurally.
fn kind_eq(a: &SyntaxKind, b: &SyntaxKind) -> bool {
    match (a, b) {
        (SyntaxKind::Nil, SyntaxKind::Nil) => true,
        (SyntaxKind::Bool(a), SyntaxKind::Bool(b)) => a == b,
        (SyntaxKind::Int(a), SyntaxKind::Int(b)) => a == b,
        (SyntaxKind::Float(a), SyntaxKind::Float(b)) => {
            // NaN == NaN for our purposes (both are NaN)
            if a.is_nan() && b.is_nan() {
                true
            } else {
                a.to_bits() == b.to_bits()
            }
        }
        (SyntaxKind::Symbol(a), SyntaxKind::Symbol(b)) => a == b,
        (SyntaxKind::Keyword(a), SyntaxKind::Keyword(b)) => a == b,
        (SyntaxKind::String(a), SyntaxKind::String(b)) => a == b,
        (SyntaxKind::List(a), SyntaxKind::List(b)) => {
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| syntax_eq(x, y))
        }
        (SyntaxKind::Tuple(a), SyntaxKind::Tuple(b)) => {
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| syntax_eq(x, y))
        }
        (SyntaxKind::Array(a), SyntaxKind::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| syntax_eq(x, y))
        }
        (SyntaxKind::Struct(a), SyntaxKind::Struct(b)) => {
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| syntax_eq(x, y))
        }
        (SyntaxKind::Table(a), SyntaxKind::Table(b)) => {
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| syntax_eq(x, y))
        }
        (SyntaxKind::Quote(a), SyntaxKind::Quote(b)) => syntax_eq(a, b),
        (SyntaxKind::Quasiquote(a), SyntaxKind::Quasiquote(b)) => syntax_eq(a, b),
        (SyntaxKind::Unquote(a), SyntaxKind::Unquote(b)) => syntax_eq(a, b),
        (SyntaxKind::UnquoteSplicing(a), SyntaxKind::UnquoteSplicing(b)) => syntax_eq(a, b),
        (SyntaxKind::SyntaxLiteral(_), SyntaxKind::SyntaxLiteral(_)) => {
            // SyntaxLiteral is internal-only, shouldn't appear in normal roundtrips
            true
        }
        _ => false,
    }
}

/// Strategy for generating valid Elle source code strings.
///
/// Generates source that is guaranteed to parse successfully.
/// The display of the parsed Syntax should re-parse to the same structure.
fn arb_source() -> BoxedStrategy<String> {
    arb_source_depth(3)
}

fn arb_source_depth(depth: u32) -> BoxedStrategy<String> {
    if depth == 0 {
        prop_oneof![
            // Integers
            10 => (INT_MIN..=INT_MAX).prop_map(|n| format!("{}", n)),
            // Booleans
            2 => prop::bool::ANY.prop_map(|b| if b { "true".to_string() } else { "false".to_string() }),
            // nil
            1 => Just("nil".to_string()),
            // Symbols (simple identifier-safe names, excluding reserved words)
            5 => "[a-z][a-z0-9\\-]{0,8}"
                .prop_filter("not a keyword literal", |s| !matches!(s.as_str(), "nil" | "true" | "false"))
                .prop_map(|s| s),
            // Keywords
            3 => "[a-z][a-z0-9\\-]{0,8}".prop_map(|s| format!(":{}", s)),
            // Strings (with limited character set to avoid escape issues)
            3 => "[a-zA-Z0-9 ]{0,20}".prop_map(|s| format!("\"{}\"", s)),
        ]
        .boxed()
    } else {
        let leaf = arb_source_depth(0);
        let inner = arb_source_depth(depth - 1);
        prop_oneof![
            // Leaf values (high weight to keep things manageable)
            10 => leaf,
            // Lists
            3 => prop::collection::vec(inner.clone(), 0..=4)
                .prop_map(|items| format!("({})", items.join(" "))),
            // Tuples
            2 => prop::collection::vec(arb_source_depth(depth - 1), 0..=4)
                .prop_map(|items| format!("[{}]", items.join(" "))),
            // Arrays
            1 => prop::collection::vec(arb_source_depth(depth - 1), 0..=4)
                .prop_map(|items| format!("@[{}]", items.join(" "))),
            // Quote
            1 => arb_source_depth(depth - 1)
                .prop_map(|s| format!("'{}", s)),
        ]
        .boxed()
    }
}

/// Strategy for generating floats that survive the display roundtrip.
///
/// Rust's `{}` format for f64 can drop trailing zeros (1.0 -> "1"),
/// which would be parsed as an integer. We only test floats that
/// format with a decimal point.
fn arb_roundtrippable_float() -> BoxedStrategy<String> {
    prop::num::f64::NORMAL
        .prop_filter("must format with decimal point", |f| {
            let s = format!("{}", f);
            s.contains('.')
        })
        .prop_map(|f| format!("{}", f))
        .boxed()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    // =========================================================================
    // Roundtrip: read(display(read(s))) == read(s)
    // =========================================================================

    #[test]
    fn integer_roundtrip(n in INT_MIN..=INT_MAX) {
        let source = format!("{}", n);
        let parsed = read_syntax(&source).unwrap();
        let displayed = format!("{}", parsed);
        let reparsed = read_syntax(&displayed).unwrap();
        prop_assert!(syntax_eq(&parsed, &reparsed),
            "Integer roundtrip failed: {} -> {} -> {:?}", source, displayed, reparsed.kind);
    }

    #[test]
    fn bool_roundtrip(b in prop::bool::ANY) {
        let source = if b { "true" } else { "false" };
        let parsed = read_syntax(source).unwrap();
        let displayed = format!("{}", parsed);
        let reparsed = read_syntax(&displayed).unwrap();
        prop_assert!(syntax_eq(&parsed, &reparsed),
            "Bool roundtrip failed: {} -> {}", source, displayed);
    }

    #[test]
    fn bool_legacy_roundtrip(b in prop::bool::ANY) {
        let source = if b { "#t" } else { "#f" };
        let parsed = read_syntax(source).unwrap();
        // Display now emits "true"/"false"
        let displayed = format!("{}", parsed);
        let expected_display = if b { "true" } else { "false" };
        prop_assert_eq!(&displayed, expected_display);
        let reparsed = read_syntax(&displayed).unwrap();
        prop_assert!(syntax_eq(&parsed, &reparsed));
    }

    #[test]
    fn string_roundtrip(s in "[a-zA-Z0-9 ]{0,30}") {
        let source = format!("\"{}\"", s);
        let parsed = read_syntax(&source).unwrap();
        let displayed = format!("{}", parsed);
        let reparsed = read_syntax(&displayed).unwrap();
        prop_assert!(syntax_eq(&parsed, &reparsed),
            "String roundtrip failed: {} -> {}", source, displayed);
    }

    #[test]
    fn symbol_roundtrip(s in "[a-z][a-z0-9\\-]{0,8}") {
        let parsed = read_syntax(&s).unwrap();
        let displayed = format!("{}", parsed);
        let reparsed = read_syntax(&displayed).unwrap();
        prop_assert!(syntax_eq(&parsed, &reparsed),
            "Symbol roundtrip failed: {} -> {}", s, displayed);
    }

    #[test]
    fn keyword_roundtrip(s in "[a-z][a-z0-9\\-]{0,8}") {
        let source = format!(":{}", s);
        let parsed = read_syntax(&source).unwrap();
        let displayed = format!("{}", parsed);
        let reparsed = read_syntax(&displayed).unwrap();
        prop_assert!(syntax_eq(&parsed, &reparsed),
            "Keyword roundtrip failed: {} -> {}", source, displayed);
    }

    #[test]
    fn float_roundtrip(source in arb_roundtrippable_float()) {
        let parsed = read_syntax(&source).unwrap();
        let displayed = format!("{}", parsed);
        let reparsed = read_syntax(&displayed).unwrap();
        prop_assert!(syntax_eq(&parsed, &reparsed),
            "Float roundtrip failed: {} -> {}", source, displayed);
    }

    #[test]
    fn list_roundtrip(source in arb_source()) {
        // Only test sources that parse successfully
        if let Ok(parsed) = read_syntax(&source) {
            let displayed = format!("{}", parsed);
            if let Ok(reparsed) = read_syntax(&displayed) {
                prop_assert!(syntax_eq(&parsed, &reparsed),
                    "Roundtrip failed:\n  source:    {}\n  displayed: {}\n  original:  {:?}\n  reparsed:  {:?}",
                    source, displayed, parsed.kind, reparsed.kind);
            }
        }
    }

    // =========================================================================
    // Reader never panics on arbitrary input
    // =========================================================================

    #[test]
    fn reader_never_panics(s in "[ -~]{0,50}") {
        // Any printable ASCII — reader must return Ok or Err, never panic
        let _ = read_syntax(&s);
    }

    #[test]
    fn reader_never_panics_with_delimiters(s in "[\\(\\)\\[\\]\\{\\} a-z0-9\"':,`@#]{0,30}") {
        let _ = read_syntax(&s);
    }

    // =========================================================================
    // Specific roundtrip properties
    // =========================================================================

    #[test]
    fn nil_roundtrip(_dummy in 0..1i32) {
        let parsed = read_syntax("nil").unwrap();
        let displayed = format!("{}", parsed);
        let reparsed = read_syntax(&displayed).unwrap();
        prop_assert!(syntax_eq(&parsed, &reparsed));
    }

    #[test]
    fn empty_list_roundtrip(_dummy in 0..1i32) {
        let parsed = read_syntax("()").unwrap();
        let displayed = format!("{}", parsed);
        let reparsed = read_syntax(&displayed).unwrap();
        prop_assert!(syntax_eq(&parsed, &reparsed));
    }

    #[test]
    fn empty_array_roundtrip(_dummy in 0..1i32) {
        let parsed = read_syntax("@[]").unwrap();
        let displayed = format!("{}", parsed);
        let reparsed = read_syntax(&displayed).unwrap();
        prop_assert!(syntax_eq(&parsed, &reparsed));
    }

    #[test]
    fn nested_list_roundtrip(depth in 1usize..6) {
        let mut source = "42".to_string();
        for _ in 0..depth {
            source = format!("({})", source);
        }
        let parsed = read_syntax(&source).unwrap();
        let displayed = format!("{}", parsed);
        let reparsed = read_syntax(&displayed).unwrap();
        prop_assert!(syntax_eq(&parsed, &reparsed),
            "Nested list roundtrip failed at depth {}", depth);
    }

    #[test]
    fn quoted_roundtrip(source in arb_source_depth(1)) {
        let quoted = format!("'{}", source);
        if let Ok(parsed) = read_syntax(&quoted) {
            let displayed = format!("{}", parsed);
            if let Ok(reparsed) = read_syntax(&displayed) {
                prop_assert!(syntax_eq(&parsed, &reparsed),
                    "Quote roundtrip failed: {} -> {}", quoted, displayed);
            }
        }
    }

    #[test]
    fn multi_element_list_roundtrip(
        elems in prop::collection::vec(-100i64..100, 1..=8)
    ) {
        let inner = elems.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(" ");
        let source = format!("({})", inner);
        let parsed = read_syntax(&source).unwrap();
        let displayed = format!("{}", parsed);
        let reparsed = read_syntax(&displayed).unwrap();
        prop_assert!(syntax_eq(&parsed, &reparsed),
            "Multi-element list roundtrip failed: {} -> {}", source, displayed);
    }

    #[test]
    fn array_roundtrip(
        elems in prop::collection::vec(-100i64..100, 0..=8)
    ) {
        let inner = elems.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(" ");
        let source = format!("@[{}]", inner);
        let parsed = read_syntax(&source).unwrap();
        let displayed = format!("{}", parsed);
        let reparsed = read_syntax(&displayed).unwrap();
        prop_assert!(syntax_eq(&parsed, &reparsed),
            "Array roundtrip failed: {} -> {}", source, displayed);
    }

    #[test]
    fn table_roundtrip(
        pairs in prop::collection::vec(
            ("[a-z]{1,5}".prop_map(|s| s), -100i64..100),
            1..=4
        )
    ) {
        let inner = pairs.iter()
            .map(|(k, v)| format!(":{} {}", k, v))
            .collect::<Vec<_>>()
            .join(" ");
        let source = format!("@{{{}}}", inner);
        let parsed = read_syntax(&source).unwrap();
        let displayed = format!("{}", parsed);
        let reparsed = read_syntax(&displayed).unwrap();
        prop_assert!(syntax_eq(&parsed, &reparsed),
            "Table roundtrip failed: {} -> {}", source, displayed);
    }
}
