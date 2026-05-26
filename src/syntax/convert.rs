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
        | SyntaxKind::Array(items)
        | SyntaxKind::ArrayMut(items)
        | SyntaxKind::Struct(items)
        | SyntaxKind::StructMut(items)
        | SyntaxKind::Set(items)
        | SyntaxKind::SetMut(items) => items.iter().any(contains_syntax_literal),
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
        TableKey::EmptyList => SyntaxKind::List(vec![]),
        TableKey::Array(keys) => {
            let elements: Result<Vec<_>, _> = keys
                .iter()
                .map(|k| table_key_to_syntax(k, symbols, span))
                .collect();
            return Ok(Syntax::new(SyntaxKind::Array(elements?), span.clone()));
        }
        TableKey::Heap(_) => {
            return Err("Cannot convert heap key to Syntax".to_string());
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
            SyntaxKind::String(s) => Value::string_permanent(s.clone()),
            SyntaxKind::StringMut(s) => Value::string_mut(s.as_bytes().to_vec()),
            SyntaxKind::List(items) => {
                let values: Vec<Value> = items.iter().map(|item| item.to_value(symbols)).collect();
                crate::value::list_permanent(values)
            }
            SyntaxKind::Array(items) => {
                let values: Vec<Value> = items.iter().map(|item| item.to_value(symbols)).collect();
                Value::array_permanent(values)
            }
            SyntaxKind::ArrayMut(items) => {
                let values: Vec<Value> = items.iter().map(|item| item.to_value(symbols)).collect();
                Value::array_mut(values)
            }
            SyntaxKind::Bytes(items) => {
                // Convert to (bytes e1 e2 ...) list
                let bytes_sym = symbols.intern("bytes");
                let mut values = vec![Value::symbol(bytes_sym.0)];
                values.extend(items.iter().map(|item| item.to_value(symbols)));
                crate::value::list_permanent(values)
            }
            SyntaxKind::BytesMut(items) => {
                // Convert to (@bytes e1 e2 ...) list
                let bytes_mut_sym = symbols.intern("@bytes");
                let mut values = vec![Value::symbol(bytes_mut_sym.0)];
                values.extend(items.iter().map(|item| item.to_value(symbols)));
                crate::value::list_permanent(values)
            }
            SyntaxKind::Struct(items) => {
                // Convert to (struct k1 v1 k2 v2 ...) list
                let struct_sym = symbols.intern("struct");
                let mut values = vec![Value::symbol(struct_sym.0)];
                values.extend(items.iter().map(|item| item.to_value(symbols)));
                crate::value::list_permanent(values)
            }
            SyntaxKind::StructMut(items) => {
                // Convert to (@struct k1 v1 k2 v2 ...) list
                let struct_mut_sym = symbols.intern("@struct");
                let mut values = vec![Value::symbol(struct_mut_sym.0)];
                values.extend(items.iter().map(|item| item.to_value(symbols)));
                crate::value::list_permanent(values)
            }
            SyntaxKind::Set(items) => {
                // Convert to (set e1 e2 ...) list
                let set_sym = symbols.intern("set");
                let mut values = vec![Value::symbol(set_sym.0)];
                values.extend(items.iter().map(|item| item.to_value(symbols)));
                crate::value::list_permanent(values)
            }
            SyntaxKind::SetMut(items) => {
                // Convert to (@set e1 e2 ...) list
                let set_mut_sym = symbols.intern("@set");
                let mut values = vec![Value::symbol(set_mut_sym.0)];
                values.extend(items.iter().map(|item| item.to_value(symbols)));
                crate::value::list_permanent(values)
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
        // The intro scope WILL be added by add_scope_recursive, which
        // is correct: it distinguishes this expansion's symbols from
        // other scopes. Both template symbols (from quasiquote) and
        // argument symbols (from unquote) get the intro scope on top
        // of their existing scopes, enabling proper hygiene resolution.
        if let Some(syntax_rc) = value.as_syntax() {
            let mut s = syntax_rc.clone();
            // Mark scope_exempt so the intro scope isn't added to
            // call-site identifiers that survived the Value round-trip.
            // Template symbols from quasiquote also come through here
            // (via SyntaxLiteral) and are exempt — their definition-site
            // scopes are sufficient for correct resolution.
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
        } else if let Some(data) = value.as_string_mut() {
            let bytes = data.borrow();
            let s = String::from_utf8(bytes.clone())
                .map_err(|_| "Cannot convert non-UTF-8 @string to Syntax")?;
            SyntaxKind::StringMut(s)
        } else if value.is_empty_list() {
            SyntaxKind::List(vec![])
        } else if value.as_pair().is_some() {
            let items = value.list_to_vec().map_err(|e| e.to_string())?;
            let syntaxes: Result<Vec<Syntax>, String> = items
                .iter()
                .map(|v| Syntax::from_value(v, symbols, span.clone()))
                .collect();
            SyntaxKind::List(syntaxes?)
        } else if let Some(elems) = value.as_array() {
            let syntaxes: Result<Vec<Syntax>, String> = elems
                .iter()
                .map(|v| Syntax::from_value(v, symbols, span.clone()))
                .collect();
            SyntaxKind::Array(syntaxes?)
        } else if let Some(vec_ref) = value.as_array_mut() {
            let items = vec_ref.borrow().clone();
            let syntaxes: Result<Vec<Syntax>, String> = items
                .iter()
                .map(|v| Syntax::from_value(v, symbols, span.clone()))
                .collect();
            SyntaxKind::ArrayMut(syntaxes?)
        } else if let Some(data) = value.as_bytes() {
            let syntaxes: Vec<Syntax> = data
                .iter()
                .map(|b| Syntax::new(SyntaxKind::Int(*b as i64), span.clone()))
                .collect();
            SyntaxKind::Bytes(syntaxes)
        } else if let Some(data) = value.as_bytes_mut() {
            let bytes = data.borrow();
            let syntaxes: Vec<Syntax> = bytes
                .iter()
                .map(|b| Syntax::new(SyntaxKind::Int(*b as i64), span.clone()))
                .collect();
            SyntaxKind::BytesMut(syntaxes)
        } else if let Some(elems) = value.as_set() {
            let syntaxes: Result<Vec<Syntax>, String> = elems
                .iter()
                .map(|v| Syntax::from_value(v, symbols, span.clone()))
                .collect();
            SyntaxKind::Set(syntaxes?)
        } else if let Some(set_ref) = value.as_set_mut() {
            let items = set_ref.borrow();
            let syntaxes: Result<Vec<Syntax>, String> = items
                .iter()
                .map(|v| Syntax::from_value(v, symbols, span.clone()))
                .collect();
            SyntaxKind::SetMut(syntaxes?)
        } else if let Some(struct_ref) = value.as_struct() {
            let mut syntaxes = Vec::with_capacity(struct_ref.len() * 2);
            for (k, v) in struct_ref.iter() {
                syntaxes.push(table_key_to_syntax(k, symbols, &span)?);
                syntaxes.push(Syntax::from_value(v, symbols, span.clone())?);
            }
            SyntaxKind::Struct(syntaxes)
        } else if let Some(table_ref) = value.as_struct_mut() {
            let items = table_ref.borrow();
            let mut syntaxes = Vec::with_capacity(items.len() * 2);
            for (k, v) in items.iter() {
                syntaxes.push(table_key_to_syntax(k, symbols, &span)?);
                syntaxes.push(Syntax::from_value(v, symbols, span.clone())?);
            }
            SyntaxKind::StructMut(syntaxes)
        } else {
            return Err(format!("Cannot convert {:?} to Syntax", value));
        };
        Ok(Syntax::new(kind, span))
    }
}

// Tests migrated to tests/elle/syntax-roundtrip.lisp
