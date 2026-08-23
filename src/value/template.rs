//! Compile-time templates for heap literals.
//!
//! A quoted compound literal (`'(a b c)`, `'[1 2]`, nested data) is **not** a
//! pre-baked heap `Value` — a heap literal is an ordinary, reclaimable
//! allocation. Instead the code object carries the
//! literal's immutable *structure* as a [`ConstTemplate`]: plain compile-time
//! data (no heap pointers, fully `Send`-safe for cross-thread LIR transfer). The
//! `MaterializeConst` instruction builds a **fresh** value from the template each
//! time it executes, into the literal's OWN solver-assigned region.
//!
//! The whole structure shares that one region (an immutable aggregate): every
//! node is materialized bottom-up into the same region, so all internal
//! references are self-edges (filtered by `find_object_cross_refs`'s
//! `rid != own_id`) — no spurious cross-region RC, and freeing the one region
//! reclaims the entire structure at its `decref_point` (Rule 4/7).

use crate::hir::region::RuntimeRegion;
use crate::value::Value;

/// The immutable template of a heap literal — a recursive tree of compile-time
/// data mirroring the quoted datum. Leaves that are immediates (`Symbol`,
/// `Int`, …) materialize to immediate `Value`s with no allocation; heap nodes
/// (`String`, `Pair`, `Array`, …) allocate into the literal's region.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ConstTemplate {
    // Immediate leaves — no allocation.
    Nil,
    EmptyList,
    Bool(bool),
    Int(i64),
    Float(f64),
    /// A symbol carried BY NAME, not by id: symbol ids are per-symbol-table
    /// (per-instance), so a raw id would name a different symbol — or nothing —
    /// after the template crosses a `sys/spawn` boundary. The name is re-interned
    /// into the executing instance's table at materialize time (the `symbols`
    /// threaded into [`materialize`](Self::materialize)), exactly as a sent symbol
    /// Value re-interns (`value::send`). (spawn-eval.lisp pins this.)
    Symbol(String),
    Keyword(String),
    // Heap leaves / aggregates — allocated into the literal's region.
    String(String),
    StringMut(String),
    Pair(Box<ConstTemplate>, Box<ConstTemplate>),
    Array(Vec<ConstTemplate>),
    ArrayMut(Vec<ConstTemplate>),
    /// A hygiene-bearing macro-template symbol (`SyntaxKind::SyntaxLiteral`,
    /// always a symbol, produced by quasiquote). Materializes to a fresh
    /// `Value::syntax` wrapping a `Symbol` node carrying its scope set — a fresh
    /// ordinary (reclaimable) allocation per execution. The scope ids are
    /// carried verbatim (they are process-global) so hygiene resolution is
    /// preserved.
    SyntaxSymbol {
        name: String,
        scopes: Vec<u32>,
        span: crate::syntax::Span,
        scope_exempt: bool,
    },
}

impl ConstTemplate {
    /// If this template is a pure immediate (no heap allocation), return the
    /// `Value` it materializes to, interning a symbol's name into `symbols`.
    /// Heap templates return `None` — they must be materialized into a region via
    /// [`materialize`](Self::materialize).
    ///
    /// The lowerer uses this (with the compile-time symbol table) to keep
    /// immediate quotes (`'5`, `'foo`) on the no-region `Quote` fast path, routing
    /// only heap-rooted templates through `MaterializeConst`. This is not just an
    /// optimization: a pure-immediate `MaterializeConst` would allocate nothing,
    /// leaving its solver-assigned region with no RC-raising allocation — a Rule 2
    /// violation whose `DecrefRegion` would underflow.
    pub fn immediate_value(&self, symbols: &mut crate::symbol::SymbolTable) -> Option<Value> {
        match self {
            ConstTemplate::Nil => Some(Value::NIL),
            ConstTemplate::EmptyList => Some(Value::EMPTY_LIST),
            ConstTemplate::Bool(b) => Some(Value::bool(*b)),
            ConstTemplate::Int(n) => Some(Value::int(*n)),
            ConstTemplate::Float(f) => Some(Value::float(*f)),
            ConstTemplate::Symbol(name) => Some(Value::symbol(symbols.intern(name).0)),
            ConstTemplate::Keyword(k) => Some(Value::keyword(k)),
            ConstTemplate::String(_)
            | ConstTemplate::StringMut(_)
            | ConstTemplate::Pair(_, _)
            | ConstTemplate::Array(_)
            | ConstTemplate::ArrayMut(_)
            | ConstTemplate::SyntaxSymbol { .. } => None,
        }
    }

