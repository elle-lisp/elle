// audited: 2026-11-25
//! Conversion between Syntax and Value: source as data, a quote's constant, and macro results read back as syntax.
//!
//! docs/syntax.md
//! docs/impl/syntax.md
//! docs/impl/region/model.md
//!
//! `tests/lang/datum.lisp` and `tests/lang/syntax-roundtrip.lisp` pin the
//! round trip.

use std::collections::{BTreeMap, BTreeSet};

use super::{Span, Syntax, SyntaxArena, SyntaxKind};
use crate::primitives::ctx::NativeCtx;
use crate::symbol::SymbolTable;
use crate::value::{TableKey, Value};

/// Check if a Syntax tree contains any SyntaxLiteral nodes. `from_value`
/// asserts that a syntax object a transformer answers holds none, because
/// quasiquote builds no syntax literal inside one.
pub(crate) fn contains_syntax_literal(s: &Syntax) -> bool {
    match &s.kind {
        SyntaxKind::SyntaxLiteral(_) => true,
        other => other.children().iter().any(contains_syntax_literal),
    }
}

/// Record every keyword spelling in `syntax` into the instance memo — the
/// keyword analogue of interning identifiers. The expander calls it on each
/// tree it enters, because macro arguments and quasiquote templates round-trip
/// keywords as bare hashes, so their spellings must be learned first. The
/// analyzer calls it as a pre-pass (docs/impl/symbol.md). Idempotent; the memo
/// dedups.
pub(crate) fn learn_keywords(syntax: &Syntax, symbols: &mut SymbolTable) {
    match &syntax.kind {
        SyntaxKind::Keyword(name) => {
            symbols.keyword(name);
        }
        // A syntax literal reports no children to the shared walk, but its
        // captured node can hold keywords, so it is named here.
        SyntaxKind::SyntaxLiteral(inner) => learn_keywords(inner, symbols),
        other => {
            for item in other.children() {
                learn_keywords(item, symbols);
            }
        }
    }
}

/// Convert a TableKey back to a Syntax node born in `arena`.
fn table_key_to_syntax(
    arena: &SyntaxArena,
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
            SyntaxKind::Symbol(arena.text(name))
        }
        TableKey::String(v) => {
            SyntaxKind::String(arena.text(v.as_str().expect("a string key holds a string")))
        }
        TableKey::Keyword(hash) => {
            let name = crate::value::keyword::resolve_keyword_name(Some(symbols), *hash)
                .ok_or_else(|| format!("Unknown keyword {:#x} in table key", hash))?;
            SyntaxKind::Keyword(arena.text(name))
        }
        TableKey::EmptyList => SyntaxKind::List(arena.nodes(&[])),
        TableKey::Array(v) => {
            let elements: Result<Vec<_>, String> = v
                .as_array()
                .expect("an array key holds an array")
                .iter()
                .map(|elem| {
                    let k = TableKey::from_value(elem).expect("from_value validated every element");
                    table_key_to_syntax(arena, &k, symbols, span)
                })
                .collect();
            return Ok(Syntax::array(arena, &elements?, *span));
        }
        // A list, set, struct or bytes key: the value it holds, which a
        // struct literal may write as a key.
        TableKey::Heap(v) => return Syntax::from_value(arena, v, symbols, *span),
    };
    Ok(Syntax::new(kind, *span))
}

/// The values `items` convert to, each through `Syntax::to_value_in`.
fn values_of(
    items: &[Syntax],
    symbols: &mut SymbolTable,
    ctx: &mut NativeCtx,
) -> Result<Vec<Value>, String> {
    items
        .iter()
        .map(|item| item.to_value_in(symbols, ctx))
        .collect()
}

/// A quoting form or a bytes literal as data: the list `(name items…)`.
fn call_of(
    name: &str,
    items: &[Syntax],
    symbols: &mut SymbolTable,
    ctx: &mut NativeCtx,
) -> Result<Value, String> {
    let mut values = vec![Value::symbol(symbols.intern(name))];
    values.extend(values_of(items, symbols, ctx)?);
    Ok(ctx.list(values))
}

