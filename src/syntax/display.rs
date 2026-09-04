//! Display implementations for Syntax

use super::{Syntax, SyntaxKind};
use std::fmt;

impl fmt::Display for Syntax {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind)
    }
}

/// Write `items` space-separated between `open` and `close`.
fn delimited(f: &mut fmt::Formatter<'_>, open: &str, items: &[Syntax], close: &str) -> fmt::Result {
    f.write_str(open)?;
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            f.write_str(" ")?;
        }
        write!(f, "{}", item)?;
    }
    f.write_str(close)
}

impl fmt::Display for SyntaxKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SyntaxKind::Nil => write!(f, "nil"),
            SyntaxKind::Bool(b) => write!(f, "{}", if *b { "true" } else { "false" }),
            SyntaxKind::Int(n) => write!(f, "{}", n),
            // Debug (`{:?}`), not Display (`{}`): this text is re-read by the
            // reader (`splice_includes` round-trips each user form through this
            // impl), and the reader only produces a `Float` when the token
            // carries a '.' or exponent. `{}` on an integral f64 drops the
            // fraction (`7.0` → "7"), which re-reads as `Int(7)` — silently
            // retyping the literal. `{:?}` emits the shortest round-trippable
            // form ("7.0", "1.5", "1e21"), all of which the reader lexes as
            // floats. Pinned by `test_display_float_integral_round_trips`.
            SyntaxKind::Float(n) => write!(f, "{:?}", n),
            SyntaxKind::Symbol(s) => write!(f, "{}", s),
            SyntaxKind::Keyword(s) => write!(f, ":{}", s),
            SyntaxKind::String(s) => write!(f, "\"{}\"", s.escape_default()),
            SyntaxKind::StringMut(s) => write!(f, "@\"{}\"", s.escape_default()),
            SyntaxKind::List(items) => delimited(f, "(", items, ")"),
            SyntaxKind::Array(items) => delimited(f, "[", items, "]"),
            SyntaxKind::ArrayMut(items) => delimited(f, "@[", items, "]"),
            SyntaxKind::Struct(items) => delimited(f, "{", items, "}"),
            SyntaxKind::StructMut(items) => delimited(f, "@{", items, "}"),
            SyntaxKind::Set(items) => delimited(f, "|", items, "|"),
            SyntaxKind::SetMut(items) => delimited(f, "@|", items, "|"),
            SyntaxKind::Bytes(items) => delimited(f, "b[", items, "]"),
            SyntaxKind::BytesMut(items) => delimited(f, "@b[", items, "]"),
            SyntaxKind::Quote(inner) => write!(f, "'{}", **inner),
            SyntaxKind::Quasiquote(inner) => write!(f, "`{}", **inner),
            SyntaxKind::Unquote(inner) => write!(f, ",{}", **inner),
            SyntaxKind::UnquoteSplicing(inner) => write!(f, ",;{}", **inner),
            SyntaxKind::Splice(inner) => write!(f, ";{}", **inner),
            SyntaxKind::SyntaxLiteral(v) => write!(f, "#<syntax-literal:{:?}>", **v),
        }
    }
}