    /// Materialize a FRESH value from this template into `region`. Heap nodes are
    /// built bottom-up (children first) so each parent's contents already live in
    /// `region` when its allocation is scanned — every internal reference is a
    /// self-edge, taking no cross-region RC. Mirrors the immutable constructors
    /// in `value/repr/constructors.rs`, into the explicit resolved region.
    ///
    /// A quoted `Symbol` leaf re-interns its name into `symbols` (the executing
    /// instance's table, threaded explicitly); the id is then valid HERE even
    /// across a `sys/spawn` boundary. `symbols` must be `Some` whenever the
    /// template contains a `Symbol` leaf (the only arm that needs it); a `None`
    /// there panics.
    pub fn materialize(
        &self,
        heap: &mut crate::value::fiberheap::FiberHeap,
        region: RuntimeRegion,
        mut symbols: Option<&mut crate::symbol::SymbolTable>,
    ) -> Value {
        use crate::primitives::traitregistry::default_traits_for;
        use crate::value::arena::{alloc_in_region, alloc_region_slice_in_region};
        use crate::value::heap::{HeapObject, HeapTag, Pair};
        use std::cell::RefCell;
        use std::rc::Rc;

        match self {
            // Immediates — no allocation.
            ConstTemplate::Nil => Value::NIL,
            ConstTemplate::EmptyList => Value::EMPTY_LIST,
            ConstTemplate::Bool(b) => Value::bool(*b),
            ConstTemplate::Int(n) => Value::int(*n),
            ConstTemplate::Float(f) => Value::float(*f),
            ConstTemplate::Keyword(k) => Value::keyword(k),
            // Re-intern the symbol's name into the executing instance's table so
            // the id is valid HERE — across a `sys/spawn` boundary the sender's id
            // would name a different symbol (spawn-eval.lisp).
            ConstTemplate::Symbol(name) => {
                let symbols = symbols.expect("materialize: no symbol table for a quoted symbol");
                Value::symbol(symbols.intern(name).0)
            }

            ConstTemplate::String(s) => {
                let slice = alloc_region_slice_in_region::<u8>(heap, s.as_bytes(), region);
                let traits = default_traits_for(heap, HeapTag::LString);
                alloc_in_region(heap, HeapObject::LString { s: slice, traits }, region)
            }
            ConstTemplate::StringMut(s) => {
                let data = Rc::new(RefCell::new(s.as_bytes().to_vec()));
                let traits = default_traits_for(heap, HeapTag::LStringMut);
                alloc_in_region(heap, HeapObject::LStringMut { data, traits }, region)
            }
            ConstTemplate::Pair(car, cdr) => {
                let first = car.materialize(heap, region, symbols.as_deref_mut());
                let rest = cdr.materialize(heap, region, symbols);
                let traits = default_traits_for(heap, HeapTag::Pair);
                alloc_in_region(
                    heap,
                    HeapObject::Pair(Pair {
                        first,
                        rest,
                        traits,
                    }),
                    region,
                )
            }
            ConstTemplate::Array(elems) => {
                let vals: Vec<Value> = elems
                    .iter()
                    .map(|e| e.materialize(heap, region, symbols.as_deref_mut()))
                    .collect();
                let slice = alloc_region_slice_in_region::<Value>(heap, &vals, region);
                let traits = default_traits_for(heap, HeapTag::LArray);
                alloc_in_region(
                    heap,
                    HeapObject::LArray {
                        elements: slice,
                        traits,
                    },
                    region,
                )
            }
            ConstTemplate::ArrayMut(elems) => {
                let vals: Vec<Value> = elems
                    .iter()
                    .map(|e| e.materialize(heap, region, symbols.as_deref_mut()))
                    .collect();
                let traits = default_traits_for(heap, HeapTag::LArrayMut);
                alloc_in_region(
                    heap,
                    HeapObject::LArrayMut {
                        data: Rc::new(RefCell::new(vals)),
                        traits,
                    },
                    region,
                )
            }
            ConstTemplate::SyntaxSymbol {
                name,
                scopes,
                span,
                scope_exempt,
            } => {
                use crate::syntax::{ScopeId, Syntax, SyntaxKind};
                let mut syn = Syntax::with_scopes(
                    SyntaxKind::Symbol(name.clone()),
                    span.clone(),
                    scopes.iter().map(|&s| ScopeId(s)).collect(),
                );
                syn.scope_exempt = *scope_exempt;
                alloc_in_region(
                    heap,
                    HeapObject::Syntax {
                        syntax: Box::new(syn),
                        traits: Value::NIL,
                    },
                    region,
                )
            }
        }
    }

