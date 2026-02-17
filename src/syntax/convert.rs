//! Conversion between Syntax and Value
//!
//! These conversions are needed for:
//! - Runtime quote (Syntax → Value)
//! - Macro results that return runtime Values (Value → Syntax)

use super::{Span, Syntax, SyntaxKind};
use crate::symbol::SymbolTable;
use crate::value::Value;

impl Syntax {
    /// Convert Syntax to runtime Value
    /// Used for quote expressions at runtime
    pub fn to_value(&self, symbols: &mut SymbolTable) -> Value {
        match &self.kind {
            SyntaxKind::Nil => Value::NIL,
            SyntaxKind::Bool(b) => Value::bool(*b),
            SyntaxKind::Int(n) => Value::int(*n),
            SyntaxKind::Float(n) => Value::float(*n),
            SyntaxKind::Symbol(s) => {
                let id = symbols.intern(s);
                Value::symbol(id.0)
            }
            SyntaxKind::Keyword(s) => {
                let id = symbols.intern(s);
                Value::keyword(id.0)
            }
            SyntaxKind::String(s) => Value::string(s.clone()),
            SyntaxKind::List(items) => {
                let values: Vec<Value> = items.iter().map(|item| item.to_value(symbols)).collect();
                crate::value::list(values)
            }
            SyntaxKind::Vector(items) => {
                let values: Vec<Value> = items.iter().map(|item| item.to_value(symbols)).collect();
                Value::vector(values)
            }
            SyntaxKind::Quote(inner) => {
                let quote_sym = symbols.intern("quote");
                crate::value::list(vec![Value::symbol(quote_sym.0), inner.to_value(symbols)])
            }
            SyntaxKind::Quasiquote(inner) => {
                let sym = symbols.intern("quasiquote");
                crate::value::list(vec![Value::symbol(sym.0), inner.to_value(symbols)])
            }
            SyntaxKind::Unquote(inner) => {
                let sym = symbols.intern("unquote");
                crate::value::list(vec![Value::symbol(sym.0), inner.to_value(symbols)])
            }
            SyntaxKind::UnquoteSplicing(inner) => {
                let sym = symbols.intern("unquote-splicing");
                crate::value::list(vec![Value::symbol(sym.0), inner.to_value(symbols)])
            }
        }
    }

    /// Convert runtime Value to Syntax
    /// Used for analyzing macro results
    pub fn from_value(value: &Value, symbols: &SymbolTable, span: Span) -> Result<Syntax, String> {
        let kind = if value.is_nil() {
            SyntaxKind::Nil
        } else if let Some(b) = value.as_bool() {
            SyntaxKind::Bool(b)
        } else if let Some(n) = value.as_int() {
            SyntaxKind::Int(n)
        } else if let Some(n) = value.as_float() {
            SyntaxKind::Float(n)
        } else if let Some(id) = value.as_symbol() {
            let name = symbols
                .name(crate::value_old::SymbolId(id))
                .ok_or("Unknown symbol")?;
            SyntaxKind::Symbol(name.to_string())
        } else if let Some(id) = value.as_keyword() {
            let name = symbols
                .name(crate::value_old::SymbolId(id))
                .ok_or("Unknown keyword")?;
            SyntaxKind::Keyword(name.to_string())
        } else if let Some(s) = value.as_string() {
            SyntaxKind::String(s.to_string())
        } else if value.as_cons().is_some() {
            let items = value.list_to_vec().map_err(|e| e.to_string())?;
            let syntaxes: Result<Vec<Syntax>, String> = items
                .iter()
                .map(|v| Syntax::from_value(v, symbols, span.clone()))
                .collect();
            SyntaxKind::List(syntaxes?)
        } else if let Some(vec_ref) = value.as_vector() {
            let items = vec_ref.borrow().clone();
            let syntaxes: Result<Vec<Syntax>, String> = items
                .iter()
                .map(|v| Syntax::from_value(v, symbols, span.clone()))
                .collect();
            SyntaxKind::Vector(syntaxes?)
        } else {
            return Err(format!("Cannot convert {:?} to Syntax", value));
        };
        Ok(Syntax::new(kind, span))
    }
}
