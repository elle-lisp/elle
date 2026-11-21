// audited: 2026-11-21
//! `compile/lir`'s answer: each function of a frozen unit as Elle data, in the data form.
//!
//! docs/spec/form.md

use crate::hir::arena::TEMP_NAMES;
use crate::lir::code::form::{self, Field, Form};
use crate::lir::code::BlockRef;
use crate::lir::{ConstRef, FrozenModule, LirOwned};
use crate::primitives::ctx::Alloc;
use crate::signals::registry::with_registry;
use crate::symbol::SymbolTable;
use crate::syntax::Span;
use crate::value::heap::TableKey;
use crate::value::{Arity, ConstTemplate, SymbolId, Value};
use std::collections::BTreeMap;

/// Each function of `module` as a struct: the entry function, then each
/// closure in the order of its closure id. Every keyword and symbol made here
/// has its spelling recorded in `symbols`, so each one prints.
pub fn unit(ctx: &Alloc, symbols: &mut SymbolTable, module: &FrozenModule) -> Value {
    // A temp's name is in no source text, so nothing else teaches the
    // instance its spelling.
    for name in TEMP_NAMES {
        symbols.intern(name);
    }
    let mut data = Data { ctx, symbols };
    let functions = std::iter::once(&module.entry)
        .chain(&module.closures)
        .map(|f| data.function(f))
        .collect();
    ctx.array(functions)
}

/// The ctx the values are born in, and the table that spells them.
struct Data<'c, 'h> {
    ctx: &'c Alloc<'h>,
    symbols: &'c mut SymbolTable,
}

impl Data<'_, '_> {
    fn keyword(&mut self, name: &str) -> Value {
        Value::keyword_from_hash(self.symbols.keyword(name))
    }

    fn record(&mut self, entries: Vec<(&str, Value)>) -> Value {
        let mut fields = BTreeMap::new();
        for (key, value) in entries {
            self.symbols.keyword(key);
            fields.insert(TableKey::keyword(key), value);
        }
        self.ctx.struct_from(fields)
    }

    fn ints(&self, ns: impl IntoIterator<Item = i64>) -> Value {
        self.ctx.array(ns.into_iter().map(Value::int).collect())
    }

    /// The binding named `name`: its symbol, or `:synthetic` for a binding
    /// a compiler pass made with no name.
    fn binding(&mut self, name: SymbolId) -> Value {
        if name == SymbolId::SYNTHETIC {
            self.keyword("synthetic")
        } else {
            Value::symbol(name)
        }
    }

    fn function(&mut self, f: &LirOwned) -> Value {
        let (view, code) = (f.view(), f.code());
        let id = view
            .closure_id()
            .map_or(Value::NIL, |c| Value::int(c.0.into()));
        let name = view
            .name()
            .map_or(Value::NIL, |n| Value::symbol(self.symbols.intern(n)));
        let (min, max) = match view.arity() {
            Arity::Exact(n) => (n, Some(n)),
            Arity::AtLeast(n) => (n, None),
            Arity::Range(min, max) => (min, Some(max)),
        };
        let max = max.map_or(Value::NIL, |m| Value::int(m as i64));
        let arity = self.ctx.array(vec![Value::int(min as i64), max]);
        let captures = code.captures.iter().map(|c| self.binding(c.name()));
        let captures: Vec<Value> = captures.collect();
        let captures = self.ctx.array(captures);
        // A function the lowerer did not build, such as a test's fixture,
        // names no slot.
        let slots: Vec<Value> = (0..view.num_locals() as usize)
            .map(|i| {
                let name = code.slot_names.get(i).copied().flatten();
                name.map_or(Value::NIL, |name| self.binding(name))
            })
            .collect();
        let slots = self.ctx.array(slots);
        let blocks = view.blocks().map(|b| self.block(b)).collect();
        let blocks = self.ctx.array(blocks);
        self.record(vec![
            ("id", id),
            ("name", name),
            ("arity", arity),
            ("params", Value::int(view.num_params() as i64)),
            ("captures", captures),
            ("slots", slots),
            ("entry", Value::int(view.entry().0.into())),
            ("blocks", blocks),
        ])
    }

