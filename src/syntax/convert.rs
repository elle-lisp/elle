//! Conversion between Syntax and Value
//!
//! These conversions are needed for:
//! - Runtime quote (Syntax → Value)
//! - Macro results that return runtime Values (Value → Syntax)

use super::{Span, Syntax, SyntaxKind};
use crate::symbol::SymbolTable;
use crate::value::Value;
use std::rc::Rc;

impl Syntax {
    /// Convert Syntax to runtime Value
    /// Used for quote expressions at runtime
    pub fn to_value(&self, symbols: &mut SymbolTable) -> Value {
        match &self.kind {
            SyntaxKind::Nil => Value::Nil,
            SyntaxKind::Bool(b) => Value::Bool(*b),
            SyntaxKind::Int(n) => Value::Int(*n),
            SyntaxKind::Float(n) => Value::Float(*n),
            SyntaxKind::Symbol(s) => Value::Symbol(symbols.intern(s)),
            SyntaxKind::Keyword(s) => Value::Keyword(symbols.intern(s)),
            SyntaxKind::String(s) => Value::String(Rc::from(s.as_str())),
            SyntaxKind::List(items) => {
                let values: Vec<Value> = items.iter().map(|item| item.to_value(symbols)).collect();
                crate::value::list(values)
            }
            SyntaxKind::Vector(items) => {
                let values: Vec<Value> = items.iter().map(|item| item.to_value(symbols)).collect();
                Value::Vector(Rc::new(values))
            }
            SyntaxKind::Quote(inner) => {
                let quote_sym = symbols.intern("quote");
                crate::value::list(vec![Value::Symbol(quote_sym), inner.to_value(symbols)])
            }
            SyntaxKind::Quasiquote(inner) => {
                let sym = symbols.intern("quasiquote");
                crate::value::list(vec![Value::Symbol(sym), inner.to_value(symbols)])
            }
            SyntaxKind::Unquote(inner) => {
                let sym = symbols.intern("unquote");
                crate::value::list(vec![Value::Symbol(sym), inner.to_value(symbols)])
            }
            SyntaxKind::UnquoteSplicing(inner) => {
                let sym = symbols.intern("unquote-splicing");
                crate::value::list(vec![Value::Symbol(sym), inner.to_value(symbols)])
            }
        }
    }

    /// Convert runtime Value to Syntax
    /// Used for analyzing macro results
    pub fn from_value(value: &Value, symbols: &SymbolTable, span: Span) -> Result<Syntax, String> {
        let kind = match value {
            Value::Nil => SyntaxKind::Nil,
            Value::Bool(b) => SyntaxKind::Bool(*b),
            Value::Int(n) => SyntaxKind::Int(*n),
            Value::Float(n) => SyntaxKind::Float(*n),
            Value::Symbol(id) => {
                let name = symbols.name(*id).ok_or("Unknown symbol")?;
                SyntaxKind::Symbol(name.to_string())
            }
            Value::Keyword(id) => {
                let name = symbols.name(*id).ok_or("Unknown keyword")?;
                SyntaxKind::Keyword(name.to_string())
            }
            Value::String(s) => SyntaxKind::String(s.to_string()),
            Value::Cons(_) => {
                let items = value.list_to_vec()?;
                let syntaxes: Result<Vec<Syntax>, String> = items
                    .iter()
                    .map(|v| Syntax::from_value(v, symbols, span.clone()))
                    .collect();
                SyntaxKind::List(syntaxes?)
            }
            Value::Vector(items) => {
                let syntaxes: Result<Vec<Syntax>, String> = items
                    .iter()
                    .map(|v| Syntax::from_value(v, symbols, span.clone()))
                    .collect();
                SyntaxKind::Vector(syntaxes?)
            }
            _ => return Err(format!("Cannot convert {:?} to Syntax", value)),
        };
        Ok(Syntax::new(kind, span))
    }
}
