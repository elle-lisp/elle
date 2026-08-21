//! Display implementations for Syntax

use super::{Syntax, SyntaxKind};
use std::fmt;

impl fmt::Display for Syntax {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind)
    }
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
            SyntaxKind::List(items) => {
                write!(f, "(")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, ")")
            }
            SyntaxKind::Array(items) => {
                write!(f, "[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, "]")
            }
            SyntaxKind::ArrayMut(items) => {
                write!(f, "@[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, "]")
            }
            SyntaxKind::Struct(items) => {
                write!(f, "{{")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, "}}")
            }
            SyntaxKind::StructMut(items) => {
                write!(f, "@{{")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, "}}")
            }
            SyntaxKind::Set(items) => {
                write!(f, "|")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, "|")
            }
            SyntaxKind::SetMut(items) => {
                write!(f, "@|")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, "|")
            }
            SyntaxKind::Bytes(items) => {
                write!(f, "b[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, "]")
            }
            SyntaxKind::BytesMut(items) => {
                write!(f, "@b[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, "]")
            }
            SyntaxKind::Quote(inner) => write!(f, "'{}", inner),
            SyntaxKind::Quasiquote(inner) => write!(f, "`{}", inner),
            SyntaxKind::Unquote(inner) => write!(f, ",{}", inner),
            SyntaxKind::UnquoteSplicing(inner) => write!(f, ",;{}", inner),
            SyntaxKind::Splice(inner) => write!(f, ";{}", inner),
            SyntaxKind::SyntaxLiteral(v) => write!(f, "#<syntax-literal:{:?}>", v),
        }
    }
}
