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
            SyntaxKind::Keyword(s) => Value::keyword(s),
            SyntaxKind::String(s) => Value::string(s.clone()),
            SyntaxKind::List(items) => {
                let values: Vec<Value> = items.iter().map(|item| item.to_value(symbols)).collect();
                crate::value::list(values)
            }
            SyntaxKind::Array(items) => {
                let values: Vec<Value> = items.iter().map(|item| item.to_value(symbols)).collect();
                Value::array(values)
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
            // Only reached during macro expansion. The value is a syntax object
            // that will be processed by from_value() after VM evaluation.
            SyntaxKind::SyntaxLiteral(v) => *v,
        }
    }

    /// Convert runtime Value to Syntax
    /// Used for analyzing macro results.
    /// When encountering a syntax object, returns it directly — preserving
    /// scopes from the original Syntax. The passed `span` is ignored in
    /// this case; the syntax object carries its own (more accurate) span.
    pub fn from_value(value: &Value, symbols: &SymbolTable, span: Span) -> Result<Syntax, String> {
        // Syntax objects pass through directly, preserving scopes
        if let Some(syntax_rc) = value.as_syntax() {
            return Ok(syntax_rc.as_ref().clone());
        }
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
                .name(crate::value::SymbolId(id))
                .ok_or("Unknown symbol")?;
            SyntaxKind::Symbol(name.to_string())
        } else if let Some(name) = value.as_keyword_name() {
            SyntaxKind::Keyword(name.to_string())
        } else if let Some(s) = value.as_string() {
            SyntaxKind::String(s.to_string())
        } else if value.is_empty_list() {
            SyntaxKind::List(vec![])
        } else if value.as_cons().is_some() {
            let items = value.list_to_vec().map_err(|e| e.to_string())?;
            let syntaxes: Result<Vec<Syntax>, String> = items
                .iter()
                .map(|v| Syntax::from_value(v, symbols, span.clone()))
                .collect();
            SyntaxKind::List(syntaxes?)
        } else if let Some(vec_ref) = value.as_array() {
            let items = vec_ref.borrow().clone();
            let syntaxes: Result<Vec<Syntax>, String> = items
                .iter()
                .map(|v| Syntax::from_value(v, symbols, span.clone()))
                .collect();
            SyntaxKind::Array(syntaxes?)
        } else {
            return Err(format!("Cannot convert {:?} to Syntax", value));
        };
        Ok(Syntax::new(kind, span))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol::SymbolTable;

    fn test_span() -> Span {
        Span::synthetic()
    }

    #[test]
    fn test_roundtrip_nil() {
        let mut symbols = SymbolTable::new();
        let syntax = Syntax::new(SyntaxKind::Nil, test_span());
        let value = syntax.to_value(&mut symbols);
        let result = Syntax::from_value(&value, &symbols, test_span()).unwrap();
        assert!(matches!(result.kind, SyntaxKind::Nil));
    }

    #[test]
    fn test_roundtrip_int() {
        let mut symbols = SymbolTable::new();
        let syntax = Syntax::new(SyntaxKind::Int(42), test_span());
        let value = syntax.to_value(&mut symbols);
        let result = Syntax::from_value(&value, &symbols, test_span()).unwrap();
        assert!(matches!(result.kind, SyntaxKind::Int(42)));
    }

    #[test]
    fn test_roundtrip_float() {
        let mut symbols = SymbolTable::new();
        let syntax = Syntax::new(SyntaxKind::Float(1.5), test_span());
        let value = syntax.to_value(&mut symbols);
        let result = Syntax::from_value(&value, &symbols, test_span()).unwrap();
        match result.kind {
            SyntaxKind::Float(f) => assert!((f - 1.5).abs() < f64::EPSILON),
            other => panic!("expected Float, got {:?}", other),
        }
    }

    #[test]
    fn test_roundtrip_bool() {
        let mut symbols = SymbolTable::new();
        for b in [true, false] {
            let syntax = Syntax::new(SyntaxKind::Bool(b), test_span());
            let value = syntax.to_value(&mut symbols);
            let result = Syntax::from_value(&value, &symbols, test_span()).unwrap();
            assert!(matches!(result.kind, SyntaxKind::Bool(v) if v == b));
        }
    }

    #[test]
    fn test_roundtrip_string() {
        let mut symbols = SymbolTable::new();
        let syntax = Syntax::new(SyntaxKind::String("hello".to_string()), test_span());
        let value = syntax.to_value(&mut symbols);
        let result = Syntax::from_value(&value, &symbols, test_span()).unwrap();
        assert!(matches!(result.kind, SyntaxKind::String(ref s) if s == "hello"));
    }

    #[test]
    fn test_roundtrip_symbol() {
        let mut symbols = SymbolTable::new();
        let syntax = Syntax::new(SyntaxKind::Symbol("foo".to_string()), test_span());
        let value = syntax.to_value(&mut symbols);
        let result = Syntax::from_value(&value, &symbols, test_span()).unwrap();
        assert!(matches!(result.kind, SyntaxKind::Symbol(ref s) if s == "foo"));
    }

    #[test]
    fn test_roundtrip_keyword() {
        let mut symbols = SymbolTable::new();
        let syntax = Syntax::new(SyntaxKind::Keyword("bar".to_string()), test_span());
        let value = syntax.to_value(&mut symbols);
        let result = Syntax::from_value(&value, &symbols, test_span()).unwrap();
        assert!(matches!(result.kind, SyntaxKind::Keyword(ref s) if s == "bar"));
    }

    #[test]
    fn test_roundtrip_empty_list() {
        let mut symbols = SymbolTable::new();
        let syntax = Syntax::new(SyntaxKind::List(vec![]), test_span());
        let value = syntax.to_value(&mut symbols);
        assert!(value.is_empty_list());
        let result = Syntax::from_value(&value, &symbols, test_span()).unwrap();
        match result.kind {
            SyntaxKind::List(items) => assert!(items.is_empty()),
            other => panic!("expected empty List, got {:?}", other),
        }
    }

    #[test]
    fn test_roundtrip_list() {
        let mut symbols = SymbolTable::new();
        let syntax = Syntax::new(
            SyntaxKind::List(vec![
                Syntax::new(SyntaxKind::Int(1), test_span()),
                Syntax::new(SyntaxKind::Int(2), test_span()),
            ]),
            test_span(),
        );
        let value = syntax.to_value(&mut symbols);
        let result = Syntax::from_value(&value, &symbols, test_span()).unwrap();
        match result.kind {
            SyntaxKind::List(items) => {
                assert_eq!(items.len(), 2);
                assert!(matches!(items[0].kind, SyntaxKind::Int(1)));
                assert!(matches!(items[1].kind, SyntaxKind::Int(2)));
            }
            other => panic!("expected List, got {:?}", other),
        }
    }

    #[test]
    fn test_roundtrip_array() {
        let mut symbols = SymbolTable::new();
        let syntax = Syntax::new(
            SyntaxKind::Array(vec![Syntax::new(SyntaxKind::Int(1), test_span())]),
            test_span(),
        );
        let value = syntax.to_value(&mut symbols);
        let result = Syntax::from_value(&value, &symbols, test_span()).unwrap();
        match result.kind {
            SyntaxKind::Array(items) => {
                assert_eq!(items.len(), 1);
                assert!(matches!(items[0].kind, SyntaxKind::Int(1)));
            }
            other => panic!("expected Array, got {:?}", other),
        }
    }

    #[test]
    fn test_from_value_rejects_closure() {
        let result = Syntax::from_value(
            &Value::native_fn(|_| (0, Value::NIL)),
            &SymbolTable::new(),
            test_span(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_roundtrip_syntax_with_scopes() {
        use crate::syntax::ScopeId;
        // A Syntax node with scopes, wrapped as Value::syntax, should
        // survive from_value and preserve its scopes.
        let scoped = Syntax::with_scopes(
            SyntaxKind::Symbol("x".to_string()),
            test_span(),
            vec![ScopeId(1), ScopeId(2)],
        );
        let value = Value::syntax(scoped.clone());
        let result = Syntax::from_value(&value, &SymbolTable::new(), test_span()).unwrap();
        assert!(matches!(result.kind, SyntaxKind::Symbol(ref s) if s == "x"));
        assert_eq!(result.scopes.len(), 2);
        assert_eq!(result.scopes[0], ScopeId(1));
        assert_eq!(result.scopes[1], ScopeId(2));
    }

    #[test]
    fn test_roundtrip_list_with_scoped_children() {
        use crate::syntax::ScopeId;
        let symbols = SymbolTable::new();
        // A list containing a syntax-object element should preserve
        // the element's scopes through from_value.
        let scoped_child = Syntax::with_scopes(
            SyntaxKind::Symbol("y".to_string()),
            test_span(),
            vec![ScopeId(3)],
        );
        let child_value = Value::syntax(scoped_child);
        let plain_child = Value::int(42);
        let list_value = crate::value::list(vec![child_value, plain_child]);
        let result = Syntax::from_value(&list_value, &symbols, test_span()).unwrap();
        match result.kind {
            SyntaxKind::List(items) => {
                assert_eq!(items.len(), 2);
                // First element: syntax object with scopes preserved
                assert!(matches!(items[0].kind, SyntaxKind::Symbol(ref s) if s == "y"));
                assert_eq!(items[0].scopes, vec![ScopeId(3)]);
                // Second element: plain int, no scopes
                assert!(matches!(items[1].kind, SyntaxKind::Int(42)));
                assert!(items[1].scopes.is_empty());
            }
            other => panic!("expected List, got {:?}", other),
        }
    }

    #[test]
    fn test_no_scopes_produces_plain_value() {
        let mut symbols = SymbolTable::new();
        // Syntax with empty scopes through to_value produces a plain value,
        // not a syntax object.
        let syntax = Syntax::new(SyntaxKind::Symbol("z".to_string()), test_span());
        let value = syntax.to_value(&mut symbols);
        // Should be a plain symbol, not a syntax object
        assert!(value.as_symbol().is_some());
        assert!(!value.is_syntax());
    }
}
