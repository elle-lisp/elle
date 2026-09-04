//! Conversion between Syntax and Value
//!
//! These conversions are needed for:
//! - Runtime quote (Syntax → Value)
//! - Macro results that return runtime Values (Value → Syntax)

use super::{Span, Syntax, SyntaxArena, SyntaxKind};
use crate::primitives::ctx::NativeCtx;
use crate::symbol::SymbolTable;
use crate::value::{TableKey, Value};

/// Check if a Syntax tree contains any SyntaxLiteral nodes. Used as a debug
/// assertion in `from_value` to catch arena pointer escapes — a cloned Syntax
/// must not carry a heap-pointer `SyntaxLiteral` Value that would dangle once its
/// arena region frees.
pub(crate) fn contains_syntax_literal(s: &Syntax) -> bool {
    match &s.kind {
        SyntaxKind::SyntaxLiteral(_) => true,
        other => other.children().iter().any(contains_syntax_literal),
    }
}

/// Record every keyword spelling in `syntax` into the instance memo — the
/// keyword analogue of interning identifiers. Called by the expander on each
/// tree it enters (macro arguments and quasiquote templates round-trip
/// keywords as bare hashes, so their spellings must be learned before the
/// round-trip) and by the analyzer as a pre-pass
/// (docs/impl/symbol.md § "The display memo"). Idempotent; the memo dedups.
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
        TableKey::Heap(_) => {
            return Err("Cannot convert heap key to Syntax".to_string());
        }
    };
    Ok(Syntax::new(kind, *span))
}

impl Syntax {
    /// Convert Syntax to a runtime Value as an **ordinary allocation** into the
    /// ctx's own region (reclaimed by RC). Used by the read-time primitives `read`
    /// / `read-all` / `syntax->datum`, whose native call mints a fresh region for
    /// the result (region/model.md, "Constants lower as ordinary allocations").
    ///
    /// `ctx` is the allocation capability — the read-time primitives pass their
    /// call's ctx (docs/impl/region/ctx.md). The whole tree lands in the ctx's
    /// region, reclaimed at the caller's `decref_point`.
    pub fn to_value(&self, symbols: &mut SymbolTable, ctx: &mut NativeCtx) -> Value {
        self.to_value_in(symbols, ctx)
    }

