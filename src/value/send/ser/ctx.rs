//! Per-call serialization context threaded through `from_value_inner`.
//!
//! The context is split out from the serialization logic itself so the big
//! `match` over heap tags reads as pure "what does each tag serialize to",
//! with all the mutable bookkeeping (intern table, cycle-detection map,
//! sender-side heap table, symbol-name table) collected here.

use super::super::*;
use crate::value::SymbolId;
use rustc_hash::FxHashMap;

/// Per-call serialization context for `SendBundle::from_value`.
pub(in crate::value::send) struct SerContext<'s> {
    /// Intern table being built; read back by callers after serialization.
    pub(in crate::value::send) closures: Vec<SendableClosure>,
    /// Maps `value.payload` (heap pointer address) → intern table index.
    /// Inserted BEFORE recursing into a closure's fields, so back-references find it.
    pub(super) visited: HashMap<u64, usize>,
    /// The SENDER's heap — `send_traits` reads its default-traits table to skip
    /// registry-default traitsets (the receiver rebuilds them). The value being
    /// serialized lives on this heap, so it is the right table to compare against.
    pub(super) heap: &'s crate::value::fiberheap::FiberHeap,
    /// The SENDER's display memo — [`note_symbol`](Self::note_symbol) reads it
    /// to name each symbol the serializer meets. `None` when the caller has no
    /// memo (the receiver then prints those symbols as `#<symbol:hash>`).
    memo: Option<&'s crate::symbol::SymbolTable>,
    /// Names collected for the bundle's name table, deduplicated by id.
    names: FxHashMap<u64, Box<str>>,
}

impl<'s> SerContext<'s> {
    pub(in crate::value::send) fn new(
        heap: &'s crate::value::fiberheap::FiberHeap,
        memo: Option<&'s crate::symbol::SymbolTable>,
    ) -> Self {
        SerContext {
            visited: HashMap::new(),
            closures: Vec::new(),
            heap,
            memo,
            names: FxHashMap::default(),
        }
    }

    /// Record `id`'s name into the bundle's name table, if the sender's memo
    /// knows one. Called at every serializer site that meets a symbol: the
    /// immediate-value arm and the struct-key clones (keys are `TableKey`s,
    /// never routed through `from_value_inner`).
    pub(super) fn note_symbol(&mut self, id: SymbolId) {
        self.note(id.0, self.memo.and_then(|m| m.name(id)));
    }

    /// [`note_symbol`](Self::note_symbol) for a keyword payload — the same
    /// spelling table, resolved through the memo and the static vocabulary.
    pub(super) fn note_keyword(&mut self, hash: u64) {
        self.note(
            hash,
            crate::value::keyword::resolve_keyword_name(self.memo, hash),
        );
    }

    fn note(&mut self, hash: u64, name: Option<&str>) {
        if self.names.contains_key(&hash) {
            return;
        }
        if let Some(name) = name {
            self.names.insert(hash, name.into());
        }
    }

    /// The collected name table, in the bundle's `Vec` form.
    pub(in crate::value::send) fn take_names(&mut self) -> Vec<(u64, Box<str>)> {
        std::mem::take(&mut self.names).into_iter().collect()
    }
}