/// The entries a struct literal's children write, keys and values taking
/// turns. A repeated key keeps its last value, as the running struct does.
fn struct_entries(
    items: &[Syntax],
    symbols: &mut SymbolTable,
    ctx: &mut NativeCtx,
) -> Result<BTreeMap<TableKey, Value>, String> {
    if !items.len().is_multiple_of(2) {
        return Err(format!(
            "a struct literal holds {} forms, which is no set of key-value pairs",
            items.len()
        ));
    }
    let mut entries = BTreeMap::new();
    for pair in items.chunks(2) {
        let key = pair[0].to_value_in(symbols, ctx)?;
        let key = TableKey::from_value(&key).ok_or_else(|| {
            format!(
                "a struct key must be an immutable value other than a float, got {}",
                key.type_name()
            )
        })?;
        entries.insert(key, pair[1].to_value_in(symbols, ctx)?);
    }
    Ok(entries)
}

/// The members a set literal's children write, each frozen as `set` freezes
/// it. A repeated member is one member.
fn set_members(
    items: &[Syntax],
    symbols: &mut SymbolTable,
    ctx: &mut NativeCtx,
) -> Result<BTreeSet<Value>, String> {
    let mut members = BTreeSet::new();
    for item in items {
        let member = item.to_value_in(symbols, ctx)?;
        members.insert(crate::primitives::sets::freeze_value(member, ctx));
    }
    Ok(members)
}

impl Syntax {
    /// Convert Syntax to the data `read`, `read-all` and `syntax/->datum`
    /// answer, as an ordinary allocation into the ctx's own region
    /// (docs/syntax.md, docs/impl/region/model.md).
    ///
    /// A literal answers the value it writes, a struct or set included. A
    /// bytes literal answers a call of `bytes`, because its elements are
    /// expressions. A struct key that no struct holds is the error.
    ///
    /// `ctx` is the allocation capability (docs/impl/region/ctx.md). The whole
    /// tree lands in the ctx's region, reclaimed at the caller's
    /// `decref_point`.
    pub fn to_value(
        &self,
        symbols: &mut SymbolTable,
        ctx: &mut NativeCtx,
    ) -> Result<Value, String> {
        self.to_value_in(symbols, ctx)
    }

    /// The recursive half of [`to_value`](Self::to_value). Immediates need no
    /// region, and every heap leaf is born in the ctx's region.
    fn to_value_in(self, symbols: &mut SymbolTable, ctx: &mut NativeCtx) -> Result<Value, String> {
        Ok(match &self.kind {
            SyntaxKind::Nil => Value::NIL,
            SyntaxKind::Bool(b) => Value::bool(*b),
            SyntaxKind::Int(n) => Value::int(*n),
            SyntaxKind::Float(n) => Value::float(*n),
            SyntaxKind::Symbol(s) => Value::symbol(symbols.intern(s)),
            SyntaxKind::Keyword(s) => {
                // A learning site: read-time keyword data records its spelling
                // as the analyzer does for compiled source.
                symbols.keyword(s);
                Value::keyword(s)
            }
            SyntaxKind::String(s) => ctx.string(s),
            SyntaxKind::StringMut(s) => ctx.string_mut(s.as_bytes().to_vec()),
            SyntaxKind::List(items) => {
                let values = values_of(items, symbols, ctx)?;
                ctx.list(values)
            }
            SyntaxKind::Array(items) => {
                let values = values_of(items, symbols, ctx)?;
                ctx.array(values)
            }
            SyntaxKind::ArrayMut(items) => {
                let values = values_of(items, symbols, ctx)?;
                ctx.array_mut(values)
            }
            SyntaxKind::Struct(items) => {
                let entries = struct_entries(items, symbols, ctx)?;
                ctx.struct_from(entries)
            }
            SyntaxKind::StructMut(items) => {
                let entries = struct_entries(items, symbols, ctx)?;
                ctx.struct_mut_from(entries)
            }
            SyntaxKind::Set(items) => {
                let members = set_members(items, symbols, ctx)?;
                ctx.set(members)
            }
            SyntaxKind::SetMut(items) => {
                let members = set_members(items, symbols, ctx)?;
                ctx.set_mut(members)
            }
            SyntaxKind::Bytes(items) => return call_of("bytes", items, symbols, ctx),
            SyntaxKind::BytesMut(items) => return call_of("@bytes", items, symbols, ctx),
            SyntaxKind::Quote(inner) => return call_of("quote", &[**inner], symbols, ctx),
            SyntaxKind::Quasiquote(inner) => {
                return call_of("quasiquote", &[**inner], symbols, ctx)
            }
            SyntaxKind::Unquote(inner) => return call_of("unquote", &[**inner], symbols, ctx),
            SyntaxKind::UnquoteSplicing(inner) => {
                return call_of("unquote-splicing", &[**inner], symbols, ctx)
            }
            SyntaxKind::Splice(inner) => return call_of("splice", &[**inner], symbols, ctx),
            // A hygiene-bearing template symbol. Materialize a fresh syntax object
            // (ordinary allocation into the ctx's region) wrapping the carried
            // `Syntax` — processed by from_value() after VM evaluation.
            SyntaxKind::SyntaxLiteral(s) => ctx.syntax(**s),
        })
    }