    /// Shared recursive materializer threading the `ctx` capability through every
    /// heap leaf. Immediates (nil/bool/int/float/symbol/keyword) need no region;
    /// every heap leaf (`String`, the list spine, `Array`, the `Rc`-backed mutable
    /// aggregates, syntax) is born in the ctx's region via the `ctx.*`
    /// constructors.
    fn to_value_in(self, symbols: &mut SymbolTable, ctx: &mut NativeCtx) -> Value {
        match &self.kind {
            SyntaxKind::Nil => Value::NIL,
            SyntaxKind::Bool(b) => Value::bool(*b),
            SyntaxKind::Int(n) => Value::int(*n),
            SyntaxKind::Float(n) => Value::float(*n),
            SyntaxKind::Symbol(s) => {
                let id = symbols.intern(s);
                Value::symbol(id)
            }
            SyntaxKind::Keyword(s) => {
                // A learning site: read-time keyword data (`read`, `read-all`,
                // `syntax->datum`) records its spelling like the analyzer does
                // for compiled source.
                symbols.keyword(s);
                Value::keyword(s)
            }
            SyntaxKind::String(s) => ctx.string(s),
            SyntaxKind::StringMut(s) => ctx.string_mut(s.as_bytes().to_vec()),
            SyntaxKind::List(items) => {
                let values: Vec<Value> = items
                    .iter()
                    .map(|item| item.to_value_in(symbols, ctx))
                    .collect();
                ctx.list(values)
            }
            SyntaxKind::Array(items) => {
                let values = items
                    .iter()
                    .map(|item| item.to_value_in(symbols, ctx))
                    .collect();
                ctx.array(values)
            }
            SyntaxKind::ArrayMut(items) => {
                let values = items
                    .iter()
                    .map(|item| item.to_value_in(symbols, ctx))
                    .collect();
                ctx.array_mut(values)
            }
            SyntaxKind::Bytes(items) => {
                // Convert to (bytes e1 e2 ...) list
                let bytes_sym = symbols.intern("bytes");
                let mut values = vec![Value::symbol(bytes_sym)];
                values.extend(items.iter().map(|item| item.to_value_in(symbols, ctx)));
                ctx.list(values)
            }
            SyntaxKind::BytesMut(items) => {
                // Convert to (@bytes e1 e2 ...) list
                let bytes_mut_sym = symbols.intern("@bytes");
                let mut values = vec![Value::symbol(bytes_mut_sym)];
                values.extend(items.iter().map(|item| item.to_value_in(symbols, ctx)));
                ctx.list(values)
            }
            SyntaxKind::Struct(items) => {
                // Convert to (struct k1 v1 k2 v2 ...) list
                let struct_sym = symbols.intern("struct");
                let mut values = vec![Value::symbol(struct_sym)];
                values.extend(items.iter().map(|item| item.to_value_in(symbols, ctx)));
                ctx.list(values)
            }
            SyntaxKind::StructMut(items) => {
                // Convert to (@struct k1 v1 k2 v2 ...) list
                let struct_mut_sym = symbols.intern("@struct");
                let mut values = vec![Value::symbol(struct_mut_sym)];
                values.extend(items.iter().map(|item| item.to_value_in(symbols, ctx)));
                ctx.list(values)
            }
            SyntaxKind::Set(items) => {
                // Convert to (set e1 e2 ...) list
                let set_sym = symbols.intern("set");
                let mut values = vec![Value::symbol(set_sym)];
                values.extend(items.iter().map(|item| item.to_value_in(symbols, ctx)));
                ctx.list(values)
            }
            SyntaxKind::SetMut(items) => {
                // Convert to (@set e1 e2 ...) list
                let set_mut_sym = symbols.intern("@set");
                let mut values = vec![Value::symbol(set_mut_sym)];
                values.extend(items.iter().map(|item| item.to_value_in(symbols, ctx)));
                ctx.list(values)
            }
            SyntaxKind::Quote(inner) => {
                let quote_sym = symbols.intern("quote");
                let inner_val = inner.to_value_in(symbols, ctx);
                ctx.list(vec![Value::symbol(quote_sym), inner_val])
            }
            SyntaxKind::Quasiquote(inner) => {
                let sym = symbols.intern("quasiquote");
                let inner_val = inner.to_value_in(symbols, ctx);
                ctx.list(vec![Value::symbol(sym), inner_val])
            }
            SyntaxKind::Unquote(inner) => {
                let sym = symbols.intern("unquote");
                let inner_val = inner.to_value_in(symbols, ctx);
                ctx.list(vec![Value::symbol(sym), inner_val])
            }
            SyntaxKind::UnquoteSplicing(inner) => {
                let sym = symbols.intern("unquote-splicing");
                let inner_val = inner.to_value_in(symbols, ctx);
                ctx.list(vec![Value::symbol(sym), inner_val])
            }
            SyntaxKind::Splice(inner) => {
                let sym = symbols.intern("splice");
                let inner_val = inner.to_value_in(symbols, ctx);
                ctx.list(vec![Value::symbol(sym), inner_val])
            }
            // A hygiene-bearing template symbol. Materialize a fresh syntax object
            // (ordinary allocation into the ctx's region) wrapping the carried
            // `Syntax` — processed by from_value() after VM evaluation.
            SyntaxKind::SyntaxLiteral(s) => ctx.syntax(**s),
        }
    }

