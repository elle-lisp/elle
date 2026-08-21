//! Per-call serialization context threaded through `from_value_inner`.
//!
//! The context is split out from the serialization logic itself so the big
//! `match` over heap tags reads as pure "what does each tag serialize to",
//! with all the mutable bookkeeping (intern table, cycle-detection map,
//! sender-side symbol/heap tables) collected here.

use super::super::*;

/// Per-call serialization context for `SendBundle::from_value`.
pub(in crate::value::send) struct SerContext<'s> {
    /// Intern table being built; read back by callers after serialization.
    pub(in crate::value::send) closures: Vec<SendableClosure>,
    /// Maps `value.payload` (heap pointer address) → intern table index.
    /// Inserted BEFORE recursing into a closure's fields, so back-references find it.
    pub(super) visited: HashMap<u64, usize>,
    /// The SENDER's symbol table — used to resolve a symbol value's id to its name
    /// so it crosses the thread boundary by name (ids are per-table). Threaded
    /// explicitly (docs/impl/region/ctx.md § "Symbols through the ctx").
    pub(in crate::value::send) symbols: &'s crate::symbol::SymbolTable,
    /// The SENDER's heap — `send_traits` reads its default-traits table to skip
    /// registry-default traitsets (the receiver rebuilds them). The value being
    /// serialized lives on this heap, so it is the right table to compare against.
    pub(super) heap: &'s crate::value::fiberheap::FiberHeap,
}

impl<'s> SerContext<'s> {
    pub(in crate::value::send) fn new(
        heap: &'s crate::value::fiberheap::FiberHeap,
        symbols: &'s crate::symbol::SymbolTable,
    ) -> Self {
        SerContext {
            visited: HashMap::new(),
            closures: Vec::new(),
            symbols,
            heap,
        }
    }
}