    /// Serialize this template into the (reclaimable) bytecode instruction
    /// stream, preorder. The template lives inline in the bytecode — plain
    /// compile-time data, never a pinned pool `Value` — and is decoded and
    /// materialized fresh on each execution by `decode`/`materialize`. The
    /// encoding is self-delimiting (each node's size is implied by its tag and
    /// payload), so `decode` needs no separate length table.
    pub fn encode(&self, out: &mut Vec<u8>) {
        fn put_str(out: &mut Vec<u8>, s: &str) {
            out.extend_from_slice(&(s.len() as u32).to_be_bytes());
            out.extend_from_slice(s.as_bytes());
        }
        fn put_seq(out: &mut Vec<u8>, tag: u8, elems: &[ConstTemplate]) {
            out.push(tag);
            out.extend_from_slice(&(elems.len() as u32).to_be_bytes());
            for e in elems {
                e.encode(out);
            }
        }
        match self {
            ConstTemplate::Nil => out.push(TAG_NIL),
            ConstTemplate::EmptyList => out.push(TAG_EMPTY_LIST),
            ConstTemplate::Bool(b) => {
                out.push(TAG_BOOL);
                out.push(*b as u8);
            }
            ConstTemplate::Int(n) => {
                out.push(TAG_INT);
                out.extend_from_slice(&n.to_be_bytes());
            }
            ConstTemplate::Float(f) => {
                out.push(TAG_FLOAT);
                out.extend_from_slice(&f.to_bits().to_be_bytes());
            }
            ConstTemplate::Symbol(name) => {
                out.push(TAG_SYMBOL);
                put_str(out, name);
            }
            ConstTemplate::Keyword(k) => {
                out.push(TAG_KEYWORD);
                put_str(out, k);
            }
            ConstTemplate::String(s) => {
                out.push(TAG_STRING);
                put_str(out, s);
            }
            ConstTemplate::StringMut(s) => {
                out.push(TAG_STRING_MUT);
                put_str(out, s);
            }
            ConstTemplate::Pair(car, cdr) => {
                out.push(TAG_PAIR);
                car.encode(out);
                cdr.encode(out);
            }
            ConstTemplate::Array(elems) => put_seq(out, TAG_ARRAY, elems),
            ConstTemplate::ArrayMut(elems) => put_seq(out, TAG_ARRAY_MUT, elems),
            ConstTemplate::SyntaxSymbol {
                name,
                scopes,
                span,
                scope_exempt,
            } => {
                out.push(TAG_SYNTAX_SYMBOL);
                put_str(out, name);
                out.extend_from_slice(&(scopes.len() as u32).to_be_bytes());
                for &s in scopes {
                    out.extend_from_slice(&s.to_be_bytes());
                }
                out.extend_from_slice(&(span.start as u64).to_be_bytes());
                out.extend_from_slice(&(span.end as u64).to_be_bytes());
                out.extend_from_slice(&span.line.to_be_bytes());
                out.extend_from_slice(&span.col.to_be_bytes());
                match &span.file {
                    Some(f) => {
                        out.push(1);
                        put_str(out, f);
                    }
                    None => out.push(0),
                }
                out.push(*scope_exempt as u8);
            }
        }
    }