    /// Convert Syntax to a `ConstTemplate` — the allocation-free compile-time
    /// form of [`to_value`](Self::to_value): plain recursive data that
    /// `MaterializeConst` materializes fresh into a reclaimable region each
    /// execution (region/model.md, "Constants lower as ordinary allocations").
    /// The desugaring matches `to_value` — a quoted `{:a 1}` becomes the list
    /// template `(struct :a 1)`, etc. — so the quoted datum's value is unchanged.
    ///
    /// A hygiene-bearing `SyntaxLiteral` (always a symbol) becomes a
    /// `ConstTemplate::SyntaxSymbol` carrying its scope set verbatim, so it too
    /// materializes ordinarily with hygiene intact.
    ///
    /// Symbols are carried BY NAME (not interned to a per-table id) so the
    /// template is portable across a `sys/spawn` boundary; they re-intern into
    /// the executing table at materialize time. So this needs no `SymbolTable`.
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
            // allocation with hygiene intact (region/model.md, "Constants lower
            // as ordinary allocations").
            SyntaxKind::SyntaxLiteral(s) => {
                if let SyntaxKind::Symbol(name) = &s.kind {
                    T::SyntaxSymbol {
                        name: name.to_string(),
                        scopes: s.scopes().iter().map(|sc| sc.0).collect(),
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

    /// Convert runtime Value to Syntax
    /// Used for analyzing macro results.
    /// When encountering a syntax object, returns it directly — preserving
    /// scopes from the original Syntax. The passed `span` is ignored in
    /// this case; the syntax object carries its own (more accurate) span.
    pub fn from_value(
        arena: &SyntaxArena,
        value: &Value,
        symbols: &SymbolTable,
        span: Span,
    ) -> Result<Syntax, String> {
        // Syntax objects pass through directly, preserving scopes — the
        // post-expansion hygiene FLIP (flip_scope_recursive) relies on
        // them arriving intact: argument-origin nodes carry their use-site
        // scopes plus the pre-stamped intro scope (which the flip removes),
        // template-origin nodes carry definition-site scopes only (the flip
        // adds the intro scope). Nodes from `datum->syntax` carry their own
        // scope_exempt flag and dodge the flip — do NOT blanket-exempt here:
        // blanket-exempting disables hygiene entirely by shielding every
        // identifier from the intro scope.
        if let Some(embedded) = value.as_syntax() {
            // COPY, do not share: the value's tree lives in the value's own
            // region, and the macro-scope reclaim frees that region as soon as
            // this conversion returns. The copy lands in `arena`, which
            // outlives the expansion.
            let s = embedded.copy_into(arena);
            // Safety check: the copied Syntax must not contain SyntaxLiteral
            // children. SyntaxLiteral holds a heap-pointer Value that may be
            // arena-allocated; if it survives into the result Syntax, it will
            // dangle after arena release. Current code paths don't produce
            // nested SyntaxLiterals, but this assertion catches future regressions.
            debug_assert!(
                !contains_syntax_literal(&s),
                "from_value: copied Syntax contains SyntaxLiteral (arena pointer would escape)"
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
        } else if let Some(data) = value.as_string_mut() {
            let bytes = data.borrow();
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
            SyntaxKind::List(arena.nodes(&syntaxes?))
        } else if let Some(elems) = value.as_array() {
            let syntaxes: Result<Vec<Syntax>, String> = elems
                .iter()
                .map(|v| Syntax::from_value(arena, v, symbols, span))
                .collect();
            SyntaxKind::Array(arena.nodes(&syntaxes?))
        } else if let Some(vec_ref) = value.as_array_mut() {
            let items = vec_ref.borrow().clone();
            let syntaxes: Result<Vec<Syntax>, String> = items
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
        } else if let Some(data) = value.as_bytes_mut() {
            let bytes = data.borrow();
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
        } else if let Some(set_ref) = value.as_set_mut() {
            let items = set_ref.borrow();
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
        } else if let Some(table_ref) = value.as_struct_mut() {
            let items = table_ref.borrow();
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

// Tests migrated to tests/elle/syntax-roundtrip.lisp
