use crate::value::SymbolId;
use rustc_hash::FxHashMap;
use std::rc::Rc;

/// Symbol interning table for fast symbol comparison
///
/// Uses `Rc<str>` for symbol names to avoid duplication:
/// - Single allocation via `Rc::from(name)`
/// - Shared reference counting between map and names vector
/// - Reduces memory fragmentation
#[derive(Debug)]
pub struct SymbolTable {
    map: FxHashMap<Rc<str>, SymbolId>,
    names: Vec<Rc<str>>,
}

impl SymbolTable {
    pub fn new() -> Self {
        SymbolTable {
            map: FxHashMap::default(),
            names: Vec::new(),
        }
    }

    /// Intern a symbol, returning its ID
    ///
    /// Uses `Rc::from()` for a single allocation that's shared between
    /// the map and names vector, avoiding the previous double-allocation.
    pub fn intern(&mut self, name: &str) -> SymbolId {
        if let Some(&id) = self.map.get(name) {
            return id;
        }

        let id = SymbolId(self.names.len() as u32);
        let shared_name: Rc<str> = Rc::from(name); // Single allocation
        self.names.push(shared_name.clone());
        self.map.insert(shared_name, id);
        id
    }

    /// Get the name of a symbol by ID
    pub fn name(&self, id: SymbolId) -> Option<&str> {
        self.names.get(id.0 as usize).map(|s| s.as_ref())
    }

    /// Check if a symbol exists
    pub fn get(&self, name: &str) -> Option<SymbolId> {
        self.map.get(name).copied()
    }

    /// Extract all symbol ID → name mappings.
    /// Used for cross-thread symbol portability.
    pub fn all_names(&self) -> std::collections::HashMap<u32, String> {
        self.names
            .iter()
            .enumerate()
            .map(|(i, name)| (i as u32, name.to_string()))
            .collect()
    }
}

impl Default for SymbolTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_interning() {
        let mut table = SymbolTable::new();
        let id1 = table.intern("foo");
        let id2 = table.intern("bar");
        let id3 = table.intern("foo");

        assert_eq!(id1, id3);
        assert_ne!(id1, id2);
        assert_eq!(table.name(id1), Some("foo"));
        assert_eq!(table.name(id2), Some("bar"));
    }
}