    fn block(&mut self, b: BlockRef<'_>) -> Value {
        let instrs = b
            .nodes()
            .map(|n| self.form(form::instr(&n.instr()), Some(n.span())))
            .collect();
        let instrs = self.ctx.array(instrs);
        let term = self.form(form::terminator(&b.terminator()), None);
        self.record(vec![
            ("label", Value::int(b.label().0.into())),
            ("instrs", instrs),
            ("term", term),
        ])
    }

    /// `form` as a struct. An instruction's struct also holds `:at`, from
    /// `span`; a terminator's does not.
    fn form(&mut self, form: Form<'_>, span: Option<Span>) -> Value {
        let mut entries = vec![("kind", self.keyword(&form.kind))];
        for (key, field) in form.fields {
            entries.push((key, self.field(field)));
        }
        if let Some(span) = span {
            // A synthetic span has no line.
            let at = (span.line != 0).then(|| self.ints([span.line.into(), span.col.into()]));
            entries.push(("at", at.unwrap_or(Value::NIL)));
        }
        self.record(entries)
    }

    fn field(&mut self, field: Field<'_>) -> Value {
        match field {
            Field::Int(n) => Value::int(n),
            Field::Ints(ns) => self.ints(ns),
            Field::Absent => Value::NIL,
            Field::Const(c) => constant(c),
            Field::Consts(cs) => self.ctx.array(cs.into_iter().map(constant).collect()),
            Field::Value(v) => v,
            Field::Template(t) => self.template(&t.decode()),
            Field::Offers(offers) => {
                let pairs = offers.into_iter().map(|(p, s)| self.ints([p, s]));
                self.ctx.array(pairs.collect())
            }
            Field::Keyword(name) => self.keyword(name),
            Field::Signal(bits) => {
                let symbols = &mut *self.symbols;
                let names = with_registry(|r| r.bits_to_keywords(bits, Some(symbols)));
                self.ctx.set(names.into_iter().collect())
            }
            // The one symbol a field holds is a capture cell's binding.
            Field::Symbol(s) => self.binding(s),
            Field::Bool(b) => Value::bool(b),
        }
    }

    /// The literal a template stands for, built through the ctx. A hygienic
    /// symbol is its plain symbol.
    fn template(&mut self, t: &ConstTemplate) -> Value {
        match t {
            ConstTemplate::Nil => Value::NIL,
            ConstTemplate::EmptyList => Value::EMPTY_LIST,
            ConstTemplate::Bool(b) => Value::bool(*b),
            ConstTemplate::Int(n) => Value::int(*n),
            ConstTemplate::Float(x) => Value::float(*x),
            ConstTemplate::Symbol(name) | ConstTemplate::SyntaxSymbol { name, .. } => {
                Value::symbol(self.symbols.intern(name))
            }
            ConstTemplate::Keyword(name) => self.keyword(name),
            ConstTemplate::String(s) => self.ctx.string(s),
            ConstTemplate::StringMut(s) => self.ctx.string_mut(s.as_bytes().to_vec()),
            ConstTemplate::Pair(head, tail) => {
                let (head, tail) = (self.template(head), self.template(tail));
                self.ctx.pair(head, tail)
            }
            ConstTemplate::Array(items) => {
                let items = items.iter().map(|i| self.template(i)).collect();
                self.ctx.array(items)
            }
            ConstTemplate::ArrayMut(items) => {
                let items = items.iter().map(|i| self.template(i)).collect();
                self.ctx.array_mut(items)
            }
        }
    }
}

fn constant(c: ConstRef) -> Value {
    match c {
        ConstRef::Nil => Value::NIL,
        ConstRef::EmptyList => Value::EMPTY_LIST,
        ConstRef::Bool(b) => Value::bool(b),
        ConstRef::Int(n) => Value::int(n),
        ConstRef::Float(x) => Value::float(x),
        ConstRef::Symbol(s) => Value::symbol(s),
        ConstRef::Keyword(hash) => Value::keyword_from_hash(hash),
    }
}