    /// Convert Syntax to a `ConstTemplate` — the allocation-free compile-time
    /// form of a quoted datum: plain recursive data that
    /// `MaterializeConst` materializes fresh into a reclaimable region each
    /// execution (docs/impl/region/model.md).
    /// A quote differs from [`to_value`](Self::to_value) at a struct, set or
    /// bytes literal, which it writes as a call: a quoted `{:a 1}` becomes the
    /// list template `(struct :a 1)` (docs/syntax.md).
    ///
    /// A hygiene-bearing `SyntaxLiteral` (always a symbol) becomes a
    /// `ConstTemplate::SyntaxSymbol` carrying its scope set verbatim, so it too
    /// materializes ordinarily with hygiene intact.
    ///
    /// Symbols are carried by name, and intern into the executing instance's
    /// table when the template materializes. So this needs no `SymbolTable`.
    pub fn to_const_template(&self) -> crate::value::ConstTemplate {
        use crate::value::ConstTemplate as T;
        // Build a cons-list template `(e1 e2 … en)` ending in the empty list —
        // the desugaring `to_value` performs via `list_in`.
        fn list_template(items: Vec<T>) -> T {
            items.into_iter().rev().fold(T::EmptyList, |acc, item| {
                T::Pair(Box::new(item), Box::new(acc))
            })
        }
        // A desugared `(head e1 e2 …)` list template, head being a leading
        // constructor symbol (`struct`/`set`/`bytes`/`@…`).
        let tagged_list = |head: &str, items: &[Syntax]| -> T {
            let mut vals = vec![T::Symbol(head.to_string())];
            vals.extend(items.iter().map(|item| item.to_const_template()));
            list_template(vals)
        };
        match &self.kind {
            SyntaxKind::Nil => T::Nil,
            SyntaxKind::Bool(b) => T::Bool(*b),
            SyntaxKind::Int(n) => T::Int(*n),
            SyntaxKind::Float(n) => T::Float(*n),
            SyntaxKind::Symbol(s) => T::Symbol(s.to_string()),
            SyntaxKind::Keyword(s) => T::Keyword(s.to_string()),
            SyntaxKind::String(s) => T::String(s.to_string()),
            SyntaxKind::StringMut(s) => T::StringMut(s.to_string()),
            SyntaxKind::List(items) => {
                list_template(items.iter().map(|i| i.to_const_template()).collect())
            }
            SyntaxKind::Array(items) => {
                T::Array(items.iter().map(|i| i.to_const_template()).collect())
            }
            SyntaxKind::ArrayMut(items) => {
                T::ArrayMut(items.iter().map(|i| i.to_const_template()).collect())
            }
            SyntaxKind::Bytes(items) => tagged_list("bytes", items),
            SyntaxKind::BytesMut(items) => tagged_list("@bytes", items),
            SyntaxKind::Struct(items) => tagged_list("struct", items),
            SyntaxKind::StructMut(items) => tagged_list("@struct", items),
            SyntaxKind::Set(items) => tagged_list("set", items),
            SyntaxKind::SetMut(items) => tagged_list("@set", items),
            SyntaxKind::Quote(inner) => list_template(vec![
                T::Symbol("quote".to_string()),
                inner.to_const_template(),
            ]),
            SyntaxKind::Quasiquote(inner) => list_template(vec![
                T::Symbol("quasiquote".to_string()),
                inner.to_const_template(),
            ]),
            SyntaxKind::Unquote(inner) => list_template(vec![
                T::Symbol("unquote".to_string()),
                inner.to_const_template(),
            ]),
            SyntaxKind::UnquoteSplicing(inner) => list_template(vec![
                T::Symbol("unquote-splicing".to_string()),
                inner.to_const_template(),
            ]),
            SyntaxKind::Splice(inner) => list_template(vec![
                T::Symbol("splice".to_string()),
                inner.to_const_template(),
            ]),
            // A hygiene-bearing macro-template symbol (always a `Symbol`,
            // produced by quasiquote): carry its scope set verbatim into a
            // `SyntaxSymbol` template so it materializes as an ordinary
            // allocation with hygiene intact (docs/impl/region/model.md).
            SyntaxKind::SyntaxLiteral(s) => {
                if let SyntaxKind::Symbol(name) = &s.kind {
                    T::SyntaxSymbol {
                        name: name.to_string(),
                        scopes: s.scopes().to_vec(),
                        span: s.span,
                        scope_exempt: s.scope_exempt,
                    }
                } else {
                    unreachable!(
                        "to_const_template: non-symbol SyntaxLiteral cannot arise (quasiquote only wraps symbols)"
                    )
                }
            }
        }
    }