    /// Decode a template previously written by [`encode`](Self::encode),
    /// advancing `ip` past it. The bytes originated from this compiler, so the
    /// stream is well-formed; a malformed stream is a compiler bug, hence the
    /// panics rather than error propagation.
    pub fn decode(bytes: &[u8], ip: &mut usize) -> ConstTemplate {
        fn take<const N: usize>(bytes: &[u8], ip: &mut usize) -> [u8; N] {
            let mut buf = [0u8; N];
            buf.copy_from_slice(&bytes[*ip..*ip + N]);
            *ip += N;
            buf
        }
        fn get_str(bytes: &[u8], ip: &mut usize) -> String {
            let len = u32::from_be_bytes(take::<4>(bytes, ip)) as usize;
            let s = std::str::from_utf8(&bytes[*ip..*ip + len])
                .expect("ConstTemplate::decode: invalid UTF-8 in template")
                .to_string();
            *ip += len;
            s
        }
        fn get_seq(bytes: &[u8], ip: &mut usize) -> Vec<ConstTemplate> {
            let count = u32::from_be_bytes(take::<4>(bytes, ip)) as usize;
            (0..count)
                .map(|_| ConstTemplate::decode(bytes, ip))
                .collect()
        }
        let tag = bytes[*ip];
        *ip += 1;
        match tag {
            TAG_NIL => ConstTemplate::Nil,
            TAG_EMPTY_LIST => ConstTemplate::EmptyList,
            TAG_BOOL => {
                let b = bytes[*ip] != 0;
                *ip += 1;
                ConstTemplate::Bool(b)
            }
            TAG_INT => ConstTemplate::Int(i64::from_be_bytes(take::<8>(bytes, ip))),
            TAG_FLOAT => {
                ConstTemplate::Float(f64::from_bits(u64::from_be_bytes(take::<8>(bytes, ip))))
            }
            TAG_SYMBOL => ConstTemplate::Symbol(get_str(bytes, ip)),
            TAG_KEYWORD => ConstTemplate::Keyword(get_str(bytes, ip)),
            TAG_STRING => ConstTemplate::String(get_str(bytes, ip)),
            TAG_STRING_MUT => ConstTemplate::StringMut(get_str(bytes, ip)),
            TAG_PAIR => {
                let car = ConstTemplate::decode(bytes, ip);
                let cdr = ConstTemplate::decode(bytes, ip);
                ConstTemplate::Pair(Box::new(car), Box::new(cdr))
            }
            TAG_ARRAY => ConstTemplate::Array(get_seq(bytes, ip)),
            TAG_ARRAY_MUT => ConstTemplate::ArrayMut(get_seq(bytes, ip)),
            TAG_SYNTAX_SYMBOL => {
                let name = get_str(bytes, ip);
                let n = u32::from_be_bytes(take::<4>(bytes, ip)) as usize;
                let scopes: Vec<u32> = (0..n)
                    .map(|_| u32::from_be_bytes(take::<4>(bytes, ip)))
                    .collect();
                let start = u64::from_be_bytes(take::<8>(bytes, ip)) as usize;
                let end = u64::from_be_bytes(take::<8>(bytes, ip)) as usize;
                let line = u32::from_be_bytes(take::<4>(bytes, ip));
                let col = u32::from_be_bytes(take::<4>(bytes, ip));
                let file = if bytes[*ip] != 0 {
                    *ip += 1;
                    Some(get_str(bytes, ip))
                } else {
                    *ip += 1;
                    None
                };
                let scope_exempt = bytes[*ip] != 0;
                *ip += 1;
                let mut span = crate::syntax::Span::new(start, end, line, col);
                span.file = file;
                ConstTemplate::SyntaxSymbol {
                    name,
                    scopes,
                    span,
                    scope_exempt,
                }
            }
            other => panic!("ConstTemplate::decode: unknown template tag {other}"),
        }
    }
}

// Template node tags for the inline bytecode encoding.
const TAG_NIL: u8 = 0;
const TAG_EMPTY_LIST: u8 = 1;
const TAG_BOOL: u8 = 2;
const TAG_INT: u8 = 3;
const TAG_FLOAT: u8 = 4;
const TAG_SYMBOL: u8 = 5;
const TAG_KEYWORD: u8 = 6;
const TAG_STRING: u8 = 7;
const TAG_STRING_MUT: u8 = 8;
const TAG_PAIR: u8 = 9;
const TAG_ARRAY: u8 = 10;
const TAG_ARRAY_MUT: u8 = 11;
const TAG_SYNTAX_SYMBOL: u8 = 12;

#[cfg(test)]
mod tests;
