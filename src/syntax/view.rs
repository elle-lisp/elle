// audited: 2026-11-24
//! One level of a syntax node as data: what `syntax/e` answers, and what `match` reads a node as.
//!
//! docs/macros/syntax.md

use std::collections::{BTreeMap, BTreeSet};

use super::{Syntax, SyntaxKind};
use crate::primitives::ctx::{Alloc, NativeCtx};
use crate::symbol::SymbolTable;
use crate::value::{TableKey, Value};

/// How a view holds a child node: a copy in the ctx's region, or a header
/// over a tree that region already holds.
pub(crate) type WrapChild = fn(&Alloc<'_>, Syntax) -> Value;

impl Syntax {
    /// This node one level deep, as data. An atom answers its value, as
    /// [`to_value`](Self::to_value) does. A compound answers the collection
    /// `read` gives for it, holding each child as `wrap` makes it. A quoting
    /// form or a bytes literal answers the list `(name child…)`. Every heap
    /// value is born in the ctx's region.
    pub(crate) fn view(
        &self,
        symbols: &mut SymbolTable,
        ctx: &mut NativeCtx,
        wrap: WrapChild,
    ) -> Result<Value, String> {
        let wrapped = |items: &[Syntax], ctx: &NativeCtx| -> Vec<Value> {
            items.iter().map(|child| wrap(ctx, *child)).collect()
        };
        Ok(match &self.kind {
            SyntaxKind::List(items) => {
                let children = wrapped(items, ctx);
                ctx.list(children)
            }
            SyntaxKind::Array(items) => ctx.array(wrapped(items, ctx)),
            SyntaxKind::ArrayMut(items) => ctx.array_mut(wrapped(items, ctx)),
            SyntaxKind::Struct(items) => {
                let entries = view_entries(items, symbols, ctx, wrap)?;
                ctx.struct_from(entries)
            }
            SyntaxKind::StructMut(items) => {
                let entries = view_entries(items, symbols, ctx, wrap)?;
                ctx.struct_mut_from(entries)
            }
            SyntaxKind::Set(items) => ctx.set(wrapped(items, ctx).into_iter().collect()),
            SyntaxKind::SetMut(items) => {
                let members: BTreeSet<Value> = wrapped(items, ctx).into_iter().collect();
                ctx.set_mut(members)
            }
            SyntaxKind::Bytes(items) => headed("bytes", items, symbols, ctx, wrap),
            SyntaxKind::BytesMut(items) => headed("@bytes", items, symbols, ctx, wrap),
            SyntaxKind::Quote(inner) => headed("quote", &[**inner], symbols, ctx, wrap),
            SyntaxKind::Quasiquote(inner) => headed("quasiquote", &[**inner], symbols, ctx, wrap),
            SyntaxKind::Unquote(inner) => headed("unquote", &[**inner], symbols, ctx, wrap),
            SyntaxKind::UnquoteSplicing(inner) => {
                headed("unquote-splicing", &[**inner], symbols, ctx, wrap)
            }
            SyntaxKind::Splice(inner) => headed("splice", &[**inner], symbols, ctx, wrap),
            _ => return self.to_value(symbols, ctx),
        })
    }
}

/// The list `(name child…)`, its head a plain symbol.
fn headed(
    name: &str,
    items: &[Syntax],
    symbols: &mut SymbolTable,
    ctx: &NativeCtx,
    wrap: WrapChild,
) -> Value {
    let mut values = vec![Value::symbol(symbols.intern(name))];
    values.extend(items.iter().map(|child| wrap(ctx, *child)));
    ctx.list(values)
}

/// A struct literal's entries: each key as data, each value as `wrap` holds
/// it. A repeated key keeps its last value, as `syntax/->datum` does.
fn view_entries(
    items: &[Syntax],
    symbols: &mut SymbolTable,
    ctx: &mut NativeCtx,
    wrap: WrapChild,
) -> Result<BTreeMap<TableKey, Value>, String> {
    if !items.len().is_multiple_of(2) {
        return Err(format!(
            "a struct literal holds {} forms, which is no set of key-value pairs",
            items.len()
        ));
    }
    let mut entries = BTreeMap::new();
    for pair in items.chunks(2) {
        let key = pair[0].to_value(symbols, ctx)?;
        let key = TableKey::from_value(&key).ok_or_else(|| {
            format!(
                "a struct key must be an immutable value other than a float, got {}",
                key.type_name()
            )
        })?;
        entries.insert(key, wrap(ctx, pair[1]));
    }
    Ok(entries)
}
