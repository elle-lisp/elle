//! Symbol identity and the per-instance name memo.
//!
//! A `SymbolId` is the name's hash ([`crate::namehash`]), so identity needs no
//! table: `SymbolId::of(n)` answers it anywhere, for free. What does need a
//! table is *display*, because a hash cannot be printed as a name. This module
//! is that table — one per instance, and the only thing a symbol table is for.
//!
//! See [docs/impl/symbol.md](../../docs/impl/symbol.md) for the model and what
//! it buys.

use crate::value::SymbolId;
use rustc_hash::FxHashMap;

/// One instance's hash → spelling memo, for display.
///
/// One map serves both vocabularies: a symbol and a keyword with the same
/// spelling share one entry, because the map's domain is spellings and the tag
/// that separates `map` from `:map` lives in the `Value`. Not shared, so it
/// takes no lock, and it is dropped with the instance that owns it — a name
/// lives exactly as long as the run that learned it. That is why
/// [`name`](Self::name) hands out a borrow tied to `&self` rather than a
/// `&'static str`.
///
/// A memo answers only for names its own instance met, at the learning sites
/// docs/impl/symbol.md § "The display memo" enumerates. A value whose name was
/// never recorded is still a valid value and still compares correctly; it
/// simply has no spelling to print.
///
/// `FxHashMap`, not the default hasher: the reader interns once per identifier
/// token, so this is on the compile path, and the key is already a
/// well-distributed hash — SipHash would buy nothing over it.
#[derive(Debug, Default)]
pub struct SymbolTable {
    names: FxHashMap<u64, Box<str>>,
}

impl SymbolTable {
    pub fn new() -> Self {
        SymbolTable {
            names: FxHashMap::default(),
        }
    }

    /// Intern `name`: return its id and record the name for display.
    ///
    /// Every symbol that reaches a `Value` through this instance comes here, so
    /// every symbol the instance can print has a recoverable name.
    pub fn intern(&mut self, name: &str) -> SymbolId {
        let id = SymbolId::of(name);
        assert!(
            id != SymbolId::SYNTHETIC,
            "symbol {:?} hashes onto the reserved SYNTHETIC id",
            name
        );
        self.record(id, name);
        id
    }

    /// The keyword entry point onto the same map: record `name` and return its
    /// hash — the payload of the keyword value it names. Keyed by raw hash
    /// because a keyword payload is not a `SymbolId`.
    pub fn keyword(&mut self, name: &str) -> u64 {
        let hash = crate::value::keyword::keyword_hash(name);
        self.record_raw(hash, name);
        hash
    }

    /// Record `name` under `id`, panicking if a different name is already there.
    ///
    /// Split out from [`intern`](Self::intern) so a name that arrives already
    /// hashed — from a dump's name table, or beside a symbol that crossed a
    /// thread — is checked by the same code that checks a freshly read
    /// identifier.
    pub(crate) fn record(&mut self, id: SymbolId, name: &str) {
        self.record_raw(id.0, name);
    }

    /// A spelling arriving already hashed from either vocabulary — a bundle's
    /// name table, an image's name table.
    pub(crate) fn record_spelling(&mut self, hash: u64, name: &str) {
        self.record_raw(hash, name);
    }

    /// The one collision guard, shared by both vocabularies: recording a hash
    /// the memo already maps to a different spelling panics.
    fn record_raw(&mut self, hash: u64, name: &str) {
        match self.names.get(&hash) {
            Some(existing) => assert!(
                &**existing == name,
                "name hash collision: {:?} and {:?} both hash to {:#x}",
                existing,
                name,
                hash
            ),
            None => {
                self.names.insert(hash, name.into());
            }
        }
    }

    /// The name recorded for `id`, or `None` if this instance never met it.
    pub fn name(&self, id: SymbolId) -> Option<&str> {
        self.names.get(&id.0).map(|n| &**n)
    }

    /// The spelling recorded for a keyword payload, or `None` if this instance
    /// never met it.
    pub fn keyword_name(&self, hash: u64) -> Option<&str> {
        self.names.get(&hash).map(|n| &**n)
    }

    /// How many distinct symbol names this instance has recorded.
    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

#[cfg(test)]
mod tests;
