//! Conversion between Syntax and Value
//!
//! These conversions are needed for:
//! - Runtime quote (Syntax → Value)
//! - Macro results that return runtime Values (Value → Syntax)

use super::{Span, Syntax, SyntaxKind};
use crate::symbol::SymbolTable;
use crate::value::{TableKey, Value};

/// Check if a Syntax tree contains any SyntaxLiteral nodes.
/// Used as a debug assertion in `from_value` to catch arena pointer escapes.
fn contains_syntax_literal(s: &Syntax) -> bool {
    match &s.kind {
        SyntaxKind::SyntaxLiteral(_) => true,
        SyntaxKind::List(items)
        | SyntaxKind::Tuple(items)
        | SyntaxKind::Array(items)
        | SyntaxKind::Struct(items)
        | SyntaxKind::Table(items) => items.iter().any(contains_syntax_literal),
        SyntaxKind::Quote(inner)
        | SyntaxKind::Quasiquote(inner)
        | SyntaxKind::Unquote(inner)
        | SyntaxKind::UnquoteSplicing(inner)
        | SyntaxKind::Splice(inner) => contains_syntax_literal(inner),
        _ => false,
    }
}

/// Convert a TableKey back to a Syntax node.
fn table_key_to_syntax(
    key: &TableKey,
    symbols: &SymbolTable,
    span: &Span,
) -> Result<Syntax, String> {
    let kind = match key {
        TableKey::Nil => SyntaxKind::Nil,
        TableKey::Bool(b) => SyntaxKind::Bool(*b),
        TableKey::Int(n) => SyntaxKind::Int(*n),
        TableKey::Symbol(id) => {
            let name = symbols.name(*id).ok_or("Unknown symbol in table key")?;
            SyntaxKind::Symbol(name.to_string())
        }
        TableKey::String(s) => SyntaxKind::String(s.clone()),
        TableKey::Keyword(s) => SyntaxKind::Keyword(s.clone()),
        TableKey::Identity(_) => {
            return Err("Cannot convert identity key to Syntax".to_string());
        }
    };
    Ok(Syntax::new(kind, span.clone()))
}

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
            SyntaxKind::Tuple(items) => {
                let values: Vec<Value> = items.iter().map(|item| item.to_value(symbols)).collect();
                Value::tuple(values)
            }
            SyntaxKind::Array(items) => {
                let values: Vec<Value> = items.iter().map(|item| item.to_value(symbols)).collect();
                Value::array(values)
            }
            SyntaxKind::Struct(items) => {
                // Convert to (struct k1 v1 k2 v2 ...) list
                let struct_sym = symbols.intern("struct");
                let mut values = vec![Value::symbol(struct_sym.0)];
                values.extend(items.iter().map(|item| item.to_value(symbols)));
                crate::value::list(values)
            }
            SyntaxKind::Table(items) => {
                // Convert to (table k1 v1 k2 v2 ...) list
                let table_sym = symbols.intern("table");
                let mut values = vec![Value::symbol(table_sym.0)];
                values.extend(items.iter().map(|item| item.to_value(symbols)));
                crate::value::list(values)
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
            SyntaxKind::Splice(inner) => {
                let sym = symbols.intern("splice");
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
        // Syntax objects pass through directly, preserving scopes.
        // Mark as scope_exempt so the intro scope isn't stamped on
        // call-site identifiers that survived the Value round-trip.
        if let Some(syntax_rc) = value.as_syntax() {
            let mut s = syntax_rc.as_ref().clone();
            s.scope_exempt = true;
            // Safety check: the cloned Syntax must not contain SyntaxLiteral
            // children. SyntaxLiteral holds a heap-pointer Value that may be
            // arena-allocated; if it survives into the result Syntax, it will
            // dangle after arena release. Current code paths don't produce
            // nested SyntaxLiterals, but this assertion catches future regressions.
            debug_assert!(
                !contains_syntax_literal(&s),
                "from_value: cloned Syntax contains SyntaxLiteral (arena pointer would escape)"
            );
            return Ok(s);
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
        } else if let Some(s) = value.with_string(|s| s.to_string()) {
            SyntaxKind::String(s)
        } else if value.is_empty_list() {
            SyntaxKind::List(vec![])
        } else if value.as_cons().is_some() {
            let items = value.list_to_vec().map_err(|e| e.to_string())?;
            let syntaxes: Result<Vec<Syntax>, String> = items
                .iter()
                .map(|v| Syntax::from_value(v, symbols, span.clone()))
                .collect();
            SyntaxKind::List(syntaxes?)
        } else if let Some(elems) = value.as_tuple() {
            let syntaxes: Result<Vec<Syntax>, String> = elems
                .iter()
                .map(|v| Syntax::from_value(v, symbols, span.clone()))
                .collect();
            SyntaxKind::Tuple(syntaxes?)
        } else if let Some(vec_ref) = value.as_array() {
            let items = vec_ref.borrow().clone();
            let syntaxes: Result<Vec<Syntax>, String> = items
                .iter()
                .map(|v| Syntax::from_value(v, symbols, span.clone()))
                .collect();
            SyntaxKind::Array(syntaxes?)
        } else if let Some(struct_ref) = value.as_struct() {
            let mut syntaxes = Vec::with_capacity(struct_ref.len() * 2);
            for (k, v) in struct_ref.iter() {
                syntaxes.push(table_key_to_syntax(k, symbols, &span)?);
                syntaxes.push(Syntax::from_value(v, symbols, span.clone())?);
            }
            SyntaxKind::Struct(syntaxes)
        } else if let Some(table_ref) = value.as_table() {
            let items = table_ref.borrow();
            let mut syntaxes = Vec::with_capacity(items.len() * 2);
            for (k, v) in items.iter() {
                syntaxes.push(table_key_to_syntax(k, symbols, &span)?);
                syntaxes.push(Syntax::from_value(v, symbols, span.clone())?);
            }
            SyntaxKind::Table(syntaxes)
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
    fn test_roundtrip_tuple() {
        let mut symbols = SymbolTable::new();
        let syntax = Syntax::new(
            SyntaxKind::Tuple(vec![Syntax::new(SyntaxKind::Int(1), test_span())]),
            test_span(),
        );
        let value = syntax.to_value(&mut symbols);
        let result = Syntax::from_value(&value, &symbols, test_span()).unwrap();
        match result.kind {
            SyntaxKind::Tuple(items) => {
                assert_eq!(items.len(), 1);
                assert!(matches!(items[0].kind, SyntaxKind::Int(1)));
            }
            other => panic!("expected Tuple, got {:?}", other),
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

    #[test]
    fn test_roundtrip_struct() {
        use crate::value::TableKey;
        use std::collections::BTreeMap;

        let symbols = SymbolTable::new();
        // Build a struct Value directly (to_value produces a list, not a struct)
        let mut fields = BTreeMap::new();
        fields.insert(TableKey::Keyword("alpha".to_string()), Value::int(1));
        fields.insert(TableKey::Keyword("bravo".to_string()), Value::int(2));
        let value = Value::struct_from(fields);

        let result = Syntax::from_value(&value, &symbols, test_span()).unwrap();
        match result.kind {
            SyntaxKind::Struct(items) => {
                assert_eq!(
                    items.len(),
                    4,
                    "expected 4 items (2 key-value pairs), got {}",
                    items.len()
                );
                // BTreeMap sorts by key, so order may differ from insertion.
                // Just verify all keys and values are present.
                let keys: Vec<_> = items.iter().step_by(2).collect();
                let vals: Vec<_> = items.iter().skip(1).step_by(2).collect();
                assert!(keys
                    .iter()
                    .any(|k| matches!(&k.kind, SyntaxKind::Keyword(s) if s == "alpha")));
                assert!(keys
                    .iter()
                    .any(|k| matches!(&k.kind, SyntaxKind::Keyword(s) if s == "bravo")));
                assert!(vals.iter().any(|v| matches!(&v.kind, SyntaxKind::Int(1))));
                assert!(vals.iter().any(|v| matches!(&v.kind, SyntaxKind::Int(2))));
            }
            other => panic!("expected Struct, got {:?}", other),
        }
    }

    #[test]
    fn test_roundtrip_table() {
        use crate::value::TableKey;
        use std::collections::BTreeMap;

        let symbols = SymbolTable::new();
        // Build a table Value directly (to_value produces a list, not a table)
        let mut entries = BTreeMap::new();
        entries.insert(TableKey::Keyword("charlie".to_string()), Value::int(3));
        let value = Value::table_from(entries);

        let result = Syntax::from_value(&value, &symbols, test_span()).unwrap();
        match result.kind {
            SyntaxKind::Table(items) => {
                assert_eq!(
                    items.len(),
                    2,
                    "expected 2 items (1 key-value pair), got {}",
                    items.len()
                );
                assert!(matches!(&items[0].kind, SyntaxKind::Keyword(s) if s == "charlie"));
                assert!(matches!(&items[1].kind, SyntaxKind::Int(3)));
            }
            other => panic!("expected Table, got {:?}", other),
        }
    }
}