    /// Convert runtime Value to Syntax, born in `arena`.
    /// Used for analyzing macro results.
    /// A syntax object comes back as a copy in `arena`, with the scopes of the
    /// original Syntax. The passed `span` is ignored in this case; the syntax
    /// object carries its own (more accurate) span.
    pub fn from_value(
        arena: &SyntaxArena,
        value: &Value,
        symbols: &SymbolTable,
        span: Span,
    ) -> Result<Syntax, String> {
        // Syntax objects pass through directly, preserving scopes, because the
        // post-expansion hygiene FLIP (flip_scope_recursive) relies on them
        // arriving intact. Argument-origin nodes carry their use-site scopes
        // plus the pre-stamped intro scope, which the flip removes.
        // Template-origin nodes carry definition-site scopes only, and the flip
        // adds the intro scope. Nodes from `syntax/<-datum` carry their own
        // scope_exempt flag and dodge the flip — do NOT blanket-exempt here:
        // blanket-exempting disables hygiene entirely by shielding every
        // identifier from the intro scope.
        if let Some(embedded) = value.as_syntax() {
            // COPY, do not share: the value's tree lives in the value's own
            // region, and the macro-scope reclaim frees that region as soon as
            // this conversion returns. The copy lands in `arena`, which
            // outlives the expansion.
            let s = embedded.copy_into(arena);
            // `copy_into` copies a syntax literal's node as well, so none
            // could dangle. The expander builds no syntax literal inside a
            // syntax object, and the assertion keeps it that way.
            debug_assert!(
                !contains_syntax_literal(&s),
                "from_value: a syntax object holds a syntax literal"
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
            let name = symbols.name(id).ok_or("Unknown symbol")?;
            SyntaxKind::Symbol(arena.text(name))
        } else if let Some(hash) = value.keyword_hash() {
            let name = crate::value::keyword::resolve_keyword_name(Some(symbols), hash)
                .ok_or_else(|| format!("Unknown keyword {:#x}", hash))?;
            SyntaxKind::Keyword(arena.text(name))
        } else if let Some(s) = value.with_string(|s| arena.text(s)) {
            SyntaxKind::String(s)
        } else if let Some(bytes) = value.string_mut_read() {
            let s = std::str::from_utf8(&bytes)
                .map_err(|_| "Cannot convert non-UTF-8 @string to Syntax")?;
            SyntaxKind::StringMut(arena.text(s))
        } else if value.is_empty_list() {
            SyntaxKind::List(arena.nodes(&[]))
        } else if value.as_pair().is_some() {
            let items = value.list_to_vec().map_err(|e| e.to_string())?;
            let syntaxes: Result<Vec<Syntax>, String> = items
                .iter()
                .map(|v| Syntax::from_value(arena, v, symbols, span))
                .collect();
            SyntaxKind::list(arena.nodes(&syntaxes?))
        } else if let Some(elems) = value.as_array() {
            let syntaxes: Result<Vec<Syntax>, String> = elems
                .iter()
                .map(|v| Syntax::from_value(arena, v, symbols, span))
                .collect();
            SyntaxKind::Array(arena.nodes(&syntaxes?))
        } else if let Some(elems) = value.array_mut_read() {
            let syntaxes: Result<Vec<Syntax>, String> = elems
                .iter()
                .map(|v| Syntax::from_value(arena, v, symbols, span))
                .collect();
            SyntaxKind::ArrayMut(arena.nodes(&syntaxes?))
        } else if let Some(data) = value.as_bytes() {
            let syntaxes: Vec<Syntax> = data
                .iter()
                .map(|b| Syntax::new(SyntaxKind::Int(*b as i64), span))
                .collect();
            SyntaxKind::Bytes(arena.nodes(&syntaxes))
        } else if let Some(bytes) = value.bytes_mut_read() {
            let syntaxes: Vec<Syntax> = bytes
                .iter()
                .map(|b| Syntax::new(SyntaxKind::Int(*b as i64), span))
                .collect();
            SyntaxKind::BytesMut(arena.nodes(&syntaxes))
        } else if let Some(elems) = value.as_set() {
            let syntaxes: Result<Vec<Syntax>, String> = elems
                .iter()
                .map(|v| Syntax::from_value(arena, v, symbols, span))
                .collect();
            SyntaxKind::Set(arena.nodes(&syntaxes?))
        } else if let Some(items) = value.set_mut_read() {
            let syntaxes: Result<Vec<Syntax>, String> = items
                .iter()
                .map(|v| Syntax::from_value(arena, v, symbols, span))
                .collect();
            SyntaxKind::SetMut(arena.nodes(&syntaxes?))
        } else if let Some(struct_ref) = value.as_struct() {
            let mut syntaxes = Vec::with_capacity(struct_ref.len() * 2);
            for (k, v) in struct_ref.iter() {
                syntaxes.push(table_key_to_syntax(arena, k, symbols, &span)?);
                syntaxes.push(Syntax::from_value(arena, v, symbols, span)?);
            }
            SyntaxKind::Struct(arena.nodes(&syntaxes))
        } else if let Some(items) = value.struct_mut_read() {
            let mut syntaxes = Vec::with_capacity(items.len() * 2);
            for (k, v) in items.iter() {
                syntaxes.push(table_key_to_syntax(arena, k, symbols, &span)?);
                syntaxes.push(Syntax::from_value(arena, v, symbols, span)?);
            }
            SyntaxKind::StructMut(arena.nodes(&syntaxes))
        } else {
            return Err(format!("Cannot convert {:?} to Syntax", value));
        };
        Ok(Syntax::new(kind, span))
    }
}
