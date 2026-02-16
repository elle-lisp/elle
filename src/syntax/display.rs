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
            SyntaxKind::Bool(b) => write!(f, "{}", if *b { "#t" } else { "#f" }),
            SyntaxKind::Int(n) => write!(f, "{}", n),
            SyntaxKind::Float(n) => write!(f, "{}", n),
            SyntaxKind::Symbol(s) => write!(f, "{}", s),
            SyntaxKind::Keyword(s) => write!(f, ":{}", s),
            SyntaxKind::String(s) => write!(f, "\"{}\"", s.escape_default()),
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
            SyntaxKind::Vector(items) => {
                write!(f, "[")?;
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
            SyntaxKind::UnquoteSplicing(inner) => write!(f, ",@{}", inner),
        }
    }
}
